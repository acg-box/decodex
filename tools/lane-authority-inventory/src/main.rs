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
struct SemanticSymbolFact {
	language: String,
	owner_signature: Option<String>,
	resolution_hint: String,
	role: String,
	signature: String,
	signature_digest: String,
	source_node_id: String,
	syntax_site_id: String,
}

#[derive(Serialize)]
struct ParseOutput {
	candidate_site_edges: Vec<CandidateSiteEdge>,
	semantic_symbol_facts: Vec<SemanticSymbolFact>,
	source_nodes: Vec<ParsedSource>,
	syntax_sites: Vec<SyntaxSite>,
}

struct ParsedSourceOutput {
	candidate_site_edges: Vec<CandidateSiteEdge>,
	semantic_symbol_facts: Vec<SemanticSymbolFact>,
	source: ParsedSource,
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

fn declaration_name_node<'tree>(language: &str, node: Node<'tree>) -> Option<Node<'tree>> {
	let declaration = match language {
		"python" => matches!(node.kind(), "class_definition" | "function_definition"),
		"rust" => matches!(
			node.kind(),
			"enum_item" | "function_item" | "struct_item" | "trait_item" | "type_item"
		),
		"shell" => node.kind() == "function_definition",
		"swift" => matches!(node.kind(), "class_declaration" | "function_declaration"),
		_ => false,
	};
	declaration.then(|| node.child_by_field_name("name")).flatten()
}

fn call_target_node<'tree>(language: &str, node: Node<'tree>) -> Option<Node<'tree>> {
	match (language, node.kind()) {
		("rust", "call_expression") | ("python", "call") => node.child_by_field_name("function"),
		("rust", "macro_invocation") => node.child_by_field_name("macro"),
		("swift", "call_expression") =>
			node.child_by_field_name("called_expression").or_else(|| {
				let mut cursor = node.walk();
				node.named_children(&mut cursor).next()
			}),
		("shell", "command") => {
			let mut cursor = node.walk();
			node.named_children(&mut cursor).next()
		},
		_ => None,
	}
}

fn classify_symbol_signature(text: &str, node_kind: &str) -> (String, String) {
	let exact = !text.is_empty()
		&& text.len() <= 256
		&& text.chars().all(|character| character.is_alphanumeric() || character == '_');
	let qualified = !text.is_empty()
		&& text.len() <= 256
		&& text.chars().all(|character| {
			character.is_alphanumeric()
				|| matches!(character, '_' | ':' | '.' | '!' | '#' | '$' | '-')
		});
	if exact {
		(text.to_owned(), "exact".to_owned())
	} else if qualified {
		(text.to_owned(), "qualified".to_owned())
	} else {
		(format!("<dynamic:{node_kind}>"), "dynamic".to_owned())
	}
}

fn symbol_signature(bytes: &[u8], node: Node<'_>) -> (String, String) {
	classify_symbol_signature(node.utf8_text(bytes).unwrap_or_default().trim(), node.kind())
}

fn enclosing_owner_signature(bytes: &[u8], language: &str, node: Node<'_>) -> Option<String> {
	let mut ancestor = node.parent();
	while let Some(current) = ancestor {
		let owner = match (language, current.kind()) {
			("rust", "impl_item") => current.child_by_field_name("type"),
			("python", "class_definition") | ("swift", "class_declaration") =>
				current.child_by_field_name("name"),
			_ => None,
		};
		if let Some(owner) = owner {
			let (signature, hint) = symbol_signature(bytes, owner);
			return (hint != "dynamic").then_some(signature);
		}
		ancestor = current.parent();
	}
	None
}

