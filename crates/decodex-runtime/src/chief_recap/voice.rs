//! Keep voice provenance without inventing a total voice/native ordering.
use super::{excerpts, history, prompt};
use decodex_database::ChiefVoiceHistory;

const PROVENANCE: &str = "Source notes: Native task turns and spoken dialogue are separate sources. Voice sequence is recording order within a call. Partial or unknown-completeness captions may be flushed by role on close and do not establish spoken sentence order. A call baseline identifies the native turn before the call began; it does not date each sentence relative to later task turns. Section order does not establish which correction is newer. If conflicting instructions cannot be ordered from these facts, preserve that uncertainty. Native task output may also be spoken; repeated wording is not additional completed work. Internal voice delegation instructions are omitted.\n\n";

pub(super) fn compose(native: &history::History, voice: &ChiefVoiceHistory) -> Option<String> {
	let mut messages = Vec::new();
	for call in &voice.calls {
		for entry in &call.entries {
			messages.push(history::Message {
				user: entry.role == "user",
				text: format!(
					"[Voice call {}, sequence {}, baseline native turn {}, caption {}]\n{}",
					serde_json::json!(call.session_id),
					entry.sequence,
					serde_json::json!(call.baseline_turn_id),
					match entry.complete {
						Some(true) => "complete",
						Some(false) => "partial",
						None => "completeness unknown",
					},
					entry.text
				),
			});
		}
	}
	messages.reverse();
	let spoken = history::select(&messages);
	if native.exchanges.is_empty() && spoken.is_empty() {
		return None;
	}
	if spoken.is_empty() && voice.revision.calls == 0 {
		return Some(prompt::build(&excerpts::render(&native.exchanges)));
	}
	let omitted =
		if voice.truncated { "Older voice calls or sentences were omitted.\n" } else { "" };
	let missing = if voice.calls.iter().any(|call| call.entries.is_empty()) {
		"Some selected voice calls have no stored transcript. Do not infer that no new instruction was spoken.\n"
	} else {
		""
	};
	let headers = format!("{PROVENANCE}{omitted}{missing}Native task messages:\n");
	let separator = "\n\nSpoken dialogue:\n";
	let budget = prompt::HISTORY_MAX_BYTES - headers.len() - separator.len();
	let voice_budget = if native.exchanges.is_empty() {
		budget
	} else {
		(budget / 2).min(excerpts::render(&spoken).len())
	};
	let native = excerpts::render_budget(&native.exchanges, budget - voice_budget);
	let spoken = excerpts::render_budget(&spoken, voice_budget);
	Some(prompt::build(&format!("{headers}{native}{separator}{spoken}")))
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_database::{
		ChiefVoiceHistoryRevision, ChiefVoiceTranscript, ChiefVoiceTranscriptCall,
	};
	fn snapshot(text: String) -> ChiefVoiceHistory {
		ChiefVoiceHistory {
			revision: ChiefVoiceHistoryRevision { calls: 1, ..Default::default() },
			truncated: false,
			calls: vec![ChiefVoiceTranscriptCall {
				session_id: "voice-session".into(),
				baseline_turn_id: Some("native-before-call".into()),
				entries: vec![ChiefVoiceTranscript {
					sequence: 1,
					role: "user".into(),
					text,
					complete: Some(true),
				}],
			}],
		}
	}
	#[test]
	fn voice_and_native_sources_keep_provenance_corrections_and_utf8_budget() {
		let native = history::History {
			latest_turn: Some("latest-native".into()),
			exchanges: vec![excerpts::Exchange {
				user: "Fix the issue".into(),
				assistant: format!("TESTED{}NOT INSTALLED", "字".repeat(20000)),
			}],
		};
		let voice = snapshot(format!("SPOKEN START{}DO NOT PUBLISH", "声".repeat(20000)));
		let rendered = compose(&native, &voice).expect("combined recap input");
		assert!(rendered.len() <= prompt::MAX_BYTES);
		for expected in [
			"TESTED",
			"NOT INSTALLED",
			"SPOKEN START",
			"DO NOT PUBLISH",
			"voice-session",
			"native-before-call",
			"does not establish which correction is newer",
		] {
			assert!(rendered.contains(expected), "missing {expected}");
		}
	}
	#[test]
	fn voice_only_history_can_supply_the_recap_without_fabricating_native_messages() {
		let native = history::History { latest_turn: None, exchanges: vec![] };
		let rendered = compose(&native, &snapshot("Summarize what we agreed in voice.".into()))
			.expect("voice context");
		assert!(rendered.contains("Pending user request:"));
		assert!(rendered.contains("Summarize what we agreed in voice."));
	}
}

#[cfg(test)]
mod missing_tests {
	use super::*;
	#[test]
	fn missing_voice_transcript_is_disclosed_instead_of_inferred_empty() {
		let native = history::History {
			latest_turn: Some("native".into()),
			exchanges: vec![excerpts::Exchange {
				user: "Validate the change".into(),
				assistant: "Tests passed".into(),
			}],
		};
		let voice = ChiefVoiceHistory {
			revision: decodex_database::ChiefVoiceHistoryRevision {
				calls: 1,
				..Default::default()
			},
			truncated: false,
			calls: vec![decodex_database::ChiefVoiceTranscriptCall {
				session_id: "missing".into(),
				baseline_turn_id: Some("native".into()),
				entries: vec![],
			}],
		};
		let rendered = compose(&native, &voice).expect("partial native evidence");
		assert!(rendered.contains("Tests passed"));
		assert!(rendered.contains("no stored transcript"));
	}
}
