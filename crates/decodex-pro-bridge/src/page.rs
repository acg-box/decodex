//! Small official browser API calls. Rust owns workflow and durable state.

use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::{Answer, Config, Link, Result, native::NativeSession, store::Material};

mod input;
pub(crate) use input::fill;

// Read-only DOM queries can outlast the extension's 3 s CDP limit while ChatGPT
// initializes or expands a large message. This never retries a Send or input action.
const READ: &str = r#"
var proRead=async read=>{
 const deadline=Date.now()+30000;
 for(;;){try{return await read();}catch(error){
  const text=String(error);
  const timeout=/^(Error: )?Timed out after \d+ms waiting for /.test(text)&&
   /(waiting for CDP command (Runtime\.evaluate|Page\.getFrameTree)\.|CDP operation exceeded its deadline before command dispatch)/.test(text);
  const dispatch=/^(Error: )?CDP operation exceeded its deadline before command dispatch/.test(text);
  if(Date.now()>=deadline||!(timeout||dispatch))throw error;
  await new Promise(resolve=>setTimeout(resolve,500));
 }}
};
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

pub(crate) async fn connect(
	native: &mut NativeSession,
	config: &Config,
	instance: Option<&str>,
) -> Result<String> {
	let module = serde_json::to_string(&config.browser_client).map_err(|e| e.to_string())?;
	let text = native
		.eval(
			format!(
				"var {{setupBrowserRuntime}}=await import({module});var proAgent=await setupBrowserRuntime();\
		 nodeRepl.write(JSON.stringify(await proAgent.browsers.list()));"
			),
			"Discover the existing official Chrome extension",
		)
		.await?;
	let browsers: Vec<Value> =
		serde_json::from_str(&text).map_err(|_| "Browser discovery returned invalid data.")?;
	let browser = select_browser(&browsers, instance)?;
	let extension_instance = browser["metadata"]["extensionInstanceId"]
		.as_str()
		.filter(|id| !id.is_empty())
		.ok_or("The official Chrome extension did not expose its instance identity.")?
		.to_owned();
	let id = serde_json::to_string(&browser["id"]).map_err(|e| e.to_string())?;
	// Read the installed API contract before any page action. It is not stored or logged.
	native
		.eval(
			format!(
				"var proBrowser=await proAgent.browsers.get({id});var proApi=await proBrowser.documentation();\
		 {READ}nodeRepl.write(JSON.stringify({{apiChars:proApi.length}}));"
			),
			"Read the official browser API contract",
		)
		.await?;
	Ok(extension_instance)
}

#[cfg(test)]
pub(crate) async fn open(
	native: &mut NativeSession,
	config: &Config,
	url: Option<&str>,
) -> Result<()> {
	connect(native, config, config.extension_instance.as_deref()).await?;
	open_connected(native, url).await
}

pub(crate) async fn open_connected(native: &mut NativeSession, url: Option<&str>) -> Result<()> {
	let target = match url {
		Some(url) => conversation_url(url)?,
		None => "https://chatgpt.com/".into(),
	};
	let quoted = serde_json::to_string(&target).map_err(|e| e.to_string())?;
	// Set ownership before the RPC: an uncertain create is cleaned by native turn shutdown.
	native.tab_open = true;
	native
		.eval(
			format!(
				"await proBrowser.nameSession(\"🧪 Decodex Pro\");var proTab=await proBrowser.tabs.new();\
		 await proTab.goto({quoted});\
		 nodeRepl.write(JSON.stringify({{tabId:proTab.id}}));"
			),
			"Open only this component's request tab",
		)
		.await?;
	wait_ready(native).await?;
	if url.is_some() {
		native
			.eval(
				"await proRead(()=>proTab.playwright.getByRole(\"region\",{name:\"Conversation\",exact:true})\
			.waitFor({state:\"visible\",timeoutMs:20000}));await proRead(()=>proTab.playwright.getByRole(\"heading\",\
			{name:\"You said:\",exact:true}).first().waitFor({state:\"visible\",timeoutMs:20000}));\
			nodeRepl.write(\"ready\");"
					.into(),
				"Wait for the saved conversation",
			)
			.await?;
	}
	Ok(())
}

pub(crate) fn bound_instance<'a>(
	configured: Option<&'a str>,
	saved: Option<&'a str>,
) -> Result<Option<&'a str>> {
	if let (Some(configured), Some(saved)) = (configured, saved)
		&& configured != saved
	{
		return Err("This conversation belongs to a different Chrome extension instance. Use its saved profile; no page was opened or message sent.".into());
	}
	Ok(saved.or(configured))
}

