//! Program-only Adaptive Factory presentation for the native GPUI shell.

use std::path::PathBuf;

use gpui::{
	AnyElement, App, Context, Entity, EventEmitter, FontWeight, Render, Role, SharedString, Window,
	div, prelude::*, px, rgb, rgba,
};

use decodex_protocol::{
	ConversationWorkingDirectory, DEVELOPMENT_DOMAIN_PACK_ID, DomainEntityDto,
	DomainPackCapabilityStatus, EntityId, MAX_PROGRAM_NODES, PAPER_INVESTMENT_DOMAIN_PACK_ID,
	ProgramContinuationDraftDto, ProgramCycleDraftDto, ProgramNodeDto, ProgramNodeKind,
	ProgramReviewClassification, ProgramReviewDraftDto, WireText,
};

use crate::{
	composer_input::{ComposerEvent, ComposerInput},
	program_graph::{self, ProgramGraphEvent, ProgramGraphSurface},
	programs::{
		ProgramCommandState, ProgramInputError, Programs, ProgramsLoadState, ProgramsSnapshot,
		entity_id,
	},
	ui_theme,
};

const FACTORY_MIN_WIDTH: f32 = 1_180.0;
const COMPLETE_PROGRAM_CYCLE_NODE_COST: usize = 9;
const SURFACE: u32 = ui_theme::CANVAS;
const SURFACE_OVERLAY: u32 = ui_theme::SURFACE_OVERLAY;
const LINE_MUTED: u32 = ui_theme::LINE;
const TEXT: u32 = ui_theme::TEXT;
const TEXT_MUTED: u32 = ui_theme::TEXT_MUTED;
const TEXT_FAINT: u32 = ui_theme::TEXT_FAINT;
const BLUE: u32 = ui_theme::BLUE;
const GREEN: u32 = ui_theme::GREEN;
const AMBER: u32 = ui_theme::AMBER;

pub(crate) fn bind_keys(cx: &mut App) {
	program_graph::bind_keys(cx);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FactoryEvent {
	StartProgramWorkItem {
		work_item_id: EntityId,
		message: String,
		working_directory: ConversationWorkingDirectory,
	},
	OpenProgramConversation {
		conversation_id: EntityId,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProgramPackChoice {
	Development,
	PaperInvestment,
}

impl ProgramPackChoice {
	const ALL: [Self; 2] = [Self::Development, Self::PaperInvestment];

	const fn id(self) -> &'static str {
		match self {
			Self::Development => DEVELOPMENT_DOMAIN_PACK_ID,
			Self::PaperInvestment => PAPER_INVESTMENT_DOMAIN_PACK_ID,
		}
	}

	const fn name(self) -> &'static str {
		match self {
			Self::Development => "Software Development",
			Self::PaperInvestment => "Paper Investment Research",
		}
	}

	const fn summary(self) -> &'static str {
		match self {
			Self::Development => "Repository · change · validation",
			Self::PaperInvestment => "Asset · thesis · scenario",
		}
	}

	const fn color(self) -> u32 {
		match self {
			Self::Development => BLUE,
			Self::PaperInvestment => GREEN,
		}
	}
}

struct ProgramCreationInputs {
	name: Entity<ComposerInput>,
	purpose: Entity<ComposerInput>,
	non_goal: Entity<ComposerInput>,
	review_policy: Entity<ComposerInput>,
	signal_source: Entity<ComposerInput>,
	signal: Entity<ComposerInput>,
	claim: Entity<ComposerInput>,
	proposal: Entity<ComposerInput>,
	objective: Entity<ComposerInput>,
	work_item_title: Entity<ComposerInput>,
	work_item_instructions: Entity<ComposerInput>,
	working_directory: Entity<ComposerInput>,
}

impl ProgramCreationInputs {
	fn new(cx: &mut Context<FactorySurface>) -> Self {
		let input = |index, placeholder, label, cx: &mut Context<FactorySurface>| {
			cx.new(|cx| ComposerInput::with_placeholder(index, placeholder, label, cx))
		};
		Self {
			name: input(4, "Program name", "Program name", cx),
			purpose: input(5, "Long-term purpose", "Program purpose", cx),
			non_goal: input(6, "Explicit non-goal", "Program non-goal", cx),
			review_policy: input(7, "When and how progress is reviewed", "Review policy", cx),
			signal_source: input(8, "Signal source", "Signal source", cx),
			signal: input(9, "What changed or was observed", "Signal summary", cx),
			claim: input(10, "What the signal implies", "Claim", cx),
			proposal: input(11, "Finite proposed response", "Proposal", cx),
			objective: input(12, "Observable outcome for this cycle", "Objective", cx),
			work_item_title: input(13, "One concrete Codex task", "Work item title", cx),
			work_item_instructions: input(
				14,
				"Exact instructions for Codex",
				"Work item instructions",
				cx,
			),
			working_directory: input(15, "/absolute/path/to/repository", "Working directory", cx),
		}
	}

	fn all(&self) -> [&Entity<ComposerInput>; 12] {
		[
			&self.name,
			&self.purpose,
			&self.non_goal,
			&self.review_policy,
			&self.signal_source,
			&self.signal,
			&self.claim,
			&self.proposal,
			&self.objective,
			&self.work_item_title,
			&self.work_item_instructions,
			&self.working_directory,
		]
	}
}

struct ProgramReviewInputs {
	deterministic: Entity<ComposerInput>,
	external_source: Entity<ComposerInput>,
	external: Entity<ComposerInput>,
	rationale: Entity<ComposerInput>,
}

impl ProgramReviewInputs {
	fn new(cx: &mut Context<FactorySurface>) -> Self {
		Self {
			deterministic: cx.new(|cx| {
				ComposerInput::with_placeholder(
					16,
					"Deterministic check and result",
					"Deterministic validation evidence",
					cx,
				)
			}),
			external_source: cx.new(|cx| {
				ComposerInput::with_placeholder(
					17,
					"External source",
					"External evidence source",
					cx,
				)
			}),
			external: cx.new(|cx| {
				ComposerInput::with_placeholder(
					18,
					"Observed external outcome",
					"External evidence",
					cx,
				)
			}),
			rationale: cx.new(|cx| {
				ComposerInput::with_placeholder(
					19,
					"Why this classification follows",
					"Review rationale",
					cx,
				)
			}),
		}
	}

	fn all(&self) -> [&Entity<ComposerInput>; 4] {
		[&self.deterministic, &self.external_source, &self.external, &self.rationale]
	}
}

struct ProgramContinuationInputs {
	signal_source: Entity<ComposerInput>,
	signal: Entity<ComposerInput>,
	claim: Entity<ComposerInput>,
	proposal: Entity<ComposerInput>,
	objective: Entity<ComposerInput>,
	work_item_title: Entity<ComposerInput>,
	work_item_instructions: Entity<ComposerInput>,
	working_directory: Entity<ComposerInput>,
}

struct ProgramInspectorSelection<'a> {
	node: Option<&'a ProgramNodeDto>,
	domain: Option<&'a DomainEntityDto>,
	program: bool,
}

impl ProgramContinuationInputs {
	fn new(cx: &mut Context<FactorySurface>) -> Self {
		let input = |index, placeholder, label, cx: &mut Context<FactorySurface>| {
			cx.new(|cx| ComposerInput::with_placeholder(index, placeholder, label, cx))
		};
		Self {
			signal_source: input(
				20,
				"Review, observation, or external source",
				"Signal source",
				cx,
			),
			signal: input(21, "What changed since the prior Review", "Signal summary", cx),
			claim: input(22, "What the new signal implies", "Claim", cx),
			proposal: input(23, "Finite proposed response", "Proposal", cx),
			objective: input(24, "Observable outcome for this cycle", "Objective", cx),
			work_item_title: input(25, "One concrete Codex task", "Work item title", cx),
			work_item_instructions: input(
				26,
				"Exact instructions for Codex",
				"Work item instructions",
				cx,
			),
			working_directory: input(27, "/absolute/path/to/repository", "Working directory", cx),
		}
	}

