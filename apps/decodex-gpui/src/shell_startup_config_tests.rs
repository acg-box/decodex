//! Native diagnostics stay explicit, keep their source and do not promote service health.
use gpui::{AppContext as _, Entity, IntoElement, Render, TestAppContext, Window};

use super::{Context, Shell, Source, content, source};
use crate::client_lifecycle::ConnectionView;
use decodex_protocol::{
	NativeStartupConfigStatus, NativeStartupConfiguration, NativeStartupLoginMethod,
};

struct NativeConfigurationView(Entity<Shell>);
impl Render for NativeConfigurationView {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.0.update(cx, |shell, cx| content(shell, cx))
	}
}

#[gpui::test]
fn startup_diagnostics_open_without_running_copy_the_safe_result_and_hide_an_old_source(
	cx: &mut TestAppContext,
) {
	use std::{fs, os::unix::fs::PermissionsExt as _};
	let fixture = tempfile::tempdir_in("/tmp").expect("local profile fixture");
	let root = fixture.path().canonicalize().expect("fixture root");
	fs::create_dir(root.join("server")).expect("private namespace");
	fs::set_permissions(root.join("server"), fs::Permissions::from_mode(0o700))
		.expect("private namespace permissions");
	fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("private root");
	let config = root.join("config.toml");
	fs::write(&config,format!("version=1\nactive_profile='local'\n[cache]\n[profiles.local]\nkind='local'\npolicy='same_uid'\nservice_owner_uid={}\nexpected_server_identity='20000000-0000-4000-8000-000000000001'\n",unsafe{libc::geteuid()})).expect("local profile");
	fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).expect("private profile");
	let profile = decodex_protocol::ClientProfile::load(&root, None).expect("local client");
	let (view, visual) = cx.add_window_view(|window, cx| {
		NativeConfigurationView(cx.new(|cx| Shell::new(window, cx, ConnectionView::Stopped)))
	});
	view.update(visual, |v, cx| {
		v.0.update(cx, |shell, cx| {
			shell.reset_cards.profile = Some(profile);
			cx.notify();
		})
	});
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(visual.debug_bounds("native-startup-check").is_some());
	assert!(visual.debug_bounds("native-startup-result").is_none());
	view.update(visual, |v, cx| {
		v.0.update(cx, |shell, _| {
			assert!(shell.diagnostics.startup.task.is_none());
			assert!(shell.diagnostics.startup.result.is_none());
		})
	});
	let (expected, health) = view.update(visual, |v, cx| {
		v.0.update(cx, |shell, cx| {
			let owner = source(shell, cx).expect("local fixture source");
			let result = NativeStartupConfiguration {
				directory: Some(owner.directory.to_string_lossy().into_owned()),
				codex_sha256: Some("a".repeat(64)),
				status: NativeStartupConfigStatus::Loaded {
					version: "0.0.0".into(),
					login_methods: Some(vec![NativeStartupLoginMethod::Api]),
					requirements_present: true,
				},
			};
			let expected = result.report_text();
			shell.diagnostics.startup.source = Some(owner);
			shell.diagnostics.startup.result = Some(result);
			cx.notify();
			(expected, (shell.health.load, shell.health.report.clone()))
		})
	});
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(visual.debug_bounds("native-startup-result").is_some());
	let copy = visual.debug_bounds("native-startup-copy").expect("copy safe configuration result");
	visual.simulate_click(copy.center(), Default::default());
	visual.update(|_, cx| {
		assert_eq!(
			cx.read_from_clipboard().expect("clipboard").text().expect("safe report"),
			expected
		)
	});
	view.update(visual, |v, cx| {
		v.0.update(cx, |shell, cx| {
			assert_eq!(
				(shell.health.load, shell.health.report.clone()),
				health,
				"config success is not service health"
			);
			let owner = shell.diagnostics.startup.source.as_ref().expect("captured source");
			shell.diagnostics.startup.source = Some(Source {
				directory: owner.directory.join("different-folder"),
				profile: owner.profile.clone(),
			});
			cx.notify();
		})
	});
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(visual.debug_bounds("native-startup-result").is_none());
	assert!(visual.debug_bounds("native-startup-copy").is_none());
}
