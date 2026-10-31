//! Opt-in acceptance against the installed official browser runtime. No message is sent.

use serde_json::{Value, json};

use crate::{Config, Result, native::NativeSession, page};

#[tokio::test]
#[ignore = "Requires DECODEX_PRO_LIVE_APP, DECODEX_PRO_LIVE_EXTENSION, and signed-in Chrome. Operates one owned test tab; no message is sent."]
async fn installed_official_runtime_selects_pro_in_background() {
	let mut config = Config::installed_app(
		std::env::var_os("DECODEX_PRO_LIVE_APP").expect("installed app path"),
	);
	config.extension_instance =
		Some(std::env::var("DECODEX_PRO_LIVE_EXTENSION").expect("exact Chrome extension instance"));
	let directory = tempfile::tempdir().expect("private test directory");
	let mut native =
		NativeSession::spawn(&config, directory.path()).expect("spawn official runtime");
	let outcome = selection_test(&mut native, &config, directory.path()).await;
	native.shutdown().await;
	outcome.expect("official Rust browser acceptance");
}

async fn selection_test(
	native: &mut NativeSession,
	config: &Config,
	cwd: &std::path::Path,
) -> Result<()> {
	native.start(cwd, json!({"test":"Rust component acceptance","actions":["create one owned test tab","select Medium then Pro","check inactive tab state","close the test tab"],"messagesToSend":0})).await?;
	page::open(native, config, None).await?;
	native.eval("var proManagement=await proBrowser.capabilities.get('management');\
		var proManagementDocs=await proManagement.documentation();nodeRepl.write(JSON.stringify({apiChars:proManagementDocs.length}));".into(),
		"Read official browser management support for this owned test tab").await?;
	let before = active(native).await?;
	native.eval("await proTab.playwright.getByRole('button',{name:'Select ChatGPT model',exact:true}).click();nodeRepl.write('opened');".into(),
		"Set a known non-Pro starting point in the owned test tab").await?;
	let mut medium = false;
	for _ in 0..6 {
		let label = native.eval("nodeRepl.write(JSON.stringify(await proTab.playwright.getByRole('menuitem',{name:'Select model',exact:true}).innerText()));".into(),
			"Read the test model-power starting point").await?;
		if serde_json::from_str::<String>(&label).map_err(|e| e.to_string())? == "Medium" {
			medium = true;
			break;
		}
		native.eval("await proTab.playwright.getByRole('menuitem',{name:'Power',exact:true}).press('ArrowLeft');nodeRepl.write('adjusted');".into(),
			"Adjust only the owned test page to Medium").await?;
	}
	if !medium {
		return Err("The test could not establish a Medium starting point.".into());
	}
	native.eval("await proTab.playwright.getByRole('menuitem',{name:'Power',exact:true}).press('Escape');nodeRepl.write('ready');".into(),
		"Close the model controls before exercising automatic Pro selection").await?;
	page::select_pro(native).await?;
	let after = active(native).await?;
	let observation = page::observe(native).await?;
	if before || after || observation.model != "Pro" || !observation.messages.is_empty() {
		return Err("The owned test tab did not remain inactive with Pro selected and no submitted message.".into());
	}
	Ok(())
}

async fn active(native: &mut NativeSession) -> Result<bool> {
	let text = native.eval("nodeRepl.write(JSON.stringify((await proManagement.tabs.get(Number(proTab.id))).active));".into(),
		"Read activity only for this component's own test tab").await?;
	let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
	value.as_bool().ok_or_else(|| "The test tab activity state was unavailable.".into())
}