	fn all(&self) -> [&Entity<ComposerInput>; 8] {
		[
			&self.signal_source,
			&self.signal,
			&self.claim,
			&self.proposal,
			&self.objective,
			&self.work_item_title,
			&self.work_item_instructions,
			&self.working_directory,
		]
	}
}

/// One Program-only native surface backed by daemon projections.
pub(crate) struct FactorySurface {
	programs: Option<Programs>,
	programs_snapshot: Option<ProgramsSnapshot>,
	program_graph: Entity<ProgramGraphSurface>,
	program_pack: ProgramPackChoice,
	program_inputs: ProgramCreationInputs,
	program_review_inputs: ProgramReviewInputs,
	program_continuation_inputs: ProgramContinuationInputs,
	program_review_classification: ProgramReviewClassification,
	program_status: Option<SharedString>,
	program_intake_visible: bool,
	program_continuation_visible: bool,
}

impl EventEmitter<FactoryEvent> for FactorySurface {}

impl FactorySurface {
	pub(crate) fn new(cx: &mut Context<Self>) -> Self {
		let program_inputs = ProgramCreationInputs::new(cx);
		let program_review_inputs = ProgramReviewInputs::new(cx);
		let program_continuation_inputs = ProgramContinuationInputs::new(cx);
		let program_graph = cx.new(ProgramGraphSurface::new);
		cx.subscribe(&program_graph, |_surface, _, event: &ProgramGraphEvent, cx| match event {
			ProgramGraphEvent::SelectionChanged => cx.notify(),
			ProgramGraphEvent::OpenConversation(conversation_id) => {
				cx.emit(FactoryEvent::OpenProgramConversation {
					conversation_id: conversation_id.clone(),
				});
			},
		})
		.detach();
		for input in program_inputs
			.all()
			.into_iter()
			.chain(program_review_inputs.all())
			.chain(program_continuation_inputs.all())
		{
			cx.subscribe(input, |surface, _, _: &ComposerEvent, cx| {
				surface.program_status = None;
				cx.notify();
			})
			.detach();
		}
		Self {
			programs: None,
			programs_snapshot: None,
			program_graph,
			program_pack: ProgramPackChoice::Development,
			program_inputs,
			program_review_inputs,
			program_continuation_inputs,
			program_review_classification: ProgramReviewClassification::OutcomeProgress,
			program_status: None,
			program_intake_visible: false,
			program_continuation_visible: false,
		}
	}

	pub(crate) fn bind_programs(&mut self, programs: Programs, cx: &mut Context<Self>) {
		self.programs = Some(programs.clone());
		self.programs_snapshot = Some(programs.snapshot());
		self.synchronize_program_graph(cx);
		cx.notify();
	}

	pub(crate) fn synchronize_programs(&mut self, cx: &mut Context<Self>) {
		let Some(programs) = self.programs.as_ref() else {
			return;
		};
		let snapshot = programs.snapshot();
		if self.programs_snapshot.as_ref() != Some(&snapshot) {
			let previous_cycle = self
				.programs_snapshot
				.as_ref()
				.and_then(|snapshot| snapshot.cycle.as_ref())
				.map(|cycle| cycle.program.program_id.clone());
			let current_cycle =
				snapshot.cycle.as_ref().map(|cycle| cycle.program.program_id.clone());
			if current_cycle.is_some() && current_cycle != previous_cycle {
				self.program_intake_visible = false;
				self.program_status = None;
			}
			self.programs_snapshot = Some(snapshot);
			self.synchronize_program_graph(cx);
			cx.notify();
		}
	}

