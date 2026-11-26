//! The desktop shortcut editor uses the same GPUI bindings as keyboard dispatch and native menus.
use std::collections::BTreeMap;

use gpui::{Action, App, Global, KeyBinding, Keystroke, OperatingSystem};

gpui::actions!(decodex_shell, [OpenSideChat]);

type Overrides = BTreeMap<String, Vec<String>>;
const PREFERENCE: &str = "DecodexKeyboardShortcuts";

pub(crate) struct Command {
	pub id: String,
	pub label: String,
	pub editable: bool,
	scope: String,
	binding: KeyBinding,
	defaults: Vec<String>,
}
pub(crate) struct Shortcuts {
	os: OperatingSystem,
	pub commands: Vec<Command>,
	overrides: Overrides,
	// Context guards retain GPUI priority over the underlying bindings.
	guards: Vec<KeyBinding>,
	refresh_menu: fn(&App),
}
impl Global for Shortcuts {}

fn words(name: &str) -> String {
	let mut label = String::new();
	for (i, ch) in name.chars().enumerate() {
		if i > 0 && ch.is_uppercase() {
			label.push(' ');
			label.extend(ch.to_lowercase());
		} else {
			label.push(ch);
		}
	}
	label
}

pub(crate) fn initialize(cx: &mut App, refresh_menu: fn(&App)) {
	let os = cx.platform_info().os;
	let mut commands: Vec<Command> = Vec::new();
	let mut guards = Vec::new();
	for binding in cx.key_bindings().borrow().bindings() {
		let action = binding.action();
		if action.as_any().is::<gpui::NoAction>() {
			guards.push(binding.clone());
			continue;
		}
		let scope = binding.predicate().map(|p| p.to_string()).unwrap_or_default();
		let default =
			binding.keystrokes().iter().map(|k| k.unparse(os)).collect::<Vec<_>>().join(" ");
		if let Some(command) =
			commands.iter_mut().find(|c| c.scope == scope && c.binding.action().partial_eq(action))
		{
			command.defaults.push(default);
			continue;
		}
		let name = action.name().rsplit("::").next().unwrap_or(action.name());
		let motion = crate::composer_input::shortcut_motion(action);
		let id = format!("{}:{}:{}", action.name(), motion.as_deref().unwrap_or(""), scope);
		let label = motion.map(|m| words(&m)).unwrap_or_else(|| match name {
			"OpenSideChat" => "New side chat".into(),
			"OpenGlobalCommands" => "Search and commands".into(),
			"ActivateSettings" => "Open settings".into(),
			"ActivateAgent" => "Open workspace".into(),
			"ActivateHealth" => "Open diagnostics".into(),
			"SubmitComposer" => "Send message".into(),
			"InterruptReply" => "Dismiss or interrupt reply".into(),
			"AcceptSuggestion" => "Use prompt suggestion".into(),
			"ToggleInspector" => "Toggle collaboration panel".into(),
			_ => words(name),
		});
		let editable = matches!(
			name,
			"OpenSideChat"
				| "OpenGlobalCommands"
				| "ActivateAgent"
				| "ActivateHealth"
				| "ReloadWorkspace"
				| "ToggleSidebar"
				| "ToggleInspector"
				| "ToggleGraph"
				| "ToggleContentFullscreen"
				| "ShrinkPanel"
				| "GrowPanel"
				| "ResetPanel"
				| "ShrinkPanels"
				| "GrowPanels"
				| "ResetPanels"
				| "MoveAccountUp"
				| "MoveAccountDown"
		);
		commands.push(Command {
			id,
			label,
			scope,
			binding: binding.clone(),
			defaults: vec![default],
			editable,
		});
	}
	let mut overrides = crate::ui_preferences::string(PREFERENCE, None)
		.and_then(|value| serde_json::from_str::<Overrides>(&value).ok())
		.unwrap_or_default();
	// Older preferences must never shadow a platform binding.
	overrides.retain(|id, keys| {
		commands.iter().find(|c| &c.id == id && c.editable).is_some_and(|c| {
			keys.iter().all(|k| {
				!commands.iter().any(|other| {
					!other.editable && other.defaults.contains(k) && overlaps(c, other)
				})
			})
		})
	});
	cx.set_global(Shortcuts { commands, overrides, guards, refresh_menu, os });
	apply(cx);
}
fn overlaps(a: &Command, b: &Command) -> bool {
	match (a.binding.predicate(), b.binding.predicate()) {
		(Some(a), Some(b)) => a.is_superset(&b) || b.is_superset(&a),
		_ => true,
	}
}

pub(crate) fn search_key(text: &str, os: OperatingSystem) -> Option<String> {
	let raw = text
		.trim()
		.to_lowercase()
		.replace("command", "cmd")
		.replace("control", "ctrl")
		.replace("option", "alt")
		.replace('⌘', "cmd-")
		.replace('⌥', "alt-")
		.replace('⇧', "shift-")
		.replace('⌃', "ctrl-")
		.replace('⇥', "tab")
		.replace('↩', "enter")
		.replace('⎋', "escape");
	let raw = if raw == "+" { raw } else { raw.replace('+', "-") };
	Keystroke::parse_with_os(&raw, os).ok().map(|k| k.unparse(os))
}
impl Shortcuts {
	pub fn search(&self, query: &str) -> Vec<usize> {
		let query = query.trim().to_lowercase();
		let key = search_key(&query, self.os);
		(0..self.commands.len())
			.filter(|&i| {
				let name = self.commands[i].label.to_lowercase();
				query.split_whitespace().all(|word| name.contains(word))
					|| key.as_ref().is_some_and(|k| self.keys(i).contains(k))
					|| self.keys(i).iter().any(|k| display(k, self.os).to_lowercase() == query)
			})
			.collect()
	}

