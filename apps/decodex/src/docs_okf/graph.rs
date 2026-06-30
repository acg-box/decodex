//! OKF bundle query and graph construction.

#[allow(clippy::wildcard_imports)] use super::*;

pub(crate) fn query_okf_bundle(root: &Path, query: &OkfQuery) -> Result<Vec<OkfConceptSummary>> {
	let files = read_okf_files(root)?;
	let mut concepts = Vec::new();

	for file in files.iter().filter(|file| is_concept_markdown(&file.relative_path)) {
		let Some(concept) = concept_summary(file) else {
			continue;
		};

		if okf_query_matches(&concept, query) {
			concepts.push(concept);
		}
	}

	concepts.sort_by(|left, right| left.path.cmp(&right.path));

	Ok(concepts)
}

/// Build an OKF concept graph from Markdown links and `related` frontmatter.
pub(crate) fn build_okf_graph(root: &Path) -> Result<OkfGraph> {
	let files = read_okf_files(root)?;
	let concept_paths = okf_concept_path_set(&files);
	let mut concepts = Vec::new();
	let mut edges = Vec::new();
	let mut broken_links = Vec::new();

	for file in files.iter().filter(|file| is_concept_markdown(&file.relative_path)) {
		let Some(concept) = concept_summary(file) else {
			continue;
		};
		let source = concept.id.clone();

		collect_markdown_graph_edges(
			file,
			root,
			&concept_paths,
			&source,
			&mut edges,
			&mut broken_links,
		)?;
		collect_related_graph_edges(
			file,
			root,
			&concept_paths,
			&source,
			&mut edges,
			&mut broken_links,
		);

		concepts.push(concept);
	}

	let orphan_concepts = okf_orphan_concepts(&concepts, &edges);

	concepts.sort_by(|left, right| left.id.cmp(&right.id));
	edges.sort_by(|left, right| {
		(&left.source, &left.target, &left.kind).cmp(&(&right.source, &right.target, &right.kind))
	});
	broken_links.sort_by(|left, right| {
		(&left.source, &left.target, &left.kind).cmp(&(&right.source, &right.target, &right.kind))
	});

	Ok(OkfGraph { concepts, edges, broken_links, orphan_concepts })
}

/// Render an OKF graph as JSON.
pub(crate) fn render_okf_graph_json(graph: &OkfGraph) -> Result<String> {
	Ok(format!("{}\n", serde_json::to_string_pretty(graph)?))
}

/// Render a compact text graph summary.
pub(crate) fn render_okf_graph_summary(root: &Path, graph: &OkfGraph) -> String {
	format!(
		"okf graph: concepts={} edges={} broken_links={} orphans={} root={}\n",
		graph.concepts.len(),
		graph.edges.len(),
		graph.broken_links.len(),
		graph.orphan_concepts.len(),
		root.display()
	)
}

fn concept_summary(file: &DocsFile) -> Option<OkfConceptSummary> {
	let content = file.content.as_deref()?;
	let (frontmatter, _) = split_yaml_frontmatter(content)?;
	let serde_yaml::Value::Mapping(fields) =
		serde_yaml::from_str::<serde_yaml::Value>(frontmatter).ok()?
	else {
		return None;
	};
	let concept_type = frontmatter_string(&fields, "type")?.to_owned();
	let path = path_to_string(&file.relative_path);
	let title = frontmatter_string(&fields, "title")
		.filter(|title| !title.is_empty())
		.map_or_else(|| concept_id(&file.relative_path), str::to_owned);
	let description = frontmatter_string(&fields, "description")
		.filter(|description| !description.is_empty())
		.map(str::to_owned);
	let resource = frontmatter_string(&fields, "resource")
		.filter(|resource| !resource.is_empty())
		.map(str::to_owned);
	let tags = frontmatter_string_list_lossy(&fields, "tags");
	let source_refs = frontmatter_string_list_lossy(&fields, "source_refs");
	let code_refs = frontmatter_string_list_lossy(&fields, "code_refs");
	let related = frontmatter_string_list_lossy(&fields, "related");

	Some(OkfConceptSummary {
		id: concept_id(&file.relative_path),
		path,
		concept_type,
		title,
		description,
		resource,
		tags,
		source_refs,
		code_refs,
		related,
	})
}

fn concept_id(path: &Path) -> String {
	let mut id = path.to_path_buf();

	id.set_extension("");

	path_to_string(&id)
}

fn path_to_string(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}

fn frontmatter_string_list_lossy(fields: &Mapping, key: &str) -> Vec<String> {
	match frontmatter_value(fields, key) {
		Some(serde_yaml::Value::Sequence(items)) => items
			.iter()
			.filter_map(|item| match item {
				serde_yaml::Value::String(value) if !value.trim().is_empty() =>
					Some(value.trim().to_owned()),
				_ => None,
			})
			.collect(),
		_ => Vec::new(),
	}
}