	fn synchronize_program_graph(&mut self, cx: &mut Context<Self>) {
		let cycle = self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.clone());
		self.program_graph.update(cx, |graph, cx| graph.set_cycle(cycle, cx));
	}

	fn create_program(&mut self, cx: &mut Context<Self>) {
		let Some(programs) = self.programs.as_ref() else {
			self.program_status = Some("Program authority is not connected.".into());
			cx.notify();
			return;
		};
		let build = || -> Result<ProgramCycleDraftDto, ProgramInputError> {
			let text = |input: &Entity<ComposerInput>| {
				let value = input.read(cx).content().trim().to_owned();
				if value.is_empty() {
					return Err(ProgramInputError::InvalidDraft);
				}
				WireText::new(value).map_err(|_| ProgramInputError::InvalidDraft)
			};
			let objective = text(&self.program_inputs.objective)?;
			let working_directory = ConversationWorkingDirectory::new(
				self.program_inputs.working_directory.read(cx).content().trim().to_owned(),
			)
			.map_err(|_| ProgramInputError::InvalidDraft)?;
			Ok(ProgramCycleDraftDto {
				program_id: entity_id()?,
				domain_pack_id: WireText::new(self.program_pack.id())
					.expect("built-in Domain Pack identifier is bounded"),
				signal_id: entity_id()?,
				claim_id: entity_id()?,
				proposal_id: entity_id()?,
				objective_id: entity_id()?,
				work_item_id: entity_id()?,
				name: text(&self.program_inputs.name)?,
				purpose: text(&self.program_inputs.purpose)?,
				non_goals: vec![text(&self.program_inputs.non_goal)?],
				review_policy: text(&self.program_inputs.review_policy)?,
				signal_source: text(&self.program_inputs.signal_source)?,
				signal_summary: text(&self.program_inputs.signal)?,
				signal_observed_at_micros: current_micros()?,
				claim_statement: text(&self.program_inputs.claim)?,
				proposal_summary: text(&self.program_inputs.proposal)?,
				proposal_expected_effect: objective.clone(),
				proposal_risk: WireText::new(
					"The finite WorkItem may not produce the stated observable outcome.",
				)
				.expect("fixed Program risk is bounded"),
				proposal_evidence_need: WireText::new(
					"A settled Codex run, deterministic validation, and external evidence.",
				)
				.expect("fixed Program evidence need is bounded"),
				objective_outcome: objective.clone(),
				acceptance_criteria: vec![objective],
				validation_criteria: vec![
					WireText::new(
						"The bound Conversation settles and the review cites reproducible evidence.",
					)
					.expect("fixed Program validation criterion is bounded"),
				],
				work_item_title: text(&self.program_inputs.work_item_title)?,
				work_item_instructions: text(&self.program_inputs.work_item_instructions)?,
				working_directory,
			})
		};

		match build().and_then(|draft| programs.create(draft)) {
			Ok(()) => self.program_status = Some("Creating the persisted Program cycle…".into()),
			Err(error) => self.program_status = Some(program_error_label(error).into()),
		}
		self.programs_snapshot = Some(programs.snapshot());
		cx.notify();
	}

	fn select_program_pack(&mut self, pack: ProgramPackChoice, cx: &mut Context<Self>) {
		self.program_pack = pack;
		self.program_status = None;
		cx.notify();
	}

	fn load_paper_example(&mut self, cx: &mut Context<Self>) {
		self.program_pack = ProgramPackChoice::PaperInvestment;
		for (input, value) in [
			(&self.program_inputs.name, "June Treasury Curve Research"),
			(
				&self.program_inputs.purpose,
				"Evaluate one reproducible 2s10s yield-curve thesis through a bounded Program loop.",
			),
			(
				&self.program_inputs.non_goal,
				"Do not fetch live market data or place any paper or real order.",
			),
			(
				&self.program_inputs.review_policy,
				"Review after Codex verifies the frozen fixture and cites deterministic results.",
			),
			(&self.program_inputs.signal_source, "Frozen official U.S. Treasury June 2025 fixture"),
			(
				&self.program_inputs.signal,
				"The June 2025 2-year and 10-year par yields provide a finite curve sample.",
			),
			(
				&self.program_inputs.claim,
				"The sample can test whether the 2s10s slope stayed positive during the month.",
			),
			(
				&self.program_inputs.proposal,
				"Have Codex independently verify the frozen observations and spread bounds.",
			),
			(
				&self.program_inputs.objective,
				"Produce a cited, reproducible conclusion for the June 2025 2s10s slope.",
			),
			(&self.program_inputs.work_item_title, "Verify the June 2025 Treasury 2s10s thesis"),
			(
				&self.program_inputs.work_item_instructions,
				"Inspect the bundled June 2025 U.S. Treasury fixture. Recompute observation count, first and last spread, minimum, maximum, and range. Report whether the slope remained positive. Do not use live data or take any external action.",
			),
		] {
			input.update(cx, |input, cx| input.set_content(value, cx));
		}
		self.program_status = Some("Example loaded. Choose an absolute working directory.".into());
		cx.notify();
	}

	fn bind_selected_domain_pack(&mut self, pack: ProgramPackChoice, cx: &mut Context<Self>) {
		let Some(programs) = self.programs.as_ref() else {
			return;
		};
		let Some(cycle) =
			self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref())
		else {
			return;
		};
		let result = programs.bind_domain_pack(
			cycle.program.program_id.clone(),
			WireText::new(pack.id()).expect("built-in Domain Pack identifier is bounded"),
			cycle.program.revision,
		);
		self.program_status = Some(match result {
			Ok(()) => format!("Binding {}…", pack.name()).into(),
			Err(error) => program_error_label(error).into(),
		});
		self.programs_snapshot = Some(programs.snapshot());
		cx.notify();
	}

	fn select_program(&mut self, program_id: EntityId, cx: &mut Context<Self>) {
		let Some(programs) = self.programs.as_ref() else {
			return;
		};
		if programs.select(program_id) {
			self.programs_snapshot = Some(programs.snapshot());
			self.synchronize_program_graph(cx);
			self.program_continuation_visible = false;
			self.program_status = None;
			cx.notify();
		}
	}

	fn continue_program(&mut self, cx: &mut Context<Self>) {
		let Some(programs) = self.programs.as_ref() else {
			return;
		};
		let Some(cycle) =
			self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref())
		else {
			self.program_status = Some("No Program cycle is selected.".into());
			cx.notify();
			return;
		};
		let Some(predecessor) =
			cycle.nodes.last().filter(|node| node.kind == ProgramNodeKind::Review)
		else {
			self.program_status =
				Some("The current cycle needs a Review before continuation.".into());
			cx.notify();
			return;
		};
		let build = || -> Result<ProgramContinuationDraftDto, ProgramInputError> {
			let text = |input: &Entity<ComposerInput>| {
				let value = input.read(cx).content().trim().to_owned();
				if value.is_empty() {
					return Err(ProgramInputError::InvalidDraft);
				}
				WireText::new(value).map_err(|_| ProgramInputError::InvalidDraft)
			};
			let objective = text(&self.program_continuation_inputs.objective)?;
			let working_directory = ConversationWorkingDirectory::new(
				self.program_continuation_inputs
					.working_directory
					.read(cx)
					.content()
					.trim()
					.to_owned(),
			)
			.map_err(|_| ProgramInputError::InvalidDraft)?;
			Ok(ProgramContinuationDraftDto {
				program_id: cycle.program.program_id.clone(),
				predecessor_review_id: predecessor.id.clone(),
				signal_id: entity_id()?,
				claim_id: entity_id()?,
				proposal_id: entity_id()?,
				objective_id: entity_id()?,
				work_item_id: entity_id()?,
				signal_source: text(&self.program_continuation_inputs.signal_source)?,
				signal_summary: text(&self.program_continuation_inputs.signal)?,
				signal_observed_at_micros: current_micros()?,
				claim_statement: text(&self.program_continuation_inputs.claim)?,
				proposal_summary: text(&self.program_continuation_inputs.proposal)?,
				proposal_expected_effect: objective.clone(),
				proposal_risk: WireText::new(
					"The finite WorkItem may not produce the stated observable outcome.",
				)
				.expect("fixed Program risk is bounded"),
				proposal_evidence_need: WireText::new(
					"A settled Codex run, deterministic validation, and external evidence.",
				)
				.expect("fixed Program evidence need is bounded"),
				objective_outcome: objective.clone(),
				acceptance_criteria: vec![objective],
				validation_criteria: vec![
					WireText::new(
						"The bound Conversation settles and the review cites reproducible evidence.",
					)
					.expect("fixed Program validation criterion is bounded"),
				],
				work_item_title: text(&self.program_continuation_inputs.work_item_title)?,
				work_item_instructions: text(
					&self.program_continuation_inputs.work_item_instructions,
				)?,
				working_directory,
			})
		};
		match build().and_then(|draft| programs.continue_program(draft, cycle.program.revision)) {
			Ok(()) => {
				self.program_continuation_visible = false;
				self.program_status = Some("Appending the next Program cycle…".into());
			},
			Err(error) => self.program_status = Some(program_error_label(error).into()),
		}
		self.programs_snapshot = Some(programs.snapshot());
		cx.notify();
	}

	fn refresh_program(&mut self, cx: &mut Context<Self>) {
		let Some(programs) = self.programs.as_ref() else {
			return;
		};
		self.program_status =
			programs.refresh_selected().err().map(program_error_label).map(Into::into);
		self.programs_snapshot = Some(programs.snapshot());
		cx.notify();
	}

	fn select_program_node(&mut self, node_id: EntityId, cx: &mut Context<Self>) {
		self.program_graph.update(cx, |graph, cx| {
			graph.select(node_id, cx);
		});
	}

	fn start_program_work_item(&mut self, cx: &mut Context<Self>) {
		let Some(cycle) =
			self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref())
		else {
			return;
		};
		let Some(work_item) =
			cycle.nodes.iter().rev().find(|node| {
				node.kind == ProgramNodeKind::WorkItem && node.state.as_str() == "ready"
			})
		else {
			self.program_status = Some("The Program WorkItem is not ready to start.".into());
			cx.notify();
			return;
		};
		let Some(directory) = work_item
			.fields
			.iter()
			.find(|field| field.label.as_str() == "Working directory")
			.and_then(|field| ConversationWorkingDirectory::new(field.value.as_str()).ok())
		else {
			self.program_status = Some("The WorkItem working directory is invalid.".into());
			cx.notify();
			return;
		};
		cx.emit(FactoryEvent::StartProgramWorkItem {
			work_item_id: work_item.id.clone(),
			message: work_item.summary.as_str().to_owned(),
			working_directory: directory,
		});
		self.program_status = Some("Starting the bound Codex Conversation…".into());
		cx.notify();
	}

	fn record_program_review(&mut self, cx: &mut Context<Self>) {
		let Some(programs) = self.programs.as_ref() else {
			return;
		};
		let Some(cycle) =
			self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref())
		else {
			self.program_status = Some("No Program cycle is selected.".into());
			cx.notify();
			return;
		};
		let Some(work_item) =
			cycle.nodes.iter().rev().find(|node| node.kind == ProgramNodeKind::WorkItem)
		else {
			return;
		};
		let build = || -> Result<ProgramReviewDraftDto, ProgramInputError> {
			let text = |input: &Entity<ComposerInput>| {
				let value = input.read(cx).content().trim().to_owned();
				if value.is_empty() {
					return Err(ProgramInputError::InvalidDraft);
				}
				WireText::new(value).map_err(|_| ProgramInputError::InvalidDraft)
			};
			let observed_at_micros = current_micros()?;
			Ok(ProgramReviewDraftDto {
				review_id: entity_id()?,
				program_id: cycle.program.program_id.clone(),
				work_item_id: work_item.id.clone(),
				deterministic: decodex_protocol::ProgramEvidenceDraftDto {
					evidence_id: entity_id()?,
					source: WireText::new("Local deterministic validation")
						.expect("fixed evidence source is bounded"),
					summary: text(&self.program_review_inputs.deterministic)?,
					observed_at_micros,
				},
				external: decodex_protocol::ProgramEvidenceDraftDto {
					evidence_id: entity_id()?,
					source: text(&self.program_review_inputs.external_source)?,
					summary: text(&self.program_review_inputs.external)?,
					observed_at_micros,
				},
				classification: self.program_review_classification,
				rationale: text(&self.program_review_inputs.rationale)?,
			})
		};
		match build().and_then(|review| programs.record_review(review)) {
			Ok(()) => self.program_status = Some("Recording evidence and Program Review…".into()),
			Err(error) => self.program_status = Some(program_error_label(error).into()),
		}
		self.programs_snapshot = Some(programs.snapshot());
		cx.notify();
	}

	fn program_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
		let snapshot = self.programs_snapshot.as_ref().expect("Program toolbar has a snapshot");
		let selected = snapshot.selected.clone();
		let tabs = snapshot.programs.iter().enumerate().map(|(index, program)| {
			let program_id = program.program_id.clone();
			let active = selected.as_ref() == Some(&program_id);
			let label = program.name.as_str().to_owned();
			div()
				.id(("program-tab", index))
				.role(Role::Tab)
				.aria_label(format!("Select Program {label}"))
				.aria_selected(active)
				.h(px(28.0))
				.px_3()
				.flex()
				.items_center()
				.rounded(px(7.0))
				.border_1()
				.border_color(if active { rgba(0xffffff22) } else { rgba(0xffffff0d) })
				.bg(if active { rgba(0xffffff10) } else { rgba(0x00000000) })
				.text_size(px(9.0))
				.text_color(if active { rgb(TEXT) } else { rgb(TEXT_MUTED) })
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(TEXT)))
				.on_click(cx.listener(move |surface, _, _, cx| {
					surface.select_program(program_id.clone(), cx);
				}))
				.child(label)
		});
		let current = snapshot
			.selected_program()
			.map(|program| program.purpose.as_str())
			.unwrap_or("No active Program")
			.to_owned();

		div()
			.id("program-toolbar")
			.role(Role::Navigation)
			.aria_label("Program controls")
			.h(px(48.0))
			.min_h(px(48.0))
			.px_3()
			.flex()
			.items_center()
			.gap_3()
			.border_b_1()
			.border_color(rgba(0xffffff0e))
			.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
			.child(
				div()
					.w(px(250.0))
					.min_w(px(200.0))
					.flex()
					.flex_col()
					.gap_1()
					.child(
						div()
							.text_size(px(11.0))
							.font_weight(FontWeight::SEMIBOLD)
							.child("ADAPTIVE FACTORY"),
					)
					.child(
						div()
							.max_w(px(250.0))
							.overflow_hidden()
							.whitespace_nowrap()
							.text_ellipsis()
							.font_family("SF Mono")
							.text_size(px(8.0))
							.text_color(rgb(TEXT_FAINT))
							.child(current),
					),
			)
			.child(
				div()
					.id("program-tabs")
					.flex_1()
					.min_w_0()
					.flex()
					.items_center()
					.gap_1()
					.overflow_x_scroll()
					.children(tabs),
			)
			.child(
				div()
					.flex()
					.items_center()
					.gap_2()
					.child(
						div()
							.font_family("SF Mono")
							.text_size(px(8.0))
							.text_color(program_command_color(snapshot.command))
							.child(program_command_label(snapshot.command)),
					)
					.child(program_toolbar_button(
						"program-refresh",
						"REFRESH",
						cx,
						|surface, cx| {
							surface.refresh_program(cx);
						},
					))
					.child(program_toolbar_button(
						"program-new",
						"NEW PROGRAM",
						cx,
						|surface, cx| {
							surface.program_intake_visible = !surface.program_intake_visible;
							cx.notify();
						},
					)),
			)
			.into_any_element()
	}

	fn program_factory_canvas(&self, cx: &mut Context<Self>) -> AnyElement {
		let snapshot = self.programs_snapshot.as_ref().expect("Program canvas has a snapshot");
		if self.program_intake_visible
			|| snapshot.load == ProgramsLoadState::NoPrograms
			|| snapshot.cycle.is_none()
		{
			return self.program_intake(snapshot, cx);
		}
		let cycle = snapshot.cycle.as_ref().expect("checked Program cycle");
		let selected = self.program_graph.read(cx).selected().cloned();
		let selected_node = selected
			.as_ref()
			.and_then(|selected| cycle.nodes.iter().find(|node| &node.id == selected));
		let selected_domain = selected.as_ref().and_then(|selected| {
			cycle
				.domain_pack
				.as_ref()
				.and_then(|pack| pack.entities.iter().find(|entity| &entity.id == selected))
		});
		let latest_work_item =
			cycle.nodes.iter().rev().find(|node| node.kind == ProgramNodeKind::WorkItem);
		let latest_run = latest_work_item
			.and_then(|work_item| work_item.conversation_id.as_ref())
			.and_then(|conversation_id| {
				cycle.nodes.iter().find(|node| {
					node.kind == ProgramNodeKind::Run
						&& node.conversation_id.as_ref() == Some(conversation_id)
				})
			});
		let can_review = latest_work_item.is_some_and(|node| node.state.as_str() == "running")
			&& latest_run.is_some_and(|node| node.state.as_str() == "ready");
		let can_continue = cycle.program.state.as_str() == "active"
			&& cycle.nodes.last().is_some_and(|node| node.kind == ProgramNodeKind::Review)
			&& cycle.nodes.len().saturating_add(COMPLETE_PROGRAM_CYCLE_NODE_COST)
				<= MAX_PROGRAM_NODES
			&& snapshot.can_mutate;

		div()
			.id("program-factory-canvas")
			.role(Role::Group)
			.aria_label("Authoritative Program causal graph")
			.flex_1()
			.min_h_0()
			.h_full()
			.w_full()
			.min_w(px(FACTORY_MIN_WIDTH))
			.flex()
			.bg(rgba(ui_theme::SURFACE_MATERIAL))
			.child(self.program_pulse(cycle, snapshot, cx))
			.child(
				div()
					.flex_1()
					.min_w_0()
					.min_h_0()
					.p_4()
					.flex()
					.flex_col()
					.gap_3()
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(
								div()
									.flex()
									.flex_col()
									.gap_1()
									.child(
										div()
											.text_size(px(12.0))
											.font_weight(FontWeight::SEMIBOLD)
											.child("CAUSAL GRAPH"),
									)
									.child(
										div()
											.font_family("SF Mono")
											.text_size(px(8.0))
											.text_color(rgb(TEXT_FAINT))
											.child(format!(
												"{} ACCEPTED ENTITIES · {} DERIVED RELATIONS",
												cycle.nodes.len() + 1,
												cycle.edges.len()
											)),
									),
							)
							.child(
								div()
									.font_family("SF Mono")
									.text_size(px(8.0))
									.text_color(rgb(GREEN))
									.child("SQLITE AUTHORITY · LIVE PROJECTION"),
							),
					)
					.child(self.program_graph.clone()),
			)
			.child(self.program_inspector(
				ProgramInspectorSelection {
					node: selected_node,
					domain: selected_domain,
					program: selected.as_ref() == Some(&cycle.program.program_id),
				},
				cycle,
				can_review,
				can_continue,
				cx,
			))
			.into_any_element()
	}

	fn program_intake(&self, snapshot: &ProgramsSnapshot, cx: &mut Context<Self>) -> AnyElement {
		let inputs = self.program_inputs.all();
		let selected_pack = self.program_pack;
		let pack_choices = ProgramPackChoice::ALL
			.into_iter()
			.map(|pack| program_pack_choice(pack, pack == selected_pack, cx))
			.collect::<Vec<_>>();
		let labels = [
			"PROGRAM NAME",
			"PURPOSE",
			"NON-GOAL",
			"REVIEW POLICY",
			"SIGNAL SOURCE",
			"SIGNAL",
			"CLAIM",
			"PROPOSAL",
			"OBJECTIVE",
			"WORK ITEM",
			"CODEX INSTRUCTIONS",
			"WORKING DIRECTORY",
		];
		let fields = inputs
			.into_iter()
			.zip(labels)
			.enumerate()
			.map(|(index, (input, label))| program_input_field(index, label, input.clone()));
		div()
            .id("program-intake")
            .flex_1()
            .min_h_0()
            .w_full()
            .p_5()
            .flex()
            .justify_center()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(940.0))
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(18.0))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Create one closed Program cycle"),
                                    )
                                    .child(
                                        div()
                                            .max_w(px(650.0))
                                            .text_size(px(10.5))
                                            .text_color(rgb(TEXT_MUTED))
                                            .child("One signal becomes one explicit claim, proposal, objective and Codex WorkItem. The proposal itself never executes."),
                                    ),
                            )
                            .child(
                                div()
                                    .font_family("SF Mono")
                                    .text_size(px(8.0))
                                    .text_color(load_color(snapshot.load))
                                    .child(program_load_label(snapshot.load)),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(
                                    div()
                                        .font_family("SF Mono")
                                        .text_size(px(8.0))
                                        .text_color(rgb(TEXT_FAINT))
                                        .child("DOMAIN PACK · IMMUTABLE AFTER CREATE"),
                                )
                                .child(div().grid().grid_cols(2).gap_3().children(pack_choices)),
                        )
                        .when(
                            self.program_pack == ProgramPackChoice::PaperInvestment,
                            |form| {
                                form.child(program_action_button(
                                    "load-paper-example",
                                    "LOAD TREASURY EXAMPLE",
                                    GREEN,
                                    true,
                                    cx,
                                    |surface, cx| surface.load_paper_example(cx),
                                ))
                            },
                        )
                        .child(
                            div()
                            .grid()
                            .grid_cols(2)
                            .gap_3()
                            .children(fields),
                    )
                    .when_some(self.program_status.clone(), |form, status| {
                        form.child(div().text_size(px(9.0)).text_color(rgb(TEXT_MUTED)).child(status))
                    })
                    .child(
                        div()
                            .id("create-program-cycle")
                            .role(Role::Button)
                            .aria_label("Create persisted Program cycle")
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.0))
                            .bg(if snapshot.can_mutate { rgb(TEXT) } else { rgb(SURFACE_OVERLAY) })
                            .text_size(px(10.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if snapshot.can_mutate { rgb(SURFACE) } else { rgb(TEXT_FAINT) })
                            .when(snapshot.can_mutate, |button| {
                                button
                                    .cursor_pointer()
                                    .hover(|style| style.opacity(0.9))
                                    .on_click(cx.listener(|surface, _, _, cx| surface.create_program(cx)))
                            })
                            .child("CREATE PROGRAM CYCLE"),
                    ),
            )
            .into_any_element()
	}

	fn program_pulse(
		&self,
		cycle: &decodex_protocol::ProgramCycleDto,
		snapshot: &ProgramsSnapshot,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let current_work_item =
			cycle.nodes.iter().rev().find(|node| node.kind == ProgramNodeKind::WorkItem);
		let work_item_ready = current_work_item.is_some_and(|node| node.state.as_str() == "ready")
			&& cycle.domain_pack.is_some();
		let conversation_id = current_work_item.and_then(|node| node.conversation_id.clone());
		let cycle_count =
			cycle.nodes.iter().filter(|node| node.kind == ProgramNodeKind::Signal).count();
		div()
			.id("program-pulse")
			.w(px(258.0))
			.min_w(px(258.0))
			.p_4()
			.flex()
			.flex_col()
			.gap_4()
			.border_r_1()
			.border_color(rgba(0xffffff10))
			.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
			.child(
				div()
					.flex()
					.flex_col()
					.gap_2()
					.child(
						div()
							.flex()
							.items_center()
							.gap_2()
							.child(div().size(px(7.0)).rounded_full().bg(rgb(GREEN)))
							.child(
								div()
									.font_family("SF Mono")
									.text_size(px(8.0))
									.text_color(rgb(GREEN))
									.child(cycle.program.state.as_str().to_uppercase()),
							),
					)
					.child(
						div()
							.text_size(px(16.0))
							.font_weight(FontWeight::SEMIBOLD)
							.child(cycle.program.name.as_str().to_owned()),
					)
					.child(
						div()
							.text_size(px(10.0))
							.text_color(rgb(TEXT_MUTED))
							.child(cycle.program.purpose.as_str().to_owned()),
					),
			)
			.child(program_pulse_section("REVIEW POLICY", cycle.review_policy.as_str()))
			.child(program_pulse_section("CURRENT CYCLE", &format!("C{cycle_count}")))
			.child(program_pulse_section(
				"NON-GOAL",
				cycle.non_goals.first().map_or("None", WireText::as_str),
			))
			.child(program_pulse_section(
				"DOMAIN PACK",
				cycle.domain_pack.as_ref().map_or("UNBOUND", |pack| pack.descriptor.name.as_str()),
			))
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.font_family("SF Mono")
					.text_size(px(8.0))
					.text_color(rgb(TEXT_FAINT))
					.child(format!("REV {}", cycle.program.revision.0))
					.child(format!("{} NODES", cycle.nodes.len())),
			)
			.when(work_item_ready, |panel| {
				panel.child(program_action_button(
					"start-program-work-item",
					"START CODEX WORK",
					GREEN,
					snapshot.can_mutate,
					cx,
					|surface, cx| surface.start_program_work_item(cx),
				))
			})
			.when_some(conversation_id, |panel, conversation_id| {
				panel.child(program_action_button(
					"open-program-conversation",
					"OPEN CODEX CONVERSATION",
					BLUE,
					true,
					cx,
					move |_, cx| {
						cx.emit(FactoryEvent::OpenProgramConversation {
							conversation_id: conversation_id.clone(),
						});
					},
				))
			})
			.when_some(self.program_status.clone(), |panel, status| {
				panel.child(
					div().mt_auto().text_size(px(9.0)).text_color(rgb(TEXT_MUTED)).child(status),
				)
			})
			.into_any_element()
	}

	fn program_inspector(
		&self,
		selection: ProgramInspectorSelection<'_>,
		cycle: &decodex_protocol::ProgramCycleDto,
		can_review: bool,
		can_continue: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut panel = div()
			.id("program-inspector")
			.w(px(292.0))
			.min_w(px(292.0))
			.p_4()
			.flex()
			.flex_col()
			.gap_3()
			.border_l_1()
			.border_color(rgba(0xffffff10))
			.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
			.overflow_y_scroll()
			.child(
				div()
					.font_family("SF Mono")
					.text_size(px(8.0))
					.text_color(rgb(TEXT_FAINT))
					.child("SEMANTIC INSPECTOR"),
			);
		if selection.program {
			panel = panel
				.child(
					div()
						.flex()
						.items_center()
						.gap_2()
						.child(div().size(px(8.0)).rounded_full().bg(rgb(TEXT)))
						.child(
							div()
								.text_size(px(13.0))
								.font_weight(FontWeight::SEMIBOLD)
								.child(cycle.program.name.as_str().to_owned()),
						),
				)
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(TEXT_MUTED))
						.child(format!(
							"PROGRAM · {} · REV {}",
							cycle.program.state.as_str(),
							cycle.program.revision.0
						)),
				)
				.child(
					div()
						.text_size(px(10.0))
						.text_color(rgb(TEXT_MUTED))
						.child(cycle.program.purpose.as_str().to_owned()),
				)
				.child(program_pulse_section("IDENTITY", cycle.program.program_id.as_str()))
				.child(program_pulse_section("REVIEW POLICY", cycle.review_policy.as_str()));
		}
		if let Some(node) = selection.node {
			panel = panel
				.child(
					div()
						.flex()
						.items_center()
						.gap_2()
						.child(div().size(px(8.0)).bg(rgb(program_node_color(node.kind))))
						.child(
							div()
								.text_size(px(13.0))
								.font_weight(FontWeight::SEMIBOLD)
								.child(node.title.as_str().to_owned()),
						),
				)
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(program_node_color(node.kind)))
						.child(format!("{} · {}", node_kind_label(node.kind), node.state.as_str())),
				)
				.child(
					div()
						.text_size(px(10.0))
						.text_color(rgb(TEXT_MUTED))
						.child(node.summary.as_str().to_owned()),
				)
				.when_some(node.source.as_ref(), |panel, source| {
					panel.child(program_pulse_section("SOURCE", source.as_str()))
				});
			for field in &node.fields {
				panel =
					panel.child(program_pulse_section(field.label.as_str(), field.value.as_str()));
			}
			if let Some(conversation_id) = node.conversation_id.clone() {
				panel = panel.child(program_action_button(
					"inspect-program-conversation",
					"OPEN CONVERSATION",
					BLUE,
					true,
					cx,
					move |_, cx| {
						cx.emit(FactoryEvent::OpenProgramConversation {
							conversation_id: conversation_id.clone(),
						});
					},
				));
			}
		}
		if let Some(entity) = selection.domain {
			let color = domain_entity_color(entity.kind.as_str());
			panel = panel
				.child(
					div()
						.flex()
						.items_center()
						.gap_2()
						.child(div().size(px(8.0)).rounded_full().bg(rgb(color)))
						.child(
							div()
								.text_size(px(13.0))
								.font_weight(FontWeight::SEMIBOLD)
								.child(entity.title.as_str().to_owned()),
						),
				)
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(color))
						.child(format!("{} · {}", entity.kind.as_str(), entity.state.as_str())),
				)
				.child(
					div()
						.text_size(px(10.0))
						.text_color(rgb(TEXT_MUTED))
						.child(entity.summary.as_str().to_owned()),
				)
				.when_some(entity.source.as_ref(), |panel, source| {
					panel.child(program_pulse_section("SOURCE", source.as_str()))
				});
			for field in &entity.fields {
				panel =
					panel.child(program_pulse_section(field.label.as_str(), field.value.as_str()));
			}
		}
		if let Some(pack) = cycle.domain_pack.as_ref() {
			panel = panel
				.child(
					div()
						.mt_2()
						.pt_3()
						.border_t_1()
						.border_color(rgba(0xffffff12))
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(TEXT_FAINT))
						.child("DOMAIN PACK CONTRACT"),
				)
				.child(program_pulse_section("PACK", pack.descriptor.id.as_str()))
				.child(program_pulse_section("VERSION", pack.descriptor.version.as_str()))
				.child(program_pulse_section("DIGEST", &pack.descriptor.digest.as_str()[..12]))
				.child(program_pulse_section(
					"SCHEMA",
					&format!(
						"{} entity types · {} relation types",
						pack.descriptor.entity_types.len(),
						pack.descriptor.relation_types.len()
					),
				));
			for capability in &pack.descriptor.capabilities {
				let state = match capability.status {
					DomainPackCapabilityStatus::Granted => "GRANTED",
					DomainPackCapabilityStatus::Unavailable => "UNAVAILABLE",
				};
				panel = panel.child(program_pulse_section(capability.id.as_str(), state));
			}
			panel = panel.child(program_pulse_section("UNDECLARED CAPABILITIES", "DENIED"));
		} else {
			let can_bind =
				self.programs_snapshot.as_ref().is_some_and(|snapshot| snapshot.can_mutate);
			panel = panel
				.child(program_pulse_section("DOMAIN PACK", "UNBOUND LEGACY PROGRAM"))
				.child(program_action_button(
					"bind-development-pack",
					"BIND DEVELOPMENT PACK",
					BLUE,
					can_bind,
					cx,
					|surface, cx| {
						surface.bind_selected_domain_pack(ProgramPackChoice::Development, cx);
					},
				))
				.child(program_action_button(
					"bind-paper-pack",
					"BIND PAPER PACK",
					GREEN,
					can_bind,
					cx,
					|surface, cx| {
						surface.bind_selected_domain_pack(ProgramPackChoice::PaperInvestment, cx);
					},
				));
		}
		if can_review {
			panel = panel.child(self.program_review_form(cycle, cx));
		}
		if can_continue {
			panel = if self.program_continuation_visible {
				panel.child(self.program_continuation_form(cycle, cx))
			} else {
				panel.child(program_action_button(
					"show-program-continuation",
					"CONTINUE PROGRAM",
					GREEN,
					true,
					cx,
					|surface, cx| {
						surface.program_continuation_visible = true;
						cx.notify();
					},
				))
			};
		}
		panel.into_any_element()
	}

	fn program_continuation_form(
		&self,
		cycle: &decodex_protocol::ProgramCycleDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let labels = [
			"SIGNAL SOURCE",
			"SIGNAL",
			"CLAIM",
			"PROPOSAL",
			"OBJECTIVE",
			"WORK ITEM",
			"CODEX INSTRUCTIONS",
			"WORKING DIRECTORY",
		];
		let fields =
			self.program_continuation_inputs.all().into_iter().zip(labels).enumerate().map(
				|(index, (input, label))| program_input_field(index + 20, label, input.clone()),
			);
		let next_cycle =
			cycle.nodes.iter().filter(|node| node.kind == ProgramNodeKind::Signal).count() + 1;
		div()
			.mt_3()
			.pt_4()
			.flex()
			.flex_col()
			.gap_2()
			.border_t_1()
			.border_color(rgba(0xffffff12))
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.child(
						div()
							.text_size(px(11.0))
							.font_weight(FontWeight::SEMIBOLD)
							.child(format!("NEXT CYCLE · C{next_cycle}")),
					)
					.child(
						div()
							.font_family("SF Mono")
							.text_size(px(7.5))
							.text_color(rgb(TEXT_FAINT))
							.child(format!("REV {}", cycle.program.revision.0)),
					),
			)
			.child(div().text_size(px(8.5)).text_color(rgb(TEXT_FAINT)).child(
				"Manual continuation preserves the prior Review and replaces any unresolved Objective.",
			))
			.children(fields)
			.child(program_action_button(
				"append-program-cycle",
				"APPEND CYCLE",
				GREEN,
				true,
				cx,
				|surface, cx| surface.continue_program(cx),
			))
			.child(program_action_button(
				"cancel-program-continuation",
				"CANCEL",
				TEXT_MUTED,
				true,
				cx,
				|surface, cx| {
					surface.program_continuation_visible = false;
					cx.notify();
				},
			))
			.into_any_element()
	}

	fn program_review_form(
		&self,
		_: &decodex_protocol::ProgramCycleDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let classifications = [
			ProgramReviewClassification::OutcomeProgress,
			ProgramReviewClassification::KnowledgeProgress,
			ProgramReviewClassification::CapabilityProgress,
			ProgramReviewClassification::NoMaterialChange,
			ProgramReviewClassification::Regression,
			ProgramReviewClassification::Unknown,
		]
		.into_iter()
		.enumerate()
		.map(|(index, classification)| {
			let active = classification == self.program_review_classification;
			div()
				.id(("program-review-classification", index))
				.role(Role::Button)
				.aria_label(classification.as_str())
				.px_2()
				.h(px(25.0))
				.flex()
				.items_center()
				.rounded(px(6.0))
				.border_1()
				.border_color(if active { rgb(GREEN) } else { rgb(LINE_MUTED) })
				.text_size(px(7.5))
				.text_color(if active { rgb(GREEN) } else { rgb(TEXT_FAINT) })
				.cursor_pointer()
				.on_click(cx.listener(move |surface, _, _, cx| {
					surface.program_review_classification = classification;
					cx.notify();
				}))
				.child(classification.as_str().replace('_', " "))
		});
		div()
			.mt_3()
			.pt_4()
			.flex()
			.flex_col()
			.gap_2()
			.border_t_1()
			.border_color(rgba(0xffffff12))
			.child(
				div().text_size(px(11.0)).font_weight(FontWeight::SEMIBOLD).child("PROGRAM REVIEW"),
			)
			.child(
				div()
					.text_size(px(8.5))
					.text_color(rgb(TEXT_FAINT))
					.child("Evidence closes the loop. Unknown remains a valid result."),
			)
			.child(div().h(px(42.0)).child(self.program_review_inputs.deterministic.clone()))
			.child(div().h(px(42.0)).child(self.program_review_inputs.external_source.clone()))
			.child(div().h(px(42.0)).child(self.program_review_inputs.external.clone()))
			.child(div().h(px(52.0)).child(self.program_review_inputs.rationale.clone()))
			.child(div().flex().flex_wrap().gap_1().children(classifications))
			.child(program_action_button(
				"record-program-review",
				"RECORD REVIEW",
				GREEN,
				true,
				cx,
				|surface, cx| surface.record_program_review(cx),
			))
			.into_any_element()
	}

	fn program_timeline(&self, cx: &mut Context<Self>) -> AnyElement {
		let Some(cycle) =
			self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref())
		else {
			return div().into_any_element();
		};
		let selected = self.program_graph.read(cx).selected().cloned();
		let origin = cycle.nodes.iter().filter_map(|node| node.observed_at_micros).min();
		let mut cycle_number = 0;
		let moments = cycle.nodes.iter().enumerate().map(|(index, node)| {
			if node.kind == ProgramNodeKind::Signal {
				cycle_number += 1;
			}
			let node_id = node.id.clone();
			let active = selected.as_ref() == Some(&node.id);
			let color = program_node_color(node.kind);
			let time = match (origin, node.observed_at_micros) {
				(Some(origin), Some(value)) => relative_timeline_time(value.saturating_sub(origin)),
				_ => format!("STEP {}", index + 1),
			};
			div()
				.id(("program-timeline-node", index))
				.role(Role::Button)
				.aria_label(format!("Inspect timeline node {}", node.title.as_str()))
				.w(px(138.0))
				.min_w(px(138.0))
				.flex()
				.flex_col()
				.items_center()
				.gap_1()
				.text_center()
				.cursor_pointer()
				.on_click(cx.listener(move |surface, _, _, cx| {
					surface.select_program_node(node_id.clone(), cx);
				}))
				.child(
					div()
						.size(px(if active { 13.0 } else { 9.0 }))
						.rounded(px(3.0))
						.border_1()
						.border_color(rgb(color))
						.when(active, |marker| marker.bg(rgb(color))),
				)
				.child(
					div()
						.max_w(px(132.0))
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.text_size(px(9.0))
						.text_color(rgb(if active { TEXT } else { TEXT_MUTED }))
						.child(node.title.as_str().to_owned()),
				)
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(7.5))
						.text_color(rgb(TEXT_FAINT))
						.child(format!("C{cycle_number} · {time}")),
				)
		});
		div()
			.id("program-causal-timeline")
			.role(Role::Group)
			.aria_label("Program causal timeline")
			.h(px(138.0))
			.min_h(px(138.0))
			.w_full()
			.px_4()
			.flex()
			.items_center()
			.gap_4()
			.border_t_1()
			.border_color(rgba(0xffffff12))
			.bg(rgba(ui_theme::SURFACE_OVERLAY_MATERIAL))
			.child(
				div()
					.w(px(148.0))
					.min_w(px(148.0))
					.flex()
					.flex_col()
					.gap_1()
					.child(
						div()
							.text_size(px(11.0))
							.font_weight(FontWeight::SEMIBOLD)
							.child("CAUSAL TIMELINE"),
					)
					.child(
						div()
							.font_family("SF Mono")
							.text_size(px(8.0))
							.text_color(rgb(TEXT_FAINT))
							.child("RECONSTRUCTED FROM ACCEPTED FACTS"),
					),
			)
			.child(
				div()
					.id("program-timeline-scroll")
					.flex_1()
					.min_w_0()
					.flex()
					.items_center()
					.overflow_x_scroll()
					.children(moments),
			)
			.into_any_element()
	}
}