async fn wait_ready(native: &mut NativeSession) -> Result<()> {
	native.eval(r#"
var proReadyDeadline=Date.now()+30000;
for(;;){
 var proReady=await proRead(()=>proTab.playwright.evaluate(()=>{
   const button=document.querySelector('button[aria-label="Select ChatGPT model"]');
   const editor=document.querySelector('[role="textbox"][aria-label="Ask ChatGPT"]');
   return !!button&&button.getClientRects().length>0&&!!editor&&editor.getClientRects().length>0;
  },undefined,{timeoutMs:20000}));
 if(proReady)break;
 if(Date.now()>=proReadyDeadline){
  const state=await proRead(()=>proTab.playwright.evaluate(()=>({title:document.title,
   headings:Array.from(document.querySelectorAll('h1,h2,[role="alert"],[role="status"]')).map(n=>n.innerText).filter(Boolean).slice(0,8),
   buttons:Array.from(document.querySelectorAll('button')).filter(b=>b.getClientRects().length>0).map(b=>({name:b.getAttribute('aria-label')??b.innerText,disabled:b.disabled})).slice(-12)})));
  throw new Error('The ChatGPT model control did not become ready. No message was sent. '+JSON.stringify(state));
 }
 await new Promise(resolve=>setTimeout(resolve,500));
}
nodeRepl.write('ready');
"#.into(),"Wait for the visible ChatGPT model control after application initialization").await?;
	Ok(())
}

fn select_browser<'a>(browsers: &'a [Value], instance: Option<&str>) -> Result<&'a Value> {
	let candidates: Vec<_> = browsers
		.iter()
		.filter(|browser| {
			browser["type"] == "extension"
				&& browser["family"] == "chrome"
				&& instance.is_none_or(|id| browser["metadata"]["extensionInstanceId"] == id)
		})
		.collect();
	if candidates.len() == 1 {
		return Ok(candidates[0]);
	}
	Err(if candidates.is_empty() && instance.is_some() {
		"The required Chrome extension instance is not connected. Reconnect its original profile; no other profile was selected."
	} else if candidates.is_empty() {
		"The official Chrome extension is not connected."
	} else {
		"Multiple Chrome instances are connected. Select an extension instance explicitly."
	}
	.into())
}

pub(crate) async fn select_pro(native: &mut NativeSession) -> Result<()> {
	if observe(native).await?.model == "Pro" {
		return Ok(());
	}
	native.eval("await proTab.playwright.getByRole(\"button\",{name:\"Select ChatGPT model\",exact:true}).click();\
		nodeRepl.write(\"opened\");".into(), "Open the ChatGPT model controls").await?;
	for _ in 0..6 {
		let label = native
			.eval(
				"nodeRepl.write(JSON.stringify(await proTab.playwright.getByRole(\"menuitem\",\
			{name:\"Select model\",exact:true}).innerText()));"
					.into(),
				"Read the visible thinking-power label",
			)
			.await?;
		if serde_json::from_str::<String>(&label).map_err(|_| "The model label was invalid.")?
			== "Pro"
		{
			native.eval("await proTab.playwright.getByRole(\"menuitem\",{name:\"Power\",exact:true}).press(\"Escape\");\
				nodeRepl.write(\"selected\");".into(), "Keep the automatically selected Pro setting").await?;
			return if observe(native).await?.model == "Pro" {
				Ok(())
			} else {
				Err("ChatGPT did not retain the Pro setting.".into())
			};
		}
		native.eval("await proTab.playwright.getByRole(\"menuitem\",{name:\"Power\",exact:true}).press(\"ArrowRight\");\
			nodeRepl.write(\"adjusted\");".into(), "Adjust the visible Power control with its keyboard shortcut").await?;
	}
	Err("The visible model controls did not expose Pro. No message was sent.".into())
}

pub(crate) async fn send(native: &mut NativeSession) -> Result<()> {
	native
		.eval(
			"await proTab.playwright.getByRole(\"button\",{name:\"Send\",exact:true}).click();\
		nodeRepl.write(\"send_attempted\");"
				.into(),
			"Send this request exactly once after its durable fence",
		)
		.await?;
	Ok(())
}

pub(crate) async fn current_url(native: &mut NativeSession) -> Result<Option<String>> {
	let text = native
		.eval(
			r#"var proRouteDeadline=Date.now()+10000;var proRoute=null;
for(;;){const url=await proTab.url();if(new URL(url).pathname.startsWith('/c/')){proRoute=url;break;}
 if(Date.now()>=proRouteDeadline)break;await new Promise(resolve=>setTimeout(resolve,500));}
nodeRepl.write(JSON.stringify(proRoute));"#.into(),
			"Wait for only this owned request tab's submitted URL for durable recovery",
		)
		.await?;
	serde_json::from_str(&text).map_err(|_| "The owned tab URL was invalid.".into())
}