fn semantic_symbol_fact(
	bytes: &[u8],
	source_id: &str,
	site: Node<'_>,
	symbol: Node<'_>,
	role: &str,
	language: &str,
) -> SemanticSymbolFact {
	let (signature, resolution_hint) = symbol_signature(bytes, symbol);
	SemanticSymbolFact {
		language: language.to_owned(),
		owner_signature: enclosing_owner_signature(bytes, language, site),
		resolution_hint,
		role: role.to_owned(),
		signature_digest: sha256(&[signature.as_bytes()]),
		signature,
		source_node_id: source_id.to_owned(),
		syntax_site_id: site_id(source_id, site),
	}
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
) -> Result<ParsedSourceOutput, String> {
	if source.status == "deleted" {
		return Ok(ParsedSourceOutput {
			candidate_site_edges: Vec::new(),
			semantic_symbol_facts: Vec::new(),
			source: ParsedSource {
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
			syntax_sites: Vec::new(),
		});
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
		if *node == tree.root_node()
			|| materialize_kind(node.kind())
			|| declaration_name_node(&source.language, *node).is_some()
		{
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
	let mut semantic_symbol_facts = Vec::new();
	for node in selected.values().copied() {
		if let Some(name) = declaration_name_node(&source.language, node) {
			semantic_symbol_facts.push(semantic_symbol_fact(
				&bytes,
				&source.source_node_id,
				node,
				name,
				"declaration",
				&source.language,
			));
		}
		if let Some(target) = call_target_node(&source.language, node) {
			semantic_symbol_facts.push(semantic_symbol_fact(
				&bytes,
				&source.source_node_id,
				node,
				target,
				"call_target",
				&source.language,
			));
		}
	}
	let receipt_id = format!("tool:{}:parser:common", source.language);
	Ok(ParsedSourceOutput {
		candidate_site_edges: edges,
		semantic_symbol_facts,
		source: ParsedSource {
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
		syntax_sites: sites,
	})
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
		semantic_symbol_facts: Vec::new(),
		source_nodes: Vec::new(),
		syntax_sites: Vec::new(),
	};
	for source in inventory.records {
		let candidates =
			candidates_by_source.get(&source.source_node_id).map(Vec::as_slice).unwrap_or_default();
		let mut parsed = parse_source(&root, source, candidates)?;
		output.source_nodes.push(parsed.source);
		output.syntax_sites.append(&mut parsed.syntax_sites);
		output.candidate_site_edges.append(&mut parsed.candidate_site_edges);
		output.semantic_symbol_facts.append(&mut parsed.semantic_symbol_facts);
	}
	output
		.source_nodes
		.sort_by(|left, right| left.identity.source_node_id.cmp(&right.identity.source_node_id));
	output.syntax_sites.sort_by(|left, right| left.site_id.cmp(&right.site_id));
	output.candidate_site_edges.sort_by(|left, right| left.candidate_id.cmp(&right.candidate_id));
	output.semantic_symbol_facts.sort_by(|left, right| {
		left.syntax_site_id
			.cmp(&right.syntax_site_id)
			.then_with(|| left.role.cmp(&right.role))
			.then_with(|| left.signature.cmp(&right.signature))
	});
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

#[cfg(test)]
mod tests {
	use super::{
		classify_symbol_signature, collect_nodes, declaration_name_node, enclosing_owner_signature,
		language,
	};
	use tree_sitter::Parser;

	#[test]
	fn classifies_exact_and_qualified_symbol_signatures() {
		assert_eq!(
			("dispatch".to_owned(), "exact".to_owned()),
			classify_symbol_signature("dispatch", "identifier")
		);
		assert_eq!(
			("runtime::dispatch".to_owned(), "qualified".to_owned()),
			classify_symbol_signature("runtime::dispatch", "scoped_identifier")
		);
	}

	#[test]
	fn dynamic_symbol_signature_does_not_retain_source_text() {
		assert_eq!(
			("<dynamic:closure_expression>".to_owned(), "dynamic".to_owned()),
			classify_symbol_signature("factory()(secret)", "closure_expression")
		);
	}

	#[test]
	fn extracts_rust_impl_owner_for_method_declarations_and_calls() {
		let source = b"struct StateStore; impl StateStore { fn open() { Self::open(); } }";
		let mut parser = Parser::new();
		parser.set_language(&language("rust").expect("Rust grammar")).expect("set Rust grammar");
		let tree = parser.parse(source, None).expect("Rust syntax tree");
		let nodes = collect_nodes(tree.root_node());
		let owned_nodes: Vec<_> = nodes
			.iter()
			.filter(|node| matches!(node.kind(), "function_item" | "call_expression"))
			.collect();
		assert_eq!(2, owned_nodes.len());
		for node in owned_nodes {
			assert_eq!(
				Some("StateStore".to_owned()),
				enclosing_owner_signature(source, "rust", *node)
			);
		}
	}

	#[test]
	fn includes_type_declarations_in_the_symbol_universe() {
		for (language_name, source, declaration_kind, expected_name) in [
			("python", "class Store:\n  pass\n", "class_definition", "Store"),
			("rust", "struct Store;", "struct_item", "Store"),
			("swift", "struct Store {}", "class_declaration", "Store"),
		] {
			let mut parser = Parser::new();
			parser
				.set_language(&language(language_name).expect("language grammar"))
				.expect("set language grammar");
			let tree = parser.parse(source, None).expect("syntax tree");
			let declaration = collect_nodes(tree.root_node())
				.into_iter()
				.find(|node| node.kind() == declaration_kind)
				.expect("type declaration");
			let name = declaration_name_node(language_name, declaration).expect("declaration name");
			assert_eq!(expected_name, name.utf8_text(source.as_bytes()).expect("UTF-8 name"));
		}
	}
}
