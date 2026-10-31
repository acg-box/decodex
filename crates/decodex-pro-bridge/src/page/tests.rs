use serde_json::json;

use super::{bound_instance, conversation_url, select_browser};

#[test]
fn recovery_accepts_only_exact_canonical_chatgpt_conversations() {
	assert_eq!(
		conversation_url("https://chatgpt.com/c/owned-test").expect("owned"),
		"https://chatgpt.com/c/owned-test"
	);
	for url in [
		"https://chatgpt.com.evil.test/c/one",
		"http://chatgpt.com/c/one",
		"https://user@chatgpt.com/c/one",
		"https://chatgpt.com/c/one?model=pro",
		"https://chatgpt.com/c/one#part",
		"https://chatgpt.com:443/c/one",
		"https://chatgpt.com/",
	] {
		assert!(conversation_url(url).is_err(), "{url}");
	}
}

#[test]
fn multiple_browsers_need_an_explicit_instance_instead_of_an_arbitrary_first_profile() {
	let browsers = vec![
		json!({"id":"1","type":"extension","family":"chrome","profileName":"Your Chrome","metadata":{"extensionInstanceId":"first"}}),
		json!({"id":"2","type":"extension","family":"chrome","metadata":{"extensionInstanceId":"second"}}),
	];
	assert!(select_browser(&browsers, None).is_err());
	assert_eq!(select_browser(&browsers, Some("second")).expect("explicit")["id"], "2");
}

#[test]
fn recovery_keeps_the_saved_profile_when_inventory_ids_or_defaults_change() {
	let browsers = vec![
		json!({"id":"1","type":"extension","family":"chrome","profileName":"Your Chrome","metadata":{"extensionInstanceId":"other"}}),
		json!({"id":"9","type":"extension","family":"chrome","metadata":{"extensionInstanceId":"saved"}}),
	];
	let instance = bound_instance(None, Some("saved")).expect("saved binding");
	assert_eq!(select_browser(&browsers, instance).expect("same instance")["id"], "9");
	assert!(select_browser(&browsers[..1], instance).is_err());
	assert!(bound_instance(Some("other"), Some("saved")).is_err());
	assert_eq!(bound_instance(Some("saved"), Some("saved")).expect("same"), Some("saved"));
}
