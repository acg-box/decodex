//! Durable Send fencing and recovery, independent of browser transport details.

use std::time::Duration;

use tokio::{
	sync::{oneshot, watch::Receiver},
	time::{self, Instant},
};

use crate::{
	Answer, Phase, ProBridge, Receipt, Request, Result,
	chrome::Chrome,
	page::{self, Message, Observation},
	projects,
	store::{self, Material, Record, Store},
};

pub(crate) async fn run(
	bridge: ProBridge,
	request: Option<Request>,
	id: Option<String>,
	candidate: Option<String>,
) -> Result<Receipt> {
	let (reply, received) = oneshot::channel();
	let mut cancelled = bridge.cancelled.subscribe();

	if *cancelled.borrow() {
		return Err("The request owner was stopped. Resume with a new owner.".into());
	}

	// The worker owns shutdown even if its caller drops the returned future.
	tokio::spawn(async move {
		let result = work(bridge, request, id, candidate, &mut cancelled).await;
		let _ = reply.send(result);
	});

	received.await.map_err(|_| "The Pro component stopped before returning a receipt.")?
}

// A final transport message can precede the page's final render. Require two
// unchanged observations of the complete text, code and links before returning.
fn wait_for_stable_answer(observed: &mut Observation, previous: &mut Option<Answer>) {
	let Some(message) = observed
		.messages
		.last_mut()
		.filter(|message| message.role == "assistant" && message.complete && !observed.generating)
	else {
		*previous = None;

		return;
	};
	let answer = message.answer();

	if previous.as_ref() != Some(&answer) {
		message.complete = false;
	}

	*previous = Some(answer);
}

fn validate_before_send(record: &Record, observed: &Observation) -> Result<()> {
	if observed.generating {
		return Err(
			"The conversation already has an answer in progress. No message was sent.".into()
		);
	}

	match &record.parent_answer_id {
		Some(parent) => {
			if observed
				.messages
				.last()
				.is_none_or(|message| &message.id != parent || !message.complete)
			{
				return Err(format!(
					"The saved conversation changed or its last answer is not loaded (expected {parent}; observed {}; {} rendered messages). No message was sent.",
					observed.messages.last().map_or("none", |m| m.id.as_str()),
					observed.messages.len()
				));
			}
			if !record
				.receipt
				.conversation_url
				.as_deref()
				.is_some_and(|saved| page::same_conversation(saved, &observed.url))
			{
				return Err(
					"The continuation opened a different conversation. No message was sent.".into(),
				);
			}
		},
		None if !observed.messages.is_empty()
			|| (observed.url != "https://chatgpt.com/"
				&& Some(observed.url.as_str()) != record.receipt.project_url.as_deref()) =>
			return Err(
				"A new request requires an empty owned ChatGPT page. No message was sent.".into()
			),
		None => {},
	}

	Ok(())
}

fn bind_attachment(record: &mut Record, user: &Message) -> Result<bool> {
	if let Some(expected) = &mut record.receipt.attachment {
		let Some(files) = &user.attachments else { return Ok(false) };
		let [file] = files.as_slice() else {
			return Err("The submitted message does not contain exactly the saved source attachment. This request will not be resent.".into());
		};

		if file.name != expected.name
			|| file.bytes != expected.bytes
			|| expected.file_id.as_ref().is_some_and(|id| id != &file.id)
		{
			return Err("The submitted source attachment differs from the saved manifest. This request will not be resent.".into());
		}

		expected.file_id = Some(file.id.clone());
	}

	Ok(true)
}

