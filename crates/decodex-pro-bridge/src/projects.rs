//! Bind stable local projects to ChatGPT's ordinary Project UI, without private API calls.

use std::{
	collections::BTreeSet,
	fs,
	io::{ErrorKind, Write as _},
	path::Path,
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use tokio::time::{self, Duration, Instant};
use url::Url;

use crate::{Project, Result, chrome::Chrome, page, store};

#[derive(Deserialize, Serialize)]
struct Binding {
	project: Project,
	// None fences a Create click whose outcome must be recovered, never repeated.
	url: Option<String>,
}

pub(crate) fn project_url(raw: &str) -> Result<String> {
	let url = Url::parse(raw).map_err(|e| e.to_string())?;
	let parts: Vec<_> = url.path().split('/').collect();

	if url.origin().ascii_serialization() != "https://chatgpt.com"
		|| !url.username().is_empty()
		|| url.password().is_some()
		|| url.query().is_some()
		|| url.fragment().is_some()
		|| url.as_str() != raw
		|| parts.len() != 4
		|| parts[1] != "g"
		|| parts[3] != "project"
		|| !project_segment(parts[2])
	{
		return Err("Expected an exact ChatGPT Project URL.".into());
	}

	Ok(raw.into())
}

pub(crate) fn project_segment(segment: &str) -> bool {
	segment_key(segment).is_some_and(|id| {
		!id.is_empty() && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
	})
}

pub(crate) fn contains_url(project: &str, page: &str) -> bool {
	let Ok(project) = Url::parse(project) else { return false };
	let Ok(page) = Url::parse(page) else { return false };
	let expected = project.path_segments().and_then(|mut p| p.nth(1)).and_then(segment_key);
	let observed = page.path_segments().and_then(|mut p| p.nth(1)).and_then(segment_key);

	expected.is_some() && expected == observed
}

pub(crate) async fn enter(browser: &mut Chrome, root: &Path, project: &Project) -> Result<String> {
	let path = root.join(format!("project-{}.json", store::digest(project.id.as_bytes())));
	let saved = match fs::read(&path) {
		Ok(bytes) => Some(serde_json::from_slice::<Binding>(&bytes).map_err(|e| e.to_string())?),
		Err(e) if e.kind() == ErrorKind::NotFound => None,
		Err(e) => return Err(e.to_string()),
	};

	if let Some(binding) = &saved {
		if binding.project.id != project.id {
			return Err("The saved project identity changed.".into());
		}

		if let Some(url) = &binding.url {
			return navigate(browser, &project_url(url)?).await;
		}
	}

	let name = format!("Decodex · {}", saved.as_ref().map_or(&project.name, |b| &b.project.name));

	open_projects(browser).await?;

	let existing = find(browser, &name).await?;

	if saved.is_some() {
		let url = existing.ok_or("Project creation has an uncertain outcome. Open its setup tab to recover; no duplicate project was created.")?;

		save(&path, project, Some(url.clone()))?;

		return navigate(browser, &url).await;
	}
	if existing.is_some() {
		return Err(format!(
			"A ChatGPT project named {name} already exists without a saved binding. It was not adopted or changed."
		));
	}

	prepare_create(browser, &name).await?;
	save(&path, project, None)?;

	browser.evaluate(r#"(()=>{const d=Array.from(document.querySelectorAll('[role="dialog"],dialog')).find(d=>d.getClientRects().length);const b=Array.from(d?.querySelectorAll('button')??[]).find(b=>/^(create project|create)$/i.test(b.innerText.trim())&&!b.disabled);if(!b)throw Error('Create project is unavailable');b.click();return true;})()"#).await?;

	let deadline = Instant::now() + Duration::from_secs(20);

	while Instant::now() < deadline {
		if let Some(url) = find(browser, &name).await? {
			save(&path, project, Some(url.clone()))?;

			return navigate(browser, &url).await;
		}

		time::sleep(Duration::from_millis(250)).await;
	}

	Err("ChatGPT has not confirmed project creation. Resume the same request; creation will not be repeated.".into())
}

fn segment_key(segment: &str) -> Option<&str> {
	let id = segment.strip_prefix("g-p-")?;

	if id.len() >= 32
		&& id.as_bytes()[..32].iter().all(u8::is_ascii_hexdigit)
		&& (id.len() == 32 || id.as_bytes()[32] == b'-')
	{
		Some(&id[..32])
	} else {
		Some(id)
	}
}

fn save(path: &Path, project: &Project, url: Option<String>) -> Result<()> {
	let bytes = serde_json::to_vec_pretty(&Binding { project: project.clone(), url })
		.map_err(|e| e.to_string())?;
	let mut staged = NamedTempFile::new_in(path.parent().ok_or("Missing project directory")?)
		.map_err(|e| e.to_string())?;

	staged
		.write_all(&bytes)
		.and_then(|()| staged.as_file().sync_all())
		.map_err(|e| e.to_string())?;
	staged.persist(path).map_err(|e| e.to_string())?;

	Ok(())
}

async fn find(browser: &mut Chrome, name: &str) -> Result<Option<String>> {
	let name = serde_json::to_string(name).map_err(|e| e.to_string())?;
	let value = browser.evaluate(&format!(r#"(()=>{{const name={name};const urls=Array.from(document.querySelectorAll('a[href]')).filter(a=>a.innerText.trim()===name&&a.getAttribute('href')?.includes('/g/g-p-')).map(a=>a.href);if(Array.from(document.querySelectorAll('h1')).some(h=>h.innerText.trim()===name)&&location.pathname.endsWith('/project'))urls.push(location.href);return urls;}})()"#)).await?;
	let urls: BTreeSet<_> = value
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|v| v.as_str())
		.filter_map(|v| project_url(v).ok())
		.collect();

	if urls.len() > 1 {
		return Err(
			"More than one ChatGPT project has this name. Resolve its binding before research."
				.into(),
		);
	}

	Ok(urls.into_iter().next())
}

async fn prepare_create(browser: &mut Chrome, name: &str) -> Result<()> {
	let deadline = Instant::now() + Duration::from_secs(15);

	loop {
		// A cold Projects view can expose its server-rendered Create button before
		// its event handler is mounted. Retry opening this reversible dialog only;
		// the final Create project action remains fenced and is never repeated.
		let ready = browser.evaluate(r#"(()=>{const d=Array.from(document.querySelectorAll('[role="dialog"],dialog')).find(d=>d.getClientRects().length&&d.innerText.includes('Create project'));const input=d?.querySelector('input:not([type="hidden"])');if(input){input.focus();return true;}const b=Array.from(document.querySelectorAll('button')).find(b=>b.getClientRects().length&&location.pathname==='/projects'&&b.innerText.trim()==='Create');if(b)b.click();return false;})()"#).await?;

		if ready == true {
			break;
		}
		if Instant::now() >= deadline {
			return Err("ChatGPT project dialog did not open. No project was created.".into());
		}

		time::sleep(Duration::from_millis(300)).await;
	}

	browser.command("Input.insertText", serde_json::json!({"text":name})).await?;

	let observed = browser.evaluate(r#"Array.from(document.querySelectorAll('[role="dialog"] input,dialog input')).find(e=>e.getClientRects().length)?.value"#).await?;

	if observed.as_str() != Some(name) {
		return Err(
			"The complete project name could not be verified. No project was created.".into()
		);
	}

	Ok(())
}

async fn open_projects(browser: &mut Chrome) -> Result<()> {
	// This is an ordinary observed navigation route, not a private service endpoint.
	browser
		.command("Page.navigate", serde_json::json!({"url":"https://chatgpt.com/projects"}))
		.await?;

	let deadline = Instant::now() + Duration::from_secs(10);

	loop {
		let ready=browser.evaluate(r#"document.readyState==='complete'&&location.pathname==='/projects'&&Array.from(document.querySelectorAll('button')).some(b=>b.getClientRects().length&&b.innerText.trim()==='Create')"#).await?;

		if ready == true {
			return Ok(());
		}
		if Instant::now() >= deadline {
			return Err("ChatGPT Projects did not load. No project was created.".into());
		}

		time::sleep(Duration::from_millis(100)).await;
	}
}

async fn navigate(browser: &mut Chrome, url: &str) -> Result<String> {
	browser.command("Page.navigate", serde_json::json!({"url":url})).await?;

	page::ready(browser).await?;

	let location = browser.evaluate("location.href").await?;
	let actual = location.as_str().ok_or("ChatGPT has no project location")?;

	if project_url(actual).is_err() || !contains_url(url, actual) {
		return Err("The saved ChatGPT Project is unavailable. No message was sent.".into());
	}

	Ok(actual.into())
}

#[cfg(test)]
mod tests {

	use futures_util::{SinkExt as _, StreamExt as _};

	use crate::{
		native_host,
		projects::{self, Chrome, Project},
		store,
	};

	#[tokio::test]
	async fn uncertain_creation_is_never_replayed_and_unrelated_projects_are_not_adopted() {
		for pending in [true, false] {
			let root = tempfile::tempdir().unwrap();
			let project = Project { id: "stable-id".into(), name: "Same title".into() };
			let path =
				root.path().join(format!("project-{}.json", store::digest(project.id.as_bytes())));

			if pending {
				projects::save(&path, &project, None).unwrap();
			}

			let listener =
				tokio::net::UnixListener::bind(native_host::socket_path(root.path())).unwrap();
			let server = tokio::spawn(async move {
				let (stream, _) = listener.accept().await.unwrap();
				let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();

				while let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) =
					socket.next().await
				{
					let request: serde_json::Value = serde_json::from_str(&text).unwrap();
					let result = match request["method"].as_str().unwrap() {
						"Decodex.ping" =>
							serde_json::json!({"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}),
						"Runtime.evaluate" => {
							if request["params"]["expression"]
								.as_str()
								.unwrap()
								.contains("document.readyState")
							{
								serde_json::json!({"result":{"value":true}})
							} else {
								assert!(
									request["params"]["expression"]
										.as_str()
										.unwrap()
										.contains("a[href]"),
									"A Create action must not run"
								);

								serde_json::json!({"result":{"value":if pending {serde_json::json!([])} else {serde_json::json!(["https://chatgpt.com/g/g-p-user/project"])}}})
							}
						},
						"Page.navigate" => serde_json::json!({}),
						method => panic!("Unexpected operation {method}"),
					};

					socket
						.send(tokio_tungstenite::tungstenite::Message::Text(
							serde_json::json!({"id":request["id"],"result":result})
								.to_string()
								.into(),
						))
						.await
						.unwrap();
				}
			});
			let mut browser = Chrome::connect(root.path()).await.unwrap();
			let error = projects::enter(&mut browser, root.path(), &project).await.unwrap_err();

			assert!(
				error.contains(if pending { "uncertain outcome" } else { "not adopted" }),
				"{error}"
			);

			drop(browser);

			server.await.unwrap();
		}
	}
}
