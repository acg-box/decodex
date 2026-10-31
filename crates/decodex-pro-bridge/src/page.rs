//! Small ChatGPT UI adapter. Page selectors are separate from durable workflow state.

use serde::Deserialize;
use serde_json::json;
use tokio::time::{Instant, sleep};
use url::Url;

use crate::{Answer, Connection, ConnectionState, Link, Result, chrome::Chrome, store::Material};

const CONTROLS: &str = r#"
const visible=e=>!!e&&e.getClientRects().length>0;
const editor=document.querySelector('#prompt-textarea,[role="textbox"][aria-label="Ask ChatGPT"]');
const model=document.querySelector('[data-testid="model-switcher-dropdown-button"],button[aria-label="Select ChatGPT model"]');
const buttons=Array.from(document.querySelectorAll('button'));
const send=document.querySelector('[data-testid="send-button"],button[aria-label="Send prompt"],button[aria-label="Send"]');
"#;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Message {
	pub role: String,
	pub id: String,
	pub text: String,
	#[serde(default)]
	pub text_sha256: Option<String>,
	pub complete: bool,
	pub code_blocks: Vec<String>,
	pub links: Vec<Link>,
}

impl Message {
	pub fn answer(&self) -> Answer {
		Answer {
			message_id: self.id.clone(),
			text: self.text.clone(),
			code_blocks: self.code_blocks.clone(),
			links: self.links.clone(),
		}
	}
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Observation {
	pub url: String,
	pub model: String,
	pub generating: bool,
	pub messages: Vec<Message>,
	#[serde(default)]
	pub notices: Vec<String>,
}

pub(crate) async fn connection(
	browser: &mut Chrome,
	wait: std::time::Duration,
) -> Result<Connection> {
	let deadline = Instant::now() + wait;
	loop {
		let state = browser.evaluate(&format!(r#"(()=>{{ {CONTROLS}
            const ready=visible(editor)&&visible(model);
            const verification=!ready&&(!!document.querySelector('script[src*="/cdn-cgi/challenge-platform/"]')||
              (document.title==='Just a moment...'&&(document.body?.innerText??'').includes('Cloudflare')));
            return {{ready,verification,model:visible(model)?model.innerText:null,
                login:buttons.some(b=>visible(b)&&/^(log in|sign in)$/i.test(b.innerText.trim()))}};
        }})()"#)).await?;
		if state["ready"] == true {
			return Ok(Connection {
				state: ConnectionState::Connected,
				selected_model: state["model"].as_str().map(str::to_owned),
			});
		}
		if state["verification"] == true {
			return Ok(Connection {
				state: ConnectionState::VerificationRequired,
				selected_model: None,
			});
		}
		if state["login"] == true {
			return Ok(Connection { state: ConnectionState::SignInRequired, selected_model: None });
		}
		if Instant::now() >= deadline {
			return Ok(Connection {
				state: ConnectionState::AttentionRequired,
				selected_model: None,
			});
		}
		sleep(std::time::Duration::from_millis(250)).await;
	}
}

pub(crate) async fn ready(browser: &mut Chrome) -> Result<()> {
	match connection(browser, std::time::Duration::from_secs(15)).await?.state {
		ConnectionState::Connected => Ok(()),
		ConnectionState::SignInRequired =>
			Err("Sign in to ChatGPT in the dedicated research browser, then resume this request."
				.into()),
		ConnectionState::VerificationRequired => Err("ChatGPT requires a security check in the research browser. Research is waiting; no message was sent.".into()),
		ConnectionState::AttentionRequired =>
			Err("ChatGPT needs attention in the research browser. No message was sent.".into()),
	}
}