fn reconcile(record: &mut Record, observed: &Observation) -> Result<bool> {
	let wire_prompt = record.wire_prompt();
	let prompt_sha256 = store::digest(wire_prompt.trim().as_bytes());
	// A previously submitted editor hard break can render as one final backslash.
	// Match that observed representation by its complete hash, only for the exact
	// single-LF original. New submissions omit terminal LF before filling the editor.
	let hard_break_sha256 = wire_prompt
		.strip_suffix('\n')
		.filter(|text| !text.ends_with(['\n', '\r']))
		.map(|text| store::digest(format!("{text}\\").trim().as_bytes()));
	let matches: Vec<_> = observed
		.messages
		.iter()
		.enumerate()
		.filter(|(_, message)| {
			message.role == "user"
				&& (message.text_sha256.as_deref().is_some_and(|hash| {
					hash == prompt_sha256 || Some(hash) == hard_break_sha256.as_deref()
				}) || message.markdown_text_sha256.as_deref() == Some(prompt_sha256.as_str()))
		})
		.collect();

	if matches.len() > 1 {
		return Err(
			"The request marker occurs more than once; manual reconciliation is required.".into()
		);
	}

	let Some((index, user)) = matches.first().copied() else {
		return Ok(false);
	};

	if observed.messages[index + 1..].iter().any(|message| message.role == "user") {
		return Err(
			"Another message advanced the conversation. This request was not replayed.".into()
		);
	}

	// Chrome's route update can lag behind the rendered user message. Keep the Send
	// fence until a canonical conversation URL is available; never submit again.
	let url = match page::conversation_url(&observed.url) {
		Ok(url) => url,
		Err(_)
			if record.receipt.phase == Phase::Sending
				&& record.receipt.conversation_url.is_none() =>
			return Ok(false),
		Err(error) => return Err(error),
	};

	if record
		.receipt
		.conversation_url
		.as_ref()
		.is_some_and(|saved| !page::same_conversation(saved, &url))
		|| record.receipt.user_message_id.as_ref().is_some_and(|saved| saved != &user.id)
	{
		return Err("The saved submission identity changed.".into());
	}

	if let Some(project) = record.receipt.project_url.as_deref()
		&& url.contains("/g/g-p-")
		&& !projects::contains_url(project, &url)
	{
		return Err("The research conversation moved to a different project.".into());
	}

	record.receipt.conversation_url = Some(url);
	record.receipt.user_message_id = Some(user.id.clone());
	record.receipt.input_binding = Some(
		if user.text_sha256.as_deref() == Some(prompt_sha256.as_str()) {
			"verbatim"
		} else if user.markdown_text_sha256.as_deref() == Some(prompt_sha256.as_str()) {
			"markdown_text"
		} else {
			"terminal_hard_break"
		}
		.into(),
	);
	record.receipt.phase = Phase::Submitted;
	record.receipt.last_error = None;

	if !bind_attachment(record, user)? {
		return Ok(false);
	}

	let answer =
		observed.messages[index + 1..].iter().rev().find(|message| message.role == "assistant");

	if let Some(answer) =
		answer.filter(|answer| !observed.generating && answer.complete && !answer.text.is_empty())
	{
		record.answer = Some(answer.answer());

		if !record
			.receipt
			.execution
			.as_ref()
			.is_some_and(|identity| identity.message_id == answer.id && identity.is_pro())
		{
			return Err("The full answer was saved, but its Pro execution identity is unconfirmed. Resume to read the original conversation; this request will not be sent again.".into());
		}

		record.receipt.phase = Phase::Complete;

		return Ok(true);
	}

	Ok(false)
}

