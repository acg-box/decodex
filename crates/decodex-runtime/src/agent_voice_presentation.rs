//! Attach recorded call timing and estimated spend without generating new content.
use crate::agent_host::AgentHost;
use decodex_protocol::{AgentHistoryResult, AgentVoicePresentation as Presentation};

impl AgentHost {
	pub(crate) async fn enrich_voice_history(&self, work: &str, history: &mut AgentHistoryResult) {
		let AgentHistoryResult::Available { entries, .. } = history else { return };
		let Ok(calls) = self.store.agent_voice_presentations(work.into()).await else { return };
		for call in calls {
			for entry in &mut *entries {
				if let Some(receipt) = entry
					.receipt
					.as_mut()
					.filter(|r| r.voice_session_id.as_deref() == Some(&call.session))
				{
					receipt.voice = Some(Presentation {
						spend: call.duration_ms.and_then(voice_spend),
						duration_ms: call.duration_ms,
					});
					break;
				}
			}
		}
	}
}

// Desktop Voice rate verified at https://learn.chatgpt.com/docs/pricing on 2026-11-14.
const VOICE_USD_MICROS_PER_MINUTE: u64 = 50_000;
fn voice_spend(duration_ms: u64) -> Option<decodex_protocol::AgentSpendDto> {
	let usd_micros = u64::try_from(
		(u128::from(duration_ms) * u128::from(VOICE_USD_MICROS_PER_MINUTE) + 30_000) / 60_000,
	)
	.ok()?;
	Some(decodex_protocol::AgentSpendDto {
		usd_micros,
		estimated: true,
		partial: false,
		explanation: format!(
			"Voice usage estimated from elapsed call time at ${:.2}/minute (published desktop Voice rate, checked November 14, 2026). Backend model work is separate.",
			VOICE_USD_MICROS_PER_MINUTE as f64 / 1_000_000.
		),
	})
}

#[cfg(test)]
mod tests {
	#[test]
	fn voice_spend_uses_elapsed_minutes_without_fabricating_token_counts() {
		assert_eq!(super::voice_spend(60_000).unwrap().usd_micros, 50_000);
		assert_eq!(super::voice_spend(9_000).unwrap().usd_micros, 7_500);
		assert_eq!(super::voice_spend(369_405).unwrap().usd_micros, 307_838);
		assert!(super::voice_spend(60_000).unwrap().estimated);
	}
}