pub(crate) async fn select_pro(browser: &mut Chrome) -> Result<()> {
	let selected = observe(browser).await?.model;
	if selected.trim().eq_ignore_ascii_case("pro")
		|| selected.split_whitespace().last() == Some("Pro")
	{
		return Ok(());
	}
	browser.evaluate(&format!(r#"(()=>{{ {CONTROLS} if(!visible(model))throw Error('Model selector unavailable');model.click();return true; }})()"#)).await?;
	// Read the visible menu. Never force an inaccessible option or edit React state.
	sleep(std::time::Duration::from_millis(150)).await;
	let result = browser.evaluate(r#"(()=>{
        const visible=e=>e.getClientRects().length>0;
        const choices=Array.from(document.querySelectorAll('[role="menuitem"],[role="radio"],[role="option"],button'))
          .filter(e=>visible(e)&&/^Pro(?:\s|$)/.test(e.innerText.trim()));
        if(choices.length===1){choices[0].click();return {selected:true};}
        const power=Array.from(document.querySelectorAll('[role="menuitem"],[role="slider"]'))
          .find(e=>visible(e)&&/Power/i.test(e.getAttribute('aria-label')??e.innerText));
        return {selected:false,power:!!power};
    })()"#).await?;
	if result["selected"] != true {
		return Err("The visible ChatGPT model menu does not expose one unambiguous Pro option. No message was sent.".into());
	}
	browser.evaluate("document.activeElement?.blur();true").await?;
	sleep(std::time::Duration::from_millis(100)).await;
	let selected = observe(browser).await?.model;
	if !selected.split_whitespace().any(|part| part == "Pro") {
		return Err("ChatGPT did not retain the Pro selection. No message was sent.".into());
	}
	Ok(())
}

pub(crate) async fn fill(browser: &mut Chrome, material: &Material) -> Result<()> {
	let bytes = std::fs::read(&material.path).map_err(|e| e.to_string())?;
	if bytes.len() != material.bytes || crate::store::digest(&bytes) != material.sha256 {
		return Err("The immutable request changed. No message was sent.".into());
	}
	let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
	let text = text.trim_end_matches('\n');
	let draft = browser
		.evaluate(&format!(
			r#"(()=>{{ {CONTROLS}
        if(!visible(editor))throw Error('ChatGPT composer unavailable');
        editor.focus();return editor.innerText;
    }})()"#
		))
		.await?;
	if draft.as_str().is_some_and(|draft| !draft.is_empty() && !text.starts_with(draft)) {
		return Err("The owned composer has a different draft. No message was sent.".into());
	}
	// CDP inserts ordinary text through Chromium's input path, without clipboard or paste
	// attachments.
	browser.command("Input.dispatchKeyEvent", json!({"type":"keyDown","key":"a","code":"KeyA","modifiers":if cfg!(target_os="macos"){4}else{2},"windowsVirtualKeyCode":65})).await?;
	browser.command("Input.dispatchKeyEvent", json!({"type":"keyUp","key":"a","code":"KeyA","modifiers":if cfg!(target_os="macos"){4}else{2},"windowsVirtualKeyCode":65})).await?;
	browser.command("Input.insertText", json!({"text":text})).await?;
	let prepared = browser.evaluate(&format!(r#"(()=>{{ {CONTROLS}
        const block=n=>n?.nodeType===1&&/^(P|DIV)$/.test(n.tagName);
        const read=n=>{{
          if(n.nodeType===3)return n.nodeValue??'';
          if(n.nodeType!==1)return '';
          if(n.tagName==='BR')return n.classList.contains('ProseMirror-trailingBreak')?'':'\n';
          if(block(n)&&n.childNodes.length===1&&n.firstChild.nodeName==='BR')return '';
          return Array.from(n.childNodes).map((c,i,all)=>(i>0&&(block(c)||block(all[i-1]))?'\n':'')+read(c)).join('');
        }};
        return {{text:editor?read(editor):'',ready:visible(send)&&!send.disabled}};
    }})()"#)).await?;
	if prepared["text"].as_str().map(|s| crate::store::digest(s.trim().as_bytes())).as_deref()
		!= Some(material.normalized_sha256.as_str())
		|| prepared["ready"] != true
	{
		return Err(format!(
			"The complete composer text could not be verified (expected {} bytes, observed {}; send ready: {}). No message was sent.",
			text.len(),
			prepared["text"].as_str().map_or(0, str::len),
			prepared["ready"]
		));
	}
	Ok(())
}

