//! Retry a one-time code only after a connection failure, before any HTTP response.
#[cfg(target_os = "macos")]
#[path = "system_proxy_macos.rs"]
mod macos;

use std::{
	future::Future,
	time::{Duration, Instant},
};

use reqwest::{Client, Proxy, Response, redirect::Policy};
use tokio::time;

use crate::{Cancellation, Config, Error};

pub(super) enum Route {
	Direct,
	Proxy(Box<Proxy>),
}

#[cfg(target_os = "macos")]
enum RouteFailureClass {
	InvalidProxyConfig,
	ProxyResolutionUnavailable,
	UnsupportedProxyScheme,
	ConnectTimeout,
}

#[cfg(target_os = "macos")]
enum SystemProxyDecision {
	Direct,
	Proxy { url: String },
	Unavailable { failure: RouteFailureClass },
}

// Async login and blocking service refresh share the same transport policy.
// Neither redirects nor automatic retries may replay a one-time credential.
macro_rules! configure_client {
	($builder:expr, $timeout:expr, $connect_timeout:expr, $route:expr) => {{
		let mut builder = $builder
			.redirect(Policy::none())
			.retry(reqwest::retry::never())
			.connect_timeout($timeout.min($connect_timeout))
			.timeout($timeout)
			.user_agent(concat!("decodex/", env!("CARGO_PKG_VERSION")))
			.default_headers({
				let mut headers = reqwest::header::HeaderMap::new();
				headers.insert(
					"originator",
					reqwest::header::HeaderValue::from_static(crate::OAUTH_ORIGINATOR),
				);
				headers
			});
		if let Some(route) = $route {
			builder = builder.no_proxy();
			if let Route::Proxy(proxy) = route {
				builder = builder.proxy(*proxy);
			}
		}
		builder
	}};
}

pub(super) fn client(config: &Config, route: Option<Route>) -> Result<Client, Error> {
	#[cfg(test)]
	let route = route.or_else(|| config.fallback_proxy_fixture.as_ref().map(|_| Route::Direct));
	configure_client!(Client::builder(), config.http_timeout, Duration::from_secs(10), route)
		.build()
		.map_err(|_| Error::Unavailable)
}

/// Secret-free transport outcome. A request that might have reached the provider is not replayed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshTransportError {
	/// No usable connection or route was established.
	Unavailable,
	/// The provider may have consumed the refresh token.
	Ambiguous,
}

// Native external-auth callbacks time out after 10 seconds (upstream c2f7fe89).
// Leave time for persistence and the reply; a proxy fallback shares this budget.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(8);

/// Blocking OAuth transport for the account service's blocking worker.
/// Uses the same network policy and destination-specific proxy resolution as login.
pub struct RefreshTransport {
	client: reqwest::blocking::Client,
}
impl RefreshTransport {
	/// Construct the transport without reading account credentials.
	pub fn new() -> Result<Self, RefreshTransportError> {
		Ok(Self { client: Self::client(None)? })
	}

	fn client(route: Option<Route>) -> Result<reqwest::blocking::Client, RefreshTransportError> {
		configure_client!(
			reqwest::blocking::Client::builder(),
			REFRESH_TIMEOUT,
			Duration::from_secs(2),
			route
		)
		.build()
		.map_err(|_| RefreshTransportError::Unavailable)
	}

	/// Exchange a refresh token once; retry only a connection failure using the system route.
	pub fn refresh(
		&self,
		endpoint: &str,
		refresh_token: &str,
	) -> Result<reqwest::blocking::Response, RefreshTransportError> {
		self.refresh_with_route(endpoint, refresh_token, resolve_blocking)
	}

