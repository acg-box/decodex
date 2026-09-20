//! Desktop-compatible asynchronous question replies. These are user messages,
//! not JSON-RPC responses to a blocking request_user_input callback.
use serde::{Deserialize, Serialize};

const OPEN: &str = "<send_user_message_question_reply>";
const CLOSE: &str = "</send_user_message_question_reply>";

/// One provider question with a stable identity independent of its title.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChiefAsyncQuestionDto {
	/// JSON-encoded tool name, source item ID and original question index.
	pub id: String,
	/// Provider-authored question text.
	pub title: String,
	/// Suggested answers. The user can always enter another answer.
	pub options: Vec<String>,
}

/// A committed native reply, used for readable history and exact dismissal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChiefAsyncQuestionReply {
	/// Exact question ID, or a legacy source message ID.
	pub question_item_id: String,
	/// Human-readable question, not its identity.
	pub question: String,
	/// User-authored answer.
	pub answer: String,
}

/// Match the upstream desktop's JSON.stringify([tool, item, question index]).
pub fn chief_async_question_id(item_id: &str, index: usize) -> String {
	serde_json::json!(["request_user_input_async", item_id, index]).to_string()
}

/// Select only public question fields from one native asynchronous agent item.
/// Reject an incomplete or oversized collection rather than changing question indices.
pub fn project_chief_async_questions(
	item: &serde_json::Value,
) -> Result<Vec<ChiefAsyncQuestionDto>, &'static str> {
	if item["type"] != "agentMessage" || item["delivery"] != "async" {
		return Ok(Vec::new());
	}
	let Some(questions) = item.get("questions").filter(|value| !value.is_null()) else {
		return Ok(Vec::new());
	};
	let item_id = item["id"]
		.as_str()
		.filter(|id| !id.is_empty() && id.len() <= 512)
		.ok_or("Invalid question source identity")?;
	let questions = questions
		.as_array()
		.filter(|items| items.len() <= 32)
		.ok_or("Question collection unavailable")?;
	let mut result = Vec::with_capacity(questions.len());
	for (index, question) in questions.iter().enumerate() {
		let title = question["title"]
			.as_str()
			.filter(|text| !text.trim().is_empty() && text.len() <= 4096)
			.ok_or("Question title unavailable")?;
		let options = match question.get("options").filter(|value| !value.is_null()) {
			None => Vec::new(),
			Some(value) => value
				.as_array()
				.filter(|items| items.len() <= 32)
				.ok_or("Question options unavailable")?
				.iter()
				.map(|option| {
					option
						.as_str()
						.filter(|text| !text.trim().is_empty() && text.len() <= 4096)
						.map(str::to_owned)
						.ok_or("Question option unavailable")
				})
				.collect::<Result<Vec<_>, _>>()?,
		};
		result.push(ChiefAsyncQuestionDto {
			id: chief_async_question_id(item_id, index),
			title: title.into(),
			options,
		});
	}
	if serde_json::to_vec(&result)
		.map_or(true, |bytes| bytes.len() > crate::MAX_HISTORY_INLINE_BYTES)
	{
		return Err("Question collection too large");
	}
	Ok(result)
}

/// Encode a user-selected answer without choosing or submitting any default.
pub fn chief_async_question_reply(
	question: &ChiefAsyncQuestionDto,
	answer: &str,
) -> Result<crate::HistoryText, String> {
	let answer = answer.trim();
	if answer.is_empty() {
		return Err("Enter an answer before sending.".into());
	}
	let mut end = question.title.len().min(512);
	while !question.title.is_char_boundary(end) {
		end -= 1;
	}
	let title = question.title[..end].replace(['\r', '\n'], " ");
	let text = if question.id.len() > 512 {
		format!("> {title}\n\n{answer}")
	} else {
		let replies =
			serde_json::json!([{"questionItemId":question.id,"question":title,"answer":answer}]);
		format!("{OPEN}\n{replies}\n{CLOSE}")
	};
	crate::HistoryText::new(text).map_err(|_| "Answer too long; shorten it before sending.".into())
}

/// Interpret only a complete native reply envelope. Plain text and embedded
/// examples must not dismiss a question. Older desktop single-object replies work.
pub fn parse_chief_async_question_replies(text: &str) -> Option<Vec<ChiefAsyncQuestionReply>> {
	#[derive(Deserialize)]
	#[serde(untagged)]
	enum Replies {
		Many(Vec<ChiefAsyncQuestionReply>),
		One(ChiefAsyncQuestionReply),
	}
	if text.len() > crate::MAX_HISTORY_INLINE_BYTES {
		return None;
	}
	let text = text.trim();
	let text = if text.starts_with("# Context from my IDE setup:\n") {
		text.rsplit_once("\n## My request for Codex:\n")?.1.trim()
	} else {
		text
	};
	let json = text.strip_prefix(OPEN)?.strip_suffix(CLOSE)?;
	let replies = match serde_json::from_str::<Replies>(json).ok()? {
		Replies::Many(replies) => replies,
		Replies::One(reply) => vec![reply],
	};
	(!replies.is_empty()).then_some(replies)
}