pub(crate) async fn send(browser: &mut Chrome) -> Result<()> {
	browser.evaluate(&format!(r#"(()=>{{ {CONTROLS} if(!visible(send)||send.disabled)throw Error('Send unavailable');send.click();return true; }})()"#)).await?;
	Ok(())
}

pub(crate) async fn observe(browser: &mut Chrome) -> Result<Observation> {
	observe_request(browser, "").await
}

pub(crate) async fn observe_request(browser: &mut Chrome, id: &str) -> Result<Observation> {
	let marker = serde_json::to_string(&format!("[Decodex Pro request: {id}]"))
		.map_err(|e| e.to_string())?;
	let value = browser.evaluate(&format!(r#"(()=>{{ {CONTROLS}
        const marker={marker};
        const generating=buttons.some(b=>visible(b)&&/^(stop|stop generating|stop streaming)$/i.test(b.getAttribute('aria-label')??''));
        const traditional=Array.from(document.querySelectorAll('[data-message-author-role][data-message-id]'));
        const elements=traditional.length?traditional:Array.from(document.querySelectorAll('[data-chatgpt-search-message-ids]'));
        const messages=elements.map(m=>{{
          const role=m.getAttribute('data-message-author-role')??(m.getAttribute('data-chatgpt-search-unit-key')??'').split(':').pop();
          const turn=m.closest('article,[data-talvt-turn-state]')??m;
          const id=m.getAttribute('data-message-id')??m.querySelector('[data-chatgpt-selection-message-id]')?.getAttribute('data-chatgpt-selection-message-id')??(m.getAttribute('data-chatgpt-search-message-ids')??'').split(/\s+/)[0];
          const body=role==='assistant'?(m.querySelector('.markdown,[data-markdown-text-style="assistant-message"]')??m):(m.querySelector('[data-user-message-bubble] [dir="auto"]')??m);
          if(role==='user'&&body.textContent.trimStart().startsWith(marker)){{
            const more=Array.from(turn.querySelectorAll('button')).find(b=>/^(show more|read more)$/i.test(b.innerText.trim()));more?.click();
          }}
          return {{role,id,text:body.innerText,complete:!generating&&!!turn.querySelector('button[data-testid="copy-turn-action-button"],button[aria-label="Copy"],button[aria-label="Regenerate response"]'),
            codeBlocks:role==='assistant'?Array.from(body.querySelectorAll('pre code')).map(c=>c.textContent):[],
            links:role==='assistant'?Array.from(body.querySelectorAll('a[href]')).map(a=>({{label:a.innerText,url:a.href}})):[]}};
        }}).filter(m=>m.role==='user'||m.role==='assistant');
        return {{url:location.href,model:model?.innerText??'',generating,messages,
            notices:Array.from(document.querySelectorAll('[role="alert"],[role="alertdialog"]')).map(e=>e.innerText).filter(Boolean)}};
    }})()"#)).await?;
	let mut observation: Observation =
		serde_json::from_value(value).map_err(|e| format!("ChatGPT page changed: {e}"))?;
	for message in &mut observation.messages {
		if message.id.is_empty() {
			return Err("A rendered ChatGPT message has no identity yet.".into());
		}
		if message.role == "user" {
			message.text_sha256 = Some(crate::store::digest(message.text.trim().as_bytes()));
			message.text.clear();
		}
	}
	browser.collect_identity().await?;
	Ok(observation)
}

pub(crate) fn conversation_url(raw: &str) -> Result<String> {
	let url = Url::parse(raw).map_err(|_| "The ChatGPT conversation URL is invalid.")?;
	let parts: Vec<_> = url.path_segments().ok_or("Invalid conversation path.")?.collect();
	if url.scheme() != "https"
		|| url.host_str() != Some("chatgpt.com")
		|| url.port().is_some()
		|| !url.username().is_empty()
		|| url.password().is_some()
		|| url.query().is_some()
		|| url.fragment().is_some()
		|| url.as_str() != raw
		|| parts.len() != 2
		|| parts[0] != "c"
		|| parts[1].is_empty()
		|| !parts[1].bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
	{
		return Err("Only an exact https://chatgpt.com/c/ conversation URL can be resumed.".into());
	}
	Ok(url.to_string())
}

#[cfg(test)] mod tests;
