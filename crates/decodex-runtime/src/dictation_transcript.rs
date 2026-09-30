//! Subscription transcript revisions belong to the Rust session, not the audio adapter.
use decodex_protocol::DictationBuffer;
use serde::Deserialize;

#[derive(Deserialize)]
struct Segment {
	id: String,
	revision: i64,
	text: String,
	finalized: bool,
}

#[derive(Default)]
pub(super) struct Transcript(Vec<Segment>);

impl Transcript {
	pub(super) fn apply(&mut self, event: serde_json::Value) -> Result<DictationBuffer, ()> {
		let incoming: Segment = serde_json::from_value(event).map_err(|_| ())?;
		if incoming.id.chars().count() > 256 || incoming.text.len() > 32_768 {
			return Err(());
		}
		match self.0.iter_mut().find(|segment| segment.id == incoming.id) {
			Some(old) if old.finalized || incoming.revision < old.revision => {},
			Some(old) => *old = incoming,
			None => self.0.push(incoming),
		}
		let text = self
			.0
			.iter()
			.map(|s| s.text.trim())
			.filter(|s| !s.is_empty())
			.collect::<Vec<_>>()
			.join(" ");
		if text.len() > 32_768 {
			return Err(());
		}
		DictationBuffer::new(text).map_err(|_| ())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn final_correction_preserves_order_and_rejects_stale_or_late_revisions() {
		let mut transcript = Transcript::default();
		let mut apply = |id, revision, text, finalized| {
			transcript
				.apply(json!({"id":id,"revision":revision,"text":text,"finalized":finalized}))
				.unwrap()
		};
		apply("a", 1, " This is", false);
		apply("a", 2, " This is live", false);
		apply("b", 1, "第二句话。", true);
		apply("a", 1, "stale", false);
		assert_eq!(apply("a", 3, "This is live.", true).as_str(), "This is live. 第二句话。");
		assert_eq!(apply("a", 4, "late", false).as_str(), "This is live. 第二句话。");
	}

	#[test]
	fn invalid_and_oversized_segments_fail_instead_of_replacing_the_draft() {
		let mut transcript = Transcript::default();
		assert!(transcript.apply(json!({"id":"a","text":"missing revision"})).is_err());
		assert!(
			transcript
				.apply(json!({"id":"a","revision":1,"text":"x".repeat(32769),"finalized":false}))
				.is_err()
		);
		transcript
			.apply(json!({"id":"a","revision":1,"text":"x".repeat(20000),"finalized":true}))
			.unwrap();
		assert!(
			transcript
				.apply(json!({"id":"b","revision":1,"text":"x".repeat(20000),"finalized":true}))
				.is_err()
		);
	}
}
