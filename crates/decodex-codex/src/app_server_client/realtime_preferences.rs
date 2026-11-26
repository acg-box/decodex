//! Read and save the connected ChatGPT account's voice.
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{sync::mpsc::Sender, time};

use crate::app_server_client::{AppServerClient, ClientError, Outbound};

/// Voice ID and display name supplied by the account.
#[derive(Clone, Deserialize, Serialize)]
pub struct NativeVoiceOption {
	/// ID used when saving a preference.
	pub voice: String,
	/// Human-readable name.
	pub name: String,
}

/// The account selection observed on one native connection.
#[derive(Clone)]
pub struct NativeVoiceSettings {
	connection: Sender<Outbound>,
	/// Currently selected voice ID.
	pub selected: String,
	/// Choices available to this account.
	pub voices: Vec<NativeVoiceOption>,
}

impl AppServerClient {
	/// Read the selected voice and available choices together.
	pub async fn realtime_voice_settings(&self) -> Result<NativeVoiceSettings, ClientError> {
		#[derive(Deserialize)]
		struct Response {
			selected: String,
			voices: Vec<NativeVoiceOption>,
		}
		let value = time::timeout(
			Duration::from_secs(15),
			self.request("account/voiceSettings/read", Value::Null),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let response: Response =
			serde_json::from_value(value).map_err(|_| ClientError::InvalidFrame)?;
		Ok(NativeVoiceSettings {
			connection: self.outbound.clone(),
			selected: response.selected,
			voices: response.voices,
		})
	}

	/// Save an ID from this connection's catalog once. The caller can refresh the selection.
	pub async fn write_realtime_voice(
		&self,
		observed: &NativeVoiceSettings,
		voice: &str,
	) -> Result<(), ClientError> {
		if !self.outbound.same_channel(&observed.connection)
			|| !observed.voices.iter().any(|option| option.voice == voice)
		{
			return Err(ClientError::InvalidFrame);
		}
		time::timeout(
			Duration::from_secs(15),
			self.request("account/voiceSettings/write", serde_json::json!({"voice":voice})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		Ok(())
	}
}

#[cfg(test)]
#[path = "realtime_preferences_tests.rs"]
mod tests;
