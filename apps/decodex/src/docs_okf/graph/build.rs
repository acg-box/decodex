use crate::docs_okf::{
	self, BTreeSet, DocsFile, OkfBrokenLink, OkfConceptSummary, OkfGraph, OkfGraphEdge, Path,
	PathBuf, Regex, Result,
	graph::summary,
	serde_yaml::{self, Value},
};

struct GraphTargetContext<'a> {
	file: &'a DocsFile,
	bundle_root: &'a Path,
	concept_paths: &'a BTreeSet<PathBuf>,
	source: &'a str,
	edges: &'a mut Vec<OkfGraphEdge>,
	broken_links: &'a mut Vec<OkfBrokenLink>,
}

/// Build an OKF concept graph from Markdown links and `related` frontmatter.
pub(crate) fn build_okf_graph(root: &Path) -> Result<OkfGraph> {
	let files = docs_okf::read_okf_files(root)?;
	let concept_paths = okf_concept_path_set(&files);
	let mut concepts = Vec::new();
	let mut edges = Vec::new();
	let mut broken_links = Vec::new();

	for file in files.iter().filter(|file| docs_okf::is_concept_markdown(&file.relative_path)) {
		let Some(concept) = summary::concept_summary(file) else {
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

fn okf_concept_path_set(files: &[DocsFile]) -> BTreeSet<PathBuf> {
	files
		.iter()
		.filter(|file| docs_okf::is_concept_markdown(&file.relative_path))
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

		if docs_okf::should_skip_link_target(target) {
			continue;
		}

		let mut context =
			GraphTargetContext { file, bundle_root, concept_paths, source, edges, broken_links };

		push_graph_target(&mut context, target, "markdown");
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
	let Some((frontmatter, _)) = docs_okf::split_yaml_frontmatter(content) else {
		return;
	};
	let Ok(serde_yaml::Value::Mapping(fields)) = serde_yaml::from_str::<Value>(frontmatter) else {
		return;
	};
	let mut context =
		GraphTargetContext { file, bundle_root, concept_paths, source, edges, broken_links };

	for target in summary::frontmatter_string_list_lossy(&fields, "related") {
		push_graph_target(&mut context, &target, "related");
	}
}

fn push_graph_target(context: &mut GraphTargetContext<'_>, target: &str, kind: &str) {
	let Some(target_path) =
		docs_okf::resolve_link_target(&context.file.path, context.bundle_root, target)
	else {
		return;
	};
	let Ok(relative_target) = target_path.strip_prefix(context.bundle_root) else {
		return;
	};
	let relative_target = relative_target.to_path_buf();

	if context.concept_paths.contains(&relative_target) {
		context.edges.push(OkfGraphEdge {
			source: context.source.to_owned(),
			target: summary::concept_id(&relative_target),
			kind: kind.to_owned(),
		});
	} else if !target_path.exists() {
		context.broken_links.push(broken_link(context.source, target, kind));
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
