use super::{Titles, parse, prompt};
use crate::agent_message_history::{History, excerpts::Exchange};

#[test]
fn title_response_preserves_unicode_and_rejects_invalid_or_private_metadata() {
	assert_eq!(parse(r#"{"title":" 自动整理旁聊标题 "}"#).as_deref(), Some("自动整理旁聊标题"));
	for output in [
		r#"{"title":""}"#,
		r#"{"title":"two\nlines"}"#,
		r#"{"title":"valid","extra":true}"#,
		r#"{"title":"Bearer secret-token"}"#,
	] {
		assert!(parse(output).is_none());
	}
	assert!(parse(&serde_json::json!({"title":"字".repeat(36)}).to_string()).is_some());
	assert!(parse(&serde_json::json!({"title":"字".repeat(37)}).to_string()).is_none());
}

#[test]
fn title_prompt_keeps_recent_focus_bounded_and_preserves_current_title() {
	let exchanges = (0..8)
		.map(|n| Exchange {
			user: format!("question-{n} {} end-user-{n}", "字".repeat(3000)),
			assistant: format!("answer-{n} {} final-{n}", "中".repeat(3000)),
		})
		.collect();
	let text =
		prompt(Some("Keep this title"), &History { exchanges, latest_turn: Some("last".into()) });
	assert!(text.contains("Keep this title"));
	assert!(!text.contains("question-0"));
	assert!(text.contains("question-7") && text.contains("final-7"));
	assert!(text.len() < 6000);
	assert!(text.contains("return it exactly unless"));
}

#[test]
fn title_jobs_coalesce_exact_completion_and_cancel_stale_results() {
	let titles = Titles::default();
	let one = titles.start("work", "thread", "one").unwrap();
	assert!(titles.start("work", "thread", "one").is_none());
	let two = titles.start("work", "thread", "two").unwrap();
	assert!(*one.borrow());
	titles.finish("work", "thread", "one");
	assert!(titles.start("work", "thread", "two").is_none());
	let event = decodex_codex::app_server_client::ServerEvent::Notification {
		method: "thread/name/updated".into(),
		params: serde_json::json!({"threadId":"thread","threadName":"User title"}),
	};
	titles.observe(&event);
	assert!(*two.borrow());
	titles.stop();
	assert!(titles.start("work", "thread", "two").is_some());
}
