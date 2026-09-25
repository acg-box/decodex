//! Retain the ChatGPT infrastructure routing cookie without retaining account cookies.
use reqwest::{
	Url,
	cookie::{CookieStore, Jar},
	header::HeaderValue,
};

#[derive(Default)]
pub(super) struct RoutingCookies(Jar);

fn allowed_origin(url: &Url) -> bool {
	url.scheme() == "https" && url.host_str() == Some("chatgpt.com")
}

impl CookieStore for RoutingCookies {
	fn set_cookies(&self, headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
		if !allowed_origin(url) {
			return;
		}
		let mut routing = headers.filter(|header| {
			header
				.to_str()
				.ok()
				.and_then(|value| value.split_once('='))
				.is_some_and(|(name, _)| name.trim() == "__oailb")
		});
		self.0.set_cookies(&mut routing, url);
	}

	fn cookies(&self, url: &Url) -> Option<HeaderValue> {
		if !allowed_origin(url) {
			return None;
		}
		let mut header = self.0.cookies(url)?;
		header.set_sensitive(true);
		Some(header)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn routing_cookie_obeys_scope_and_expiration_without_account_cookies() {
		let store = RoutingCookies::default();
		let source = Url::parse("https://chatgpt.com/backend-api/wham/usage").unwrap();
		let headers = [
			HeaderValue::from_static(
				"__oailb=route; Path=/backend-api; Max-Age=3600; Secure; HttpOnly",
			),
			HeaderValue::from_static("session=private; Path=/; Secure"),
			HeaderValue::from_static("__Secure-next-auth.session-token=private; Path=/; Secure"),
		];
		store.set_cookies(&mut headers.iter(), &source);
		let target = Url::parse("https://chatgpt.com/backend-api/wham/profiles/me").unwrap();
		let header = store.cookies(&target).unwrap();
		assert_eq!(header, "__oailb=route");
		assert!(header.is_sensitive());
		for outside in [
			"https://chatgpt.com/",
			"http://chatgpt.com/backend-api/wham/usage",
			"https://other.chatgpt.com/backend-api/wham/usage",
			"https://api.openai.com/backend-api/wham/usage",
		] {
			assert!(store.cookies(&Url::parse(outside).unwrap()).is_none());
		}
		let expired = HeaderValue::from_static("__oailb=; Path=/backend-api; Max-Age=0; Secure");
		store.set_cookies(&mut std::iter::once(&expired), &source);
		assert!(store.cookies(&target).is_none());
	}

	#[test]
	fn foreign_and_insecure_responses_cannot_seed_the_jar() {
		for origin in ["http://chatgpt.com/", "https://other.chatgpt.com/", "https://example.com/"]
		{
			let store = RoutingCookies::default();
			let cookie = HeaderValue::from_static("__oailb=foreign; Domain=chatgpt.com; Path=/");
			store.set_cookies(&mut std::iter::once(&cookie), &Url::parse(origin).unwrap());
			assert!(store.cookies(&Url::parse("https://chatgpt.com/").unwrap()).is_none());
		}
	}
}