impl Render for FactorySurface {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let body = if self.programs_snapshot.is_some() {
			div()
				.flex_1()
				.min_h_0()
				.flex()
				.flex_col()
				.child(self.program_toolbar(cx))
				.child(self.program_factory_canvas(cx))
				.child(self.program_timeline(cx))
				.into_any_element()
		} else {
			div()
				.flex_1()
				.flex()
				.items_center()
				.justify_center()
				.text_size(px(11.0))
				.text_color(rgb(TEXT_MUTED))
				.child("Program authority is not connected.")
				.into_any_element()
		};
		div()
			.id("adaptive-program-factory")
			.role(Role::Main)
			.aria_label("Adaptive Program factory")
			.size_full()
			.min_w(px(FACTORY_MIN_WIDTH))
			.flex()
			.flex_col()
			.bg(rgba(ui_theme::SURFACE_MATERIAL))
			.text_color(rgb(TEXT))
			.child(body)
	}
}

fn current_micros() -> Result<i64, ProgramInputError> {
	let micros = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_err(|_| ProgramInputError::IdentityUnavailable)?
		.as_micros();
	i64::try_from(micros).map_err(|_| ProgramInputError::IdentityUnavailable)
}

const fn program_error_label(error: ProgramInputError) -> &'static str {
	match error {
		ProgramInputError::Offline => "Program authority is offline.",
		ProgramInputError::Busy => "Wait for the current Program operation.",
		ProgramInputError::InvalidDraft =>
			"Complete every field with bounded credential-free text.",
		ProgramInputError::NoSelection => "Select one Program first.",
		ProgramInputError::IdentityUnavailable => "A stable Program identity could not be created.",
	}
}

