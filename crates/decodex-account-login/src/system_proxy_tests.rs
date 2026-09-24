use super::*;
use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _};

async fn server(reply: Option<&'static str>) -> (String, tokio::task::JoinHandle<String>) {
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("fixture listener");
	let url = format!("http://{}", listener.local_addr().expect("fixture address"));
	let task = tokio::spawn(async move {
		let (socket, _) = listener.accept().await.expect("fixture request");
		let mut reader = tokio::io::BufReader::new(socket);
		let mut request = String::new();
		let mut length = 0;
		loop {
			let mut line = String::new();
			assert!(reader.read_line(&mut line).await.expect("header") > 0);
			if line == "\r\n" {
				break;
			}
			if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
				length = value.trim().parse::<usize>().expect("body length");
			}
			request.push_str(&line);
		}
		assert!(length < 4096);
		let mut body = vec![0; length];
		reader.read_exact(&mut body).await.expect("request body");
		request.push_str(std::str::from_utf8(&body).expect("form body"));
		if let Some(reply) = reply {
			reader.get_mut().write_all(reply.as_bytes()).await.expect("response");
		}
		request
	});
	(url, task)
}

fn config(url: &str) -> Config {
	let mut config =
		Config::test(url::Url::parse(url).expect("issuer URL"), 1, Duration::from_secs(5))
			.expect("config");
	config.system_proxy_fallback = true;
	config
}

#[tokio::test]
async fn connection_failure_retries_once_using_the_destination_route() {
	for direct in [false, true] {
		let (url, request) =
			server(Some("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}"))
				.await;
		let config = config(if direct { &url } else { "http://127.0.0.1:0" });
		let initial = client(
			&config,
			Some(Route::Proxy(Box::new(
				Proxy::all("http://127.0.0.1:0").expect("unreachable proxy"),
			))),
		)
		.expect("initial client");
		let expected_url = format!("{}/oauth/token", config.issuer.as_str().trim_end_matches('/'));
		let response = exchange_with_route(
			&config,
			&initial,
			"code=once&grant_type=authorization_code",
			&Cancellation::default(),
			Instant::now() + Duration::from_secs(5),
			|target| async move {
				assert_eq!(target, expected_url);
				Ok(if direct {
					Route::Direct
				} else {
					Route::Proxy(Box::new(Proxy::all(url).expect("proxy")))
				})
			},
		)
		.await
		.expect("fallback response");
		assert_eq!(response.status(), 200);
		let request = request.await.expect("server completed");
		assert!(request.starts_with("POST "));
		assert!(request.contains("/oauth/token HTTP/1.1"));
		assert!(request.contains("content-type: application/x-www-form-urlencoded"));
		assert!(request.ends_with("code=once&grant_type=authorization_code"));
	}
}

#[tokio::test]
async fn responses_and_post_delivery_failures_never_resolve_a_fallback() {
	for reply in [
		Some("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}"),
		Some("HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n"),
		Some(
			"HTTP/1.1 307 Temporary Redirect\r\nLocation: http://127.0.0.1:0\r\nContent-Length: 0\r\n\r\n",
		),
		None,
	] {
		let (url, request) = server(reply).await;
		let config = config(&url);
		let initial = client(&config, Some(Route::Direct)).expect("initial client");
		let result = exchange_with_route(
			&config,
			&initial,
			"code=once",
			&Cancellation::default(),
			Instant::now() + Duration::from_secs(5),
			|_| async { panic!("must not replay a delivered code") },
		)
		.await;
		if reply.is_some() {
			assert!(result.is_ok());
		} else {
			assert!(matches!(result, Err(Error::Unavailable)));
		}
		assert!(request.await.expect("server completed").ends_with("code=once"));
	}
}

#[tokio::test]
async fn disabled_fallback_and_cancelled_resolution_do_not_send_a_second_post() {
	let mut config = config("http://127.0.0.1:0");
	let initial = client(&config, Some(Route::Direct)).expect("initial client");
	config.system_proxy_fallback = false;
	let result = exchange_with_route(
		&config,
		&initial,
		"code=once",
		&Cancellation::default(),
		Instant::now() + Duration::from_secs(5),
		|_| async { panic!("disabled resolver") },
	)
	.await;
	assert!(matches!(result, Err(Error::Unavailable)));
	config.system_proxy_fallback = true;
	let cancellation = Cancellation::default();
	let result = exchange_with_route(
		&config,
		&initial,
		"code=once",
		&cancellation,
		Instant::now() + Duration::from_secs(5),
		|_| async {
			cancellation.cancel();
			std::future::pending().await
		},
	)
	.await;
	assert!(matches!(result, Err(Error::Cancelled)));
}

#[tokio::test]
async fn a_response_timeout_never_replays_the_authorization_code() {
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
	let mut config = config(&format!("http://{}", listener.local_addr().expect("address")));
	config.http_timeout = Duration::from_millis(100);
	let initial = client(&config, Some(Route::Direct)).expect("client");
	let (sent, received) = tokio::sync::oneshot::channel();
	let server = tokio::spawn(async move {
		let (mut socket, _) = listener.accept().await.expect("request");
		let mut bytes = [0; 4096];
		let count = socket.read(&mut bytes).await.expect("POST received");
		assert!(bytes[..count].starts_with(b"POST /oauth/token HTTP/1.1"));
		sent.send(()).expect("request witness");
		std::future::pending::<()>().await;
	});
	let result = exchange_with_route(
		&config,
		&initial,
		"code=once",
		&Cancellation::default(),
		Instant::now() + Duration::from_secs(5),
		|_| async { panic!("response timeout must not retry") },
	)
	.await;
	received.await.expect("POST was sent");
	assert!(matches!(result, Err(Error::Unavailable)));
	server.abort();
}

#[tokio::test]
async fn a_failed_fallback_is_terminal() {
	let config = config("http://127.0.0.1:0");
	let initial = client(&config, Some(Route::Direct)).expect("client");
	let resolutions = std::sync::atomic::AtomicUsize::new(0);
	let result = exchange_with_route(
		&config,
		&initial,
		"code=once",
		&Cancellation::default(),
		Instant::now() + Duration::from_secs(5),
		|_| async {
			resolutions.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
			Ok(Route::Direct)
		},
	)
	.await;
	assert!(matches!(result, Err(Error::Unavailable)));
	assert_eq!(resolutions.load(std::sync::atomic::Ordering::SeqCst), 1);
}
