use super::conversation_url;

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
