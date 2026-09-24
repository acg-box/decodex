//! Retry a one-time code only after a connection failure, before any HTTP response.
use super::{Cancellation, Config, Error, HttpResponse, cancellable, endpoint};
use reqwest::{Client, Proxy, redirect::Policy};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
#[path = "system_proxy_macos.rs"]
mod macos;

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

pub(super) enum Route {
	Direct,
	Proxy(Box<Proxy>),
}

pub(super) fn client(config: &Config, route: Option<Route>) -> Result<Client, Error> {
	let mut builder = Client::builder()
		.redirect(Policy::none())
		.connect_timeout(config.http_timeout.min(Duration::from_secs(10)))
		.read_timeout(config.http_timeout)
		.timeout(config.http_timeout)
		.user_agent(format!(
			"decodex/{} codex-login-source/rust-v0.148.0-alpha.9",
			env!("CARGO_PKG_VERSION")
		));
	#[cfg(test)]
	if config.fallback_proxy_fixture.is_some() {
		builder = builder.no_proxy();
	}
	if let Some(route) = route {
		builder = builder.no_proxy();
		if let Route::Proxy(proxy) = route {
			builder = builder.proxy(*proxy);
		}
	}
	builder.build().map_err(|_| Error::Unavailable)
}

pub(super) async fn exchange(
	config: &Config,
	client: &Client,
	body: &str,
	cancellation: &Cancellation,
	deadline: Instant,
) -> Result<HttpResponse, Error> {
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
) -> Result<HttpResponse, Error>
where
	F: FnOnce(String) -> R,
	R: std::future::Future<Output = Result<Route, Error>>,
{
	let url = endpoint(config, "oauth/token")?;
	// Preserve the transport classification until the replay decision. Redirects are
	// disabled on both clients; a timeout after sending a POST never enters this branch.
	let response = cancellable(cancellation, deadline, async {
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
				route = tokio::time::timeout(super::remaining(deadline)?, resolve_route(url.to_string())) =>
					route.map_err(|_| Error::TimedOut)??,
			};
			let fallback = client(config, Some(route))?;
			cancellable(
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
	#[cfg(target_os = "macos")]
	{
		let route = tokio::task::spawn_blocking(move || macos::resolve(&url))
			.await
			.map_err(|_| Error::Unavailable)?;
		match route {
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
