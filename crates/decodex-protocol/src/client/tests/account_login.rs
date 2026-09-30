use super::*;

#[tokio::test]
async fn login_status_requires_matching_request_session_and_valid_state() {
	let requested = EntityId::new("40000000-0000-4000-8000-000000000001").unwrap();

	for case in ["valid", "request", "session", "state"] {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.unwrap();
		let profile = ClientProfile::fixture(authority, ServerId::new(SERVER_ID).unwrap());
		let expected = requested.clone();
		let server = tokio::spawn(async move {
			let _temp = temp;
			let mut socket =
				tokio_tungstenite::accept_async(listener.accept().await.unwrap()).await.unwrap();
			let _ = socket.next().await;

			for message in initial(SERVER_ID) {
				socket.send(message).await.unwrap();
			}

			let Message::Text(wire) = socket.next().await.unwrap().unwrap() else {
				panic!("login request");
			};
			let ClientMessage::AccountLogin(request) = serde_json::from_str(&wire).unwrap() else {
				panic!("dedicated login exchange");
			};

			assert!(
				matches!(request.request, crate::AccountLoginRequest::Status { session_id } if session_id == expected)
			);

			let status = crate::AccountLoginStatus {
				session_id: if case == "session" {
					EntityId::new("40000000-0000-4000-8000-000000000002").unwrap()
				} else {
					expected
				},
				state: if case == "state" {
					crate::AccountLoginState::Completed
				} else {
					crate::AccountLoginState::Cancelled
				},
				prompt: None,
				authorization_url: None,
				failure: None,
				resolved_account_id: None,
			};

			socket
				.send(typed(ServerMessage::AccountLogin(crate::AccountLoginResponseEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).unwrap(),
					request_id: if case == "request" {
						QueryId::new("different-request").unwrap()
					} else {
						request.request_id
					},
					status,
				})))
				.await
				.unwrap();

			drop(socket);

			listener.cleanup().unwrap();
		});
		let result = crate::AccountLoginClient::new(profile).status(requested.clone()).await;

		server.await.unwrap();

		if case == "valid" {
			let status = result.unwrap();

			assert_eq!(status.session_id, requested);
			assert_eq!(status.state, crate::AccountLoginState::Cancelled);
		} else {
			assert_eq!(result, Err(ClientFailure::ProtocolMalformed), "{case}");
		}
	}
}
