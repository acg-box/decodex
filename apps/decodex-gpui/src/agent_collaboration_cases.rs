//! Join only exact source identities. Participant pairs do not identify an exchange.
use std::{cmp::Reverse, collections::BTreeMap};

use crate::shell::agent_surface::work_board::workbench::collaboration::{
	AgentExchangeDto, AgentExchangeQuestionDto, Edge, Exchange, Graph, Kind, Row,
	ui_theme::{INFO, SUCCESS, TEXT_MUTED, WARNING},
};
use decodex_protocol::{
	AgentDispatchStateDto, AgentExchangeAssessment, AgentExchangeKind, AgentPeerExchangeDto,
};

pub(super) fn exchanges(
	graph: &Graph,
	rows: &[Row],
	records: &[AgentExchangeDto],
	questions: &[AgentExchangeQuestionDto],
) -> Vec<Exchange> {
	let mut cases = managed(records, rows);

	cases.extend(peers(records, rows));

	for (order, edge) in graph.edges.iter().enumerate() {
		if matches!(edge.kind, Kind::Assigned | Kind::Resource | Kind::Spawned)
			|| (edge.source.is_none() && edge.kind != Kind::Dependency)
		{
			continue;
		}

		let (Some(from), Some(to)) = (
			rows.iter().find(|row| row.key == edge.from),
			rows.iter().find(|row| row.key == edge.to),
		) else {
			continue;
		};
		// A returned snapshot belongs to its original call item, but does not prove a reply to it.
		let id = edge
			.id
			.strip_prefix("reply:")
			.map(|suffix| format!("call:{suffix}"))
			.unwrap_or_else(|| edge.id.clone());

		if edge.kind == Kind::Returned
			&& let Some(case) = cases.iter_mut().find(|case| case.id == id)
		{
			case.steps.push((
				"Observed",
				edge.excerpt
					.as_deref()
					.map(short)
					.unwrap_or_else(|| "No reply body recorded.".into()),
			));

			case.summary = "A target snapshot is available · reply linkage unknown".into();

			continue;
		}

		let (state, color, summary) = native_state(edge);

		cases.push(Exchange {
			id,
			from: from.clone(),
			to: Some(to.clone()),
			title: edge
				.excerpt
				.as_deref()
				.map(short)
				.filter(|text| !text.is_empty())
				.unwrap_or_else(|| {
					if edge.kind == Kind::Returned {
						from.title.clone()
					} else {
						edge.label.clone()
					}
				}),
			summary: summary.into(),
			state,
			color,
			request: edge.excerpt.clone(),
			steps: vec![(
				"Source",
				if edge.excerpt.is_some() {
					"A source excerpt is available."
				} else {
					"The provider did not include a message body."
				}
				.into(),
			)],
			source: edge.id.clone(),
			source_text: edge
				.link
				.as_ref()
				.map(|link| format!("{}\n\n{link}", edge.detail))
				.unwrap_or_else(|| edge.detail.clone()),
			source_row: edge
				.source
				.as_ref()
				.and_then(|(owner, thread)| {
					rows.iter()
						.find(|row| &row.work == owner && row.thread.as_ref() == Some(thread))
				})
				.unwrap_or(from)
				.clone(),
			source_event: None,
			source_item: None,
			source_turn: None,
			source_thread: edge.source.as_ref().map(|(_, thread)| thread.clone()),
			edge: Some(edge.clone()),
			earlier: (edge.kind == Kind::Dependency && edge.label == "Prerequisite met")
				|| (edge.kind == Kind::Message && !edge.in_progress && edge.excerpt.is_none())
				|| !edge.in_progress
					&& !matches!(
						edge.kind,
						Kind::Dependency | Kind::Returned | Kind::Message | Kind::Wait
					),
			question: None,
			order: order as i64,
		});
	}

	append_questions(&mut cases, questions, rows);

	cases.sort_by_key(|case| {
		(
			case.earlier,
			!matches!(case.state, "Not sent" | "Question" | "Decision"),
			!matches!(case.state, "To review" | "Check result"),
			Reverse(case.order),
		)
	});

	cases
}

fn short(text: &str) -> String {
	text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(180).collect()
}

