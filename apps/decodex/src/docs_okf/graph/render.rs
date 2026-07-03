use crate::docs_okf::{OkfGraph, Path, Result};

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
