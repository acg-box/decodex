//! Small ChatGPT UI adapter. Page selectors are separate from durable workflow state.

use serde::Deserialize;
use serde_json::{Value, json};
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
	#[serde(default)]
	pub markdown_text_sha256: Option<String>,
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
            const ready=visible(editor)&&visible(model)&&!model.disabled&&model.getAttribute('aria-disabled')!=='true';
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
		if state["verification"] == true && Instant::now() >= deadline {
			return Ok(Connection {
				state: ConnectionState::VerificationRequired,
				selected_model: None,
			});
		}
		if state["login"] == true && state["verification"] != true {
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
			Err("Sign in to ChatGPT in the research tab, then resume this request."
				.into()),
		ConnectionState::VerificationRequired => Err("ChatGPT requires a security check in the research browser. Research is waiting; no message was sent.".into()),
		ConnectionState::AttentionRequired =>
			Err("ChatGPT needs attention in the research browser. No message was sent.".into()),
	}
}

pub(crate) async fn select_pro(browser: &mut Chrome) -> Result<()> {
	if pro_selected(browser).await? {
		key(browser, "Escape", "Escape", 27).await?;
		return Ok(());
	}
	let point = browser.evaluate(&format!(r#"(()=>{{ {CONTROLS} if(!visible(model)||model.disabled||model.getAttribute('aria-disabled')==='true')throw Error('Model selector unavailable');if(model.getAttribute('aria-expanded')==='true')return null;model.scrollIntoView({{block:'nearest',inline:'nearest'}});const r=model.getBoundingClientRect();return {{x:r.x+r.width/2,y:r.y+r.height/2}}; }})()"#)).await?;
	if !point.is_null() {
		click(browser, &point).await?;
	}
	// Read the visible menu. Never force an inaccessible option or edit React state.
	let deadline = Instant::now() + std::time::Duration::from_secs(3);
	let result = loop {
		let result = browser.evaluate(r#"(()=>{
        const visible=e=>e.getClientRects().length>0;
        const choices=Array.from(document.querySelectorAll('[role="menuitem"],[role="radio"],[role="option"],button'))
          .filter(e=>visible(e)&&!e.disabled&&e.getAttribute('aria-disabled')!=='true'&&/^Pro(?:\s|$)/.test(e.innerText.trim()));
        if(choices.length===1){choices[0].scrollIntoView({block:'nearest',inline:'nearest'});const r=choices[0].getBoundingClientRect();return {point:{x:r.x+r.width/2,y:r.y+r.height/2}};}
        const power=document.querySelector('[role="menuitem"][data-reasoning-slider][aria-label="Power"]');
        return {power:!!power&&visible(power),options:Array.from(document.querySelectorAll('[role="menuitem"],[role="radio"],[role="option"]')).filter(visible).map(e=>(e.innerText||e.getAttribute('aria-label')||'').slice(0,160)).slice(0,16)};
    })()"#).await?;
		if result.get("point").is_some() || result["power"] == true || Instant::now() >= deadline {
			break result;
		}
		sleep(std::time::Duration::from_millis(100)).await;
	};
	if let Some(point) = result.get("point") {
		click(browser, point).await?;
	} else if result["power"] == true {
		select_pro_power(browser).await?;
	} else {
		return Err(format!(
			"The visible ChatGPT model menu does not expose one unambiguous Pro option. No message was sent. Visible options: {}",
			result["options"]
		));
	}
	key(browser, "Escape", "Escape", 27).await?;
	// The slider can commit before React updates the picker label. Verify the
	// closed control, not an intermediate menu render; never Send on the range alone.
	let deadline = Instant::now() + std::time::Duration::from_secs(2);
	while !pro_selected(browser).await? {
		if Instant::now() >= deadline {
			return Err("ChatGPT did not retain the Pro selection. No message was sent.".into());
		}
		sleep(std::time::Duration::from_millis(100)).await;
	}
	Ok(())
}

async fn pro_selected(browser: &mut Chrome) -> Result<bool> {
	let state = browser.evaluate(&format!(r#"(()=>{{ {CONTROLS}
        if(!model)return false;
        const label=model.cloneNode(true);label.querySelectorAll('[aria-hidden="true"]').forEach(e=>e.remove());
        return /(^|\s)Pro($|\s)/.test(label.textContent??'');
    }})()"#)).await?;
	Ok(state == true)
}

async fn select_pro_power(browser: &mut Chrome) -> Result<()> {
	let mut previous = None;
	loop {
		if pro_selected(browser).await? {
			return Ok(());
		}
		let state = browser.evaluate(r#"(()=>{
          const power=document.querySelector('[role="menuitem"][data-reasoning-slider][aria-label="Power"]');
          const slider=power?.querySelector('[role="slider"]');
          if(!power||!slider||power.getAttribute('aria-disabled')==='true'||power.querySelector('[data-locked="true"]'))return null;
          power.focus({preventScroll:true});
          return {value:Number(slider.getAttribute('aria-valuenow')),max:Number(slider.getAttribute('aria-valuemax'))};
        })()"#).await?;
		let Some(value) = state["value"].as_u64() else {
			return Err("ChatGPT's Power control is unavailable. No message was sent.".into());
		};
		let Some(max) = state["max"].as_u64().filter(|max| *max <= 16) else {
			return Err("ChatGPT's Power range could not be read. No message was sent.".into());
		};
		if value == max {
			return Ok(());
		}
		if value > max || previous == Some(value) {
			return Err(
				"Pro is not available through ChatGPT's Power control. No message was sent.".into(),
			);
		}
		previous = Some(value);
		key(browser, "ArrowRight", "ArrowRight", 39).await?;
		sleep(std::time::Duration::from_millis(150)).await;
	}
}