	fn refresh_with_route(
		&self,
		endpoint: &str,
		refresh_token: &str,
		resolve_route: impl FnOnce(&str) -> Result<Route, Error>,
	) -> Result<reqwest::blocking::Response, RefreshTransportError> {
		#[derive(serde::Serialize)]
		struct Grant<'a> {
			client_id: &'static str,
			grant_type: &'static str,
			refresh_token: &'a str,
		}
		let grant =
			Grant { client_id: crate::OAUTH_CLIENT_ID, grant_type: "refresh_token", refresh_token };
		let started = Instant::now();
		let send = |client: &reqwest::blocking::Client, timeout| {
			client.post(endpoint).json(&grant).timeout(timeout).send()
		};
		match send(&self.client, REFRESH_TIMEOUT) {
			Ok(response) => Ok(response),
			Err(error) if error.is_connect() => {
				let route =
					resolve_route(endpoint).map_err(|_| RefreshTransportError::Unavailable)?;
				let remaining = REFRESH_TIMEOUT
					.checked_sub(started.elapsed())
					.filter(|duration| !duration.is_zero())
					.ok_or(RefreshTransportError::Unavailable)?;
				send(&Self::client(Some(route))?, remaining).map_err(classify_refresh_transport)
			},
			Err(error) => Err(classify_refresh_transport(error)),
		}
	}
}

fn classify_refresh_transport(error: reqwest::Error) -> RefreshTransportError {
	if error.is_builder() || error.is_connect() {
		RefreshTransportError::Unavailable
	} else {
		RefreshTransportError::Ambiguous
	}
}

pub(super) async fn exchange(
	config: &Config,
	client: &Client,
	body: &str,
	cancellation: &Cancellation,
	deadline: Instant,
) -> Result<Response, Error> {
	#[cfg(test)]
	if let Some(proxy) = &config.fallback_proxy_fixture {
		return exchange_with_route(config, client, body, cancellation, deadline, |_| async {
			Proxy::all(proxy).map(Box::new).map(Route::Proxy).map_err(|_| Error::Unavailable)
		})
		.await;
	}

	exchange_with_route(config, client, body, cancellation, deadline, resolve).await
}

async fn exchange_with_route<F, R>(
	config: &Config,
	initial: &Client,
	body: &str,
	cancellation: &Cancellation,
	deadline: Instant,
	resolve_route: F,
) -> Result<Response, Error>
where
	F: FnOnce(String) -> R,
	R: Future<Output = Result<Route, Error>>,
{
	let url = crate::endpoint(config, "oauth/token")?;
	// Preserve the transport classification until the replay decision. Redirects are
	// disabled on both clients; a timeout after sending a POST never enters this branch.
	let response = crate::cancellable(cancellation, deadline, async {
		Ok(initial
			.post(url.clone())
			.header("Content-Type", "application/x-www-form-urlencoded")
			.body(body.to_owned())
			.send()
			.await)
	})
	.await?;

	match response {
		Ok(response) => Ok(response),
		Err(error) if error.is_connect() && config.system_proxy_fallback => {
			let route = tokio::select! {
				biased;

				_ = cancellation.cancelled() => return Err(Error::Cancelled),
				route = time::timeout(crate::remaining(deadline)?, resolve_route(url.to_string())) =>
					route.map_err(|_| Error::TimedOut)??,
			};
			let fallback = client(config, Some(route))?;

			crate::cancellable(
				cancellation,
				deadline,
				fallback
					.post(url)
					.header("Content-Type", "application/x-www-form-urlencoded")
					.body(body.to_owned())
					.send(),
			)
			.await
		},
		Err(_) => Err(Error::Unavailable),
	}
}

async fn resolve(url: String) -> Result<Route, Error> {
	tokio::task::spawn_blocking(move || resolve_blocking(&url))
		.await
		.map_err(|_| Error::Unavailable)?
}

fn resolve_blocking(url: &str) -> Result<Route, Error> {
	#[cfg(target_os = "macos")]
	{
		match macos::resolve(url) {
			SystemProxyDecision::Proxy { url } =>
				Proxy::all(url).map(Box::new).map(Route::Proxy).map_err(|_| Error::Unavailable),
			SystemProxyDecision::Direct => Ok(Route::Direct),
			SystemProxyDecision::Unavailable { failure } => {
				let _ = failure;
				Err(Error::Unavailable)
			},
		}
	}
	#[cfg(not(target_os = "macos"))]
	{
		let _ = url;
		Err(Error::Unavailable)
	}
}

#[cfg(test)]
#[path = "system_proxy_tests.rs"]
mod tests;
