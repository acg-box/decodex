//! Transfer immutable text through bounded official UI input calls.

use serde::Deserialize;

use crate::{Result, native::NativeSession, store::Material};

const LOAD: &str = r#"
var proFile=await(await import('node:fs/promises')).readFile(proMaterial.path);
var proInputCrypto=await import('node:crypto');
if(proFile.length!==proMaterial.bytes||proInputCrypto.createHash('sha256').update(proFile).digest('hex')!==proMaterial.sha256){
 throw new Error('The immutable outbound message file changed. No Send was attempted.');
}
// ChatGPT serializes a terminal editor hard break as a visible backslash.
// Keep the immutable original intact; omit only terminal LF separators in the editor.
var proInputText=proFile.toString('utf8').replace(/\n+$/,'');proFile=undefined;
var proInputOffset=0;
var proComposer=proTab.playwright.locator('[role="textbox"][aria-label="Ask ChatGPT"]');
var proDraft=await proRead(()=>proTab.playwright.evaluate(()=>{
 const editor=document.querySelector('[role="textbox"][aria-label="Ask ChatGPT"]');
 const hints=editor?.querySelectorAll('[app-mention-name="deep-research"]');
 const hint=hints?.[0];
 const read=node=>node===hint?'':node.nodeType===3?(node.nodeValue??''):Array.from(node.childNodes).map(read).join('');
 // The home page can seed a Deep research mode chip in an otherwise empty editor.
 // Any other text or mention remains an unowned draft and must not be replaced.
 const onlyResearchHint=!!editor&&hints.length===1&&editor.querySelectorAll('[app-mention-name]').length===1&&read(editor).trim()==='';
 return {prefix:editor?.textContent?.trim().slice(0,200)??'',onlyResearchHint};
}));
if(proDraft.prefix&&!proDraft.onlyResearchHint&&!proDraft.prefix.startsWith(proInputText.slice(0,proInputText.indexOf('\n')))){
 throw new Error('The composer contains a draft from another request. No text was changed or sent.');
}
nodeRepl.write(JSON.stringify({restored:!!proDraft.prefix&&!proDraft.onlyResearchHint}));
"#;

// Keep the initial fill below ChatGPT's pasted-file threshold. Use ordinary
// typing for larger append operations, with bounded work and intact surrogate pairs.
const STEP: &str = r#"
var proStepStarted=Date.now();
for(var proStep=0;proStep<16&&proInputOffset<proInputText.length;proStep++){
 var proInputEnd=Math.min(proInputOffset+(proInputOffset===0?3000:6000),proInputText.length);
 var proLastUnit=proInputText.charCodeAt(proInputEnd-1);
 if(proInputEnd<proInputText.length&&proLastUnit>=0xd800&&proLastUnit<=0xdbff)proInputEnd--;
 var proInputPart=proInputText.slice(proInputOffset,proInputEnd);
 if(proInputOffset===0)await proComposer.fill(proInputPart,{timeoutMs:20000});else await proComposer.type(proInputPart,{timeoutMs:20000});
 proInputOffset=proInputEnd;
 if(Date.now()-proStepStarted>8000)break;
}
nodeRepl.write(JSON.stringify({offset:proInputOffset,total:proInputText.length}));
"#;

