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
	receiver_type_evidence: Option<String>,
	receiver_type_signature: Option<String>,
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
		("swift", "call_expression") => {
			node.child_by_field_name("called_expression").or_else(|| {
				let mut cursor = node.walk();
				node.named_children(&mut cursor).next()
			})
		},
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
			("python", "class_definition") | ("swift", "class_declaration") => {
				current.child_by_field_name("name")
			},
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
		receiver_type_evidence: None,
		receiver_type_signature: None,
		resolution_hint,
		role: role.to_owned(),
		signature_digest: sha256(&[signature.as_bytes()]),
		signature,
		source_node_id: source_id.to_owned(),
		syntax_site_id: site_id(source_id, site),
	}
}

fn register_rust_import(
	imports: &mut BTreeMap<String, Option<String>>,
	alias: String,
	canonical: String,
) {
	match imports.get(&alias) {
		None => {
			imports.insert(alias, Some(canonical));
		},
		Some(Some(existing)) if existing == &canonical => {},
		Some(_) => {
			imports.insert(alias, None);
		},
	}
}

fn collect_rust_use_node(
	bytes: &[u8],
	node: Node<'_>,
	prefix: Option<&str>,
	imports: &mut BTreeMap<String, Option<String>>,
) {
	match node.kind() {
		"scoped_use_list" => {
			let Some(path) = node.child_by_field_name("path") else { return };
			let path = path.utf8_text(bytes).unwrap_or_default().trim();
			let canonical_prefix = match prefix {
				Some(prefix) => format!("{prefix}::{path}"),
				None => path.to_owned(),
			};
			if let Some(list) = node.child_by_field_name("list") {
				collect_rust_use_node(bytes, list, Some(&canonical_prefix), imports);
			}
		},
		"use_list" => {
			let mut cursor = node.walk();
			for child in node.named_children(&mut cursor) {
				collect_rust_use_node(bytes, child, prefix, imports);
			}
		},
		"use_as_clause" => {
			let Some(path) = node.child_by_field_name("path") else { return };
			let Some(alias) = node.child_by_field_name("alias") else { return };
			let path = path.utf8_text(bytes).unwrap_or_default().trim();
			let alias = alias.utf8_text(bytes).unwrap_or_default().trim();
			if alias.is_empty() || path.is_empty() {
				return;
			}
			let canonical = match prefix {
				Some(prefix) => format!("{prefix}::{path}"),
				None => path.to_owned(),
			};
			register_rust_import(imports, alias.to_owned(), canonical);
		},
		"identifier" | "type_identifier" | "scoped_identifier" => {
			let text = node.utf8_text(bytes).unwrap_or_default().trim();
			if text.is_empty() || text == "self" || text == "*" {
				return;
			}
			let canonical = match prefix {
				Some(prefix) => format!("{prefix}::{text}"),
				None => text.to_owned(),
			};
			let alias = text.rsplit("::").next().unwrap_or(text).to_owned();
			register_rust_import(imports, alias, canonical);
		},
		_ => {},
	}
}