async fn work(
	bridge: ProBridge,
	request: Option<Request>,
	id: Option<String>,
	candidate: Option<String>,
	cancelled: &mut Receiver<bool>,
) -> Result<Receipt> {
	let store = Store::open(&bridge.root)?;
	let mut record = {
		let _lease = store.lease()?;

		match request {
			Some(request) => store.prepare(request)?,
			None => store.load(id.as_deref().ok_or("A saved request ID is required.")?)?,
		}
	};

	if record.receipt.phase == Phase::Complete {
		return Ok(record.receipt);
	}

	// Duplicate callers observe the same request; only one process may execute it.
	let Some(_worker) = store.worker_lease(&record.request.id)? else {
		return Ok(record.receipt);
	};
	// One generation per account by default. Queueing does not hold the journal or
	// block the user from signing in. No control-model polling is involved.
	let _execution = loop {
		if let Some(lease) = store.execution_lease()? {
			break lease;
		}

		tokio::select! {
			_ = time::sleep(std::time::Duration::from_millis(250)) => {},
			_ = cancelled.changed() => return Err("Stopped while waiting for the research browser. The request is saved.".into()),
		}
	};

	record = store.load(&record.request.id)?;

	if record.receipt.last_error.take().is_some() {
		store.save(&record)?;
	}
	if candidate.is_some()
		&& (record.receipt.phase != Phase::Sending || record.receipt.conversation_url.is_some())
	{
		return Err("A candidate URL is only used to reconcile an uncertain first Send.".into());
	}
	if candidate.is_none()
		&& record.receipt.phase == Phase::Sending
		&& record.receipt.conversation_url.is_none()
		&& record.target_id.is_none()
	{
		return Err("Send has an uncertain outcome and no saved conversation URL. Reconcile it before continuing; this component will not resend it.".into());
	}
	if bridge.config.poll_interval.is_zero() || bridge.config.answer_timeout.is_zero() {
		return Err("Polling and answer wait durations must be greater than zero.".into());
	}

	let mut browser = match Chrome::connect(&bridge.root).await {
		Ok(browser) => browser,
		Err(error) => {
			record.receipt.last_error = Some(error.clone());

			store.save(&record)?;

			return Err(error);
		},
	};
	let result = tokio::select! {
		result = execute(&bridge, &store, &mut record, &mut browser, candidate.as_deref()) => result,
		_ = cancelled.changed() => Err("The request was cancelled. Resume its saved ID to reconcile it.".into()),
	};

	// Closing a waiter must not cancel research. Keep an incomplete tab for recovery.
	if result.is_ok() {
		let _ = browser.close_tab().await;
	}

	if let Err(error) = &result {
		record.receipt.last_error = Some(error.clone());

		store.save(&record)?;
	}

	result?;

	Ok(record.receipt)
}

async fn execute(
	bridge: &ProBridge,
	store: &Store,
	record: &mut Record,
	browser: &mut Chrome,
	candidate: Option<&str>,
) -> Result<()> {
	let target =
		candidate
			.or(record.receipt.conversation_url.as_deref())
			.or((record.receipt.phase == Phase::Prepared).then_some("https://chatgpt.com/"));
	let material = store.material(record)?;

	browser.open(record.target_id.as_deref(), target, false).await?;

	record.target_id = browser.target.clone();

	store.save(record)?;

	if record.receipt.phase != Phase::Prepared && target.is_some() {
		browser.command("Page.reload", serde_json::json!({})).await?;
	}

	page::ready(browser).await?;

	// Keep this owned page responsive without activating a browser tab. Chrome's
	// normal focus emulation lets React menus and streamed content render offscreen.
	browser
		.command("Emulation.setFocusEmulationEnabled", serde_json::json!({"enabled":true}))
		.await?;

	if record.receipt.phase == Phase::Prepared {
		submit_prepared(bridge, store, record, browser, &material).await?;
	}

	let deadline = Instant::now() + bridge.config.answer_timeout;
	let mut previous_answer = None;
	let mut refreshed_final = false;

	loop {
		if record.receipt.phase == Phase::Sending
			&& record.receipt.conversation_url.is_none()
			&& let Some(url) = browser.evaluate("location.href").await?.as_str()
			&& let Ok(url) = page::conversation_url(url)
		{
			// Keep the phase fenced. The URL alone does not confirm the submitted text.
			record.receipt.conversation_url = Some(url);

			store.save(record)?;
		}

		let mut observed = page::observe_request(browser, &record.request.id).await?;

		wait_for_stable_answer(&mut observed, &mut previous_answer);

		// A stream can contain deltas instead of a final message. Read the same
		// completed conversation once to recover its original text and model fields.
		if !refreshed_final
			&& record.receipt.conversation_url.is_some()
			&& observed.messages.last().is_some_and(|message| {
				message.role == "assistant"
					&& message.complete
					&& (!browser.messages.get(&message.id).is_some_and(|native| native.complete)
						|| !browser
							.identities
							.iter()
							.any(|identity| identity.message_id == message.id))
			}) {
			refreshed_final = true;

			browser.command("Page.reload", serde_json::json!({})).await?;

			page::ready(browser).await?;

			continue;
		}

		let before = serde_json::to_vec(&record.receipt).map_err(|e| e.to_string())?;

		if let Some(identity) = browser.identities.iter().rev().find(|identity| {
			observed.messages.last().is_some_and(|message| {
				message.role == "assistant" && message.id == identity.message_id
			})
		}) {
			record.receipt.model = Some(identity.model.clone());
			record.receipt.execution = Some(identity.clone());
		}

		let complete = reconcile(record, &observed)?;

		if before != serde_json::to_vec(&record.receipt).map_err(|e| e.to_string())? {
			store.save(record)?;
		}
		if complete {
			return Ok(());
		}
		if observed.notices.iter().any(|notice| notice.contains("Resume stream unavailable")) {
			return Err("ChatGPT could not resume its response stream. The saved request is retained and will not be resent.".into());
		}

		if let Some(failure) = &browser.submission_failure {
			return Err(format!("{failure} Resume this saved request; no input will be resent."));
		}
		if let Some(failure) = &browser.resume_failure {
			return Err(format!("{failure} The saved request is retained and will not be resent."));
		}

		if Instant::now() >= deadline {
			let reason = if record.receipt.phase == Phase::Sending {
				"Send could not be confirmed. The request remains fenced and will not be resent."
			} else {
				"The Pro answer is still pending. Resume the same request ID to continue observing it."
			};

			return Err(format!("{reason} {}", observed.notices.join("\n")).trim().into());
		}

		time::sleep(bridge.config.poll_interval).await;
	}
}