const fn program_load_label(load: ProgramsLoadState) -> &'static str {
	match load {
		ProgramsLoadState::NeverRequested => "AUTHORITY · waiting",
		ProgramsLoadState::LoadingPrograms => "AUTHORITY · loading programs",
		ProgramsLoadState::LoadingCycle => "AUTHORITY · loading causal cycle",
		ProgramsLoadState::Ready => "SQLITE AUTHORITY · current",
		ProgramsLoadState::NoPrograms => "NO PROGRAMS · create the first closed cycle",
		ProgramsLoadState::Offline => "AUTHORITY · offline",
		ProgramsLoadState::Unavailable => "AUTHORITY · unavailable",
		ProgramsLoadState::Refused => "AUTHORITY · refused unsafe projection",
	}
}

fn load_color(load: ProgramsLoadState) -> gpui::Rgba {
	rgb(match load {
		ProgramsLoadState::Ready => GREEN,
		ProgramsLoadState::LoadingPrograms | ProgramsLoadState::LoadingCycle => AMBER,
		ProgramsLoadState::NeverRequested | ProgramsLoadState::NoPrograms => TEXT_FAINT,
		ProgramsLoadState::Offline
		| ProgramsLoadState::Unavailable
		| ProgramsLoadState::Refused => 0xef6b73,
	})
}