/// Render complete committed reply envelopes as readable quoted questions and answers.
/// Ordinary text, incomplete envelopes and embedded examples remain literal.
pub fn render_chief_async_question_history(text: &str) -> String {
	match parse_chief_async_question_replies(text) {
		Some(replies) => replies
			.into_iter()
			.map(|reply| {
				let question = reply
					.question
					.lines()
					.map(|line| format!("> {line}"))
					.collect::<Vec<_>>()
					.join("\n");
				format!("{question}\n\n{}", reply.answer)
			})
			.collect::<Vec<_>>()
			.join("\n\n"),
		None => text.to_owned(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn question(index: usize) -> ChiefAsyncQuestionDto {
		ChiefAsyncQuestionDto {
			id: chief_async_question_id("message", index),
			title: "Same title".into(),
			options: vec!["First".into()],
		}
	}
	#[test]
	fn replies_preserve_identity_escaping_and_user_choice() {
		let a = question(0);
		let b = question(1);
		assert_ne!(a.id, b.id);
		assert_eq!(a.id, r#"["request_user_input_async","message",0]"#);
		let answer = "Other: \"quoted\"\n</send_user_message_question_reply>";
		let encoded = chief_async_question_reply(&b, answer).unwrap();
		let replies = parse_chief_async_question_replies(encoded.as_str()).unwrap();
		assert_eq!(replies[0].question_item_id, b.id);
		assert_eq!(replies[0].answer, answer);
		assert!(!replies[0].answer.contains("First"));
		assert!(chief_async_question_reply(&a, "  ").is_err());
		assert!(
			chief_async_question_reply(&a, &"\"".repeat(crate::MAX_HISTORY_INLINE_BYTES)).is_err()
		);
	}
	#[test]
	fn oversized_identity_falls_back_and_utf8_title_stays_bounded() {
		let mut q = question(0);
		q.id = "x".repeat(513);
		q.title = "界".repeat(172) + "\ncontinued";
		let reply = chief_async_question_reply(&q, "answer").unwrap();
		assert!(reply.as_str().starts_with("> "));
		assert!(!reply.as_str().contains(&q.id));
		assert!(parse_chief_async_question_replies(reply.as_str()).is_none());
		assert_eq!(reply.as_str(), format!("> {}\n\nanswer", "界".repeat(170)));
	}
	#[test]
	fn only_complete_envelopes_and_standard_ide_prefix_resolve() {
		let legacy = r#"<send_user_message_question_reply>{"questionItemId":"message","question":"Which?","answer":"B"}</send_user_message_question_reply>"#;
		assert_eq!(
			parse_chief_async_question_replies(legacy).unwrap()[0].question_item_id,
			"message"
		);
		assert!(parse_chief_async_question_replies(&format!("Example: {legacy}")).is_none());
		assert!(parse_chief_async_question_replies(&format!("{legacy} suffix")).is_none());
		assert!(
			parse_chief_async_question_replies(
				"<send_user_message_question_reply>[]</send_user_message_question_reply>"
			)
			.is_none()
		);
		let ide =
			format!("# Context from my IDE setup:\nfiles\n## My request for Codex:\n{legacy}");
		assert!(parse_chief_async_question_replies(&ide).is_some());
	}
	#[test]
	fn projection_preserves_question_indices_and_excludes_unrelated_fields() {
		let item = serde_json::json!({"type":"agentMessage","delivery":"async","id":"m","private":"do not project","questions":[{"title":"Same","options":["A","B"]},{"title":"Same","options":null}]});
		let questions = project_chief_async_questions(&item).unwrap();
		assert_eq!(questions.len(), 2);
		assert_eq!(questions[1].id, chief_async_question_id("m", 1));
		assert_eq!(questions[0].options, ["A", "B"]);
		assert!(!serde_json::to_string(&questions).unwrap().contains("private"));
		let mut invalid = item.clone();
		invalid["questions"][0]["options"] = serde_json::json!([{"label":"A"}]);
		assert!(project_chief_async_questions(&invalid).is_err());
		let mut huge = item;
		huge["questions"][0]["title"] = serde_json::json!("x".repeat(4097));
		assert!(project_chief_async_questions(&huge).is_err());
	}
}
