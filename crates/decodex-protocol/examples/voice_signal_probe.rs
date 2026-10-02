//! Opt-in native media qualification. Private SDP travels through inherited pipes only.
use std::{
	env,
	error::Error,
	io::{self, BufRead as _, Write as _},
	path::Path,
	thread,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util as _;
#[cfg(unix)] use libc as _;
use percent_encoding as _;
use serde as _;
use tempfile as _;
use tokio::{
	sync::mpsc,
	time::{self, Instant},
};
use tokio_tungstenite as _;
use url as _;

use decodex_core as _;
use decodex_protocol::{
	AgentClient, AgentVoicePhase, AgentVoiceRequest, ClientProfile, EntityId, VoiceSdp,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let root = env::args().nth(1).ok_or("explicit service root required")?;
	let work = env::args().nth(2).ok_or("explicit authorized test Agent required")?;
	let client = AgentClient::new(ClientProfile::load(Path::new(&root), None)?);
	let session = EntityId::new(format!(
		"voice-qualification-{}",
		SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
	))
	.map_err(|_| "invalid call identity")?;
	let (send, mut lines) = mpsc::channel(4);

	thread::spawn(move || {
		for line in io::stdin().lock().lines() {
			let Ok(line) = line else { break };

			if line.len() > 70_000 || send.blocking_send(line).is_err() {
				break;
			}
		}
	});

	let offer = lines.recv().await.ok_or("missing offer")?;
	let offer: VoiceSdp = serde_json::from_str(&offer)?;
	let start = AgentVoiceRequest::Start {
		session_id: session.clone(),
		work_id: EntityId::new(work).map_err(|_| "invalid Agent identity")?,
		offer,
		options: Default::default(),
	};
	let result = async {
		let status = client.voice(start).await?;
		let deadline = Instant::now() + Duration::from_secs(60);
		let mut status = status;
		let mut answered = false;

		loop {
			if status.phase == AgentVoicePhase::Failed {
				if let Some(message) = &status.message {
					eprintln!("{}", message.as_str());
				}

				return Err::<(), Box<dyn Error>>("service voice failed".into());
			}
			if !answered && let Some(answer) = status.answer.take() {
				println!("{}", serde_json::to_string(&answer)?);

				io::stdout().flush()?;

				answered = true;
			}
			if status.phase == AgentVoicePhase::Ended {
				return Ok(());
			}

			tokio::select! {
				_=time::sleep_until(deadline)=>return Err("voice qualification deadline".into()),
				line=lines.recv()=>{if line.as_deref()==Some("stop") || line.is_none() {return Ok(())}},
				_=time::sleep(Duration::from_millis(200))=>{},
			}

			status = client.voice(AgentVoiceRequest::Poll { session_id: session.clone() }).await?;
		}
	}
	.await;
	let _ = client.voice(AgentVoiceRequest::Stop { session_id: session }).await;

	if result.is_err() {
		println!("null");
	}

	result
}
