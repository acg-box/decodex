//! Opt-in native media qualification. Private SDP travels through inherited pipes only.
use decodex_core as _;
use decodex_protocol::{
	ChiefClient, ChiefVoicePhase, ChiefVoiceRequest, ClientProfile, EntityId, VoiceSdp,
};
use futures_util as _;
#[cfg(unix)] use libc as _;
use percent_encoding as _;
use serde as _;
use std::{
	io::{BufRead as _, Write as _},
	path::Path,
	time::{Duration, SystemTime, UNIX_EPOCH},
};
use tempfile as _;
use tokio_tungstenite as _;
use url as _;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let root = std::env::args().nth(1).ok_or("explicit service root required")?;
	let work = std::env::args().nth(2).ok_or("explicit authorized test Chief required")?;
	let client = ChiefClient::new(ClientProfile::load(Path::new(&root), None)?);
	let session = EntityId::new(format!(
		"voice-qualification-{}",
		SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
	))
	.map_err(|_| "invalid call identity")?;
	let (send, mut lines) = tokio::sync::mpsc::channel(4);
	std::thread::spawn(move || {
		for line in std::io::stdin().lock().lines() {
			let Ok(line) = line else { break };
			if line.len() > 70_000 || send.blocking_send(line).is_err() {
				break;
			}
		}
	});
	let offer = lines.recv().await.ok_or("missing offer")?;
	let offer: VoiceSdp = serde_json::from_str(&offer)?;
	let start = ChiefVoiceRequest::Start {
		session_id: session.clone(),
		work_id: EntityId::new(work).map_err(|_| "invalid Chief identity")?,
		offer,
	};
	let result = async {
		let mut status = client.voice(start).await?;
		let mut answered = false;
		let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
		loop {
			if status.phase == ChiefVoicePhase::Failed {
				if let Some(message) = &status.message {
					eprintln!("{}", message.as_str());
				}
				return Err::<(), Box<dyn std::error::Error>>("service voice failed".into());
			}
			if !answered && let Some(answer) = status.answer.take() {
				println!("{}", serde_json::to_string(&answer)?);
				std::io::stdout().flush()?;
				answered = true;
			}
			if status.phase == ChiefVoicePhase::Ended {
				return Ok(());
			}
			tokio::select! {
				_=tokio::time::sleep_until(deadline)=>return Err("voice qualification deadline".into()),
				line=lines.recv()=>{if line.as_deref()==Some("stop") || line.is_none() {return Ok(())}},
				_=tokio::time::sleep(Duration::from_millis(200))=>{},
			}
			status = client.voice(ChiefVoiceRequest::Poll { session_id: session.clone() }).await?;
		}
	}
	.await;
	let _ = client.voice(ChiefVoiceRequest::Stop { session_id: session }).await;
	if result.is_err() {
		println!("null");
	}
	result
}
