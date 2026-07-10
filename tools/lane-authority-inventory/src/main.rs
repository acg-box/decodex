use std::{
	collections::{BTreeMap, BTreeSet},
	env, fs,
	path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tree_sitter::{Language, Node, Parser};

#[derive(Deserialize)]
struct SourceInventory {
	records: Vec<SourceIdentity>,
}

#[derive(Clone, Deserialize, Serialize)]
struct SourceIdentity {
	byte_length: usize,
	content_digest: String,
	language: String,
	path: String,
	predecessor_source_node_id: Option<String>,
	provenance: String,
	scope: String,
	source_node_id: String,
	status: String,
}

#[derive(Deserialize)]
struct CandidateManifest {
	records: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
	candidate_id: String,
	line_number: usize,
	source_node_id: String,
}

#[derive(Serialize)]
struct ParsedSource {
	#[serde(flatten)]
	identity: SourceIdentity,
	parser_error_count: usize,
	parser_node_count: usize,
	parser_node_digest: String,
	parser_receipt_id: Option<String>,
	syntax_site_count: usize,
	syntax_site_ids_digest: String,
	zero_syntax_reason_code: Option<String>,
}

#[derive(Clone, Serialize)]
struct SyntaxSite {
	byte_end: usize,
	byte_start: usize,
	is_parser_root: bool,
	node_kind: String,
	recovery_state: String,
	site_id: String,
	source_node_id: String,
}

#[derive(Serialize)]
struct CandidateSiteEdge {
	candidate_id: String,
	edge_digest: String,
	site_id: String,
}

#[derive(Serialize)]
struct ParseOutput {
	candidate_site_edges: Vec<CandidateSiteEdge>,
	source_nodes: Vec<ParsedSource>,
	syntax_sites: Vec<SyntaxSite>,
}

fn sha256(parts: &[&[u8]]) -> String {
	let mut digest = Sha256::new();
	for part in parts {
		digest.update(part);
	}
	format!("{:x}", digest.finalize())
}

fn stable_id_set_digest(domain: &str, identifiers: &BTreeSet<String>) -> String {
	let mut digest = Sha256::new();
	digest.update(domain.as_bytes());
	digest.update([0]);
	for identifier in identifiers {
		digest.update(identifier.as_bytes());
		digest.update([0]);
	}
	format!("{:x}", digest.finalize())
}

fn language(name: &str) -> Result<Language, String> {
	match name {
		"bash" | "shell" => Ok(tree_sitter_bash::LANGUAGE.into()),
		"python" => Ok(tree_sitter_python::LANGUAGE.into()),
		"rust" => Ok(tree_sitter_rust::LANGUAGE.into()),
		"swift" => Ok(tree_sitter_swift::LANGUAGE.into()),
		"toml" => Ok(tree_sitter_toml_ng::LANGUAGE.into()),
		"yaml" => Ok(tree_sitter_yaml::LANGUAGE.into()),
		other => Err(format!("unsupported language: {other}")),
	}
}

fn materialize_kind(kind: &str) -> bool {
	["call", "command", "exec", "macro", "redirect"].iter().any(|needle| kind.contains(needle))
}

fn site_id(source_id: &str, node: Node<'_>) -> String {
	sha256(&[
		b"decodex/lane-authority-v2-syntax-site/1\0",
		source_id.as_bytes(),
		b"\0",
		node.kind().as_bytes(),
		b"\0",
		node.start_byte().to_string().as_bytes(),
		b"\0",
		node.end_byte().to_string().as_bytes(),
	])
}

fn node_record_digest(node: Node<'_>) -> [u8; 32] {
	let mut digest = Sha256::new();
	digest.update(node.kind().as_bytes());
	digest.update([0]);
	digest.update(node.start_byte().to_be_bytes());
	digest.update(node.end_byte().to_be_bytes());
	digest.finalize().into()
}

fn collect_nodes<'tree>(root: Node<'tree>) -> Vec<Node<'tree>> {
	let mut result = Vec::new();
	let mut pending = vec![root];
	while let Some(node) = pending.pop() {
		if node.is_named() {
			result.push(node);
		}
		let mut cursor = node.walk();
		let mut children: Vec<_> = node.children(&mut cursor).collect();
		children.reverse();
		pending.extend(children);
	}
	result
}

fn smallest_node_for_line<'tree>(nodes: &[Node<'tree>], line: usize) -> Option<Node<'tree>> {
	let row = line.checked_sub(1)?;
	nodes
		.iter()
		.copied()
		.filter(|node| node.start_position().row <= row && node.end_position().row >= row)
		.min_by_key(|node| node.end_byte().saturating_sub(node.start_byte()))
}