fn managed_state(case: &mut Exchange, record: &AgentExchangeDto, work: &Row, other: Option<&Row>) {
	match record.kind {
		AgentExchangeKind::NotSent => {
			case.state = "Not sent";
			case.color = WARNING;
			case.title = "Continue the same worker".into();
			case.request = Some(record.excerpt.clone());
			case.summary = match record.target_dispatch_state {
				Some(AgentDispatchStateDto::Idle) => "Worker idle · instruction still not sent",
				_ => "Busy · instruction not sent or queued",
			}
			.into();

			case.steps.push(("Attempt", "Busy refused this continuation.".into()));
			case.steps.push((
				"Next",
				match record.target_dispatch_state {
					Some(AgentDispatchStateDto::Idle) =>
						"The worker is idle. Retry through its owner on the same work item.",
					Some(AgentDispatchStateDto::Running | AgentDispatchStateDto::Dispatching) =>
						"Wait for the current turn, then ask the owner to retry.",
					_ => "Check the same worker before retrying. This attempt remains unsent.",
				}
				.into(),
			));
		},
		AgentExchangeKind::Instruction => {
			if let Some(other) = other {
				case.from = other.clone();
				case.to = Some(work.clone());
			}

			if record.delivered_turn_id.is_some() {
				case.summary = "Instruction delivered · result not yet linked".into();

				case.steps
					.push(("Delivered", "The provider acknowledged this instruction.".into()));
				case.steps.push(("Waiting", "No result is linked to this turn yet.".into()));

				if record.thread_id.is_none() {
					case.state = "Delivered";
					case.color = TEXT_MUTED;
					case.earlier = true;
					case.summary = "Earlier instruction · result linkage unavailable".into();
				}
			} else {
				case.state = "Unconfirmed";
				case.color = WARNING;
				case.summary = "Delivery has not been confirmed".into();

				case.steps
					.push(("Check", "Check the original worker before another attempt.".into()));
			}
		},
		AgentExchangeKind::Result => managed_result(case, record),
		AgentExchangeKind::PeerInstruction | AgentExchangeKind::PeerResult => {},
	}
}

fn managed_result(case: &mut Exchange, record: &AgentExchangeDto) {
	let completed = record.outcome.as_deref() == Some("completed");

	case.state = if completed { "To review" } else { "Check result" };
	case.color = if completed { SUCCESS } else { WARNING };
	case.summary = if completed {
		"Result received · assessment not recorded"
	} else {
		"The worker did not report successful completion"
	}
	.into();

	case.steps.push((
		"Result",
		if record.excerpt.is_empty() {
			"No result body is available.".into()
		} else {
			short(&record.excerpt)
		},
	));
	case.steps.push((
		"Receipt",
		if record.delivered_turn_id.is_some() {
			"Delivered to the responsible manager."
		} else {
			"The manager has not acknowledged delivery."
		}
		.into(),
	));

	match record.assessment {
		Some(AgentExchangeAssessment::Accepted) => {
			case.state = "Accepted";
			case.color = SUCCESS;
			case.earlier = true;
			case.summary = "The manager accepted this exact result".into();
		},
		Some(AgentExchangeAssessment::FollowUp) => {
			case.state = "Changes asked";
			case.color = WARNING;
			case.summary = "Follow-up requested · new result pending".into();
		},
		Some(AgentExchangeAssessment::Wait) => {
			case.state = "Waiting";
			case.color = WARNING;
			case.summary = "The manager deferred this assessment".into();
		},
		Some(AgentExchangeAssessment::UserDecision) => {
			case.state = "Decision";
			case.color = WARNING;
			case.summary = "The manager needs your decision".into();
		},
		None => {},
	}

	if let Some(note) = &record.assessment_note {
		case.steps.push(("Review", short(note)));
	}
}

