//! Bind stable local projects to ChatGPT's ordinary Project UI, without private API calls.

use std::{fs, io::Write as _, path::Path};

use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::time::{Duration, Instant, sleep};

use crate::{Project, Result, chrome::Chrome, page};

#[derive(Deserialize, Serialize)]
struct Binding {
	project: Project,
	// None fences a Create click whose outcome must be recovered, never repeated.
	url: Option<String>,
}

pub(crate) fn project_url(raw: &str) -> Result<String> {
	let url = url::Url::parse(raw).map_err(|e| e.to_string())?;
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
	segment.strip_prefix("g-p-").is_some_and(|id| {
		!id.is_empty() && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
	})
}

pub(crate) async fn enter(browser: &mut Chrome, root: &Path, project: &Project) -> Result<String> {
	let path = root.join(format!("project-{}.json", crate::store::digest(project.id.as_bytes())));
	let saved = match fs::read(&path) {
		Ok(bytes) => Some(serde_json::from_slice::<Binding>(&bytes).map_err(|e| e.to_string())?),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
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
		sleep(Duration::from_millis(250)).await;
	}
	Err("ChatGPT has not confirmed project creation. Resume the same request; creation will not be repeated.".into())
}

async fn find(browser: &mut Chrome, name: &str) -> Result<Option<String>> {
	let name = serde_json::to_string(name).map_err(|e| e.to_string())?;
	let value = browser.evaluate(&format!(r#"(()=>{{const name={name};const urls=Array.from(document.querySelectorAll('a[href]')).filter(a=>a.innerText.trim()===name&&a.getAttribute('href')?.includes('/g/g-p-')).map(a=>a.href);if(Array.from(document.querySelectorAll('h1')).some(h=>h.innerText.trim()===name)&&location.pathname.endsWith('/project'))urls.push(location.href);return urls;}})()"#)).await?;
	let urls: std::collections::BTreeSet<_> = value
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
	browser.evaluate(r#"(()=>{const b=Array.from(document.querySelectorAll('button,a,[role="button"]')).find(b=>b.getClientRects().length&&/^new project$/i.test(b.innerText.trim()));if(!b)throw Error('ChatGPT New project control is unavailable');b.click();return true;})()"#).await?;
	let deadline = Instant::now() + Duration::from_secs(5);
	loop {
		let ready = browser.evaluate(r#"(()=>{const d=Array.from(document.querySelectorAll('[role="dialog"],dialog')).find(d=>d.getClientRects().length);const input=d?.querySelector('input:not([type="hidden"])');if(!input)return false;input.focus();return true;})()"#).await?;
		if ready == true {
			break;
		}
		if Instant::now() >= deadline {
			return Err("ChatGPT project dialog did not open. No project was created.".into());
		}
		sleep(Duration::from_millis(100)).await;
	}
	browser.command("Input.insertText", json!({"text":name})).await?;
	let observed = browser.evaluate(r#"Array.from(document.querySelectorAll('[role="dialog"] input,dialog input')).find(e=>e.getClientRects().length)?.value"#).await?;
	if observed.as_str() != Some(name) {
		return Err(
			"The complete project name could not be verified. No project was created.".into()
		);
	}
	Ok(())
}

async fn navigate(browser: &mut Chrome, url: &str) -> Result<String> {
	browser.command("Page.navigate", json!({"url":url})).await?;
	page::ready(browser).await?;
	let location = browser.evaluate("location.href").await?;
	if location.as_str() != Some(url) {
		return Err("The saved ChatGPT Project is unavailable. No message was sent.".into());
	}
	Ok(url.into())
}

fn save(path: &Path, project: &Project, url: Option<String>) -> Result<()> {
	let bytes = serde_json::to_vec_pretty(&Binding { project: project.clone(), url })
		.map_err(|e| e.to_string())?;
	let mut staged =
		tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing project directory")?)
			.map_err(|e| e.to_string())?;
	staged
		.write_all(&bytes)
		.and_then(|()| staged.as_file().sync_all())
		.map_err(|e| e.to_string())?;
	staged.persist(path).map_err(|e| e.to_string())?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use futures_util::{SinkExt as _, StreamExt as _};

	#[tokio::test]
	async fn uncertain_creation_is_never_replayed_and_unrelated_projects_are_not_adopted() {
		for pending in [true, false] {
			let root = tempfile::tempdir().unwrap();
			let project = Project { id: "stable-id".into(), name: "Same title".into() };
			let path = root
				.path()
				.join(format!("project-{}.json", crate::store::digest(project.id.as_bytes())));
			if pending {
				save(&path, &project, None).unwrap();
			}
			let listener =
				tokio::net::UnixListener::bind(crate::native_host::socket_path(root.path()))
					.unwrap();
			let server = tokio::spawn(async move {
				let (stream, _) = listener.accept().await.unwrap();
				let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
				while let Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) =
					socket.next().await
				{
					let request: serde_json::Value = serde_json::from_str(&text).unwrap();
					let result = match request["method"].as_str().unwrap() {
						"Decodex.ping" =>
							json!({"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}),
						"Runtime.evaluate" => {
							assert!(
								request["params"]["expression"]
									.as_str()
									.unwrap()
									.contains("a[href]"),
								"A Create action must not run"
							);
							json!({"result":{"value":if pending {json!([])} else {json!(["https://chatgpt.com/g/g-p-user/project"])}}})
						},
						method => panic!("Unexpected operation {method}"),
					};
					socket
						.send(tokio_tungstenite::tungstenite::Message::Text(
							json!({"id":request["id"],"result":result}).to_string().into(),
						))
						.await
						.unwrap();
				}
			});
			let mut browser = Chrome::connect(root.path()).await.unwrap();
			let error = enter(&mut browser, root.path(), &project).await.unwrap_err();
			assert!(
				error.contains(if pending { "uncertain outcome" } else { "not adopted" }),
				"{error}"
			);
			drop(browser);
			server.await.unwrap();
		}
	}
}
