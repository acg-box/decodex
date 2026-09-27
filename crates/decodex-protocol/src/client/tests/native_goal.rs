use super::*;
use crate::{ChiefNativeGoal, ChiefNativeGoalResult, ChiefNativeGoalStatus};

fn observed(change: &str) -> ChiefNativeGoalResult {
	match change {
		"unavailable" => return ChiefNativeGoalResult::Unavailable,
		"unsupported" => return ChiefNativeGoalResult::Unsupported,
		"disabled" => return ChiefNativeGoalResult::Disabled,
		_ => {},
	}
	ChiefNativeGoalResult::Available {
		work_id: EntityId::new(if change == "work" { "other" } else { "work" })
			.expect("valid fixture identity"),
		thread_id: EntityId::new(if change == "thread" { "other" } else { "native-exact" })
			.expect("valid fixture identity"),
		observed_at_micros: 123,
		goal: matches!(change, "present" | "goal_thread").then(|| ChiefNativeGoal {
			thread_id: if change == "goal_thread" { "other" } else { "native-exact" }.into(),
			objective: "Retain the native goal".into(),
			objective_truncated: false,
			status: ChiefNativeGoalStatus::Active,
			token_budget: None,
			tokens_used: 0,
			time_used_seconds: 0,
			created_at: 1,
			updated_at: 2,
		}),
	}
}

#[tokio::test]
async fn native_goal_preserves_authoritative_empty_and_rejects_crossed_sources() {
	for change in [
		"empty",
		"present",
		"work",
		"thread",
		"goal_thread",
		"unavailable",
		"unsupported",
		"disabled",
	] {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.unwrap();
		let profile = ClientProfile::fixture(authority, ServerId::new(SERVER_ID).unwrap());
		let expected = observed(change);
		let returned = expected.clone();
		let server = tokio::spawn(async move {
			let _temp = temp;
			let mut socket =
				tokio_tungstenite::accept_async(listener.accept().await.unwrap()).await.unwrap();
			let _ = socket.next().await;
			for message in initial(SERVER_ID) {
				socket.send(message).await.unwrap();
			}
			let Message::Text(frame) = socket.next().await.unwrap().unwrap() else {
				panic!("query frame")
			};
			let ClientMessage::Query(query) = serde_json::from_str(&frame).unwrap() else {
				panic!("read-only goal query")
			};
			assert!(
				matches!(query.payload, crate::QueryPayload::GetChiefNativeGoal { work_id, thread_id } if work_id.as_str()=="work" && thread_id.as_str()=="native-exact")
			);
			socket
				.send(typed(ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).unwrap(),
					query_id: query.query_id,
					payload: QueryResultPayload::ChiefNativeGoal(returned),
				})))
				.await
				.unwrap();
			drop(socket);
			assert!(
				time::timeout(Duration::from_millis(30), listener.accept()).await.is_err(),
				"Goal reads do not reconnect or execute work"
			);
			listener.cleanup().unwrap();
		});
		let result = crate::ChiefClient::new(profile)
			.native_goal(EntityId::new("work").unwrap(), EntityId::new("native-exact").unwrap())
			.await;
		server.await.unwrap();
		if matches!(change, "work" | "thread" | "goal_thread") {
			assert_eq!(result.unwrap_err(), ClientFailure::ProtocolMalformed);
		} else {
			assert_eq!(result.unwrap(), expected);
		}
	}
}