async fn submit_prepared(
	bridge: &ProBridge,
	store: &Store,
	record: &mut Record,
	browser: &mut Chrome,
	material: &Material,
) -> Result<()> {
	if record.parent_answer_id.is_none()
		&& let Some(project) = &record.project
	{
		record.receipt.project_url = Some(projects::enter(browser, &bridge.root, project).await?);

		store.save(record)?;
	}

	observe_before_send(record, browser).await?;

	page::select_pro(browser).await?;
	page::fill(browser, material).await?;

	if let Some(attachment) = store.attachment(record)? {
		page::attach(browser, &attachment).await?;
	}

	let observed = observe_before_send(record, browser).await?;

	record.receipt.baseline_messages = observed.messages.len();
	record.receipt.phase = Phase::Sending;
	record.receipt.last_error = None;

	store.save(record)?;

	if !page::send(browser).await? {
		// The page confirmed that no click ran. Transport errors remain Sending.
		record.receipt.phase = Phase::Prepared;

		store.save(record)?;

		return Err(
			"ChatGPT disabled Send before submission. No message was sent; resume this request."
				.into(),
		);
	}

	Ok(())
}

async fn observe_before_send(record: &Record, browser: &mut Chrome) -> Result<Observation> {
	let deadline = Instant::now() + Duration::from_secs(15);

	loop {
		let observed = page::observe(browser).await?;
		// A cold page can mount its composer before it restores conversation history.
		// Wait for the exact saved answer, without treating an empty first frame as drift.
		let restored = record.parent_answer_id.as_ref().is_none_or(|parent| {
			observed.messages.last().is_some_and(|m| &m.id == parent && m.complete)
		});
		let advanced = record.parent_answer_id.as_ref().is_some_and(|parent| {
			observed.messages.iter().position(|m| &m.id == parent).is_some_and(|index| {
				observed.messages[index + 1..].iter().any(|m| m.role == "user")
			})
		});

		if restored || advanced || observed.generating || Instant::now() >= deadline {
			validate_before_send(record, &observed)?;

			return Ok(observed);
		}

		time::sleep(Duration::from_millis(100)).await;
	}
}

#[cfg(test)] mod tests;