fn managed(records: &[AgentExchangeDto], rows: &[Row]) -> Vec<Exchange> {
	let mut cases = BTreeMap::new();
	let mut sorted: Vec<_> = records.iter().collect();

	sorted.sort_by_key(|record| record.event_id);

	for record in sorted {
		if record.collaboration.is_some() {
			continue;
		}

		let Some(work) = rows.iter().find(|row| !row.native && row.work == record.work_id) else {
			continue;
		};
		let other = record
			.other_work_id
			.as_ref()
			.and_then(|id| rows.iter().find(|row| !row.native && &row.work == id));
		let id = if record.kind == AgentExchangeKind::NotSent {
			format!("unsent:{}", record.event_id)
		} else {
			format!(
				"managed:{}:{}",
				record.work_id,
				record.instruction_event_id.unwrap_or(record.event_id)
			)
		};
		let request = if record.kind == AgentExchangeKind::Instruction {
			Some(record.excerpt.clone())
		} else {
			cases.get(&id).and_then(|case: &Exchange| case.request.clone())
		};
		let mut case = Exchange {
			id: id.clone(),
			from: work.clone(),
			to: other.cloned(),
			title: work.title.clone(),
			summary: String::new(),
			state: "Continuing",
			color: SUCCESS,
			request,
			steps: vec![],
			source: record.source_event_id.clone(),
			source_row: work.clone(),
			source_text: record.excerpt.clone(),
			source_event: Some(record.event_id),
			source_item: record.item_id.clone(),
			source_turn: record.turn_id.clone(),
			source_thread: record.thread_id.clone(),
			edge: None,
			earlier: false,
			question: None,
			order: record.event_id,
		};

		managed_state(&mut case, record, work, other);

		if record.thread_id.is_some() && record.thread_id != work.thread {
			case.state = "Earlier";
			case.color = TEXT_MUTED;
			case.earlier = true;
			case.summary = "Earlier conversation binding · source retained".into();
		}
		if record.truncated {
			case.steps.push((
				"Source",
				"Excerpt shortened. Read the original for the full content.".into(),
			));
		}

		cases.insert(id, case);
	}
	// Refused attempts remain historical facts. A later instruction does not make them delivered.
	for case in cases.values_mut() {
		if matches!(case.state, "Changes asked" | "Waiting" | "Decision")
			&& records.iter().any(|record| {
				record.kind == AgentExchangeKind::Instruction
					&& record.work_id == case.source_row.work
					&& record.event_id > case.order
			}) {
			case.earlier = true;
		}
		if case.state == "Not sent"
			&& records.iter().any(|record| {
				record.kind == AgentExchangeKind::Instruction
					&& Some(record.work_id.as_str())
						== case.to.as_ref().map(|row| row.work.as_str())
					&& record.event_id > case.order
			}) {
			case.earlier = true;
		}
	}

	cases.into_values().collect()
}

fn append_questions(
	cases: &mut Vec<Exchange>,
	questions: &[AgentExchangeQuestionDto],
	rows: &[Row],
) {
	for question in questions {
		let Some(row) = rows.iter().find(|row| {
			!row.native
				&& row.work == question.work_id
				&& row.thread.as_ref() == Some(&question.thread_id)
		}) else {
			continue;
		};
		let mut steps = vec![("Question", question.question.title.clone())];

		if let Some(answer) = &question.answer {
			steps.push(("Answer", answer.text.clone()));
			steps.push((
				"Reply source",
				format!("Event {} · {}", answer.event_id, answer.source_event_id),
			));
			steps.push((
				"Delivery",
				answer
					.delivered_turn_id
					.as_ref()
					.map(|turn| format!("Acknowledged turn {turn}"))
					.unwrap_or_else(|| "Delivery is unconfirmed.".into()),
			));

			if let Some(outcome) = &answer.turn_outcome {
				steps.push((
					"Continuation",
					format!("Turn {outcome}. This does not resolve the question or Work."),
				));
			}

			if answer.truncated {
				steps.push((
					"Source",
					"Reply excerpt shortened. Read the original for full content.".into(),
				));
			}
		} else if !question.pending {
			steps.push(("History","Reply body or disposition is unavailable. A continuation or dismissal can also remove a question from pending.".into()));
		}

		let answered =
			question.answer.as_ref().is_some_and(|answer| answer.delivered_turn_id.is_some());

		cases.push(Exchange {
			id: format!("question:{}:{}", question.work_id, question.question.id),
			from: row.clone(),
			to: None,
			title: question.question.title.clone(),
			summary: if question.pending {
				"Native question · your response is needed"
			} else if answered {
				"Reply delivered · resolution is not inferred"
			} else if question.answer.is_some() {
				"Reply retained · delivery is unconfirmed"
			} else {
				"No longer pending · original source retained"
			}
			.into(),
			state: if question.pending {
				"Question"
			} else if answered {
				"Answered"
			} else if question.answer.is_some() {
				"Reply saved"
			} else {
				"Withdrawn"
			},
			color: if question.pending { WARNING } else { TEXT_MUTED },
			request: None,
			steps,
			source: question.question.id.clone(),
			source_text: question.question.title.clone(),
			source_row: row.clone(),
			source_event: None,
			source_item: Some(question.item_id.clone()),
			source_turn: Some(question.turn_id.clone()),
			source_thread: Some(question.thread_id.clone()),
			edge: None,
			earlier: !question.pending,
			question: Some(question.clone()),
			order: 0,
		});
	}
}

