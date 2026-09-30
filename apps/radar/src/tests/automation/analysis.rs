use std::{path::Path, process::Command};

use crate::{RUN_CODEX_ANALYSIS_SCRIPT, tests::env::TestEnvVars};

#[test]
fn analysis_helper_fails_closed_without_explicit_boundary_opt_in() {
	let _env = TestEnvVars::set(&[("DECODEX_ALLOW_CODEX_ANALYSIS", None)]);
	let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.and_then(Path::parent)
		.expect("apps/decodex should live two levels under the repo root");
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let bundle_path = temp_dir.path().join("missing-bundle.json");
	let output = Command::new("python3")
		.current_dir(repo_root)
		.arg(repo_root.join(RUN_CODEX_ANALYSIS_SCRIPT))
		.arg("--bundle")
		.arg(&bundle_path)
		.arg("--repo-root")
		.arg(repo_root)
		.output()
		.expect("Python analysis helper smoke command should execute");
	let stderr = String::from_utf8_lossy(&output.stderr);

	assert!(!output.status.success());
	assert!(
		stderr.contains("requires --allow-ai-analysis-boundary"),
		"unexpected stderr: {stderr}"
	);
}

#[test]
fn python_analysis_contracts_match_rust_for_invalid_json_fields() {
	use serde_json::{Value, json};
	let mut cases = Vec::new();
	let bundle = crate::tests::fixtures::valid_bundle();
	let draft = crate::tests::fixtures::valid_signal();
	cases.push(("bundle", bundle.clone(), true));
	cases.push(("draft", draft.clone(), true));
	let mut commit_only = bundle.clone();
	commit_only["analysis_mode"] = json!("commit_only");
	commit_only.as_object_mut().unwrap().remove("primary_pr");
	cases.push(("bundle", commit_only, true));
	for (kind, original, paths) in [
		(
			"bundle",
			&bundle,
			vec![
				"/files/0/path",
				"/files/0/status",
				"/files/0/additions",
				"/files/0/deletions",
				"/primary_pr/number",
				"/primary_pr/title",
				"/primary_pr/body",
				"/primary_pr/state",
				"/primary_pr/labels",
				"/primary_pr/url",
			],
		),
		("draft", &draft, vec!["/title", "/summary", "/why_it_matters"]),
	] {
		for path in paths {
			let mut value = original.clone();
			*value.pointer_mut(path).unwrap() = Value::Null;
			cases.push((kind, value, false));
		}
	}
	for path in ["/files/0/additions", "/files/0/deletions", "/primary_pr/number"] {
		for invalid in [json!(true), json!(-1), json!(1.5), json!(9_223_372_036_854_775_808_u64)] {
			let mut value = bundle.clone();
			*value.pointer_mut(path).unwrap() = invalid;
			cases.push(("bundle", value, false));
		}
	}
	for (kind, original, field) in [
		("bundle", &bundle, "analysis_mode"),
		("draft", &draft, "kind"),
		("draft", &draft, "confidence"),
		("draft", &draft, "impact"),
	] {
		for invalid in [json!([]), json!({})] {
			let mut value = original.clone();
			value[field] = invalid;
			cases.push((kind, value, false));
		}
	}
	for kind in ["bundle", "draft"] {
		for invalid in [Value::Null, json!([]), json!(false), json!("invalid")] {
			cases.push((kind, invalid, false));
		}
	}

	let mut boundaries = bundle.clone();
	boundaries["files"][0]["additions"] = json!(0);
	boundaries["files"][0]["deletions"] = json!(i64::MAX);
	boundaries["primary_pr"]["number"] = json!(i64::MAX);
	cases.push(("bundle", boundaries, true));
	for field in ["title", "summary", "why_it_matters"] {
		let mut value = draft.clone();
		value[field] = json!("");
		cases.push(("draft", value, false));
	}
	for invalid in [json!(0), json!("")] {
		let mut value = bundle.clone();
		value["primary_pr"]["number"] = invalid;
		cases.push(("bundle", value, false));
	}
	let mut invalid_labels = bundle.clone();
	invalid_labels["primary_pr"]["labels"] = json!([""]);
	cases.push(("bundle", invalid_labels, false));
	for (kind, value, expected) in &cases {
		let actual = if *kind == "bundle" {
			crate::validate_artifact_errors(value).is_empty()
		} else {
			crate::validate_analysis_draft(value).is_ok()
		};
		assert_eq!(actual, *expected, "Rust {kind}: {value}");
	}
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
	let output = Command::new("python3")
		.arg("-c")
		.arg(
			r#"
import json, sys
sys.path.insert(0, sys.argv[1])
from contracts import validate_bundle, validate_analysis_draft
from analysis_runner import cli
from argparse import Namespace
from unittest.mock import patch
cases = json.loads(sys.argv[2])
for kind, value, expected in cases:
    result = (validate_bundle if kind == 'bundle' else validate_analysis_draft)(value)
    assert result.ok == expected, (kind, value, result)
    if kind == 'bundle' and not expected:
        args = Namespace(allow_ai_analysis_boundary=True, bundle='fixture.json')
        with patch.object(cli, 'parse_args', return_value=args), patch.object(cli, 'load_json', return_value=value), patch.object(cli, 'run_codex_analysis') as analysis:
            try:
                cli.main()
            except SystemExit as error:
                assert str(error).startswith('Bundle validation failed:'), str(error)
            else:
                raise AssertionError('invalid bundle reached analysis')
            analysis.assert_not_called()
"#,
		)
		.arg(root.join("automations/radar/scripts/github"))
		.arg(serde_json::to_string(&cases).unwrap())
		.output()
		.expect("Python contracts should execute");
	assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}
