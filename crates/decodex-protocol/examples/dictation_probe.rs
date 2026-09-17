//! Opt-in subscription dictation qualification. PCM frames enter through stdin.
use decodex_core as _;
use decodex_protocol::{
	ChiefClient, ClientProfile, DictationBuffer, DictationPhase, DictationRequest, EntityId,
};
use futures_util as _;
#[cfg(unix)] use libc as _;
use percent_encoding as _;
use serde as _;
use std::{
	io::BufRead as _,
	path::Path,
	time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile as _;
use tokio_tungstenite as _;
use url as _;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let root = std::env::args().nth(1).ok_or("explicit service root required")?;
	let client = ChiefClient::new(ClientProfile::load(Path::new(&root), None)?);
	let id = EntityId::new(format!(
		"dictation-check-{}",
		SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
	))
	.map_err(|_| "invalid identity")?;
	let result=async {
        let mut status=client.dictation(DictationRequest::Start{session_id:id.clone()}).await?;
        let deadline=tokio::time::Instant::now()+Duration::from_secs(20);
        while status.phase==DictationPhase::Connecting && tokio::time::Instant::now()<deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
            status=client.dictation(DictationRequest::Poll{session_id:id.clone()}).await?;
        }
        if status.phase!=DictationPhase::Listening {
            return Err::<(),Box<dyn std::error::Error>>(status.message.map_or_else(||"Dictation did not become ready".into(),|m|m.as_str().to_owned()).into());
        }
        let mut partials=0;
        for line in std::io::stdin().lock().lines() {
            let audio=DictationBuffer::new(line?).map_err(|_|"audio frame too large")?;
            status=client.dictation(DictationRequest::Audio{session_id:id.clone(),audio}).await?;
            if status.phase==DictationPhase::Failed {return Err("dictation audio failed".into())}
            if !status.text.as_str().is_empty(){partials+=1;}
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        status=client.dictation(DictationRequest::Finish{session_id:id.clone()}).await?;
        let deadline=tokio::time::Instant::now()+Duration::from_secs(20);
        while !matches!(status.phase,DictationPhase::Complete|DictationPhase::Failed) && tokio::time::Instant::now()<deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
            status=client.dictation(DictationRequest::Poll{session_id:id.clone()}).await?;
        }
        println!("{}",serde_json::json!({"phase":status.phase,"partialsBeforeFinish":partials,"text":status.text.as_str(),"message":status.message}));
        if status.phase!=DictationPhase::Complete {return Err("final correction did not complete".into())}
        Ok(())
    }.await;
	let _ = client.dictation(DictationRequest::Cancel { session_id: id }).await;
	result
}