fn okf_query_matches(concept: &OkfConceptSummary, query: &OkfQuery) -> bool {
	query
		.concept_type
		.as_deref()
		.is_none_or(|value| concept.concept_type.eq_ignore_ascii_case(value))
		&& query
			.tags
			.iter()
			.all(|tag| concept.tags.iter().any(|candidate| candidate.eq_ignore_ascii_case(tag)))
		&& query.resource.as_deref().is_none_or(|value| {
			concept.resource.as_deref().is_some_and(|resource| contains_ci(resource, value))
		}) && query.source_ref.as_deref().is_none_or(|value| {
		concept.source_refs.iter().any(|source_ref| contains_ci(source_ref, value))
	}) && query
		.code_ref
		.as_deref()
		.is_none_or(|value| concept.code_refs.iter().any(|code_ref| contains_ci(code_ref, value)))
		&& query
			.related
			.as_deref()
			.is_none_or(|value| concept.related.iter().any(|related| contains_ci(related, value)))
		&& query.text.as_deref().is_none_or(|value| concept_text_matches(concept, value))
}

fn concept_text_matches(concept: &OkfConceptSummary, value: &str) -> bool {
	contains_ci(&concept.path, value)
		|| contains_ci(&concept.title, value)
		|| concept.description.as_deref().is_some_and(|description| contains_ci(description, value))
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
	haystack.to_lowercase().contains(&needle.to_lowercase())
}

fn okf_concept_path_set(files: &[DocsFile]) -> BTreeSet<PathBuf> {
	files
		.iter()
		.filter(|file| is_concept_markdown(&file.relative_path))
		.map(|file| file.relative_path.clone())
		.collect()
}

fn collect_markdown_graph_edges(
	file: &DocsFile,
	bundle_root: &Path,
	concept_paths: &BTreeSet<PathBuf>,
	source: &str,
	edges: &mut Vec<OkfGraphEdge>,
	broken_links: &mut Vec<OkfBrokenLink>,
) -> Result<()> {
	let Some(content) = file.content.as_deref() else {
		return Ok(());
	};
	let link_pattern = Regex::new(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+[^)]*)?\)")?;

	for captures in link_pattern.captures_iter(content) {
		let Some(target_match) = captures.get(1) else {
			continue;
		};
		let target = target_match.as_str();

		if should_skip_link_target(target) {
			continue;
		}

		push_graph_target(
			file,
			bundle_root,
			concept_paths,
			source,
			target,
			"markdown",
			edges,
			broken_links,
		);
	}

	Ok(())
}

fn collect_related_graph_edges(
	file: &DocsFile,
	bundle_root: &Path,
	concept_paths: &BTreeSet<PathBuf>,
	source: &str,
	edges: &mut Vec<OkfGraphEdge>,
	broken_links: &mut Vec<OkfBrokenLink>,
) {
	let Some(content) = file.content.as_deref() else {
		return;
	};
	let Some((frontmatter, _)) = split_yaml_frontmatter(content) else {
		return;
	};
	let Ok(serde_yaml::Value::Mapping(fields)) =
		serde_yaml::from_str::<serde_yaml::Value>(frontmatter)
	else {
		return;
	};

	for target in frontmatter_string_list_lossy(&fields, "related") {
		push_graph_target(
			file,
			bundle_root,
			concept_paths,
			source,
			&target,
			"related",
			edges,
			broken_links,
		);
	}
}

#[allow(clippy::too_many_arguments)]
fn push_graph_target(
	file: &DocsFile,
	bundle_root: &Path,
	concept_paths: &BTreeSet<PathBuf>,
	source: &str,
	target: &str,
	kind: &str,
	edges: &mut Vec<OkfGraphEdge>,
	broken_links: &mut Vec<OkfBrokenLink>,
) {
	let Some(target_path) = resolve_link_target(&file.path, bundle_root, target) else {
		return;
	};
	let Ok(relative_target) = target_path.strip_prefix(bundle_root) else {
		return;
	};
	let relative_target = relative_target.to_path_buf();

	if concept_paths.contains(&relative_target) {
		edges.push(OkfGraphEdge {
			source: source.to_owned(),
			target: concept_id(&relative_target),
			kind: kind.to_owned(),
		});
	} else if !target_path.exists() {
		broken_links.push(broken_link(source, target, kind));
	}
}

fn broken_link(source: &str, target: &str, kind: &str) -> OkfBrokenLink {
	OkfBrokenLink { source: source.to_owned(), target: target.to_owned(), kind: kind.to_owned() }
}

fn okf_orphan_concepts(concepts: &[OkfConceptSummary], edges: &[OkfGraphEdge]) -> Vec<String> {
	let connected: BTreeSet<&str> =
		edges.iter().flat_map(|edge| [edge.source.as_str(), edge.target.as_str()]).collect();

	concepts
		.iter()
		.filter(|concept| !connected.contains(concept.id.as_str()))
		.map(|concept| concept.id.clone())
		.collect()
}