fn rust_imports(bytes: &[u8], nodes: &[Node<'_>]) -> BTreeMap<String, Option<String>> {
	let mut imports = BTreeMap::new();
	for node in nodes.iter().filter(|node| node.kind() == "use_declaration") {
		if let Some(argument) = node.child_by_field_name("argument") {
			collect_rust_use_node(bytes, argument, None, &mut imports);
		}
	}
	imports
}

fn rust_base_type_node(mut node: Node<'_>) -> Option<Node<'_>> {
	loop {
		match node.kind() {
			"reference_type" | "generic_type" => node = node.child_by_field_name("type")?,
			"identifier" | "type_identifier" | "scoped_type_identifier" => return Some(node),
			_ => return None,
		}
	}
}

fn canonical_rust_type(
	bytes: &[u8],
	type_node: Node<'_>,
	imports: &BTreeMap<String, Option<String>>,
) -> Option<String> {
	let node = rust_base_type_node(type_node)?;
	let signature = node.utf8_text(bytes).ok()?.trim();
	if signature.is_empty() {
		return None;
	}
	if signature.contains("::") {
		return Some(signature.to_owned());
	}
	match imports.get(signature) {
		Some(Some(canonical)) => Some(canonical.clone()),
		Some(None) => None,
		None => Some(signature.to_owned()),
	}
}

fn rust_struct_field_types(
	bytes: &[u8],
	nodes: &[Node<'_>],
	imports: &BTreeMap<String, Option<String>>,
) -> BTreeMap<(String, String), String> {
	let mut fields = BTreeMap::new();
	for node in nodes.iter().filter(|node| node.kind() == "field_declaration") {
		let Some(name) = node.child_by_field_name("name") else { continue };
		let Some(type_node) = node.child_by_field_name("type") else { continue };
		let Some(canonical_type) = canonical_rust_type(bytes, type_node, imports) else {
			continue;
		};
		let Some(struct_item) = node.parent().and_then(|parent| parent.parent()) else { continue };
		if struct_item.kind() != "struct_item" {
			continue;
		}
		let Some(owner) = struct_item.child_by_field_name("name") else { continue };
		fields.insert(
			(
				owner.utf8_text(bytes).unwrap_or_default().trim().to_owned(),
				name.utf8_text(bytes).unwrap_or_default().trim().to_owned(),
			),
			canonical_type,
		);
	}
	fields
}

fn enclosing_rust_function(node: Node<'_>) -> Option<Node<'_>> {
	let mut current = node.parent();
	while let Some(ancestor) = current {
		if ancestor.kind() == "function_item" {
			return Some(ancestor);
		}
		current = ancestor.parent();
	}
	None
}

fn explicit_rust_binding_type(
	bytes: &[u8],
	call: Node<'_>,
	name: &str,
	nodes: &[Node<'_>],
	imports: &BTreeMap<String, Option<String>>,
) -> Option<(String, String)> {
	let function = enclosing_rust_function(call)?;
	let mut matches = nodes
		.iter()
		.copied()
		.filter(|node| {
			node.start_byte() >= function.start_byte()
				&& node.end_byte() <= function.end_byte()
				&& node.start_byte() < call.start_byte()
				&& matches!(node.kind(), "parameter" | "let_declaration")
		})
		.filter_map(|node| {
			let pattern = node.child_by_field_name("pattern")?;
			(pattern.utf8_text(bytes).ok()?.trim() == name).then_some(node)
		})
		.collect::<Vec<_>>();
	matches.sort_by_key(Node::start_byte);
	let binding = matches.pop()?;
	let type_node = binding.child_by_field_name("type")?;
	let canonical = canonical_rust_type(bytes, type_node, imports)?;
	let evidence = match binding.kind() {
		"parameter" => "explicit_parameter_type",
		"let_declaration" => "explicit_local_type",
		_ => return None,
	};
	Some((canonical, evidence.to_owned()))
}

fn rust_receiver_type(
	bytes: &[u8],
	call: Node<'_>,
	target: Node<'_>,
	nodes: &[Node<'_>],
	imports: &BTreeMap<String, Option<String>>,
	struct_fields: &BTreeMap<(String, String), String>,
) -> Option<(String, String, String)> {
	if target.kind() != "field_expression" {
		return None;
	}
	let receiver = target.child_by_field_name("value")?;
	let method = target.child_by_field_name("field")?.utf8_text(bytes).ok()?.trim().to_owned();
	if method.is_empty() {
		return None;
	}
	let (receiver_type, evidence) = if receiver.kind() == "identifier" {
		let name = receiver.utf8_text(bytes).ok()?.trim();
		explicit_rust_binding_type(bytes, call, name, nodes, imports)?
	} else if receiver.kind() == "field_expression" {
		let base = receiver.child_by_field_name("value")?;
		if base.kind() != "self" {
			return None;
		}
		let field = receiver.child_by_field_name("field")?.utf8_text(bytes).ok()?.trim();
		let owner = enclosing_owner_signature(bytes, "rust", call)?;
		let receiver_type = struct_fields.get(&(owner, field.to_owned()))?.clone();
		(receiver_type, "enclosing_struct_field_type".to_owned())
	} else {
		return None;
	};
	Some((format!("{receiver_type}::{method}"), receiver_type, evidence))
}

fn semantic_call_fact(
	bytes: &[u8],
	source_id: &str,
	call: Node<'_>,
	target: Node<'_>,
	language: &str,
	nodes: &[Node<'_>],
	imports: &BTreeMap<String, Option<String>>,
	struct_fields: &BTreeMap<(String, String), String>,
) -> SemanticSymbolFact {
	let mut fact = semantic_symbol_fact(bytes, source_id, call, target, "call_target", language);
	if language == "rust" {
		if let Some((signature, receiver_type, evidence)) =
			rust_receiver_type(bytes, call, target, nodes, imports, struct_fields)
		{
			fact.signature_digest = sha256(&[signature.as_bytes()]);
			fact.signature = signature;
			fact.resolution_hint = "qualified".to_owned();
			fact.receiver_type_evidence = Some(evidence);
			fact.receiver_type_signature = Some(receiver_type);
		}
	}
	fact
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
	let rust_imports =
		if source.language == "rust" { rust_imports(&bytes, &nodes) } else { BTreeMap::new() };
	let rust_struct_fields = if source.language == "rust" {
		rust_struct_field_types(&bytes, &nodes, &rust_imports)
	} else {
		BTreeMap::new()
	};
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
			semantic_symbol_facts.push(semantic_call_fact(
				&bytes,
				&source.source_node_id,
				node,
				target,
				&source.language,
				&nodes,
				&rust_imports,
				&rust_struct_fields,
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
		call_target_node, classify_symbol_signature, collect_nodes, declaration_name_node,
		enclosing_owner_signature, language, rust_imports, rust_receiver_type,
		rust_struct_field_types,
	};
	use tree_sitter::Parser;

	#[test]
	fn exposes_rust_receiver_type_ast_shapes() {
		let source = b"use rusqlite::{Connection, Row}; struct Store { connection: Connection } impl Store { fn read(row: &Row<'_>) { let local: Connection = todo!(); row.get(0); self.connection.prepare(\"x\"); local.execute(\"x\", []); } }";
		let mut parser = Parser::new();
		parser.set_language(&language("rust").expect("Rust grammar")).expect("set Rust grammar");
		let tree = parser.parse(source, None).expect("Rust syntax tree");
		let nodes = collect_nodes(tree.root_node());
		let imports = rust_imports(source, &nodes);
		let fields = rust_struct_field_types(source, &nodes, &imports);
		assert_eq!(Some(&Some("rusqlite::Connection".to_owned())), imports.get("Connection"));
		assert_eq!(Some(&Some("rusqlite::Row".to_owned())), imports.get("Row"));
		let resolved = nodes
			.iter()
			.copied()
			.filter(|node| node.kind() == "call_expression")
			.filter_map(|call| {
				let target = call_target_node("rust", call)?;
				rust_receiver_type(source, call, target, &nodes, &imports, &fields)
			})
			.collect::<Vec<_>>();
		assert_eq!(
			vec![
				(
					"rusqlite::Row::get".to_owned(),
					"rusqlite::Row".to_owned(),
					"explicit_parameter_type".to_owned(),
				),
				(
					"rusqlite::Connection::prepare".to_owned(),
					"rusqlite::Connection".to_owned(),
					"enclosing_struct_field_type".to_owned(),
				),
				(
					"rusqlite::Connection::execute".to_owned(),
					"rusqlite::Connection".to_owned(),
					"explicit_local_type".to_owned(),
				),
			],
			resolved
		);
	}

	#[test]
	fn rejects_ambiguous_rust_receiver_type_imports() {
		let source = b"use one::Row; use two::Row; fn read(row: &Row) { row.get(0); }";
		let mut parser = Parser::new();
		parser.set_language(&language("rust").expect("Rust grammar")).expect("set Rust grammar");
		let tree = parser.parse(source, None).expect("Rust syntax tree");
		let nodes = collect_nodes(tree.root_node());
		let imports = rust_imports(source, &nodes);
		let fields = rust_struct_field_types(source, &nodes, &imports);
		let call =
			nodes.iter().copied().find(|node| node.kind() == "call_expression").expect("call");
		let target = call_target_node("rust", call).expect("call target");
		assert_eq!(None, rust_receiver_type(source, call, target, &nodes, &imports, &fields));
	}

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