fn peer_state(peer: &AgentPeerExchangeDto) -> (&'static str, &'static str, u32) {
	if !peer.current {
		("Earlier", "Earlier instruction · no current assessment borrowed", TEXT_MUTED)
	} else if let Some(outcome) = &peer.outcome {
		match outcome.as_str() {
			"used" => ("Used", "The Work owner used this contribution", SUCCESS),
			"not_used" => ("Not used", "The Work owner did not use this contribution", TEXT_MUTED),
			"declined" => ("Declined", "The recipient declined this instruction", WARNING),
			"cancelled" => ("Cancelled", "The Work owner cancelled this request", TEXT_MUTED),
			_ => ("Failed", "The Work owner recorded a failed contribution", WARNING),
		}
	} else {
		match peer.state.as_deref() {
			Some("returned") =>
				("To review", "Contribution returned · owner assessment pending", INFO),
			Some("accepted") => ("Accepted", "Recipient accepted · result pending", SUCCESS),
			_ => ("Requested", "Independent contribution · acceptance unconfirmed", WARNING),
		}
	}
}

fn peers(records: &[AgentExchangeDto], rows: &[Row]) -> Vec<Exchange> {
	let mut cases = BTreeMap::<String, Exchange>::new();
	let mut sorted: Vec<_> = records.iter().filter(|r| r.collaboration.is_some()).collect();

	sorted.sort_by_key(|r| r.event_id);

	for record in sorted {
		let Some(peer) = &record.collaboration else { continue };
		let Some(owner) = rows.iter().find(|row| !row.native && row.work == record.work_id) else {
			continue;
		};
		let other =
			rows.iter().find(|row| !row.native && Some(&row.work) == record.other_work_id.as_ref());
		let instruction = record.kind == AgentExchangeKind::PeerInstruction;
		let id = format!("peer:{}:{}", peer.request_id, peer.instruction_id);
		let request = if instruction {
			Some(record.excerpt.clone())
		} else {
			cases.get(&id).and_then(|c| c.request.clone())
		};
		let (from, to) = (other.unwrap_or(owner).clone(), Some(owner.clone()));
		let mut steps = vec![
			("Work", peer.work_id.clone()),
			("Request", format!("{} · instruction {}", peer.request_id, peer.instruction_id)),
		];

		if !instruction {
			steps.push(("Returned", record.excerpt.clone()));
		}

		steps.push((
			"Delivery",
			record
				.delivered_turn_id
				.as_ref()
				.map(|t| format!("Acknowledged receiving turn {t}"))
				.unwrap_or_else(|| "Delivery is unconfirmed.".into()),
		));

		if let Some(note) = &peer.note {
			steps.push(("Assessment", note.clone()));
		}

		if record.truncated {
			steps.push(("Source", "Excerpt shortened. Read the original for full content.".into()));
		}

		let (state, summary, color) = peer_state(peer);
		let source_row = if instruction { owner.clone() } else { other.unwrap_or(owner).clone() };

		cases.insert(
			id.clone(),
			Exchange {
				id,
				from,
				to,
				title: format!("Contribution · {}", peer.work_id),
				summary: summary.into(),
				state,
				color,
				request,
				steps,
				source: record.source_event_id.clone(),
				source_text: record.excerpt.clone(),
				source_row,
				source_event: Some(record.event_id),
				source_item: record.item_id.clone(),
				source_turn: record.turn_id.clone(),
				source_thread: record.thread_id.clone(),
				edge: None,
				earlier: !peer.current || peer.outcome.is_some(),
				question: None,
				order: record.event_id,
			},
		);
	}

	cases.into_values().collect()
}

fn native_state(edge: &Edge) -> (&'static str, u32, &'static str) {
	match edge.kind {
		Kind::Dependency if edge.label == "Prerequisite met" =>
			("Ready", TEXT_MUTED, "Prerequisite resolved · downstream execution not confirmed"),
		Kind::Dependency => ("Waiting", WARNING, "An explicit work dependency is outstanding"),
		Kind::Wait if edge.in_progress => ("Waiting", WARNING, "Waiting for the named agent"),
		Kind::Returned => ("Reported", INFO, "A native event was recorded · acceptance unknown"),
		Kind::Context =>
			("Shared", TEXT_MUTED, "Context made available · reading is not confirmed"),
		_ if edge.label.contains("failed") || edge.label.contains("interrupted") =>
			("Unconfirmed", WARNING, "Check the source before continuing"),
		_ if edge.in_progress => ("Working", INFO, "The native call is in progress"),
		_ => ("Recorded", TEXT_MUTED, "Native route recorded · outcome not confirmed"),
	}
}