	pub fn keys(&self, i: usize) -> &[String] {
		self.overrides.get(&self.commands[i].id).unwrap_or(&self.commands[i].defaults)
	}

	pub fn changed(&self, i: usize) -> bool {
		self.overrides.contains_key(&self.commands[i].id)
	}

	pub fn owners(&self, key: &str) -> Vec<usize> {
		(0..self.commands.len()).filter(|i| self.keys(*i).iter().any(|k| k == key)).collect()
	}

	pub fn conflicts(&self, i: usize, key: &str) -> Vec<usize> {
		self.owners(key)
			.into_iter()
			.filter(|&j| j != i && overlaps(&self.commands[i], &self.commands[j]))
			.collect()
	}

	pub fn defaults(&self, i: usize) -> &[String] {
		&self.commands[i].defaults
	}

	fn set(&mut self, i: usize, keys: Vec<String>) {
		let command = &self.commands[i];
		if keys == command.defaults {
			self.overrides.remove(&command.id);
		} else {
			self.overrides.insert(command.id.clone(), keys);
		}
	}
}

pub(crate) fn assign(i: usize, keys: Vec<String>, cx: &mut App) {
	let settings = cx.global_mut::<Shortcuts>();
	if !settings.commands[i].editable
		|| keys
			.iter()
			.flat_map(|k| settings.conflicts(i, k))
			.any(|j| !settings.commands[j].editable)
	{
		return;
	}
	for key in &keys {
		for owner in settings.conflicts(i, key) {
			let remaining = settings.keys(owner).iter().filter(|s| *s != key).cloned().collect();
			settings.set(owner, remaining);
		}
	}
	settings.set(i, keys);
	let serialized = serde_json::to_string(&settings.overrides).expect("string key bindings");
	crate::ui_preferences::string(PREFERENCE, Some(&serialized));
	apply(cx);
}
fn apply(cx: &mut App) {
	let settings = cx.global::<Shortcuts>();
	let mut bindings = Vec::new();
	for (i, cmd) in settings.commands.iter().enumerate() {
		for key in settings.keys(i) {
			if let Ok(binding) = KeyBinding::load(
				key,
				cmd.binding.action().boxed_clone(),
				cmd.binding.predicate(),
				false,
				None,
				cx,
			) {
				bindings.push(binding);
			}
		}
	}
	bindings.extend(settings.guards.iter().cloned());
	let refresh = settings.refresh_menu;
	cx.clear_key_bindings();
	cx.bind_keys(bindings);
	refresh(cx);
}
pub(crate) fn key(action: &dyn Action, cx: &App) -> Option<Keystroke> {
	use gpui::AsKeystroke as _;
	cx.key_bindings()
		.borrow()
		.bindings_for_action(action)
		.next()?
		.keystrokes()
		.first()
		.map(|k| k.as_keystroke().clone())
}
pub(crate) fn matches(action: &dyn Action, stroke: &Keystroke, cx: &App) -> bool {
	cx.key_bindings()
		.borrow()
		.bindings_for_action(action)
		.any(|b| b.match_keystrokes(std::slice::from_ref(stroke)) == Some(false))
}
pub(crate) fn hint(label: &str, action: &dyn Action, cx: &App) -> String {
	key(action, cx).map_or_else(
		|| label.to_owned(),
		|k| format!("{label} ({})", k.display(cx.platform_info().os)),
	)
}
pub(crate) fn display(key: &str, os: OperatingSystem) -> String {
	Keystroke::parse_with_os(key, os)
		.map(|k| k.display(os).to_string())
		.unwrap_or_else(|_| key.into())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::shell::{OpenGlobalCommands, OpenSideChat};
	#[gpui::test]
	fn reassign_and_restore_update_the_live_keymap(cx: &mut gpui::TestAppContext) {
		cx.update(|cx| {
			crate::shell::bind_keys(cx);
			initialize(cx, |_| {});
			let map = cx.global::<Shortcuts>();
			let side = map
				.commands
				.iter()
				.position(|c| c.binding.action().partial_eq(&OpenSideChat))
				.unwrap();
			let palette = map
				.commands
				.iter()
				.position(|c| c.binding.action().partial_eq(&OpenGlobalCommands))
				.unwrap();
			assert_eq!(map.conflicts(side, "cmd-k"), [palette]);
			let settings = map.search("open settings")[0];
			assert_eq!(map.search("Command+,"), [settings]);
			assert_eq!(map.search("⌘,"), [settings]);
			assert!(map.search("side chat").contains(&side));
			assert!(!map.commands[settings].editable);
			assign(side, vec!["cmd-,".into()], cx);
			assign(settings, vec![], cx);
			assert!(matches(
				&crate::shell::ActivateSettings,
				&Keystroke::parse("cmd-,").unwrap(),
				cx
			));
			assert!(!matches(&OpenSideChat, &Keystroke::parse("cmd-,").unwrap(), cx));
			assign(side, vec!["cmd-k".into()], cx);
			assert!(matches(&OpenSideChat, &Keystroke::parse("cmd-k").unwrap(), cx));
			assert!(key(&OpenGlobalCommands, cx).is_none());
			assert!(!matches(&OpenSideChat, &Keystroke::parse("alt-cmd-s").unwrap(), cx));
			let defaults = cx.global::<Shortcuts>().defaults(side).to_vec();
			assign(side, defaults, cx);
			assert!(matches(&OpenSideChat, &Keystroke::parse("alt-cmd-s").unwrap(), cx));
		});
	}
}
