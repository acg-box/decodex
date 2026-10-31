//! Durable Send fencing and recovery, independent of browser transport details.

use tokio::{
	sync::{oneshot, watch},
	time::{Instant, sleep},
};

use crate::{
	Phase, ProBridge, Receipt, Request, Result,
	chrome::Chrome,
	page::{self, Observation},
	store::{Record, Store},
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

async fn work(
	bridge: ProBridge,
	request: Option<Request>,
	id: Option<String>,
	candidate: Option<String>,
	cancelled: &mut watch::Receiver<bool>,
) -> Result<Receipt> {
	let store = Store::open(&bridge.root)?;
	let _lease = store.lease()?;
	let mut record = match request {
		Some(request) => store.prepare(request)?,
		None => store.load(id.as_deref().ok_or("A saved request ID is required.")?)?,
	};
	if record.receipt.phase == Phase::Complete {
		return Ok(record.receipt);
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
	let mut browser = match Chrome::connect(&bridge.config, &bridge.root).await {
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
	let target = candidate.or(record.receipt.conversation_url.as_deref());
	let material = store.material(record)?;
	browser
		.open(record.target_id.as_deref(), target.unwrap_or("https://chatgpt.com/"), false)
		.await?;
	record.target_id = browser.target.clone();
	store.save(record)?;
	if record.receipt.phase != Phase::Prepared && target.is_some() {
		browser.command("Page.reload", serde_json::json!({})).await?;
	}
	page::ready(browser).await?;
	if record.receipt.phase == Phase::Prepared {
		let observed = page::observe(browser).await?;
		validate_before_send(record, &observed)?;
		page::select_pro(browser).await?;
		page::fill(browser, &material).await?;
		let observed = page::observe(browser).await?;
		validate_before_send(record, &observed)?;
		record.receipt.baseline_messages = observed.messages.len();
		record.receipt.phase = Phase::Sending;
		record.receipt.last_error = None;
		store.save(record)?;
		page::send(browser).await?;
	}
	let deadline = Instant::now() + bridge.config.answer_timeout;
	let submission_deadline = Instant::now() + std::time::Duration::from_secs(30);
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
		let observed = page::observe_request(browser, &record.request.id).await?;
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
		if record.receipt.phase == Phase::Sending && Instant::now() >= submission_deadline {
			return Err(format!(
				"Send could not be confirmed. The request remains fenced and will not be resent. {}",
				observed.notices.join("\n")
			)
			.trim()
			.into());
		}
		if Instant::now() >= deadline {
			return Err(format!(
				"The Pro answer is still pending. Resume the same request ID to continue observing it. {}",
				observed.notices.join("\n")
			)
			.trim()
			.into());
		}
		sleep(bridge.config.poll_interval).await;
	}
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
			if record.receipt.conversation_url.as_deref() != Some(observed.url.as_str()) {
				return Err(
					"The continuation opened a different conversation. No message was sent.".into(),
				);
			}
		},
		None if !observed.messages.is_empty() || observed.url != "https://chatgpt.com/" =>
			return Err(
				"A new request requires an empty owned ChatGPT page. No message was sent.".into()
			),
		None => {},
	}
	Ok(())
}

fn reconcile(record: &mut Record, observed: &Observation) -> Result<bool> {
	let wire_prompt = record.wire_prompt();
	let prompt_sha256 = crate::store::digest(wire_prompt.trim().as_bytes());
	// A previously submitted editor hard break can render as one final backslash.
	// Match that observed representation by its complete hash, only for the exact
	// single-LF original. New submissions omit terminal LF before filling the editor.
	let hard_break_sha256 = wire_prompt
		.strip_suffix('\n')
		.filter(|text| !text.ends_with(['\n', '\r']))
		.map(|text| crate::store::digest(format!("{text}\\").trim().as_bytes()));
	let matches: Vec<_> = observed
		.messages
		.iter()
		.enumerate()
		.filter(|(_, message)| {
			message.role == "user"
				&& message.text_sha256.as_deref().is_some_and(|hash| {
					hash == prompt_sha256 || Some(hash) == hard_break_sha256.as_deref()
				})
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
	if record.receipt.conversation_url.as_ref().is_some_and(|saved| saved != &url)
		|| record.receipt.user_message_id.as_ref().is_some_and(|saved| saved != &user.id)
	{
		return Err("The saved submission identity changed.".into());
	}
	record.receipt.conversation_url = Some(url);
	record.receipt.user_message_id = Some(user.id.clone());
	record.receipt.phase = Phase::Submitted;
	record.receipt.last_error = None;
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

#[cfg(test)] mod tests;