// These selectors were read from the rendered ChatGPT UI. They do not access React,
// private endpoints, cookies, local storage, or other hidden application state.
const OBSERVE: &str = r#"
var proMessages=await proRead(()=>proTab.playwright.evaluate(marker=>Array.from(document.querySelectorAll('[data-chatgpt-search-message-ids]')).map(m=>{
 const parts=text=>{const result=[];for(let i=0;i<text.length;){
  let end=Math.min(i+16000,text.length);
  const last=text.charCodeAt(end-1);if(end<text.length&&last>=0xd800&&last<=0xdbff)end--;
  result.push(text.slice(i,end));i=end;
 }return result;};
 const role=(m.getAttribute('data-chatgpt-search-unit-key')??'').split(':').pop();
 const body=role==='user'?m.querySelector('[data-user-message-bubble] [dir="auto"]'):m.querySelector('[data-markdown-text-style="assistant-message"]');
 const ids=(m.getAttribute('data-chatgpt-search-message-ids')??'').split(/\s+/).filter(Boolean);
 const id=m.querySelector('[data-chatgpt-selection-message-id]')?.getAttribute('data-chatgpt-selection-message-id')??ids[0]??'';
 const turn=m.closest('[data-talvt-turn-state]');
 const readBody=role!=='user'||(marker&&body?.textContent?.trimStart().startsWith(marker));
 return {role,id,textParts:parts(readBody?body?.innerText??'':''),complete:turn?.getAttribute('data-talvt-turn-state')==='complete'&&!!turn?.querySelector('button[aria-label="Regenerate response"]'),
 codeParts:role==='user'?[]:Array.from(body?.querySelectorAll('code')??[]).map(c=>parts(c.textContent??'')),
 links:role==='user'?[]:Array.from(body?.querySelectorAll('a[href]')??[]).map(a=>({label:a.innerText,url:a.href}))};
}).filter(m=>m.role==='user'||m.role==='assistant'),proObservedMarker));
var proReadCrypto=await import('node:crypto');
for(var proMessage of proMessages){
 proMessage.text=proMessage.textParts.join('');delete proMessage.textParts;
 proMessage.codeBlocks=proMessage.codeParts.map(parts=>parts.join(''));delete proMessage.codeParts;
 if(proMessage.role==='user'){
 proMessage.textSha256=proMessage.text.length?proReadCrypto.createHash('sha256').update(proMessage.text.trim(),'utf8').digest('hex'):null;
 proMessage.text='';proMessage.codeBlocks=[];proMessage.links=[];
}}
nodeRepl.write(JSON.stringify({url:await proTab.url(),model:await proRead(()=>proTab.playwright.getByRole('button',{name:'Select ChatGPT model',exact:true}).innerText()),
 generating:(await proRead(()=>proTab.playwright.getByRole('button',{name:'Stop',exact:true}).count()))>0,messages:proMessages,
 notices:await proRead(()=>proTab.playwright.evaluate(()=>Array.from(document.querySelectorAll('[role="alert"],[role="alertdialog"]')).map(n=>n.innerText).filter(Boolean)))}));
"#;

pub(crate) async fn observe_request(native: &mut NativeSession, id: &str) -> Result<Observation> {
	let marker = serde_json::to_string(&format!("[Decodex Pro request: {id}]"))
		.map_err(|e| e.to_string())?;
	native.eval(format!("var proCollapsed=proTab.playwright.locator('[data-user-message-bubble]')\
		.filter({{hasText:{marker}}}).getByRole('button',{{name:'Show more',exact:true}});\
		var proCollapsedCount=await proRead(()=>proCollapsed.count());\
		for(var proExpansion=0;proExpansion<proCollapsedCount;proExpansion++){{await proCollapsed.first().click();}}\
		nodeRepl.write('expanded this request only');"),"Expand only this request's matching user message before exact text verification").await?;
	read_observation(native, &marker).await
}

pub(crate) async fn observe(native: &mut NativeSession) -> Result<Observation> {
	read_observation(native, "null").await
}

async fn read_observation(native: &mut NativeSession, marker: &str) -> Result<Observation> {
	let text = native
		.eval(
			format!("var proObservedMarker={marker};{OBSERVE}"),
			"Read only this request's rendered messages and completion state",
		)
		.await?;
	let observation: Observation =
		serde_json::from_str(&text).map_err(|_| "The ChatGPT page observation was invalid.")?;
	if observation.messages.iter().any(|m| m.id.is_empty()) {
		return Err(
			"The rendered ChatGPT message structure is unavailable or still loading.".into()
		);
	}
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

pub(crate) fn scope(id: &str, material: &Material, conversation: Option<&str>) -> Value {
	json!({"requestId":id,"material":material,"conversationUrl":conversation,
		"actions":["open one owned tab","select Pro","send this prompt once if not submitted","read the complete answer","close the owned tab"],
		"recovery":"Observe the saved conversation; never replay an uncertain Send."})
}

#[cfg(test)] mod tests;