const fn program_command_label(command: ProgramCommandState) -> &'static str {
	match command {
		ProgramCommandState::Idle => "READY",
		ProgramCommandState::Sending => "SENDING",
		ProgramCommandState::AwaitingResult => "AWAITING COMMIT",
		ProgramCommandState::Accepted => "COMMITTED",
		ProgramCommandState::OutcomeUnknown => "READBACK REQUIRED",
		ProgramCommandState::Refused => "REFUSED",
	}
}

fn program_command_color(command: ProgramCommandState) -> gpui::Rgba {
	rgb(match command {
		ProgramCommandState::Idle | ProgramCommandState::Accepted => GREEN,
		ProgramCommandState::Sending | ProgramCommandState::AwaitingResult => AMBER,
		ProgramCommandState::OutcomeUnknown | ProgramCommandState::Refused => 0xef6b73,
	})
}

fn program_toolbar_button(
	id: &'static str,
	label: &'static str,
	cx: &mut Context<FactorySurface>,
	on_click: impl Fn(&mut FactorySurface, &mut Context<FactorySurface>) + 'static,
) -> AnyElement {
	div()
		.id(id)
		.role(Role::Button)
		.aria_label(label)
		.h(px(28.0))
		.px_3()
		.flex()
		.items_center()
		.rounded(px(7.0))
		.border_1()
		.border_color(rgba(0xffffff12))
		.text_size(px(8.5))
		.text_color(rgb(TEXT_MUTED))
		.cursor_pointer()
		.hover(|style| style.bg(rgba(0xffffff0d)).text_color(rgb(TEXT)))
		.active(|style| style.opacity(0.8))
		.on_click(cx.listener(move |surface, _, _, cx| on_click(surface, cx)))
		.child(label)
		.into_any_element()
}

