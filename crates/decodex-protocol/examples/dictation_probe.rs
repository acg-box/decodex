//! Opt-in subscription dictation qualification. PCM frames enter through stdin.
use std::{
	env,
	error::Error,
	io::{self, BufRead as _},
	path::Path,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util as _;
#[cfg(unix)] use libc as _;
use percent_encoding as _;
use serde as _;
use tempfile as _;
use tokio::time::{self, Instant};
use tokio_tungstenite as _;
use url as _;

use decodex_core as _;
use decodex_protocol::{
	AgentClient, ClientProfile, DictationBuffer, DictationPhase, DictationRequest, EntityId,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let root = env::args().nth(1).ok_or("explicit service root required")?;
	let client = AgentClient::new(ClientProfile::load(Path::new(&root), None)?);
	let id = EntityId::new(format!(
		"dictation-check-{}",
		SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
	))
	.map_err(|_| "invalid identity")?;
	let result=async {
        let status=client.dictation(DictationRequest::Start{session_id:id.clone()}).await?;
        let deadline=Instant::now()+Duration::from_secs(20);
        let mut status=status;

        while status.phase==DictationPhase::Connecting && Instant::now()<deadline {
            time::sleep(Duration::from_millis(100)).await;

            status=client.dictation(DictationRequest::Poll{session_id:id.clone()}).await?;
        }

        if status.phase!=DictationPhase::Listening {
            return Err::<(),Box<dyn Error>>(status.message.map_or_else(||"Dictation did not become ready".into(),|m|m.as_str().to_owned()).into());
        }

        let mut partials=0;

        for line in io::stdin().lock().lines() {
            let audio=DictationBuffer::new(line?).map_err(|_|"audio frame too large")?;

            status=client.dictation(DictationRequest::Audio{session_id:id.clone(),audio}).await?;

            if status.phase==DictationPhase::Failed {return Err("dictation audio failed".into())}
            if !status.text.as_str().is_empty(){partials+=1;}

            time::sleep(Duration::from_millis(100)).await;
        }

        status=client.dictation(DictationRequest::Finish{session_id:id.clone()}).await?;

        let deadline=Instant::now()+Duration::from_secs(20);

        while !matches!(status.phase,DictationPhase::Complete|DictationPhase::Failed) && Instant::now()<deadline {
            time::sleep(Duration::from_millis(100)).await;

            status=client.dictation(DictationRequest::Poll{session_id:id.clone()}).await?;
        }

        println!("{}",serde_json::json!({"phase":status.phase,"partialsBeforeFinish":partials,"text":status.text.as_str(),"message":status.message}));

        if status.phase!=DictationPhase::Complete {return Err("final correction did not complete".into())}

        Ok(())
    }.await;
	let _ = client.dictation(DictationRequest::Cancel { session_id: id }).await;

	result
}