async fn key(browser: &mut Chrome, key: &str, code: &str, key_code: u32) -> Result<()> {
	for kind in ["keyDown", "keyUp"] {
		browser
			.command(
				"Input.dispatchKeyEvent",
				json!({"type":kind,"key":key,"code":code,"windowsVirtualKeyCode":key_code}),
			)
			.await?;
	}
	Ok(())
}

async fn click(browser: &mut Chrome, point: &Value) -> Result<()> {
	let x = point["x"]
		.as_f64()
		.filter(|value| value.is_finite())
		.ok_or("The control is not visible.")?;
	let y = point["y"]
		.as_f64()
		.filter(|value| value.is_finite())
		.ok_or("The control is not visible.")?;
	for (kind, buttons) in [("mouseMoved", 0), ("mousePressed", 1), ("mouseReleased", 0)] {
		browser.command("Input.dispatchMouseEvent", json!({"type":kind,"x":x,"y":y,"button":if kind=="mouseMoved" {"none"} else {"left"},"buttons":buttons,"clickCount":1})).await?;
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
	if let Some(draft) =
		draft.as_str().filter(|draft| !draft.trim().is_empty() && !text.starts_with(*draft))
	{
		return Err(format!(
			"The owned composer has a different draft ({} non-whitespace characters). No message was sent.",
			draft.trim().chars().count()
		));
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
	// Use the rendered button's normal click/submit handler. Background pointer
	// coordinates can become stale when composer focus changes its layout.
	browser.evaluate(&format!(r#"(()=>{{ {CONTROLS}
        if(!visible(send)||send.disabled||send.getAttribute('aria-disabled')==='true')throw Error('Send unavailable');
        send.click();return true;
    }})()"#)).await?;
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
        return {{url:location.href,generating,messages,
            notices:Array.from(document.querySelectorAll('[role="alert"],[role="alertdialog"]')).map(e=>e.innerText).filter(Boolean)}};
    }})()"#)).await?;
	let mut observation: Observation =
		serde_json::from_value(value).map_err(|e| format!("ChatGPT page changed: {e}"))?;
	browser.bind_conversation_url(&observation.url);
	browser.collect_identity().await?;
	for message in &mut observation.messages {
		if message.id.is_empty() {
			return Err("A rendered ChatGPT message has no identity yet.".into());
		}
		if let Some(native) = browser.messages.get(&message.id) {
			apply_native_message(message, native);
		}
		if message.role == "user" {
			message.text_sha256 = Some(crate::store::digest(message.text.trim().as_bytes()));
			message.text.clear();
		}
	}
	Ok(observation)
}

fn apply_native_message(message: &mut Message, native: &crate::identity::NativeMessage) {
	if message.id != native.id || message.role != native.role || !native.complete {
		return;
	}
	message.text.clone_from(&native.text);
	if message.role == "user" && native.markdown {
		message.markdown_text_sha256 = markdown_user_text(&native.text)
			.map(|text| crate::store::digest(text.trim().as_bytes()));
	}
	if message.role == "assistant" {
		message.code_blocks = markdown_code_blocks(&native.text);
	}
}

/// Decode the provider's declared user-text representation. Only paragraphs and
/// inline presentation are accepted. Links must show their complete destination;
/// hidden destinations, titles, images and HTML cannot disappear from verification.
fn markdown_user_text(markdown: &str) -> Option<String> {
	use pulldown_cmark::{Event, Parser, Tag, TagEnd};
	let mut text = String::new();
	let mut paragraph = false;
	let mut link: Option<(String, usize)> = None;
	for event in Parser::new(markdown) {
		match event {
			Event::Start(Tag::Paragraph) => {
				if paragraph {
					text.push_str("\n\n");
				}
				paragraph = true;
			},
			Event::Start(Tag::Link { dest_url, title, .. }) => {
				if link.is_some() || !title.is_empty() {
					return None;
				}
				link = Some((dest_url.into_string(), text.len()));
			},
			Event::End(TagEnd::Link) => {
				let (destination, start) = link.take()?;
				if text[start..] != destination {
					return None;
				}
			},
			Event::Text(value) | Event::Code(value) => text.push_str(&value),
			Event::SoftBreak | Event::HardBreak => text.push('\n'),
			Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough)
			| Event::End(
				TagEnd::Paragraph | TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough,
			) => {},
			_ => return None,
		}
	}
	link.is_none().then_some(text)
}

fn markdown_code_blocks(markdown: &str) -> Vec<String> {
	let mut blocks = Vec::new();
	let mut fence: Option<(u8, usize)> = None;
	let mut content = String::new();
	for line in markdown.split_inclusive('\n') {
		let trimmed = line.trim_end_matches(['\r', '\n']);
		let indent = trimmed.bytes().take_while(|byte| *byte == b' ').count();
		let trimmed = &trimmed[indent..];
		let marker = trimmed.as_bytes().first().copied();
		let length =
			marker.map_or(0, |marker| trimmed.bytes().take_while(|byte| *byte == marker).count());
		let delimiter = indent <= 3 && matches!(marker, Some(b'`' | b'~')) && length >= 3;
		if let Some((kind, open_length)) = fence {
			if delimiter
				&& marker == Some(kind)
				&& length >= open_length
				&& trimmed[length..].trim().is_empty()
			{
				blocks.push(std::mem::take(&mut content));
				fence = None;
			} else {
				content.push_str(line);
			}
		} else if delimiter && (marker != Some(b'`') || !trimmed[length..].contains('`')) {
			fence = Some((marker.unwrap(), length));
		}
	}
	if fence.is_some() {
		blocks.push(content);
	}
	blocks
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