// Rich URL spans add layout-only newlines to innerText. Read text nodes plus the
// composer's explicit paragraph and line separators, then compare the full hash.
const VERIFY: &str = r#"
var proPreparedSnapshot=await proRead(()=>proTab.playwright.evaluate(()=>{
 const editor=document.querySelector('[role=textbox][aria-label="Ask ChatGPT"]');
 const read=node=>{
  if(node.nodeType===3)return node.nodeValue??'';
  if(node.nodeType!==1||node.getAttribute('aria-hidden')==='true')return '';
  if(node.tagName==='BR')return '\n';
  return Array.from(node.childNodes).map(read).join('')+(node.tagName==='P'&&node.nextSibling?'\n':'');
 };
 const text=editor?read(editor):'';
 const parts=[];for(let i=0;i<text.length;){
  let end=Math.min(i+16000,text.length);
  const last=text.charCodeAt(end-1);if(end<text.length&&last>=0xd800&&last<=0xdbff)end--;
  parts.push(text.slice(i,end));i=end;
 }
 return {parts,units:text.length};
}));
var proPreparedText=proPreparedSnapshot.parts.join('');
var proPrefixMatches=proPreparedText.length>0&&proPreparedText===proInputText.slice(0,proPreparedText.length);
if(proPrefixMatches)proInputOffset=proPreparedText.length;
var proFirstDifference=0;
while(proFirstDifference<proPreparedText.length&&proPreparedText[proFirstDifference]===proInputText[proFirstDifference])proFirstDifference++;
nodeRepl.write(JSON.stringify({matches:proInputCrypto.createHash('sha256').update(proPreparedText.trim(),'utf8').digest('hex')===proMaterial.normalizedSha256,
 prefixMatches:proPrefixMatches,
 expectedUnits:proInputText.length,observedUnits:proPreparedText.length,pageUnits:proPreparedSnapshot.units,firstDifference:proFirstDifference,
 ready:await proRead(()=>proTab.playwright.getByRole('button',{name:'Send',exact:true}).isEnabled())}));
"#;

#[derive(Deserialize)]
struct Progress {
	offset: usize,
	total: usize,
}

#[derive(Deserialize)]
struct Loaded {
	restored: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Prepared {
	matches: bool,
	prefix_matches: bool,
	ready: bool,
	expected_units: usize,
	observed_units: usize,
	page_units: usize,
	first_difference: usize,
}

pub(crate) async fn fill(native: &mut NativeSession, material: &Material) -> Result<()> {
	let descriptor = serde_json::to_string(material).map_err(|e| e.to_string())?;
	let loaded = native
		.eval(
			format!("var proMaterial={descriptor};{LOAD}"),
			"Load only the authorized immutable message file and verify its hash without printing its text",
		)
		.await?;
	let loaded: Loaded =
		serde_json::from_str(&loaded).map_err(|_| "The material check was invalid.")?;
	let mut offset = 0;
	if loaded.restored {
		let prepared = verify(native).await?;
		if prepared.matches {
			return require_ready(&prepared);
		}
		if prepared.prefix_matches {
			offset = prepared.observed_units;
			#[cfg(test)]
			eprintln!("Resume exact ChatGPT draft prefix at {offset} UTF-16 units");
		}
	}
	// The official append action requires focus. Select all and collapse right to
	// place the caret at the end without platform-specific editing commands.
	native.eval("await proComposer.click({timeoutMs:20000});await proComposer.press('ControlOrMeta+A',{timeoutMs:20000});await proComposer.press('ArrowRight',{timeoutMs:20000});nodeRepl.write('Owned request editor ready');".into(),"Focus only this request's editor and move the caret to the end before bounded text input").await?;
	loop {
		let progress = native
			.eval(
				STEP.into(),
				"Insert the next bounded portion of this exact authorized ChatGPT question",
			)
			.await?;
		let progress: Progress =
			serde_json::from_str(&progress).map_err(|_| "The text input progress was invalid.")?;
		if progress.offset <= offset || progress.offset > progress.total {
			return Err("The ChatGPT composer did not advance. No message was sent.".into());
		}
		offset = progress.offset;
		#[cfg(test)]
		eprintln!("ChatGPT text input: {offset}/{} UTF-16 units", progress.total);
		if offset == progress.total {
			break;
		}
	}
	let prepared = verify(native).await?;
	if !prepared.matches {
		return Err(format!(
			"The ChatGPT composer text did not match: expected {} UTF-16 units, observed {} (page {}), first difference {}. No message was sent.",
			prepared.expected_units,
			prepared.observed_units,
			prepared.page_units,
			prepared.first_difference
		));
	}
	require_ready(&prepared)
}

async fn verify(native: &mut NativeSession) -> Result<Prepared> {
	let prepared = native
		.eval(
			VERIFY.into(),
			"Verify the entire composer text against the authorized file before Send",
		)
		.await?;
	serde_json::from_str(&prepared).map_err(|_| "The composer check was invalid.".into())
}

fn require_ready(prepared: &Prepared) -> Result<()> {
	if !prepared.ready {
		return Err("The ChatGPT Send control is unavailable.".into());
	}
	Ok(())
}
