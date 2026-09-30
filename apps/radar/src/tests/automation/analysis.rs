use std::{path::Path, process::Command};

use serde_json::Value;

use crate::{
	RUN_CODEX_ANALYSIS_SCRIPT,
	tests::{env::TestEnvVars, fixtures},
};

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
	let cases = analysis_contract_cases();

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
        args = Namespace(allow_ai_analysis_boundary=True, bundle='fixture.json', repo_root=sys.argv[3])
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
        .arg(root)
		.output()
		.expect("Python contracts should execute");

	assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn analysis_helper_checks_resolved_paths_before_reading_bundle() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
	let output = Command::new("python3").arg("-c").arg(r#"
import contextlib, io, sys, tempfile
from pathlib import Path
from argparse import Namespace
from unittest.mock import patch
sys.path.insert(0, sys.argv[1])
from analysis_runner import cli
with tempfile.TemporaryDirectory() as tmp:
    parent = Path(tmp).resolve()
    root = parent / 'repo'
    cache = root / '.agent/automations/radar/cache'
    cache.mkdir(parents=True)
    (cache / 'bundle.json').write_text('{}')
    exported = root / 'target/radar-analysis/run/bundle.json'
    exported.parent.mkdir(parents=True)
    exported.write_text('{}')
    alias = root / 'alias.json'
    alias.symlink_to(cache / 'bundle.json')
    outside = parent / 'outside.json'
    outside.write_text('{}')
    outside_alias = root / 'outside-alias.json'
    outside_alias.symlink_to(outside)
    prefix = root / '.agent/automations/radar/cache-example/bundle.json'
    prefix.parent.mkdir()
    prefix.write_text('{}')
    marker = root / 'automations/radar/skills/github-signal/SKILL.md'
    marker.parent.mkdir(parents=True)
    marker.touch()
    cases = [(alias, 'private Radar cache'), (cache / '../cache/bundle.json', 'private Radar cache'), (outside, 'inside repo root'), (outside_alias, 'inside repo root'), (exported, None), (prefix, None)]
    for bundle, error in cases:
        args = Namespace(allow_ai_analysis_boundary=True, bundle=str(bundle), repo_root=str(root))
        with patch.object(cli, 'parse_args', return_value=args), patch.object(cli, 'load_json', return_value={}) as load, patch.object(cli, 'validate_bundle') as validate, patch.object(cli, 'validate_analysis_draft') as draft, patch.object(cli, 'run_codex_analysis', return_value={}) as analysis:
            validate.return_value.ok = draft.return_value.ok = True
            try:
                with contextlib.redirect_stdout(io.StringIO()): cli.main()
            except SystemExit as exc:
                assert error and error in str(exc), (bundle, str(exc))
            else:
                assert error is None, (bundle, 'accepted prohibited path')
            if error:
                load.assert_not_called()
                analysis.assert_not_called()
            else:
                load.assert_called_once_with(bundle.resolve())
                analysis.assert_called_once_with(args, bundle.resolve(), root)
    from analysis_runner.paths import repo_root_from
    assert repo_root_from(exported) == root
"#).arg(root.join("automations/radar/scripts/github")).output().expect("Python path fixture should execute");

	assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn analysis_runner_preserves_json_fences_and_cleans_temporary_output() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap();
	let output = Command::new("python3").arg("-c").arg(r#"
import json, subprocess, sys, tempfile
from pathlib import Path
from argparse import Namespace
from unittest.mock import patch
sys.path.insert(0, sys.argv[1])
from analysis_runner import command
from analysis_runner.payload import extract_json_payload
payload = {'summary': 'Example ```rust code``` stays intact.'}
for text in [json.dumps(payload), '```json\n' + json.dumps(payload) + '\n```', '```\n' + json.dumps(payload) + '\n```']:
    assert extract_json_payload(text) == payload
for text in ['[]', 'null', '{} {}', '```json\n{}\n``` extra', '```json\n{}', '```json\n{}\n```\n```json\n{}\n```']:
    try:
        extract_json_payload(text)
    except SystemExit:
        pass
    else:
        raise AssertionError(('accepted invalid output', text))
with tempfile.TemporaryDirectory() as tmp:
    root = Path(tmp).resolve()
    bundle = root / 'bundle.json'
    bundle.write_text('{}')
    args = Namespace(codex_bin='unused-codex', model='fixture-model')
    outputs = []
    def run(cmd, **kwargs):
        assert cmd[:4] == ['unused-codex', 'exec', '--model', 'fixture-model']
        assert cmd[cmd.index('--sandbox') + 1] == 'read-only'
        assert '--ephemeral' in cmd
        assert Path(cmd[cmd.index('-C') + 1]) == root
        assert Path(cmd[cmd.index('--output-schema') + 1]).is_file()
        assert 'Analyze the bundle at `bundle.json`.' in cmd[-1]
        assert kwargs == dict(check=False, capture_output=True, text=True)
        path = Path(cmd[cmd.index('-o') + 1])
        outputs.append(path)
        assert path.exists()
        path.write_text('```json\n' + json.dumps(payload) + '\n```')
        return subprocess.CompletedProcess(cmd, 0, '', '')
    with patch.object(command.subprocess, 'run', side_effect=run):
        assert command.run_codex_analysis(args, bundle, root) == payload
    assert all(not p.exists() for p in outputs)
    for stderr, stdout, expected in [('error', 'fallback', 'error'), ('', 'fallback', 'fallback'), ('', '', 'unknown error')]:
        def fail(cmd, **kwargs):
            outputs.append(Path(cmd[cmd.index('-o') + 1]))
            return subprocess.CompletedProcess(cmd, 1, stdout, stderr)
        with patch.object(command.subprocess, 'run', side_effect=fail):
            try: command.run_codex_analysis(args, bundle, root)
            except SystemExit as exc: assert str(exc) == 'codex exec failed: ' + expected
            else: raise AssertionError('failed subprocess accepted')
        assert all(not p.exists() for p in outputs)
    def invalid(cmd, **kwargs):
        path = Path(cmd[cmd.index('-o') + 1]); outputs.append(path)
        path.write_text('not json')
        return subprocess.CompletedProcess(cmd, 0, '', '')
    with patch.object(command.subprocess, 'run', side_effect=invalid):
        try: command.run_codex_analysis(args, bundle, root)
        except SystemExit as exc: assert 'not valid JSON' in str(exc)
        else: raise AssertionError('invalid output accepted')
    assert all(not p.exists() for p in outputs)
"#).arg(root.join("automations/radar/scripts/github")).output().expect("Python command fixture should execute");

	assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn analysis_contract_cases() -> Vec<(&'static str, Value, bool)> {
	let bundle = fixtures::valid_bundle();
	let draft = fixtures::valid_signal();
	let mut cases = Vec::new();

	cases.push(("bundle", bundle.clone(), true));
	cases.push(("draft", draft.clone(), true));

	let mut commit_only = bundle.clone();

	commit_only["analysis_mode"] = serde_json::json!("commit_only");

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
		for invalid in [
			serde_json::json!(true),
			serde_json::json!(-1),
			serde_json::json!(1.5),
			serde_json::json!(9_223_372_036_854_775_808_u64),
		] {
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
		for invalid in [serde_json::json!([]), serde_json::json!({})] {
			let mut value = original.clone();

			value[field] = invalid;

			cases.push((kind, value, false));
		}
	}
	for kind in ["bundle", "draft"] {
		for invalid in [
			Value::Null,
			serde_json::json!([]),
			serde_json::json!(false),
			serde_json::json!("invalid"),
		] {
			cases.push((kind, invalid, false));
		}
	}

	let mut boundaries = bundle.clone();

	boundaries["files"][0]["additions"] = serde_json::json!(0);
	boundaries["files"][0]["deletions"] = serde_json::json!(i64::MAX);
	boundaries["primary_pr"]["number"] = serde_json::json!(i64::MAX);

	cases.push(("bundle", boundaries, true));

	for field in ["title", "summary", "why_it_matters"] {
		let mut value = draft.clone();

		value[field] = serde_json::json!("");

		cases.push(("draft", value, false));
	}
	for invalid in [serde_json::json!(0), serde_json::json!("")] {
		let mut value = bundle.clone();

		value["primary_pr"]["number"] = invalid;

		cases.push(("bundle", value, false));
	}

	let mut invalid_labels = bundle.clone();

	invalid_labels["primary_pr"]["labels"] = serde_json::json!([""]);

	cases.push(("bundle", invalid_labels, false));

	cases
}