fn program_input_field(
	index: usize,
	label: &'static str,
	input: Entity<ComposerInput>,
) -> AnyElement {
	div()
		.id(("program-input-field", index))
		.flex()
		.flex_col()
		.gap_1()
		.child(
			div()
				.font_family("SF Mono")
				.text_size(px(7.5))
				.text_color(rgb(TEXT_FAINT))
				.child(label),
		)
		.child(div().h(px(42.0)).child(input))
		.into_any_element()
}

fn program_pulse_section(label: &str, value: &str) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.gap_1()
		.child(
			div()
				.font_family("SF Mono")
				.text_size(px(7.5))
				.text_color(rgb(TEXT_FAINT))
				.child(label.to_owned()),
		)
		.child(div().text_size(px(9.5)).text_color(rgb(TEXT_MUTED)).child(value.to_owned()))
		.into_any_element()
}

fn program_action_button(
	id: &'static str,
	label: &'static str,
	color: u32,
	enabled: bool,
	cx: &mut Context<FactorySurface>,
	on_click: impl Fn(&mut FactorySurface, &mut Context<FactorySurface>) + 'static,
) -> AnyElement {
	div()
		.id(id)
		.role(Role::Button)
		.aria_label(label)
		.h(px(34.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.border_1()
		.border_color(if enabled { rgb(color) } else { rgb(LINE_MUTED) })
		.text_size(px(8.5))
		.font_weight(FontWeight::SEMIBOLD)
		.text_color(if enabled { rgb(color) } else { rgb(TEXT_FAINT) })
		.when(enabled, |button| {
			button
				.cursor_pointer()
				.hover(|style| style.bg(rgba(0xffffff0d)))
				.on_click(cx.listener(move |surface, _, _, cx| on_click(surface, cx)))
		})
		.child(label)
		.into_any_element()
}

fn relative_timeline_time(delta_micros: i64) -> String {
	if delta_micros >= 1_000_000 {
		format!("T+{}s", delta_micros / 1_000_000)
	} else if delta_micros >= 1_000 {
		format!("T+{}ms", delta_micros / 1_000)
	} else {
		format!("T+{delta_micros}µs")
	}
}

fn program_pack_choice(
	pack: ProgramPackChoice,
	selected: bool,
	cx: &mut Context<FactorySurface>,
) -> AnyElement {
	let color = pack.color();
	div()
		.id(format!("program-pack/{}", pack.id()))
		.role(Role::RadioButton)
		.aria_label(format!("Select {} Domain Pack", pack.name()))
		.aria_selected(selected)
		.min_h(px(76.0))
		.p_3()
		.flex()
		.items_center()
		.gap_3()
		.border_1()
		.border_color(if selected { rgb(color) } else { rgba(0xffffff14) })
		.rounded(px(10.0))
		.bg(if selected { rgba(0xffffff0d) } else { rgba(0x00000000) })
		.cursor_pointer()
		.hover(|style| style.border_color(rgba(0xffffff32)).bg(rgba(0xffffff09)))
		.on_click(cx.listener(move |surface, _, _, cx| {
			surface.select_program_pack(pack, cx);
		}))
		.child(
			div()
				.size(px(10.0))
				.rounded_full()
				.border_1()
				.border_color(rgb(color))
				.when(selected, |dot| dot.bg(rgb(color))),
		)
		.child(
			div()
				.flex()
				.flex_col()
				.gap_1()
				.child(
					div().text_size(px(11.0)).font_weight(FontWeight::SEMIBOLD).child(pack.name()),
				)
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(7.5))
						.text_color(rgb(color))
						.child(pack.id()),
				)
				.child(div().text_size(px(8.5)).text_color(rgb(TEXT_MUTED)).child(pack.summary())),
		)
		.into_any_element()
}

