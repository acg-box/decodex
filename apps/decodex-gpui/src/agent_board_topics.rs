//! Topics from the native Board and receipts preserved in older conversations.
use super::*;
use decodex_protocol::{AgentTimelineContent, AgentTimelinePage, WireText};

pub(super) use decodex_protocol::NativeBoardPost as Post;

// Native UTC timestamps have optional fractional seconds. Sort the unpadded UTC value.
pub(super) fn order(post: &Post) -> (&str, &str) {
	(post.created_at.trim_end_matches('Z'), &post.message_id)
}

pub(super) fn time_since_topic(post: &Post, opener: &Post) -> Option<u64> {
	if post.message_id == post.thread_id || post.thread_id != opener.message_id {
		return None;
	}
	let at = |post: &Post| {
		time::OffsetDateTime::parse(
			&post.created_at,
			&time::format_description::well_known::Rfc3339,
		)
		.ok()
	};
	u64::try_from((at(post)? - at(opener)?).whole_milliseconds()).ok()
}

pub(super) fn from_entry(entry: &AgentTimelineContent, source: &str) -> Option<Post> {
	let AgentTimelineContent::Item { activity: Some(activity), text, .. } = entry else {
		return None;
	};
	if activity.status != "completed"
		|| !matches!(
			(activity.kind.as_str(), activity.detail.as_str()),
			("functionCallOutput", "collaboration/post" | "collaboration/read_post")
				| ("mcpToolCall", "codex_message_board/post" | "codex_message_board/read_post")
		) {
		return None;
	}
	let mut post: Post = serde_json::from_str(text).ok()?;
	post.posted_by_thread = activity.detail.ends_with("/post").then(|| source.to_owned());
	(!post.message_id.is_empty()
		&& !post.thread_id.is_empty()
		&& !post.channel_name.is_empty()
		&& !post.author.is_empty())
	.then_some(post)
}

#[derive(Default, Clone)]
pub(super) struct History {
	native_after: i64,
	pub(super) posts: BTreeMap<String, Post>,
	// Latest native item in a successfully scanned range. Positions can be page-relative.
	head: Option<(String, String)>,
	pending: Option<Scan>,
	pub(super) complete: bool,
}

#[derive(Clone)]
struct Scan {
	// Advance the checkpoint to this tip only after the entire gap has been covered.
	tip: Option<(String, String)>,
	before: String,
}

fn identity(entry: &AgentTimelineEntry) -> Option<(&str, &str)> {
	if let AgentTimelineContent::Item { turn_id, item_id, .. } = &entry.content {
		Some((turn_id, item_id))
	} else {
		None
	}
}

pub(super) fn merge(posts: &mut BTreeMap<String, Post>, mut post: Post) {
	use std::collections::btree_map::Entry;
	let prior = match posts.entry(post.message_id.clone()) {
		Entry::Vacant(slot) => {
			slot.insert(post);
			return;
		},
		Entry::Occupied(slot) => slot.into_mut(),
	};
	if prior.thread_id != post.thread_id
		|| prior.channel_name != post.channel_name
		|| prior.author != post.author
	{
		return;
	}
	if prior.posted_by_thread.is_none() {
		prior.posted_by_thread.clone_from(&post.posted_by_thread);
	}
	if post.text.len() > prior.text.len() || (post.text == prior.text && !post.truncated) {
		post.posted_by_thread = prior.posted_by_thread.clone();
		*prior = post;
	}
}

impl History {
	fn observe(&mut self, page: &AgentTimelinePage) -> bool {
		let mut reached = false;
		for entry in &page.entries {
			if page
				.source_threads
				.get(&entry.position)
				.is_some_and(|origin| origin != &page.thread_id)
			{
				continue;
			}
			reached |= self
				.head
				.as_ref()
				.is_some_and(|(turn, id)| identity(entry) == Some((turn.as_str(), id.as_str())));
			if let Some(post) = from_entry(&entry.content, &page.thread_id) {
				merge(&mut self.posts, post);
			}
		}
		reached
	}

	pub(super) async fn refresh(
		&mut self,
		client: &AgentClient,
		owner: &EntityId,
		thread: &EntityId,
		first: &AgentTimelinePage,
		read_native: bool,
	) {
		self.refresh_receipts(client, owner, thread, first).await;
		if read_native
			&& let Ok(Some(page)) =
				client.native_board(owner.clone(), thread.clone(), self.native_after).await
		{
			self.observe_native(page);
		}
	}

	pub(super) fn observe_native(&mut self, page: decodex_protocol::NativeBoardPage) {
		for post in page.posts {
			merge(&mut self.posts, post);
		}
		self.native_after = page.after;
		self.complete &= !page.more;
	}

	async fn refresh_receipts(
		&mut self,
		client: &AgentClient,
		owner: &EntityId,
		thread: &EntityId,
		first: &AgentTimelinePage,
	) {
		let reached = self.observe(first);
		let scan = self.pending.take().or_else(|| {
			let before = first.next_cursor.as_ref().filter(|_| !reached)?;
			Some(Scan { tip: tip(first), before: before.clone() })
		});
		let Some(scan) = scan else {
			self.head = tip(first).or_else(|| self.head.take());
			self.complete = true;
			return;
		};
		self.complete = false;
		// One older page per refresh keeps other agents and the live transcript responsive.
		let Ok(cursor) = WireText::new(&scan.before) else { return };
		let Ok(AgentTimelineResult::Available { work_id, page, .. }) =
			client.timeline(owner.clone(), thread.clone(), Some(cursor)).await
		else {
			return; // A stale cursor can be rediscovered from the latest page.
		};
		if &work_id != owner || page.thread_id != thread.as_str() {
			return;
		}
		let reached = self.observe(&page);
		if !reached && let Some(before) = page.next_cursor {
			if before != scan.before {
				self.pending = Some(Scan { before, ..scan });
			}
		} else {
			self.head = scan.tip;
			self.complete = true;
		}
	}
}

fn tip(page: &AgentTimelinePage) -> Option<(String, String)> {
	page.entries
		.iter()
		.rev()
		.filter(|entry| {
			page.source_threads.get(&entry.position).is_none_or(|id| id == &page.thread_id)
		})
		.find_map(identity)
		.map(|(turn, id)| (turn.to_owned(), id.to_owned()))
}
