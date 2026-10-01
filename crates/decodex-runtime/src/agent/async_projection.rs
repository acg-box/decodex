//! Build a complete question projection off-store before replacing durable state.
use super::{AgentError, Value, observations::is_plain_user_prompt};
use decodex_database::AgentAsyncQuestion;
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct Projection {
	pub questions: Vec<AgentAsyncQuestion>,
	pub answers: BTreeSet<String>,
	seen: BTreeSet<String>,
	bytes: usize,
	items: usize,
}
impl Projection {
	pub fn observe(&mut self, thread: &str, turn: &str, item: &Value) -> Result<(), AgentError> {
		self.bytes = self.bytes.saturating_add(item.to_string().len());
		self.items += 1;

		if self.bytes > 8 * 1_024 * 1_024 || self.items > 8_192 {
			return Err(AgentError::Invalid("native question history exceeds read bounds".into()));
		}
		if item["type"] == "agentMessage" && item["delivery"] == "async" {
			let questions = decodex_protocol::project_agent_async_questions(item)
				.map_err(|error| AgentError::Invalid(error.into()))?;
			let id = item["id"]
				.as_str()
				.ok_or_else(|| AgentError::Invalid("missing native question item".into()))?;

			for question in questions {
				if !self.seen.insert(question.id.clone()) {
					return Err(AgentError::Invalid("duplicate native question identity".into()));
				}

				self.questions.push(AgentAsyncQuestion {
					arrived_live: false,
					thread_id: thread.into(),
					turn_id: turn.into(),
					item_id: id.into(),
					question_id: question.id.clone(),
					question_json: serde_json::to_string(&question).expect("serializable question"),
				});
			}
		} else if is_plain_user_prompt(item) {
			self.answers.extend(self.seen.iter().cloned());
		} else if item["type"] == "userMessage" {
			let mut content = item["content"]
				.as_array()
				.into_iter()
				.flatten()
				.filter(|part| part["type"] != "skill" && part["type"] != "mention");

			if let Some(part) = content.next()
				&& content.next().is_none()
				&& part["type"] == "text"
				&& let Some(replies) = part["text"]
					.as_str()
					.and_then(decodex_protocol::parse_agent_async_question_replies)
			{
				self.answers.extend(replies.into_iter().map(|r| r.question_item_id));
			}
		}

		Ok(())
	}
}
