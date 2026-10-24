//! Selected Work evidence is read independently from the small chat snapshot.
use decodex_database::SqliteStore;
use decodex_protocol::{MAX_WORK_DETAIL_BYTES, WORK_DETAIL_PAGE_BYTES, WorkDetailResult};
use sha2::{Digest as _, Sha256};

pub(crate) async fn read(
	store: &SqliteStore,
	id: &str,
	expected: Option<&str>,
	offset: usize,
) -> WorkDetailResult {
	let Ok(portfolio) = store.work_portfolio(Some(id.into())).await else {
		return WorkDetailResult::Unavailable;
	};
	let Ok(text) = serde_json::to_string(&portfolio) else {
		return WorkDetailResult::Unavailable;
	};
	if text.len() > MAX_WORK_DETAIL_BYTES
		|| offset >= text.len()
		|| !text.is_char_boundary(offset)
		|| (offset > 0 && expected.is_none())
	{
		return WorkDetailResult::Unavailable;
	}
	let digest =
		Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>();
	if expected.is_some_and(|d| d != digest) {
		return WorkDetailResult::Unavailable;
	}
	let mut end = (offset + WORK_DETAIL_PAGE_BYTES).min(text.len());
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	WorkDetailResult::Page {
		digest,
		offset,
		total_bytes: text.len(),
		text: text[offset..end].into(),
		next_offset: (end < text.len()).then_some(end),
	}
}

#[cfg(test)]
mod tests {
	use super::{SqliteStore, WorkDetailResult, read};
	use decodex_core::{
		DecodexRoot,
		work::{Material, Work, WorkCommand, WorkRequirements, WorkState},
	};

	#[tokio::test]
	async fn selected_work_pages_keep_unicode_and_reject_a_changed_view() {
		let directory = tempfile::tempdir().expect("Work fixture");
		let root = DecodexRoot::new(directory.path().canonicalize().expect("Work fixture"))
			.expect("Work fixture");
		let store = SqliteStore::open(&root.paths()).expect("Work fixture");
		crate::agent::AgentCoordinator::reserve_root(&store, "sage", "Assist the user")
			.await
			.expect("Work fixture");
		let work = Work {
			id: "venue".into(),
			project_id: None,
			owner_agent_id: "sage".into(),
			title: "Venue".into(),
			requirements: WorkRequirements {
				id: "r1".into(),
				goal: "界".repeat(3000),
				criteria: "Check capacity".into(),
				source: "Synthetic user decision".into(),
			},
			state: WorkState::Open,
			reason: None,
			resume_condition: None,
			user_hold: false,
			assessment: None,
		};
		store
			.apply_work_command("sage".into(), "create".into(), WorkCommand::Create { work }, true)
			.await
			.expect("Work fixture");
		let WorkDetailResult::Page { digest, text, next_offset: Some(next), .. } =
			read(&store, "venue", None, 0).await
		else {
			panic!("first page")
		};
		let WorkDetailResult::Page { text: tail, next_offset: None, .. } =
			read(&store, "venue", Some(&digest), next).await
		else {
			panic!("last page")
		};
		let portfolio: decodex_core::work::WorkPortfolio =
			serde_json::from_str(&(text + &tail)).expect("complete Work");
		assert_eq!(portfolio.works[0].requirements.goal, "界".repeat(3000));
		assert_eq!(read(&store, "venue", None, next).await, WorkDetailResult::Unavailable);
		store
			.apply_work_command(
				"sage".into(),
				"material".into(),
				WorkCommand::AddMaterial {
					material: Material {
						id: "venue-link".into(),
						work_id: "venue".into(),
						title: "Venue".into(),
						reference: "https://example.com/venue".into(),
						sha256: None,
					},
				},
				true,
			)
			.await
			.expect("Work fixture");
		assert_eq!(read(&store, "venue", Some(&digest), next).await, WorkDetailResult::Unavailable);
		let overview = store.work_overview().await.expect("Work fixture");
		assert!(overview.summary_only && overview.works[0].requirements.goal.is_empty());
	}
}