fn parse_source(
	root: &Path,
	source: SourceIdentity,
	candidates: &[&Candidate],
) -> Result<(ParsedSource, Vec<SyntaxSite>, Vec<CandidateSiteEdge>), String> {
	if source.status == "deleted" {
		return Ok((
			ParsedSource {
				identity: source,
				parser_error_count: 0,
				parser_node_count: 0,
				parser_node_digest: sha256(&[b"decodex/lane-authority-v2-parser-nodes/1\0"]),
				parser_receipt_id: None,
				syntax_site_count: 0,
				syntax_site_ids_digest: stable_id_set_digest(
					"decodex/lane-authority-v2-source-syntax-sites/1",
					&BTreeSet::new(),
				),
				zero_syntax_reason_code: Some("deleted_tombstone".to_owned()),
			},
			Vec::new(),
			Vec::new(),
		));
	}
	let bytes = fs::read(root.join(&source.path)).map_err(|error| error.to_string())?;
	if bytes.len() != source.byte_length || sha256(&[&bytes]) != source.content_digest {
		return Err(format!("source bytes disagree with inventory: {}", source.path));
	}
	let mut parser = Parser::new();
	parser.set_language(&language(&source.language)?).map_err(|error| error.to_string())?;
	let tree = parser
		.parse(&bytes, None)
		.ok_or_else(|| format!("parser returned no tree: {}", source.path))?;
	let nodes = collect_nodes(tree.root_node());
	let parser_error_count =
		nodes.iter().filter(|node| node.is_error() || node.is_missing()).count();
	let mut digest = Sha256::new();
	digest.update(b"decodex/lane-authority-v2-parser-nodes/1\0");
	for node in &nodes {
		digest.update(node_record_digest(*node));
	}
	let candidate_nodes: BTreeMap<_, _> = candidates
		.iter()
		.filter_map(|candidate| {
			smallest_node_for_line(&nodes, candidate.line_number)
				.map(|node| (candidate.candidate_id.as_str(), node))
		})
		.collect();
	let mut selected: BTreeMap<String, Node<'_>> = BTreeMap::new();
	for node in &nodes {
		if *node == tree.root_node() || materialize_kind(node.kind()) {
			selected.insert(site_id(&source.source_node_id, *node), *node);
		}
	}
	for node in candidate_nodes.values() {
		selected.insert(site_id(&source.source_node_id, *node), *node);
	}
	let sites: Vec<_> = selected
		.iter()
		.map(|(id, node)| SyntaxSite {
			byte_end: node.end_byte(),
			byte_start: node.start_byte(),
			is_parser_root: *node == tree.root_node(),
			node_kind: node.kind().to_owned(),
			recovery_state: if node.is_error() || node.is_missing() {
				"error".to_owned()
			} else {
				"clean".to_owned()
			},
			site_id: id.clone(),
			source_node_id: source.source_node_id.clone(),
		})
		.collect();
	let site_ids: BTreeSet<_> = sites.iter().map(|site| site.site_id.clone()).collect();
	let edges = candidate_nodes
		.into_iter()
		.map(|(candidate_id, node)| {
			let id = site_id(&source.source_node_id, node);
			CandidateSiteEdge {
				edge_digest: sha256(&[
					b"decodex/lane-authority-v2-candidate-site-edge/1\0",
					candidate_id.as_bytes(),
					b"\0",
					id.as_bytes(),
				]),
				candidate_id: candidate_id.to_owned(),
				site_id: id,
			}
		})
		.collect();
	let receipt_id = format!("tool:{}:parser", source.language);
	Ok((
		ParsedSource {
			identity: source,
			parser_error_count,
			parser_node_count: nodes.len(),
			parser_node_digest: format!("{:x}", digest.finalize()),
			parser_receipt_id: Some(receipt_id),
			syntax_site_count: sites.len(),
			syntax_site_ids_digest: stable_id_set_digest(
				"decodex/lane-authority-v2-source-syntax-sites/1",
				&site_ids,
			),
			zero_syntax_reason_code: None,
		},
		sites,
		edges,
	))
}

fn arguments() -> Result<(PathBuf, PathBuf, PathBuf, PathBuf), String> {
	let mut args = env::args().skip(1);
	let root = args.next().ok_or("missing materialized root")?.into();
	let sources = args.next().ok_or("missing source inventory")?.into();
	let candidates = args.next().ok_or("missing candidate manifest")?.into();
	let output = args.next().ok_or("missing output path")?.into();
	if args.next().is_some() {
		return Err("unexpected extra arguments".to_owned());
	}
	Ok((root, sources, candidates, output))
}

fn run() -> Result<(), String> {
	let (root, source_path, candidate_path, output_path) = arguments()?;
	let inventory: SourceInventory =
		serde_json::from_slice(&fs::read(source_path).map_err(|error| error.to_string())?)
			.map_err(|error| error.to_string())?;
	let candidate_manifest: CandidateManifest =
		serde_json::from_slice(&fs::read(candidate_path).map_err(|error| error.to_string())?)
			.map_err(|error| error.to_string())?;
	let mut candidates_by_source: BTreeMap<String, Vec<&Candidate>> = BTreeMap::new();
	for candidate in &candidate_manifest.records {
		candidates_by_source.entry(candidate.source_node_id.clone()).or_default().push(candidate);
	}
	let mut output = ParseOutput {
		candidate_site_edges: Vec::new(),
		source_nodes: Vec::new(),
		syntax_sites: Vec::new(),
	};
	for source in inventory.records {
		let candidates =
			candidates_by_source.get(&source.source_node_id).map(Vec::as_slice).unwrap_or_default();
		let (parsed, mut sites, mut edges) = parse_source(&root, source, candidates)?;
		output.source_nodes.push(parsed);
		output.syntax_sites.append(&mut sites);
		output.candidate_site_edges.append(&mut edges);
	}
	output
		.source_nodes
		.sort_by(|left, right| left.identity.source_node_id.cmp(&right.identity.source_node_id));
	output.syntax_sites.sort_by(|left, right| left.site_id.cmp(&right.site_id));
	output.candidate_site_edges.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
	fs::write(output_path, serde_json::to_vec(&output).map_err(|error| error.to_string())?)
		.map_err(|error| error.to_string())?;
	Ok(())
}

fn main() {
	if let Err(error) = run() {
		eprintln!("lane-authority-inventory: {error}");
		std::process::exit(1);
	}
}
