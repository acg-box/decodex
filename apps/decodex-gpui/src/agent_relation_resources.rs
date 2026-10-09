//! Native associations establish shared references, not authorship or verification.
use super::{AgentSurface, BTreeSet, Edge, Graph, Kind, Node, Row};

impl AgentSurface {
	pub(super) fn add_relation_resources(
		&self,
		rows: &[Row],
		included: &BTreeSet<String>,
		graph: &mut Graph,
	) {
		for row in rows.iter().filter(|r| !r.native && included.contains(&r.key)) {
			let Some(brief) =
				self.work_board.briefs.get(&row.key).filter(|b| b.stamp == self.brief_stamp(row))
			else {
				continue;
			};
			let Some(resources) = &brief.resources else { continue };
			for resource in resources {
				// Exact kind + identity matches the existing native association owner.
				let key = format!(
					"resource:{}",
					serde_json::to_string(&(&resource.attachment_type, &resource.identity_key))
						.expect("string pair")
				);
				let payload: serde_json::Value =
					serde_json::from_str(&resource.payload_json).unwrap_or_default();
				let title = payload["title"]
					.as_str()
					.or_else(|| payload["name"].as_str())
					.unwrap_or(&resource.identity_key)
					.to_owned();
				if !graph.nodes.iter().any(|n| n.key == key) {
					graph.nodes.push(Node {
						key: key.clone(),
						title,
						status: resource.attachment_type.clone(),
						color: Kind::Resource.color(),
						row: None,
						x: 0.,
						y: 0.,
					});
				}
				let link = payload["url"]
					.as_str()
					.and_then(|value| reqwest::Url::parse(value).ok())
					.filter(|url| {
						matches!(url.scheme(), "http" | "https")
							&& url.username().is_empty()
							&& url.password().is_none()
					})
					.map(|url| url.to_string());
				graph.edges.push(Edge {
                    id: format!("attachment:{}:{}", row.key, resource.id), from: row.key.clone(), to: key, kind: Kind::Resource, excerpt: None, color: Kind::Resource.color(), label: "Attached".into(),
                    detail: format!("Native resource association\n\nType: {}\nIdentity: {}\nAssociation: {}\nRecorded at: {} (Unix seconds)\n\nAn attachment does not establish who created, read, or verified this resource.{}", resource.attachment_type, resource.identity_key, resource.id, resource.created_at, if resource.payload_omitted { "\nSome metadata is unavailable." } else { "" }),
                    source: row.thread.clone().map(|thread| (row.work.clone(), thread)), link,
                });
			}
		}
	}
}

pub(super) fn place_resources(graph: &mut Graph) {
	let x = graph
		.nodes
		.iter()
		.filter(|n| !n.key.starts_with("resource:"))
		.map(|n| n.x)
		.fold(24., f32::max)
		+ 480.;
	let mut cursor = 24_f32;
	let keys: Vec<_> = graph
		.nodes
		.iter()
		.filter(|n| n.key.starts_with("resource:"))
		.map(|n| n.key.clone())
		.collect();
	for key in keys {
		let owners: Vec<_> = graph
			.edges
			.iter()
			.filter(|e| e.kind == Kind::Resource && e.to == key)
			.filter_map(|e| graph.nodes.iter().find(|n| n.key == e.from))
			.collect();
		let count = owners.len();
		let y =
			if count > 0 { owners.iter().map(|n| n.y).sum::<f32>() / count as f32 } else { cursor };
		let y = y.max(cursor);
		let node = graph.nodes.iter_mut().find(|n| n.key == key).expect("resource key");
		node.x = x;
		node.y = y;
		if count > 1 {
			node.status = format!("{} · {} agents", node.status, count);
		}
		cursor = y + 124.;
	}
}