fn domain_entity_color(kind: &str) -> u32 {
	if kind.starts_with("finance.") { GREEN } else { BLUE }
}

const fn program_node_color(kind: ProgramNodeKind) -> u32 {
	match kind {
		ProgramNodeKind::Signal | ProgramNodeKind::Claim => BLUE,
		ProgramNodeKind::Proposal | ProgramNodeKind::Objective => AMBER,
		ProgramNodeKind::WorkItem | ProgramNodeKind::Run => 0x8d7cf6,
		ProgramNodeKind::Evidence | ProgramNodeKind::Review => GREEN,
	}
}

const fn node_kind_label(kind: ProgramNodeKind) -> &'static str {
	match kind {
		ProgramNodeKind::Signal => "SIGNAL",
		ProgramNodeKind::Claim => "CLAIM",
		ProgramNodeKind::Proposal => "PROPOSAL",
		ProgramNodeKind::Objective => "OBJECTIVE",
		ProgramNodeKind::WorkItem => "WORK ITEM",
		ProgramNodeKind::Run => "CODEX RUN",
		ProgramNodeKind::Evidence => "EVIDENCE",
		ProgramNodeKind::Review => "REVIEW",
	}
}

pub(crate) fn app_icon_path() -> PathBuf {
	let packaged = std::env::current_exe()
		.ok()
		.and_then(|executable| executable.parent().map(std::path::Path::to_path_buf))
		.and_then(|macos| macos.parent().map(std::path::Path::to_path_buf))
		.map(|contents| contents.join("Resources/AppIcon.png"));

	packaged.filter(|path| path.is_file()).unwrap_or_else(|| {
		PathBuf::from(env!("CARGO_MANIFEST_DIR"))
			.join("../../assets/app-icon/generated/app-icon-flat.png")
	})
}
