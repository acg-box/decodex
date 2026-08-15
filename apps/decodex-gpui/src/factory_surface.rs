//! Codex-only Agent Factory presentation slice for the native GPUI control room.
//!
//! Operate mode renders authoritative internal WorkItems. Inspect and Replay remain
//! presentation previews until their corresponding graph projections exist.

use std::path::PathBuf;

use gpui::{
	Animation, AnimationExt, AnyElement, App, Bounds, BoxShadow, Context, Entity, EventEmitter,
	Focusable, FontWeight, Hsla, KeyBinding, PathBuilder, Pixels, Render, Role, SharedString,
	Window, actions, canvas, div, ease_in_out, point, prelude::*, px, rgb, rgba,
};

use decodex_protocol::{
	EntityId, MAX_PROGRAM_NODES, ProgramContinuationDraftDto, ProgramCycleDraftDto, ProgramNodeDto,
	ProgramNodeKind, ProgramReviewClassification, ProgramReviewDraftDto, QuickTaskWorkingDirectory,
	WireText, WorkItemBoardCard, WorkItemBoardProjectId, WorkItemState,
};

use crate::{
	composer_input::{ComposerEvent, ComposerInput, SubmitComposer},
	programs::{
		ProgramCommandState, ProgramInputError, Programs, ProgramsLoadState, ProgramsSnapshot,
		entity_id,
	},
	ui_theme,
	work_items::{
		WorkItemCommandState, WorkItemInputError, WorkItems, WorkItemsLoadState, WorkItemsSnapshot,
	},
};

const REPLAY_HEIGHT: f32 = 134.0;
const FACTORY_MIN_WIDTH: f32 = 1_180.0;
const COMPLETE_PROGRAM_CYCLE_NODE_COST: usize = 9;

const SURFACE: u32 = ui_theme::CANVAS;
const SURFACE_OVERLAY: u32 = ui_theme::SURFACE_OVERLAY;
const LINE: u32 = ui_theme::LINE_STRONG;
const LINE_MUTED: u32 = ui_theme::LINE;
const TEXT: u32 = ui_theme::TEXT;
const TEXT_MUTED: u32 = ui_theme::TEXT_MUTED;
const TEXT_FAINT: u32 = ui_theme::TEXT_FAINT;
const BLUE: u32 = ui_theme::BLUE;
const GREEN: u32 = ui_theme::GREEN;
const AMBER: u32 = ui_theme::AMBER;

actions!(factory_surface, [ToggleFactoryLauncher, CloseFactoryOverlay]);

pub(crate) fn bind_keys(cx: &mut App) {
	cx.bind_keys([
		KeyBinding::new("cmd-k", ToggleFactoryLauncher, None),
		KeyBinding::new("escape", CloseFactoryOverlay, None),
	]);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FactoryRoute {
	QuickTasks,
	Accounts,
	Health,
	Settings,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FactoryEvent {
	OpenRoute(FactoryRoute),
	StartCodexConversation {
		context: &'static str,
		message: String,
	},
	StartProgramWorkItem {
		work_item_id: EntityId,
		message: String,
		working_directory: QuickTaskWorkingDirectory,
	},
	OpenWorkItemConversation {
		conversation_id: EntityId,
	},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FactoryMode {
	Operate,
	Inspect,
	Replay,
}

impl FactoryMode {
	const ALL: [Self; 3] = [Self::Operate, Self::Inspect, Self::Replay];

	const fn label(self) -> &'static str {
		match self {
			Self::Operate => "OPERATE",
			Self::Inspect => "INSPECT",
			Self::Replay => "REPLAY",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FactorySelection {
	Brief,
	Coordinator,
	RuntimeWork,
	RuntimeAgent,
	GpuiWork,
	GpuiAgent,
	Artifact,
	Review,
	ReleaseGate,
	Policy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConversationTarget {
	Coordinator,
	RuntimeWork,
	RuntimeAgent,
	GpuiWork,
	GpuiAgent,
	Review,
}

impl ConversationTarget {
	const fn title(self) -> &'static str {
		match self {
			Self::Coordinator => "Coordinator · Codex Lead",
			Self::RuntimeWork => "Work · Runtime",
			Self::RuntimeAgent => "Codex Instance · Codex-1",
			Self::GpuiWork => "Work · GPUI",
			Self::GpuiAgent => "Codex Instance · Codex-2",
			Self::Review => "Review · Independent Codex",
		}
	}

	const fn context(self) -> &'static str {
		match self {
			Self::Coordinator => "Release vNext / coordinator",
			Self::RuntimeWork => "Release vNext / runtime work",
			Self::RuntimeAgent => "Release vNext / Codex-1 runtime instance",
			Self::GpuiWork => "Release vNext / GPUI work",
			Self::GpuiAgent => "Release vNext / Codex-2 GPUI instance",
			Self::Review => "Release vNext / independent review",
		}
	}

	const fn account(self) -> &'static str {
		match self {
			Self::Coordinator => "Codex-1",
			Self::RuntimeWork | Self::RuntimeAgent => "Codex-1",
			Self::GpuiWork | Self::GpuiAgent => "Codex-2",
			Self::Review => "Codex-3 · retained review",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplayMoment {
	Brief,
	Parallel,
	Integrated,
	Checks,
	Review,
	Gate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GateState {
	NeedsDecision,
	Approved,
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

impl ProgramContinuationInputs {
	fn new(cx: &mut Context<FactorySurface>) -> Self {
		let input = |index, placeholder, label, cx: &mut Context<FactorySurface>| {
			cx.new(|cx| ComposerInput::with_placeholder(index, placeholder, label, cx))
		};
		Self {
			signal_source: input(20, "Review, observation, or external source", "Signal source", cx),
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

/// One self-contained native surface. Its state is presentation-only.
pub(crate) struct FactorySurface {
	mode: FactoryMode,
	selection: FactorySelection,
	replay: ReplayMoment,
	gate: GateState,
	conversation: Option<ConversationTarget>,
	show_launcher: bool,
	timeline_visible: bool,
	composer: Entity<ComposerInput>,
	composer_status: Option<SharedString>,
	work_items: Option<WorkItems>,
	work_items_snapshot: Option<WorkItemsSnapshot>,
	repository_root: Entity<ComposerInput>,
	work_item_title: Entity<ComposerInput>,
	work_item_description: Entity<ComposerInput>,
	work_item_status: Option<SharedString>,
	programs: Option<Programs>,
	programs_snapshot: Option<ProgramsSnapshot>,
	program_selection: Option<EntityId>,
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
		let composer = cx.new(|cx| ComposerInput::new(0, cx));
		cx.subscribe(&composer, |surface, _, _: &ComposerEvent, cx| {
			surface.composer_status = None;
			cx.notify();
		})
		.detach();
		let work_item_title = cx
			.new(|cx| ComposerInput::with_placeholder(1, "Work item title", "Work item title", cx));
		let work_item_description = cx.new(|cx| {
			ComposerInput::with_placeholder(
				2,
				"Describe the concrete result Codex should deliver",
				"Work item description",
				cx,
			)
		});
		let repository_root = cx.new(|cx| {
			ComposerInput::with_placeholder(
				3,
				"/absolute/path/to/repository",
				"Local repository path",
				cx,
			)
		});
		for input in [&work_item_title, &work_item_description, &repository_root] {
			cx.subscribe(input, |surface, _, _: &ComposerEvent, cx| {
				surface.work_item_status = None;
				cx.notify();
			})
			.detach();
		}
		let program_inputs = ProgramCreationInputs::new(cx);
		let program_review_inputs = ProgramReviewInputs::new(cx);
		let program_continuation_inputs = ProgramContinuationInputs::new(cx);
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
			mode: FactoryMode::Operate,
			selection: FactorySelection::ReleaseGate,
			replay: ReplayMoment::Gate,
			gate: GateState::NeedsDecision,
			conversation: None,
			show_launcher: false,
			timeline_visible: true,
			composer,
			composer_status: None,
			work_items: None,
			work_items_snapshot: None,
			repository_root,
			work_item_title,
			work_item_description,
			work_item_status: None,
			programs: None,
			programs_snapshot: None,
			program_selection: None,
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
		self.reconcile_program_selection();
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
			self.reconcile_program_selection();
			cx.notify();
		}
	}

	fn reconcile_program_selection(&mut self) {
		let Some(cycle) =
			self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref())
		else {
			self.program_selection = None;
			return;
		};
		if self
			.program_selection
			.as_ref()
			.is_some_and(|selected| cycle.nodes.iter().any(|node| &node.id == selected))
		{
			return;
		}
		self.program_selection = cycle.nodes.first().map(|node| node.id.clone());
	}

	pub(crate) fn bind_work_items(&mut self, work_items: WorkItems, cx: &mut Context<Self>) {
		self.work_items = Some(work_items.clone());
		self.work_items_snapshot = Some(work_items.snapshot());
		cx.notify();
	}

	pub(crate) fn synchronize_work_items(&mut self, cx: &mut Context<Self>) {
		let Some(work_items) = self.work_items.as_ref() else {
			return;
		};
		let snapshot = work_items.snapshot();
		if self.work_items_snapshot.as_ref() != Some(&snapshot) {
			self.work_items_snapshot = Some(snapshot);
			cx.notify();
		}
	}

	fn create_work_item(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		let Some(work_items) = self.work_items.as_ref() else {
			self.work_item_status = Some("Factory authority is not connected.".into());
			cx.notify();
			return;
		};
		let title = self.work_item_title.read(cx).content().to_owned();
		let description = self.work_item_description.read(cx).content().to_owned();
		match work_items.create(&title, &description) {
			Ok(()) => {
				self.work_item_title.update(cx, |input, cx| input.clear(cx));
				self.work_item_description.update(cx, |input, cx| input.clear(cx));
				self.work_item_status = Some("Creating persisted Work Item…".into());
			},
			Err(error) => self.work_item_status = Some(work_item_error_label(error).into()),
		}
		self.work_items_snapshot = Some(work_items.snapshot());
		cx.notify();
	}

	fn register_project(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		let Some(work_items) = self.work_items.as_ref() else {
			self.work_item_status = Some("Factory authority is not connected.".into());
			cx.notify();
			return;
		};
		let repository_root = self.repository_root.read(cx).content().to_owned();
		match work_items.register_project(&repository_root) {
			Ok(()) => {
				self.repository_root.update(cx, |input, cx| input.clear(cx));
				self.work_item_status = Some("Registering local repository…".into());
			},
			Err(error) => self.work_item_status = Some(work_item_error_label(error).into()),
		}
		self.work_items_snapshot = Some(work_items.snapshot());
		cx.notify();
	}

	fn start_work_item(&mut self, card: WorkItemBoardCard, cx: &mut Context<Self>) {
		let Some(work_items) = self.work_items.as_ref() else {
			return;
		};
		match work_items.start(&card) {
			Ok(()) => self.work_item_status = Some("Starting real Codex conversation…".into()),
			Err(error) => self.work_item_status = Some(work_item_error_label(error).into()),
		}
		self.work_items_snapshot = Some(work_items.snapshot());
		cx.notify();
	}

	fn accept_work_item(&mut self, card: WorkItemBoardCard, cx: &mut Context<Self>) {
		let Some(work_items) = self.work_items.as_ref() else {
			return;
		};
		match work_items.accept(&card) {
			Ok(()) => self.work_item_status = Some("Recording human acceptance…".into()),
			Err(error) => self.work_item_status = Some(work_item_error_label(error).into()),
		}
		self.work_items_snapshot = Some(work_items.snapshot());
		cx.notify();
	}

	fn select_work_item_project(
		&mut self,
		project_id: WorkItemBoardProjectId,
		cx: &mut Context<Self>,
	) {
		let Some(work_items) = self.work_items.as_ref() else {
			return;
		};
		if work_items.select_project(project_id) {
			self.work_items_snapshot = Some(work_items.snapshot());
			self.work_item_status = None;
			cx.notify();
		}
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
			let working_directory = QuickTaskWorkingDirectory::new(
				self.program_inputs.working_directory.read(cx).content().trim().to_owned(),
			)
			.map_err(|_| ProgramInputError::InvalidDraft)?;
			Ok(ProgramCycleDraftDto {
				program_id: entity_id()?,
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
						"The bound Quick Task settles and the review cites reproducible evidence.",
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

	fn select_program(&mut self, program_id: EntityId, cx: &mut Context<Self>) {
		let Some(programs) = self.programs.as_ref() else {
			return;
		};
		if programs.select(program_id) {
			self.programs_snapshot = Some(programs.snapshot());
			self.program_selection = None;
			self.program_continuation_visible = false;
			self.program_status = None;
			cx.notify();
		}
	}

	fn continue_program(&mut self, cx: &mut Context<Self>) {
		let Some(programs) = self.programs.as_ref() else {
			return;
		};
		let Some(cycle) = self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref())
		else {
			self.program_status = Some("No Program cycle is selected.".into());
			cx.notify();
			return;
		};
		let Some(predecessor) = cycle.nodes.last().filter(|node| node.kind == ProgramNodeKind::Review)
		else {
			self.program_status = Some("The current cycle needs a Review before continuation.".into());
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
			let working_directory = QuickTaskWorkingDirectory::new(
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
						"The bound Quick Task settles and the review cites reproducible evidence.",
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
		self.program_selection = Some(node_id);
		cx.notify();
	}

	fn start_program_work_item(&mut self, cx: &mut Context<Self>) {
		let Some(cycle) =
			self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref())
		else {
			return;
		};
		let Some(work_item) = cycle
			.nodes
			.iter()
			.rev()
			.find(|node| node.kind == ProgramNodeKind::WorkItem && node.state.as_str() == "ready")
		else {
			self.program_status = Some("The Program WorkItem is not ready to start.".into());
			cx.notify();
			return;
		};
		let Some(directory) = work_item
			.fields
			.iter()
			.find(|field| field.label.as_str() == "Working directory")
			.and_then(|field| QuickTaskWorkingDirectory::new(field.value.as_str()).ok())
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
		self.program_status = Some("Starting the bound Codex Quick Task…".into());
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

	fn select_mode(&mut self, mode: FactoryMode, cx: &mut Context<Self>) {
		self.mode = mode;
		if mode == FactoryMode::Inspect && self.conversation.is_none() {
			self.conversation = Some(ConversationTarget::GpuiWork);
			self.selection = FactorySelection::GpuiWork;
		}
		if mode == FactoryMode::Replay {
			self.conversation = None;
		}
		cx.notify();
	}

	fn select_entity(
		&mut self,
		selection: FactorySelection,
		conversation: Option<ConversationTarget>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.selection = selection;
		self.show_launcher = false;
		self.conversation = conversation;
		if conversation.is_some() {
			self.mode = FactoryMode::Inspect;
			window.focus(&self.composer.focus_handle(cx), cx);
		}
		cx.notify();
	}

	fn select_replay(&mut self, replay: ReplayMoment, cx: &mut Context<Self>) {
		self.replay = replay;
		self.mode = FactoryMode::Replay;
		self.conversation = None;
		cx.notify();
	}

	fn approve_gate(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		self.gate = GateState::Approved;
		self.selection = FactorySelection::ReleaseGate;
		self.replay = ReplayMoment::Gate;
		cx.notify();
	}

	fn toggle_launcher(
		&mut self,
		_: &ToggleFactoryLauncher,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.show_launcher = !self.show_launcher;
		cx.notify();
	}

	fn close_overlay(&mut self, _: &CloseFactoryOverlay, _: &mut Window, cx: &mut Context<Self>) {
		self.show_launcher = false;
		self.conversation = None;
		self.mode = FactoryMode::Operate;
		cx.notify();
	}

	fn submit_conversation(
		&mut self,
		_: &SubmitComposer,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.start_live_conversation(window, cx);
		cx.stop_propagation();
	}

	fn start_live_conversation(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		let Some(target) = self.conversation else {
			return;
		};
		let message = self.composer.read(cx).content().trim().to_owned();
		if message.is_empty() {
			self.composer_status = Some("Enter a message for Codex.".into());
			cx.notify();
			return;
		}

		self.composer.update(cx, |composer, cx| composer.clear(cx));
		self.composer_status = Some("Opening the live Quick Task conversation…".into());
		cx.emit(FactoryEvent::StartCodexConversation { context: target.context(), message });
		cx.notify();
	}

	fn embedded_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
		if self.programs_snapshot.is_some() {
			return self.program_toolbar(cx);
		}
		let workspace = self
			.work_items_snapshot
			.as_ref()
			.and_then(WorkItemsSnapshot::selected_project_summary)
			.map(|project| project.repository_identity().as_str().to_owned())
			.unwrap_or_else(|| "No active project".to_owned());
		let work_item_count =
			self.work_items_snapshot.as_ref().map_or(0, |snapshot| snapshot.cards.len());
		let timeline_available =
			self.mode != FactoryMode::Operate || self.work_items_snapshot.is_none();
		let timeline_visible = self.timeline_visible;
		let tabs = FactoryMode::ALL.into_iter().enumerate().map(|(index, mode)| {
			let active = self.mode == mode;
			div()
				.id(("embedded-factory-mode", index))
				.role(Role::Tab)
				.aria_label(mode.label())
				.aria_selected(active)
				.h(px(28.0))
				.px_3()
				.flex()
				.items_center()
				.rounded(px(7.0))
				.border_1()
				.border_color(if active { rgba(0xffffff18) } else { rgba(0x00000000) })
				.bg(if active { rgba(0xffffff10) } else { rgba(0x00000000) })
				.text_size(px(9.0))
				.font_weight(if active { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
				.text_color(if active { rgb(TEXT) } else { rgb(TEXT_MUTED) })
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(TEXT)))
				.active(|element| element.bg(rgba(0xffffff1c)).opacity(0.82))
				.focus_visible(|element| element.border_color(rgb(BLUE)))
				.on_click(cx.listener(move |surface, _, _, cx| surface.select_mode(mode, cx)))
				.child(mode.label())
		});

		div()
			.id("factory-embedded-toolbar")
			.role(Role::Navigation)
			.aria_label("Factory controls")
			.h(px(44.0))
			.min_h(px(44.0))
			.px_3()
			.flex()
			.items_center()
			.gap_3()
			.border_b_1()
			.border_color(rgba(0xffffff0e))
			.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
			.child(
				div()
					.w(px(300.0))
					.min_w(px(220.0))
					.flex()
					.items_center()
					.gap_3()
					.child(
						div()
							.text_size(px(11.0))
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(rgb(TEXT))
							.child("Factory"),
					)
					.child(
						div()
							.min_w_0()
							.overflow_hidden()
							.whitespace_nowrap()
							.text_ellipsis()
							.font_family("SF Mono")
							.text_size(px(8.0))
							.text_color(rgb(TEXT_FAINT))
							.child(workspace),
					),
			)
			.child(
				div()
					.id("factory-mode-tabs")
					.flex_1()
					.flex()
					.items_center()
					.justify_center()
					.gap_1()
					.children(tabs),
			)
			.child(
				div()
					.w(px(300.0))
					.min_w(px(250.0))
					.flex()
					.items_center()
					.justify_end()
					.gap_2()
					.child(
						div()
							.font_family("SF Mono")
							.text_size(px(8.0))
							.text_color(rgb(TEXT_FAINT))
							.child(format!("{work_item_count} work items")),
					)
					.when(timeline_available, |controls| {
						controls.child(
							div()
								.id("toggle-factory-timeline")
								.role(Role::Button)
								.aria_label("Toggle Factory causal timeline")
								.aria_expanded(timeline_visible)
								.h(px(27.0))
								.px_3()
								.flex()
								.items_center()
								.rounded(px(7.0))
								.border_1()
								.border_color(if timeline_visible {
									rgba(0xffffff20)
								} else {
									rgba(0xffffff10)
								})
								.bg(if timeline_visible {
									rgba(0xffffff10)
								} else {
									rgba(0x00000000)
								})
								.text_size(px(9.0))
								.text_color(if timeline_visible {
									rgb(TEXT)
								} else {
									rgb(TEXT_MUTED)
								})
								.cursor_pointer()
								.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(TEXT)))
								.active(|element| element.bg(rgba(0xffffff1c)).opacity(0.82))
								.focus_visible(|element| element.border_color(rgb(BLUE)))
								.on_click(cx.listener(|surface, _, _, cx| {
									surface.timeline_visible = !surface.timeline_visible;
									cx.notify();
								}))
								.child("Timeline"),
						)
					})
					.child(
						div()
							.id("factory-command-launcher")
							.role(Role::Button)
							.aria_label("Open command launcher")
							.h(px(27.0))
							.px_3()
							.flex()
							.items_center()
							.rounded(px(7.0))
							.border_1()
							.border_color(rgba(0xffffff10))
							.text_size(px(9.0))
							.text_color(rgb(TEXT_MUTED))
							.cursor_pointer()
							.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(TEXT)))
							.active(|element| element.bg(rgba(0xffffff1c)).opacity(0.82))
							.focus_visible(|element| element.border_color(rgb(BLUE)))
							.on_click(cx.listener(|surface, _, _, cx| {
								surface.show_launcher = !surface.show_launcher;
								cx.notify();
							}))
							.child("Commands"),
					),
			)
			.into_any_element()
	}

	fn factory_canvas(&self, cx: &mut Context<Self>) -> AnyElement {
		if self.programs_snapshot.is_some() {
			return self.program_factory_canvas(cx);
		}
		if self.mode == FactoryMode::Operate && self.work_items_snapshot.is_some() {
			return self.live_factory_canvas(cx);
		}
		let mut root = div()
			.id("factory-spatial-map")
			.role(Role::Image)
			.aria_label(
				"Codex factory map with plan, parallel build, integration, review and release workcells",
			)
			.flex_1()
			.min_h_0()
			.w_full()
			.min_w(px(FACTORY_MIN_WIDTH))
			.relative()
			.overflow_hidden()
			.bg(rgba(ui_theme::SURFACE_MATERIAL))
			.child(canvas_context())
			.child(self.plan_cell(cx))
			.child(self.parallel_cell(cx))
			.child(self.integration_cell(cx))
			.child(self.review_cell(cx))
			.child(self.release_cell(cx))
			.child(factory_wiring(self.gate == GateState::Approved));

		if self.selection == FactorySelection::ReleaseGate {
			root = root.child(self.gate_sheet(cx));
		}

		root.into_any_element()
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
		let selected_node = self
			.program_selection
			.as_ref()
			.and_then(|selected| cycle.nodes.iter().find(|node| &node.id == selected));
		let latest_work_item = cycle.nodes.iter().rev().find(|node| node.kind == ProgramNodeKind::WorkItem);
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
					.child(self.program_graph(cycle, cx)),
			)
			.child(self.program_inspector(selected_node, cycle, can_review, can_continue, cx))
			.into_any_element()
	}

	fn program_intake(&self, snapshot: &ProgramsSnapshot, cx: &mut Context<Self>) -> AnyElement {
		let inputs = self.program_inputs.all();
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
		let current_work_item = cycle
			.nodes
			.iter()
			.rev()
			.find(|node| node.kind == ProgramNodeKind::WorkItem);
		let work_item_ready =
			current_work_item.is_some_and(|node| node.state.as_str() == "ready");
		let conversation_id = current_work_item.and_then(|node| node.conversation_id.clone());
		let cycle_count = cycle
			.nodes
			.iter()
			.filter(|node| node.kind == ProgramNodeKind::Signal)
			.count();
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
						cx.emit(FactoryEvent::OpenWorkItemConversation {
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

	fn program_graph(
		&self,
		cycle: &decodex_protocol::ProgramCycleDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let selected = self.program_selection.clone();
		let cycle_count = cycle
			.nodes
			.iter()
			.filter(|node| node.kind == ProgramNodeKind::Signal)
			.count();
		let mut current_cycle = 0;
		let mut graph = div()
			.id("program-graph-strip")
			.flex_1()
			.min_h_0()
			.p_3()
			.flex()
			.items_center()
			.overflow_x_scroll()
			.border_1()
			.border_color(rgba(0xffffff12))
			.rounded(px(12.0))
			.bg(rgba(ui_theme::SURFACE_MATERIAL));
		for (index, node) in cycle.nodes.iter().enumerate() {
			if index > 0 {
				let relation = cycle
					.edges
					.iter()
					.find(|edge| edge.to == node.id)
					.map(|edge| relation_label(edge.kind))
					.unwrap_or("relates");
				graph = graph.child(program_edge(relation));
			}
			if node.kind == ProgramNodeKind::Signal {
				current_cycle += 1;
				graph = graph.child(program_cycle_boundary(
					current_cycle,
					current_cycle == cycle_count,
				));
			}
			graph = graph.child(program_node_card(node, selected.as_ref() == Some(&node.id), cx));
		}
		graph.into_any_element()
	}

	fn program_inspector(
		&self,
		selected: Option<&ProgramNodeDto>,
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
		if let Some(node) = selected {
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
						cx.emit(FactoryEvent::OpenWorkItemConversation {
							conversation_id: conversation_id.clone(),
						});
					},
				));
			}
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
		let fields = self
			.program_continuation_inputs
			.all()
			.into_iter()
			.zip(labels)
			.enumerate()
			.map(|(index, (input, label))| program_input_field(index + 20, label, input.clone()));
		let next_cycle = cycle
			.nodes
			.iter()
			.filter(|node| node.kind == ProgramNodeKind::Signal)
			.count()
			+ 1;
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
			.child(
				div()
					.text_size(px(8.5))
					.text_color(rgb(TEXT_FAINT))
					.child("Manual continuation preserves the prior Review and replaces any unresolved Objective."),
			)
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

	fn live_factory_canvas(&self, cx: &mut Context<Self>) -> AnyElement {
		let snapshot = self
			.work_items_snapshot
			.as_ref()
			.expect("live Factory canvas requires a WorkItem snapshot");
		let project = snapshot
			.selected_project_summary()
			.map(|project| project.repository_identity().as_str())
			.unwrap_or("No active Project");
		let ready = snapshot
			.cards
			.iter()
			.filter(|card| card.state() == WorkItemState::Ready)
			.cloned()
			.collect::<Vec<_>>();
		let running = snapshot
			.cards
			.iter()
			.filter(|card| card.state() == WorkItemState::Running)
			.cloned()
			.collect::<Vec<_>>();
		let review = snapshot
			.cards
			.iter()
			.filter(|card| card.state() == WorkItemState::Review)
			.cloned()
			.collect::<Vec<_>>();
		let done = snapshot
			.cards
			.iter()
			.filter(|card| card.state() == WorkItemState::Done)
			.cloned()
			.collect::<Vec<_>>();
		let can_mutate = snapshot.can_mutate;
		let selected_project = snapshot.selected_project.clone();
		let project_tabs = snapshot.projects.iter().enumerate().map(|(index, project)| {
			let project_id = project.project_id().clone();
			let active = selected_project.as_ref() == Some(&project_id);
			let label = project.repository_identity().as_str().to_owned();
			div()
				.id(("factory-project", index))
				.role(Role::Tab)
				.aria_label(format!("Select Project {label}"))
				.aria_selected(active)
				.h(px(24.0))
				.px_2()
				.flex()
				.items_center()
				.rounded(px(6.0))
				.border_1()
				.border_color(if active { rgba(0xffffff24) } else { rgba(0xffffff0d) })
				.bg(if active {
					rgba(ui_theme::SURFACE_OVERLAY_MATERIAL)
				} else {
					rgba(0x00000000)
				})
				.text_size(px(8.5))
				.text_color(if active { rgb(TEXT) } else { rgb(TEXT_FAINT) })
				.cursor_pointer()
				.on_click(cx.listener(move |surface, _, _, cx| {
					surface.select_work_item_project(project_id.clone(), cx);
				}))
				.child(label)
		});

		div()
			.id("live-work-item-factory")
			.role(Role::Group)
			.aria_label("Live internal Work Item factory")
			.flex_1()
			.min_h_0()
			.w_full()
			.min_w(px(FACTORY_MIN_WIDTH))
			.flex()
			.flex_col()
			.bg(rgba(ui_theme::SURFACE_MATERIAL))
			.child(
				div()
					.h(px(74.0))
					.min_h(px(74.0))
					.px_5()
					.flex()
					.items_center()
					.justify_between()
					.border_b_1()
					.border_color(rgba(0xffffff10))
					.child(
						div()
							.flex()
							.flex_col()
							.gap_1()
							.child(
								div()
									.flex()
									.items_center()
									.gap_2()
									.child(div().size(px(6.0)).rounded_full().bg(rgb(GREEN)))
									.child(
										div()
											.text_size(px(13.0))
											.font_weight(FontWeight::SEMIBOLD)
											.child(project.to_owned()),
									),
							)
							.child(
								div()
									.font_family("SF Mono")
									.text_size(px(8.5))
									.text_color(rgb(TEXT_FAINT))
									.child(live_load_label(snapshot.load)),
							),
					)
					.child(
						div().flex().items_center().gap_2().children(project_tabs).child(
							div()
								.ml_2()
								.font_family("SF Mono")
								.text_size(px(9.0))
								.text_color(command_color(snapshot.command, snapshot.can_mutate))
								.child(live_command_label(snapshot.command, snapshot.can_mutate)),
						),
					),
			)
			.child(
				div()
					.id("factory-live-lanes")
					.flex_1()
					.min_h_0()
					.p_4()
					.flex()
					.gap_3()
					.child(self.work_item_intake(can_mutate, cx))
					.child(live_lane("READY", "Queued for Codex", BLUE, ready, can_mutate, cx))
					.child(live_lane("RUNNING", "Codex App Server", AMBER, running, can_mutate, cx))
					.child(live_lane("REVIEW", "Human decision", GREEN, review, can_mutate, cx))
					.child(live_lane(
						"DONE",
						"Accepted evidence",
						TEXT_MUTED,
						done,
						can_mutate,
						cx,
					)),
			)
			.into_any_element()
	}

	fn work_item_intake(&self, can_mutate: bool, cx: &mut Context<Self>) -> AnyElement {
		let no_project = self
			.work_items_snapshot
			.as_ref()
			.is_some_and(|snapshot| snapshot.load == WorkItemsLoadState::NoProjects);
		let available = can_mutate
			&& self
				.work_items_snapshot
				.as_ref()
				.is_some_and(|snapshot| snapshot.selected_project.is_some());
		let panel = div()
			.id("work-item-intake")
			.w(px(244.0))
			.min_w(px(244.0))
			.p_3()
			.flex()
			.flex_col()
			.gap_3()
			.border_1()
			.border_color(rgba(0xffffff14))
			.rounded(px(10.0))
			.bg(rgba(ui_theme::SURFACE_MATERIAL))
			.child(
				div()
					.flex()
					.flex_col()
					.gap_1()
					.child(
						div()
							.text_size(px(11.0))
							.font_weight(FontWeight::SEMIBOLD)
							.child("NEW WORK ITEM"),
					)
					.child(
						div()
							.text_size(px(9.0))
							.text_color(rgb(TEXT_FAINT))
							.child("One concrete Codex result"),
					),
			)
			.when(no_project, |panel| {
				panel
					.child(div().h(px(42.0)).child(self.repository_root.clone()))
					.child(
						div()
							.text_size(px(8.5))
							.text_color(rgb(TEXT_FAINT))
							.child("Register one canonical local Git worktree. No scan or import."),
					)
					.child(
						div()
							.id("register-project")
							.role(Role::Button)
							.aria_label("Register local repository as Project")
							.h(px(34.0))
							.flex()
							.items_center()
							.justify_center()
							.rounded(px(7.0))
							.bg(if can_mutate { rgb(TEXT) } else { rgb(SURFACE_OVERLAY) })
							.text_size(px(10.0))
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(if can_mutate { rgb(SURFACE) } else { rgb(TEXT_FAINT) })
							.when(can_mutate, |button| {
								button.cursor_pointer().hover(|style| style.opacity(0.9)).on_click(
									cx.listener(|surface, _, window, cx| {
										surface.register_project(window, cx);
									}),
								)
							})
							.child("REGISTER REPOSITORY"),
					)
			})
			.when(!no_project, |panel| {
				panel
					.child(div().h(px(42.0)).child(self.work_item_title.clone()))
					.child(div().h(px(66.0)).child(self.work_item_description.clone()))
					.child(
						div()
							.id("create-work-item")
							.role(Role::Button)
							.aria_label("Create internal Work Item")
							.h(px(34.0))
							.flex()
							.items_center()
							.justify_center()
							.rounded(px(7.0))
							.bg(if available { rgb(TEXT) } else { rgb(SURFACE_OVERLAY) })
							.text_size(px(10.0))
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(if available { rgb(SURFACE) } else { rgb(TEXT_FAINT) })
							.when(available, |button| {
								button.cursor_pointer().hover(|style| style.opacity(0.9)).on_click(
									cx.listener(|surface, _, window, cx| {
										surface.create_work_item(window, cx);
									}),
								)
							})
							.child("CREATE"),
					)
			})
			.when_some(self.work_item_status.clone(), |panel, status| {
				panel.child(div().text_size(px(9.0)).text_color(rgb(TEXT_MUTED)).child(status))
			});
		panel.into_any_element()
	}

	fn plan_cell(&self, cx: &mut Context<Self>) -> AnyElement {
		workcell(
			"plan-cell",
			42.0,
			80.0,
			274.0,
			310.0,
			"PLAN",
			"[A1:D4]",
			matches!(self.selection, FactorySelection::Brief | FactorySelection::Coordinator),
			vec![
				self.entity_node(
					"goal-node",
					35.0,
					83.0,
					FactorySelection::Brief,
					None,
					MarkerKind::Square,
					TEXT,
					"GOAL · Ship vNext",
					"(Goal)",
					cx,
				),
				edge_label(58.0, 145.0, "requires", TEXT_MUTED),
				self.entity_node(
					"coordinator-node",
					35.0,
					171.0,
					FactorySelection::Coordinator,
					Some(ConversationTarget::Coordinator),
					MarkerKind::Diamond,
					BLUE,
					"COORDINATOR · Codex Lead",
					"(bounded AgentInstance)",
					cx,
				),
				self.entity_node(
					"run-node",
					35.0,
					241.0,
					FactorySelection::Brief,
					None,
					MarkerKind::Square,
					TEXT,
					"RUN · Release vNext",
					"(Run)",
					cx,
				),
			],
		)
	}

	fn parallel_cell(&self, cx: &mut Context<Self>) -> AnyElement {
		let branch = |id: &'static str, top: f32, children: Vec<AnyElement>| {
			div()
				.id(id)
				.absolute()
				.left(px(22.0))
				.top(px(top))
				.w(px(486.0))
				.h(px(147.0))
				.border_1()
				.border_color(rgba(0xffffff0d))
				.rounded(px(9.0))
				.bg(rgba(0xffffff05))
				.children(children)
				.into_any_element()
		};

		workcell(
			"parallel-cell",
			360.0,
			80.0,
			530.0,
			399.0,
			"PARALLEL BUILD",
			"[A3:E8]",
			matches!(
				self.selection,
				FactorySelection::RuntimeWork
					| FactorySelection::RuntimeAgent
					| FactorySelection::GpuiWork
					| FactorySelection::GpuiAgent
			),
			vec![
				branch(
					"runtime-branch",
					71.0,
					vec![
						self.entity_node(
							"runtime-work-node",
							20.0,
							20.0,
							FactorySelection::RuntimeWork,
							Some(ConversationTarget::RuntimeWork),
							MarkerKind::Square,
							BLUE,
							"WORK · Runtime",
							"(Work)",
							cx,
						),
						edge_label(160.0, 25.0, "assigned_to", BLUE),
						self.entity_node(
							"runtime-agent-node",
							248.0,
							20.0,
							FactorySelection::RuntimeAgent,
							Some(ConversationTarget::RuntimeAgent),
							MarkerKind::Diamond,
							BLUE,
							"CODEX INSTANCE · Codex-1",
							"(AgentInstance)",
							cx,
						),
						edge_label(40.0, 93.0, "produces", GREEN),
						self.entity_node(
							"runtime-account-node",
							248.0,
							91.0,
							FactorySelection::RuntimeAgent,
							Some(ConversationTarget::RuntimeAgent),
							MarkerKind::Square,
							BLUE,
							"ACCOUNT · Codex-1",
							"(Resource)",
							cx,
						),
					],
				),
				branch(
					"gpui-branch",
					231.0,
					vec![
						self.entity_node(
							"gpui-work-node",
							20.0,
							20.0,
							FactorySelection::GpuiWork,
							Some(ConversationTarget::GpuiWork),
							MarkerKind::Square,
							BLUE,
							"WORK · GPUI",
							"(Work)",
							cx,
						),
						edge_label(160.0, 25.0, "assigned_to", BLUE),
						self.entity_node(
							"gpui-agent-node",
							248.0,
							20.0,
							FactorySelection::GpuiAgent,
							Some(ConversationTarget::GpuiAgent),
							MarkerKind::Diamond,
							BLUE,
							"CODEX INSTANCE · Codex-2",
							"(AgentInstance)",
							cx,
						),
						edge_label(40.0, 93.0, "produces", GREEN),
						self.entity_node(
							"gpui-account-node",
							248.0,
							91.0,
							FactorySelection::GpuiAgent,
							Some(ConversationTarget::GpuiAgent),
							MarkerKind::Square,
							BLUE,
							"ACCOUNT · Codex-2",
							"(Resource)",
							cx,
						),
					],
				),
			],
		)
	}

	fn integration_cell(&self, cx: &mut Context<Self>) -> AnyElement {
		workcell(
			"integration-cell",
			360.0,
			496.0,
			530.0,
			214.0,
			"INTEGRATION",
			"[E4:H9]",
			self.selection == FactorySelection::Artifact,
			vec![self.entity_node(
				"artifact-node",
				140.0,
				94.0,
				FactorySelection::Artifact,
				None,
				MarkerKind::Square,
				GREEN,
				"ARTIFACT · rev-3f7c9a22",
				"(Artifact)",
				cx,
			)],
		)
	}

	fn review_cell(&self, cx: &mut Context<Self>) -> AnyElement {
		workcell(
			"review-cell",
			906.0,
			80.0,
			218.0,
			206.0,
			"REVIEW",
			"[A8:D10]",
			self.selection == FactorySelection::Review,
			vec![
				self.entity_node(
					"review-node",
					25.0,
					83.0,
					FactorySelection::Review,
					Some(ConversationTarget::Review),
					MarkerKind::Diamond,
					GREEN,
					"REVIEW · Independent Codex",
					"(Review)",
					cx,
				),
				edge_label(120.0, 126.0, "Approved", GREEN),
			],
		)
	}

	fn release_cell(&self, cx: &mut Context<Self>) -> AnyElement {
		let approved = self.gate == GateState::Approved;
		let gate_color = if approved { GREEN } else { AMBER };
		let state = if approved { "Approved" } else { "Needs decision" };

		workcell(
			"release-cell",
			1_136.0,
			278.0,
			300.0,
			378.0,
			"RELEASE",
			"[B11:G12]",
			matches!(self.selection, FactorySelection::ReleaseGate | FactorySelection::Policy),
			vec![
				self.entity_node(
					"release-gate-node",
					42.0,
					83.0,
					FactorySelection::ReleaseGate,
					None,
					MarkerKind::Diamond,
					gate_color,
					"GATE · Release approval",
					state,
					cx,
				),
				self.entity_node(
					"release-policy-node",
					25.0,
					282.0,
					FactorySelection::Policy,
					None,
					MarkerKind::Square,
					TEXT,
					"POLICY · Release policy",
					"(Policy)",
					cx,
				),
			],
		)
	}

	#[allow(clippy::too_many_arguments)]
	fn entity_node(
		&self,
		id: &'static str,
		left: f32,
		top: f32,
		selection: FactorySelection,
		conversation: Option<ConversationTarget>,
		marker_kind: MarkerKind,
		color: u32,
		title: &'static str,
		detail: &'static str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let selected = self.selection == selection;
		let (entity_type, entity_name) = title.split_once(" · ").unwrap_or((title, ""));
		div()
			.id(id)
			.role(Role::Button)
			.aria_label(format!("{title} {detail}"))
			.absolute()
			.left(px(left))
			.top(px(top))
			.max_w(px(380.0))
			.px_2()
			.py_1()
			.flex()
			.items_start()
			.gap_2()
			.rounded(px(7.0))
			.cursor_pointer()
			.when(selected, |node| {
				node.bg(rgba(0xffffff0d)).border_1().border_color(rgba(0xffffff14))
			})
			.hover(|node| node.bg(rgba(0xffffff0a)))
			.on_click(cx.listener(move |surface, _, window, cx| {
				surface.select_entity(selection, conversation, window, cx);
			}))
			.child(marker(marker_kind, color, selected))
			.child(
				div()
					.min_w_0()
					.flex()
					.flex_col()
					.gap_1()
					.child(
						div()
							.flex()
							.items_center()
							.gap_2()
							.child(
								div()
									.font_family("SF Mono")
									.text_size(px(8.5))
									.text_color(rgb(color))
									.child(entity_type),
							)
							.when(!entity_name.is_empty(), |row| {
								row.child(
									div()
										.text_size(px(11.5))
										.font_weight(FontWeight::MEDIUM)
										.text_color(rgb(TEXT))
										.child(entity_name),
								)
							}),
					)
					.child(
						div()
							.font_family("SF Mono")
							.text_size(px(8.5))
							.text_color(rgb(TEXT_FAINT))
							.child(detail),
					),
			)
			.into_any_element()
	}

	fn gate_sheet(&self, cx: &mut Context<Self>) -> AnyElement {
		let approved = self.gate == GateState::Approved;
		let state = if approved { "Approved" } else { "Needs decision" };
		let state_color = if approved { GREEN } else { AMBER };
		let action = if approved { "Release accepted" } else { "Accept release" };
		let relation = |verb: &'static str, object: &'static str| {
			div()
				.h(px(22.0))
				.flex()
				.items_center()
				.child(
					div()
						.w(px(104.0))
						.font_family("SF Mono")
						.text_size(px(8.5))
						.text_color(rgb(TEXT_FAINT))
						.child(verb),
				)
				.child(div().text_size(px(10.5)).text_color(rgb(TEXT_MUTED)).child(object))
		};

		div()
			.id("release-gate-sheet")
			.role(Role::Dialog)
			.aria_label(format!("Release approval gate: {state}"))
			.absolute()
			.right(px(20.0))
			.bottom(px(18.0))
			.w(px(392.0))
			.p_4()
			.flex()
			.flex_col()
			.text_color(rgb(TEXT))
			.bg(rgba(ui_theme::SURFACE_OVERLAY_MATERIAL))
			.border_1()
			.border_color(rgba(0xffffff1f))
			.rounded(px(12.0))
			.shadow(vec![
				BoxShadow::new(px(0.0), px(18.0), Hsla { h: 0.0, s: 0.0, l: 0.0, a: 0.52 })
					.blur_radius(px(42.0))
					.spread_radius(px(-10.0)),
			])
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
									.font_family("SF Mono")
									.text_size(px(8.5))
									.text_color(rgb(state_color))
									.child("GATE"),
							)
							.child(
								div()
									.text_size(px(14.0))
									.font_weight(FontWeight::SEMIBOLD)
									.child("Release approval"),
							),
					)
					.child(
						div()
							.h(px(24.0))
							.px_2()
							.flex()
							.items_center()
							.gap_2()
							.rounded_full()
							.bg(rgba(0xffffff0a))
							.border_1()
							.border_color(rgba(0xffffff12))
							.text_size(px(9.5))
							.text_color(rgb(state_color))
							.child(div().size(px(6.0)).rounded_full().bg(rgb(state_color)))
							.child(state),
					),
			)
			.child(
				div()
					.mt_3()
					.px_3()
					.py_2()
					.rounded(px(8.0))
					.bg(rgba(0x00000029))
					.border_1()
					.border_color(rgba(0xffffff0d))
					.child(relation("requires", "Integrated revision"))
					.child(relation("satisfied_by", "Independent review"))
					.child(relation("governed_by", "Release policy")),
			)
			.child(
				div()
					.mt_3()
					.flex()
					.items_center()
					.justify_between()
					.child(
						div()
							.flex()
							.items_center()
							.gap_2()
							.child(
								div()
									.font_family("SF Mono")
									.text_size(px(8.5))
									.text_color(rgb(TEXT_FAINT))
									.child("EVIDENCE"),
							)
							.child(
								div()
									.px_2()
									.py_1()
									.rounded(px(6.0))
									.bg(rgba(0x73ca9114))
									.text_size(px(9.5))
									.text_color(rgb(GREEN))
									.child("12 checks passed"),
							),
					)
					.child(
						div()
							.id("accept-release")
							.role(Role::Button)
							.aria_label(action)
							.w(px(128.0))
							.h(px(32.0))
							.flex()
							.items_center()
							.justify_center()
							.rounded(px(8.0))
							.bg(rgb(state_color))
							.text_size(px(10.0))
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(rgb(0x17130d))
							.cursor_pointer()
							.when(!approved, |button| {
								button.hover(|style| style.opacity(0.9)).on_click(cx.listener(
									|surface, _, window, cx| {
										surface.approve_gate(window, cx);
									},
								))
							})
							.child(action),
					),
			)
			.into_any_element()
	}

	fn replay_panel(&self, cx: &mut Context<Self>) -> AnyElement {
		let events = [
			(ReplayMoment::Brief, 210.0, "15:42", "Goal accepted", TEXT),
			(ReplayMoment::Parallel, 390.0, "15:48", "Parallel work\nstarted", TEXT),
			(ReplayMoment::Integrated, 770.0, "15:55", "Integrated revision\nrev-3f7c9a22", TEXT),
			(ReplayMoment::Checks, 932.0, "16:03", "12 checks passed", GREEN),
			(ReplayMoment::Review, 1_088.0, "16:08", "Review approved", GREEN),
			(
				ReplayMoment::Gate,
				1_238.0,
				"16:10",
				if self.gate == GateState::Approved {
					"Release gate ·\nApproved"
				} else {
					"Release gate ·\nNeeds decision"
				},
				if self.gate == GateState::Approved { GREEN } else { AMBER },
			),
		];

		let mut panel = div()
			.id("causal-replay")
			.role(Role::Group)
			.aria_label("Causal replay timeline")
			.h(px(REPLAY_HEIGHT))
			.min_h(px(REPLAY_HEIGHT))
			.w_full()
			.min_w(px(FACTORY_MIN_WIDTH))
			.relative()
			.border_t_1()
			.border_color(rgba(0xffffff12))
			.bg(rgba(ui_theme::SURFACE_OVERLAY_MATERIAL))
			.child(replay_wiring(self.gate == GateState::Approved))
			.child(
				div()
					.absolute()
					.left(px(20.0))
					.top(px(24.0))
					.text_size(px(11.5))
					.font_weight(FontWeight::SEMIBOLD)
					.child("CAUSAL REPLAY"),
			)
			.child(
				div()
					.absolute()
					.left(px(20.0))
					.top(px(48.0))
					.w(px(150.0))
					.font_family("SF Mono")
					.text_size(px(8.5))
					.text_color(rgb(TEXT_FAINT))
					.child("DEMO · reconstruct exact state"),
			)
			.child(edge_label(278.0, 28.0, "fan_out", BLUE))
			.child(edge_label(668.0, 26.0, "produces", GREEN))
			.child(edge_label(832.0, 42.0, "blocks", GREEN))
			.child(edge_label(988.0, 42.0, "reviews", GREEN))
			.child(edge_label(1_146.0, 42.0, "blocks", GREEN))
			.child(
				div()
					.absolute()
					.left(px(476.0))
					.top(px(22.0))
					.font_family("SF Mono")
					.text_size(px(8.5))
					.text_color(rgb(TEXT_FAINT))
					.child("Runtime · Codex-1"),
			)
			.child(
				div()
					.absolute()
					.left(px(476.0))
					.top(px(58.0))
					.font_family("SF Mono")
					.text_size(px(8.5))
					.text_color(rgb(TEXT_FAINT))
					.child("GPUI · Codex-2"),
			);

		for (index, (moment, x, time, label, color)) in events.into_iter().enumerate() {
			let selected = self.replay == moment;
			panel = panel.child(
				div()
					.id(("replay-event", index))
					.role(Role::Button)
					.aria_label(format!("{time} {}", label.replace('\n', " ")))
					.absolute()
					.left(px(x - 34.0))
					.top(px(37.0))
					.w(px(132.0))
					.flex()
					.flex_col()
					.items_center()
					.gap_1()
					.text_center()
					.text_size(px(9.5))
					.cursor_pointer()
					.on_click(cx.listener(move |surface, _, _, cx| {
						surface.select_replay(moment, cx);
					}))
					.child(
						div()
							.size(px(if selected { 14.0 } else { 11.0 }))
							.border_1()
							.border_color(rgb(color))
							.rounded(px(3.0))
							.when(selected, |marker| marker.bg(rgb(color))),
					)
					.child(
						div()
							.font_family("SF Mono")
							.text_size(px(8.5))
							.text_color(rgb(color))
							.child(time),
					)
					.child(stacked_text(label, color)),
			);
		}

		panel
			.child(
				div()
					.absolute()
					.right(px(35.0))
					.top(px(49.0))
					.size(px(10.0))
					.border_1()
					.border_color(rgb(LINE_MUTED))
					.rounded(px(3.0)),
			)
			.child(
				div()
					.absolute()
					.right(px(26.0))
					.top(px(72.0))
					.font_family("SF Mono")
					.text_size(px(8.5))
					.text_color(rgb(TEXT_FAINT))
					.child("NOW"),
			)
			.into_any_element()
	}

	fn program_timeline(&self, cx: &mut Context<Self>) -> AnyElement {
		let Some(cycle) =
			self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref())
		else {
			return div().into_any_element();
		};
		let selected = self.program_selection.clone();
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

	fn launcher(&self, cx: &mut Context<Self>) -> AnyElement {
		let route_item = |id: &'static str,
		                  label: &'static str,
		                  detail: &'static str,
		                  route: FactoryRoute,
		                  cx: &mut Context<Self>| {
			div()
				.id(id)
				.role(Role::Button)
				.aria_label(label)
				.px_4()
				.py_3()
				.flex()
				.flex_col()
				.gap_1()
				.rounded(px(7.0))
				.border_1()
				.border_color(rgba(0x00000000))
				.cursor_pointer()
				.hover(|style| style.bg(rgb(0x1a232a)))
				.active(|style| style.bg(rgba(0xffffff18)).opacity(0.82))
				.focus_visible(|style| style.border_color(rgb(BLUE)))
				.on_click(cx.listener(move |surface, _, _, cx| {
					cx.emit(FactoryEvent::OpenRoute(route));
					surface.show_launcher = false;
					cx.notify();
				}))
				.child(label)
				.child(div().text_size(px(10.5)).text_color(rgb(TEXT_MUTED)).child(detail))
		};

		div()
			.id("factory-launcher")
			.role(Role::Dialog)
			.aria_label("Decodex workspace launcher")
			.absolute()
			.right(px(20.0))
			.top(px(58.0))
			.w(px(272.0))
			.py_2()
			.border_1()
			.border_color(rgb(LINE))
			.rounded_md()
			.bg(rgba(ui_theme::SURFACE_OVERLAY_MATERIAL))
			.text_size(px(12.0))
			.child(
				div()
					.px_4()
					.py_2()
					.text_size(px(10.0))
					.text_color(rgb(TEXT_MUTED))
					.child("WORKSPACES"),
			)
			.child(route_item(
				"launcher-quick-tasks",
				"Quick Tasks",
				"Live free-form Codex conversations",
				FactoryRoute::QuickTasks,
				cx,
			))
			.child(route_item(
				"launcher-health",
				"System Health",
				"Daemon and capability readiness",
				FactoryRoute::Health,
				cx,
			))
			.child(route_item(
				"launcher-accounts",
				"Accounts",
				"Multi-account readiness and desktop controls",
				FactoryRoute::Accounts,
				cx,
			))
			.child(route_item(
				"launcher-settings",
				"Settings",
				"Desktop surfaces and product preferences",
				FactoryRoute::Settings,
				cx,
			))
			.child(
				div()
					.px_4()
					.py_3()
					.border_t_1()
					.border_color(rgb(LINE_MUTED))
					.child("Account management")
					.child(
						div()
							.mt_1()
							.text_size(px(10.5))
							.text_color(rgb(TEXT_MUTED))
							.child("Available in the optional embedded menu bar surface"),
					),
			)
			.into_any_element()
	}

	fn conversation_drawer(
		&self,
		target: ConversationTarget,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let timeline_visible = self.timeline_visible
			&& (self.mode != FactoryMode::Operate || self.work_items_snapshot.is_none());
		let status = self
			.composer_status
			.as_ref()
			.map_or_else(|| "Enter sends through the live Quick Task path.".into(), Clone::clone);

		div()
			.id("factory-conversation-drawer")
			.role(Role::Dialog)
			.aria_label(format!("Conversation with {}", target.title()))
			.absolute()
			.right(px(0.0))
			.top(px(56.0))
			.bottom(px(if timeline_visible { REPLAY_HEIGHT + 12.0 } else { 12.0 }))
			.w(px(420.0))
			.flex()
			.flex_col()
			.border_l_1()
			.border_color(rgb(LINE))
			.bg(rgba(ui_theme::SURFACE_OVERLAY_MATERIAL))
			.child(
				div()
					.h(px(84.0))
					.min_h(px(84.0))
					.px_5()
					.flex()
					.items_center()
					.justify_between()
					.border_b_1()
					.border_color(rgb(LINE_MUTED))
					.child(
						div()
							.flex()
							.flex_col()
							.gap_2()
							.child(div().text_size(px(11.0)).text_color(rgb(TEXT_MUTED)).child("CONVERSATION"))
							.child(div().text_size(px(14.0)).child(target.title()))
							.child(
								div()
									.text_size(px(10.5))
									.text_color(rgb(BLUE))
									.child(format!("{} · {}", target.context(), target.account())),
							),
					)
					.child(
						div()
							.id("close-factory-conversation")
							.role(Role::Button)
							.aria_label("Close conversation")
							.px_2()
							.py_1()
							.text_size(px(10.0))
							.text_color(rgb(TEXT_MUTED))
							.cursor_pointer()
							.on_click(cx.listener(|surface, _, _, cx| {
								surface.conversation = None;
								surface.mode = FactoryMode::Operate;
								cx.notify();
							}))
							.child("CLOSE"),
					),
			)
			.child(
				div()
					.flex_1()
					.min_h_0()
					.px_5()
					.py_5()
					.flex()
					.flex_col()
					.gap_4()
					.child(message_block(
						"COORDINATOR",
						"Keep this thread scoped to the selected work and its exact revision.",
						TEXT_MUTED,
					))
					.child(message_block(
						"CODEX",
						match target {
							ConversationTarget::GpuiWork | ConversationTarget::GpuiAgent =>
								"The native Operate surface is isolated from runtime authority. I can explain the GPUI branch or start a live implementation task.",
							ConversationTarget::RuntimeWork | ConversationTarget::RuntimeAgent =>
								"The runtime branch owns exact process, attempt and recovery evidence. Ask about one boundary or start a live task.",
							ConversationTarget::Coordinator =>
								"I can propose decomposition and typed coordination commands. Decodex remains the scheduler and state authority.",
							ConversationTarget::Review =>
								"Independent review is bound to the integrated revision and evidence set. Ask about a finding or acceptance risk.",
						},
						TEXT,
					)),
			)
			.child(
				div()
					.p_4()
					.flex()
					.flex_col()
					.gap_3()
					.border_t_1()
					.border_color(rgb(LINE_MUTED))
					.child(div().h(px(44.0)).child(self.composer.clone()))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(div().text_size(px(10.0)).text_color(rgb(TEXT_MUTED)).child(status))
							.child(
								div()
									.id("send-factory-conversation")
									.role(Role::Button)
									.aria_label("Send to live Codex Quick Task")
									.px_3()
									.py_2()
									.border_1()
									.border_color(rgb(BLUE))
									.rounded_sm()
									.text_size(px(10.5))
									.text_color(rgb(BLUE))
									.cursor_pointer()
									.hover(|style| style.bg(rgb(0x14263a)))
									.on_click(cx.listener(|surface, _, window, cx| {
										surface.start_live_conversation(window, cx);
									}))
									.child("SEND TO CODEX"),
							),
					),
			)
			.into_any_element()
	}
}

impl Render for FactorySurface {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let program_timeline = self.programs_snapshot.is_some();
		let timeline_available = if program_timeline {
			self.programs_snapshot.as_ref().and_then(|snapshot| snapshot.cycle.as_ref()).is_some()
		} else {
			self.mode != FactoryMode::Operate || self.work_items_snapshot.is_none()
		};
		let operating_deck = div()
			.id("factory-operating-deck")
			.m_3()
			.flex_1()
			.min_w_0()
			.min_h_0()
			.flex()
			.flex_col()
			.overflow_hidden()
			.rounded(px(14.0))
			.border_1()
			.border_color(rgba(0xffffff14))
			.bg(rgba(ui_theme::SURFACE_MATERIAL))
			.shadow(vec![
				BoxShadow::new(px(0.0), px(18.0), Hsla { h: 0.0, s: 0.0, l: 0.0, a: 0.46 })
					.blur_radius(px(42.0))
					.spread_radius(px(-12.0)),
			])
			.child(self.embedded_toolbar(cx))
			.child(
				div()
					.id("factory-canvas-scroll")
					.flex_1()
					.min_h_0()
					.overflow_x_scroll()
					.child(self.factory_canvas(cx)),
			)
			.when(timeline_available && self.timeline_visible, |deck| {
				deck.child(
					div()
						.id("factory-timeline-scroll")
						.w_full()
						.overflow_x_scroll()
						.child(if program_timeline {
							self.program_timeline(cx)
						} else {
							self.replay_panel(cx)
						})
						.with_animation(
							"factory-timeline-reveal",
							Animation::new(ui_theme::MOTION_PANEL).with_easing(ease_in_out),
							|element, delta| {
								element
									.opacity(0.3 + delta * 0.7)
									.relative()
									.top(px((1.0 - delta) * 14.0))
							},
						),
				)
			});

		let mut root = div()
			.id("factory-surface")
			.role(Role::Main)
			.aria_label("Decodex Codex factory control room")
			.on_action(cx.listener(Self::toggle_launcher))
			.on_action(cx.listener(Self::close_overlay))
			.on_action(cx.listener(Self::submit_conversation))
			.size_full()
			.min_w_0()
			.min_h_0()
			.relative()
			.flex()
			.flex_col()
			.overflow_hidden()
			.bg(rgba(0x00000000))
			.text_color(rgb(TEXT))
			.font_family(".SystemUIFont")
			.child(operating_deck);

		if self.show_launcher {
			root = root.child(self.launcher(cx));
		}
		if let Some(target) = self.conversation {
			root = root.child(self.conversation_drawer(target, cx));
		}

		root
	}
}

fn live_lane(
	title: &'static str,
	detail: &'static str,
	color: u32,
	cards: Vec<WorkItemBoardCard>,
	can_mutate: bool,
	cx: &mut Context<FactorySurface>,
) -> AnyElement {
	let count = cards.len();
	div()
		.id(format!("work-item-lane/{title}"))
		.flex_1()
		.min_w(px(172.0))
		.p_3()
		.flex()
		.flex_col()
		.gap_3()
		.border_1()
		.border_color(rgba(0xffffff14))
		.rounded(px(10.0))
		.bg(rgba(ui_theme::SURFACE_MATERIAL))
		.child(
			div()
				.flex()
				.items_start()
				.justify_between()
				.child(
					div()
						.flex()
						.flex_col()
						.gap_1()
						.child(
							div()
								.flex()
								.items_center()
								.gap_2()
								.child(div().size(px(6.0)).rounded_full().bg(rgb(color)))
								.child(
									div()
										.text_size(px(10.0))
										.font_weight(FontWeight::SEMIBOLD)
										.text_color(rgb(color))
										.child(title),
								),
						)
						.child(div().text_size(px(8.5)).text_color(rgb(TEXT_FAINT)).child(detail)),
				)
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(9.0))
						.text_color(rgb(TEXT_FAINT))
						.child(count.to_string()),
				),
		)
		.child(
			div()
				.id(format!("work-item-lane-scroll/{title}"))
				.flex_1()
				.min_h_0()
				.flex()
				.flex_col()
				.gap_2()
				.overflow_y_scroll()
				.when(cards.is_empty(), |lane| {
					lane.child(
						div()
							.mt_2()
							.p_3()
							.border_1()
							.border_color(rgba(0xffffff0d))
							.rounded(px(8.0))
							.text_size(px(9.0))
							.text_color(rgb(TEXT_FAINT))
							.child("No work in this station"),
					)
				})
				.children(
					cards.into_iter().map(|card| live_work_item_card(card, color, can_mutate, cx)),
				),
		)
		.into_any_element()
}

fn live_work_item_card(
	card: WorkItemBoardCard,
	color: u32,
	can_mutate: bool,
	cx: &mut Context<FactorySurface>,
) -> AnyElement {
	let title = card.title().as_str().to_owned();
	let description = card.description().as_str().to_owned();
	let revision = card.revision().0;
	let conversation_id = card.conversation_id().cloned();
	let state = card.state();
	let mutation_action = match state {
		WorkItemState::Ready => Some("START CODEX"),
		WorkItemState::Review => Some("ACCEPT"),
		_ => None,
	};
	let open_action = match state {
		WorkItemState::Running => Some("OPEN"),
		WorkItemState::Review => Some("REVIEW"),
		WorkItemState::Done => Some("RESULT"),
		_ => None,
	};
	let button_card = card.clone();
	let open_conversation = conversation_id.clone();
	div()
		.id(format!("work-item-card/{}", card.work_item_id().as_str()))
		.p_3()
		.flex()
		.flex_col()
		.gap_2()
		.border_1()
		.border_color(rgba(0xffffff16))
		.rounded(px(8.0))
		.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
		.child(
			div()
				.flex()
				.items_start()
				.gap_2()
				.child(div().mt_1().size(px(7.0)).min_w(px(7.0)).bg(rgb(color)))
				.child(
					div()
						.flex_1()
						.text_size(px(10.5))
						.font_weight(FontWeight::MEDIUM)
						.text_color(rgb(TEXT))
						.child(title),
				),
		)
		.child(
			div()
				.max_h(px(48.0))
				.overflow_hidden()
				.text_size(px(9.0))
				.text_color(rgb(TEXT_MUTED))
				.child(description),
		)
		.child(
			div()
				.flex()
				.items_center()
				.justify_between()
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(TEXT_FAINT))
						.child(format!("rev {revision}")),
				)
				.child(
					div()
						.flex()
						.items_center()
						.gap_1()
						.when_some(
							open_action.zip(open_conversation),
							|actions, (label, conversation_id)| {
								actions.child(
									div()
										.id(format!(
											"work-item-open/{}",
											button_card.work_item_id().as_str()
										))
										.role(Role::Button)
										.aria_label(format!("Open Work Item {label}"))
										.px_2()
										.h(px(25.0))
										.flex()
										.items_center()
										.rounded(px(6.0))
										.border_1()
										.border_color(rgb(LINE_MUTED))
										.text_size(px(8.0))
										.text_color(rgb(TEXT_MUTED))
										.cursor_pointer()
										.hover(|style| {
											style.bg(rgba(0xffffff0d)).text_color(rgb(TEXT))
										})
										.on_click(cx.listener(move |_, _, _, cx| {
											cx.emit(FactoryEvent::OpenWorkItemConversation {
												conversation_id: conversation_id.clone(),
											});
										}))
										.child(label),
								)
							},
						)
						.when_some(mutation_action, |actions, action| {
							actions.child(
								div()
									.id(format!(
										"work-item-action/{}",
										button_card.work_item_id().as_str()
									))
									.role(Role::Button)
									.aria_label(action)
									.px_2()
									.h(px(25.0))
									.flex()
									.items_center()
									.rounded(px(6.0))
									.border_1()
									.border_color(if can_mutate {
										rgb(color)
									} else {
										rgb(LINE_MUTED)
									})
									.text_size(px(8.0))
									.text_color(if can_mutate {
										rgb(color)
									} else {
										rgb(TEXT_FAINT)
									})
									.when(can_mutate, |button| {
										button
											.cursor_pointer()
											.hover(|style| style.bg(rgba(0xffffff0d)))
											.on_click(cx.listener(move |surface, _, _, cx| {
												match state {
													WorkItemState::Ready => {
														surface.start_work_item(
															button_card.clone(),
															cx,
														);
													},
													WorkItemState::Review => {
														surface.accept_work_item(
															button_card.clone(),
															cx,
														);
													},
													_ => {},
												}
											}))
									})
									.child(action),
							)
						}),
				),
		)
		.into_any_element()
}

const fn live_load_label(load: WorkItemsLoadState) -> &'static str {
	match load {
		WorkItemsLoadState::NeverRequested => "AUTHORITY · waiting",
		WorkItemsLoadState::LoadingProjects => "AUTHORITY · loading projects",
		WorkItemsLoadState::LoadingBoard => "AUTHORITY · loading work items",
		WorkItemsLoadState::Ready => "PRODUCT STORE · current",
		WorkItemsLoadState::NoProjects => "NO ACTIVE PROJECT · register one project first",
		WorkItemsLoadState::Offline => "AUTHORITY · offline",
		WorkItemsLoadState::Unavailable => "AUTHORITY · unavailable",
		WorkItemsLoadState::Refused => "AUTHORITY · refused unsafe projection",
	}
}

const fn live_command_label(command: WorkItemCommandState, can_mutate: bool) -> &'static str {
	if !can_mutate && matches!(command, WorkItemCommandState::Idle | WorkItemCommandState::Accepted)
	{
		return "LOCKED";
	}
	match command {
		WorkItemCommandState::Idle => "READY",
		WorkItemCommandState::Sending => "SENDING",
		WorkItemCommandState::AwaitingResult => "AWAITING COMMIT",
		WorkItemCommandState::Accepted => "COMMITTED",
		WorkItemCommandState::OutcomeUnknown => "READBACK REQUIRED",
		WorkItemCommandState::Refused => "REFUSED",
	}
}

fn command_color(command: WorkItemCommandState, can_mutate: bool) -> gpui::Rgba {
	if !can_mutate && matches!(command, WorkItemCommandState::Idle | WorkItemCommandState::Accepted)
	{
		return rgb(TEXT_FAINT);
	}
	rgb(match command {
		WorkItemCommandState::Idle | WorkItemCommandState::Accepted => GREEN,
		WorkItemCommandState::Sending | WorkItemCommandState::AwaitingResult => AMBER,
		WorkItemCommandState::OutcomeUnknown | WorkItemCommandState::Refused => 0xef6b73,
	})
}

const fn work_item_error_label(error: WorkItemInputError) -> &'static str {
	match error {
		WorkItemInputError::Offline => "Factory authority is offline.",
		WorkItemInputError::Busy => "Wait for the current Work Item command.",
		WorkItemInputError::NoProject => "No active Project is selected.",
		WorkItemInputError::InvalidTitle => "Enter a short concrete title.",
		WorkItemInputError::InvalidDescription => "Enter a concrete description within the limit.",
		WorkItemInputError::InvalidRepository => "Enter one normalized absolute local Git path.",
		WorkItemInputError::InvalidState => "The Work Item is not in the required lifecycle state.",
		WorkItemInputError::IdentityUnavailable => "A command identity could not be created.",
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

fn program_edge(label: &str) -> AnyElement {
	div()
		.w(px(66.0))
		.min_w(px(66.0))
		.flex()
		.flex_col()
		.items_center()
		.gap_1()
		.child(
			div()
				.font_family("SF Mono")
				.text_size(px(7.0))
				.text_color(rgb(BLUE))
				.child(label.to_owned()),
		)
		.child(div().w_full().border_t_1().border_color(rgb(BLUE)))
		.child(div().font_family("SF Mono").text_size(px(8.0)).text_color(rgb(BLUE)).child("→"))
		.into_any_element()
}

fn program_cycle_boundary(number: usize, current: bool) -> AnyElement {
	div()
		.w(px(54.0))
		.min_w(px(54.0))
		.flex()
		.flex_col()
		.items_center()
		.gap_1()
		.child(
			div()
				.font_family("SF Mono")
				.text_size(px(8.0))
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(rgb(if current { GREEN } else { TEXT_FAINT }))
				.child(format!("C{number}")),
		)
		.child(
			div()
				.font_family("SF Mono")
				.text_size(px(6.5))
				.text_color(rgb(TEXT_FAINT))
				.child(if current { "CURRENT" } else { "HISTORY" }),
		)
		.into_any_element()
}

fn program_node_card(
	node: &ProgramNodeDto,
	selected: bool,
	cx: &mut Context<FactorySurface>,
) -> AnyElement {
	let node_id = node.id.clone();
	let color = program_node_color(node.kind);
	div()
		.id(format!("program-node/{}", node.id.as_str()))
		.role(Role::Button)
		.aria_label(format!("Inspect {} {}", node_kind_label(node.kind), node.title.as_str()))
		.w(px(178.0))
		.min_w(px(178.0))
		.min_h(px(148.0))
		.p_3()
		.flex()
		.flex_col()
		.gap_2()
		.border_1()
		.border_color(if selected { rgb(color) } else { rgba(0xffffff16) })
		.rounded(px(10.0))
		.bg(if selected {
			rgba(ui_theme::SURFACE_RAISED_MATERIAL)
		} else {
			rgba(ui_theme::SURFACE_MATERIAL)
		})
		.cursor_pointer()
		.hover(|style| style.border_color(rgba(0xffffff2c)))
		.on_click(cx.listener(move |surface, _, _, cx| {
			surface.select_program_node(node_id.clone(), cx);
		}))
		.child(
			div()
				.flex()
				.items_center()
				.justify_between()
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(7.5))
						.text_color(rgb(color))
						.child(node_kind_label(node.kind)),
				)
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(7.0))
						.text_color(rgb(TEXT_FAINT))
						.child(node.state.as_str().to_owned()),
				),
		)
		.child(
			div()
				.text_size(px(11.0))
				.font_weight(FontWeight::SEMIBOLD)
				.child(node.title.as_str().to_owned()),
		)
		.child(
			div()
				.max_h(px(64.0))
				.overflow_hidden()
				.text_size(px(9.0))
				.text_color(rgb(TEXT_MUTED))
				.child(node.summary.as_str().to_owned()),
		)
		.into_any_element()
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

const fn relation_label(kind: decodex_protocol::ProgramRelationKind) -> &'static str {
	match kind {
		decodex_protocol::ProgramRelationKind::Continues => "continues",
		decodex_protocol::ProgramRelationKind::Observes => "observes",
		decodex_protocol::ProgramRelationKind::Supports => "supports",
		decodex_protocol::ProgramRelationKind::Justifies => "justifies",
		decodex_protocol::ProgramRelationKind::Proposes => "proposes",
		decodex_protocol::ProgramRelationKind::DecomposesTo => "decomposes",
		decodex_protocol::ProgramRelationKind::Executes => "executes",
		decodex_protocol::ProgramRelationKind::Produces => "produces",
		decodex_protocol::ProgramRelationKind::Validates => "validates",
	}
}

#[derive(Clone, Copy)]
enum MarkerKind {
	Square,
	Diamond,
}

fn marker(kind: MarkerKind, color: u32, selected: bool) -> AnyElement {
	match kind {
		MarkerKind::Square => div()
			.mt_1()
			.size(px(13.0))
			.min_w(px(13.0))
			.border_1()
			.border_color(rgb(color))
			.when(selected, |marker| marker.bg(rgb(color)))
			.into_any_element(),
		MarkerKind::Diamond => canvas(
			|_, _, _| (),
			move |bounds, _, window, _| {
				let center = point(bounds.origin.x + px(7.0), bounds.origin.y + px(7.0));
				let mut builder =
					if selected { PathBuilder::fill() } else { PathBuilder::stroke(px(1.4)) };
				builder.move_to(point(center.x, center.y - px(6.0)));
				builder.line_to(point(center.x + px(6.0), center.y));
				builder.line_to(point(center.x, center.y + px(6.0)));
				builder.line_to(point(center.x - px(6.0), center.y));
				builder.close();
				if let Ok(path) = builder.build() {
					window.paint_path(path, rgb(color));
				}
			},
		)
		.mt_1()
		.size(px(14.0))
		.min_w(px(14.0))
		.into_any_element(),
	}
}

#[allow(clippy::too_many_arguments)]
fn workcell(
	id: &'static str,
	left: f32,
	top: f32,
	width: f32,
	height: f32,
	title: &'static str,
	coordinates: &'static str,
	active: bool,
	children: Vec<AnyElement>,
) -> AnyElement {
	div()
		.id(id)
		.role(Role::Group)
		.aria_label(format!("{title} workcell {coordinates}"))
		.absolute()
		.left(px(left))
		.top(px(top))
		.w(px(width))
		.h(px(height))
		.border_1()
		.border_color(if active { rgba(0xffffff26) } else { rgba(0xffffff12) })
		.rounded(px(11.0))
		.bg(if active {
			rgba(ui_theme::SURFACE_RAISED_MATERIAL)
		} else {
			rgba(ui_theme::SURFACE_MATERIAL)
		})
		.when(active, |cell| {
			cell.shadow(vec![
				BoxShadow::new(px(0.0), px(8.0), Hsla { h: 0.0, s: 0.0, l: 0.0, a: 0.28 })
					.blur_radius(px(24.0))
					.spread_radius(px(-10.0)),
			])
		})
		.child(
			div()
				.absolute()
				.left(px(16.0))
				.top(px(15.0))
				.flex()
				.items_center()
				.gap_2()
				.child(
					div()
						.text_size(px(12.0))
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(if active { rgb(TEXT) } else { rgb(TEXT_MUTED) })
						.child(title),
				)
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(TEXT_FAINT))
						.child(coordinates),
				),
		)
		.children(children)
		.into_any_element()
}

fn edge_label(left: f32, top: f32, label: &'static str, color: u32) -> AnyElement {
	div()
		.absolute()
		.left(px(left))
		.top(px(top))
		.font_family("SF Mono")
		.text_size(px(8.0))
		.text_color(rgb(color))
		.child(label)
		.into_any_element()
}

fn message_block(author: &'static str, message: &'static str, color: u32) -> AnyElement {
	div()
		.p_4()
		.flex()
		.flex_col()
		.gap_2()
		.border_1()
		.border_color(rgb(LINE_MUTED))
		.rounded_sm()
		.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
		.child(div().text_size(px(9.5)).text_color(rgb(TEXT_MUTED)).child(author))
		.child(div().text_size(px(11.5)).text_color(rgb(color)).child(message))
		.into_any_element()
}

fn stacked_text(label: &'static str, color: u32) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.items_center()
		.text_color(rgb(color))
		.children(label.lines().map(|line| div().child(line)))
		.into_any_element()
}

fn canvas_context() -> AnyElement {
	div()
		.absolute()
		.top(px(16.0))
		.left(px(20.0))
		.right(px(20.0))
		.flex()
		.items_start()
		.justify_between()
		.child(
			div()
				.flex()
				.flex_col()
				.gap_1()
				.child(
					div()
						.flex()
						.items_center()
						.gap_2()
						.child(div().size(px(6.0)).rounded_full().bg(rgb(BLUE)))
						.child(
							div()
								.text_size(px(12.0))
								.font_weight(FontWeight::SEMIBOLD)
								.child("Release vNext"),
						),
				)
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(TEXT_FAINT))
						.child("DETERMINISTIC GRAPH · 12 ENTITIES · 16 RELATIONS"),
				),
		)
		.child(
			div()
				.pt_1()
				.font_family("SF Mono")
				.text_size(px(8.0))
				.text_color(rgb(TEXT_FAINT))
				.child("CODEX FACTORY / OPERATE"),
		)
		.into_any_element()
}

fn factory_wiring(gate_approved: bool) -> AnyElement {
	canvas(
		|_, _, _| (),
		move |bounds, _, window, _| {
			// Plan semantics and hand-off into the first parallel work branch.
			paint_polyline(window, bounds, &[(92.0, 181.0), (92.0, 199.0)], LINE, true);
			paint_polyline(window, bounds, &[(92.0, 268.0), (92.0, 326.0)], LINE, true);
			paint_polyline(
				window,
				bounds,
				&[(316.0, 264.0), (334.0, 264.0), (334.0, 190.0), (402.0, 190.0)],
				BLUE,
				true,
			);
			paint_arrow(window, bounds, (392.0, 190.0), (402.0, 190.0), BLUE);

			// Typed assignment and account relations inside both parallel branches.
			paint_polyline(window, bounds, &[(520.0, 190.0), (632.0, 190.0)], BLUE, false);
			paint_arrow(window, bounds, (622.0, 190.0), (632.0, 190.0), BLUE);
			paint_polyline(window, bounds, &[(657.0, 214.0), (657.0, 254.0)], BLUE, true);
			paint_polyline(window, bounds, &[(520.0, 350.0), (632.0, 350.0)], BLUE, false);
			paint_arrow(window, bounds, (622.0, 350.0), (632.0, 350.0), BLUE);
			paint_polyline(window, bounds, &[(657.0, 374.0), (657.0, 414.0)], BLUE, true);

			// Both work branches produce one integrated revision.
			paint_polyline(
				window,
				bounds,
				&[(418.0, 208.0), (418.0, 270.0), (498.0, 270.0), (498.0, 468.0)],
				GREEN,
				true,
			);
			paint_polyline(
				window,
				bounds,
				&[(418.0, 368.0), (418.0, 430.0), (498.0, 430.0), (498.0, 468.0)],
				GREEN,
				true,
			);
			paint_polyline(
				window,
				bounds,
				&[(498.0, 468.0), (520.0, 468.0), (520.0, 591.0)],
				GREEN,
				false,
			);
			paint_arrow(window, bounds, (520.0, 581.0), (520.0, 591.0), GREEN);

			// Integrated evidence is independently reviewed and also satisfies release policy.
			paint_polyline(
				window,
				bounds,
				&[(780.0, 606.0), (1_008.0, 606.0), (1_008.0, 296.0), (1_008.0, 286.0)],
				LINE,
				true,
			);
			paint_arrow(window, bounds, (1_008.0, 296.0), (1_008.0, 286.0), LINE);
			paint_polyline(
				window,
				bounds,
				&[(780.0, 606.0), (1_030.0, 606.0), (1_030.0, 576.0), (1_157.0, 576.0)],
				LINE,
				true,
			);
			paint_arrow(window, bounds, (1_147.0, 576.0), (1_157.0, 576.0), LINE);

			// Review and policy converge on the release gate, which anchors causal replay.
			paint_polyline(
				window,
				bounds,
				&[(1_124.0, 190.0), (1_194.0, 190.0), (1_194.0, 362.0)],
				GREEN,
				true,
			);
			paint_arrow(window, bounds, (1_194.0, 352.0), (1_194.0, 362.0), GREEN);
			paint_polyline(window, bounds, &[(1_194.0, 560.0), (1_194.0, 410.0)], LINE, true);
			paint_arrow(window, bounds, (1_194.0, 420.0), (1_194.0, 410.0), LINE);
			paint_polyline(
				window,
				bounds,
				&[(1_204.0, 376.0), (1_238.0, 376.0), (1_238.0, 900.0)],
				if gate_approved { GREEN } else { AMBER },
				false,
			);
		},
	)
	.absolute()
	.size_full()
	.into_any_element()
}

fn replay_wiring(gate_approved: bool) -> AnyElement {
	canvas(
		|_, _, _| (),
		move |bounds, _, window, _| {
			paint_polyline(window, bounds, &[(210.0, 54.0), (390.0, 54.0)], LINE, true);
			paint_polyline(
				window,
				bounds,
				&[(390.0, 54.0), (480.0, 29.0), (674.0, 29.0), (770.0, 54.0)],
				BLUE,
				false,
			);
			paint_polyline(
				window,
				bounds,
				&[(390.0, 54.0), (480.0, 66.0), (674.0, 66.0), (770.0, 54.0)],
				BLUE,
				false,
			);
			paint_polyline(window, bounds, &[(770.0, 54.0), (1_088.0, 54.0)], GREEN, true);
			paint_polyline(window, bounds, &[(1_088.0, 54.0), (1_430.0, 54.0)], LINE, true);
			paint_polyline(
				window,
				bounds,
				&[(1_238.0, 0.0), (1_238.0, 54.0)],
				if gate_approved { GREEN } else { AMBER },
				false,
			);
		},
	)
	.absolute()
	.size_full()
	.into_any_element()
}

fn paint_polyline(
	window: &mut Window,
	bounds: Bounds<Pixels>,
	points: &[(f32, f32)],
	color: u32,
	dashed: bool,
) {
	let mut builder = PathBuilder::stroke(px(1.0));
	if dashed {
		builder = builder.dash_array(&[px(5.0), px(4.0)]);
	}
	if let Some((x, y)) = points.first().copied() {
		builder.move_to(point(bounds.origin.x + px(x), bounds.origin.y + px(y)));
		for (x, y) in points.iter().copied().skip(1) {
			builder.line_to(point(bounds.origin.x + px(x), bounds.origin.y + px(y)));
		}
		if let Ok(path) = builder.build() {
			window.paint_path(path, rgb(color));
		}
	}
}

fn paint_arrow(
	window: &mut Window,
	bounds: Bounds<Pixels>,
	from: (f32, f32),
	to: (f32, f32),
	color: u32,
) {
	let mut builder = PathBuilder::stroke(px(1.0));
	let (fx, fy) = from;
	let (tx, ty) = to;
	builder.move_to(point(bounds.origin.x + px(fx), bounds.origin.y + px(fy)));
	builder.line_to(point(bounds.origin.x + px(tx), bounds.origin.y + px(ty)));
	let direction = if ty >= fy { 1.0 } else { -1.0 };
	builder.move_to(point(bounds.origin.x + px(tx), bounds.origin.y + px(ty)));
	builder
		.line_to(point(bounds.origin.x + px(tx - 4.0), bounds.origin.y + px(ty - 5.0 * direction)));
	builder.move_to(point(bounds.origin.x + px(tx), bounds.origin.y + px(ty)));
	builder
		.line_to(point(bounds.origin.x + px(tx + 4.0), bounds.origin.y + px(ty - 5.0 * direction)));
	if let Ok(path) = builder.build() {
		window.paint_path(path, rgb(color));
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

#[cfg(test)]
mod tests {
	use gpui::{TestAppContext, VisualTestContext, size};

	use super::*;
	use crate::composer_input;

	fn open_factory(
		cx: &mut TestAppContext,
	) -> (gpui::Entity<FactorySurface>, &mut VisualTestContext) {
		cx.update(|cx| {
			composer_input::bind_keys(cx);
			bind_keys(cx);
		});
		cx.add_window_view(|_, cx| FactorySurface::new(cx))
	}

	#[gpui::test]
	fn default_projection_matches_the_selected_gate_state(cx: &mut TestAppContext) {
		let (surface, visual) = open_factory(cx);
		let state = surface.read_with(visual, |surface, _| {
			(surface.mode, surface.selection, surface.replay, surface.gate, surface.conversation)
		});
		assert_eq!(
			state,
			(
				FactoryMode::Operate,
				FactorySelection::ReleaseGate,
				ReplayMoment::Gate,
				GateState::NeedsDecision,
				None,
			)
		);
	}

	#[gpui::test]
	fn entity_conversation_and_gate_transitions_are_bounded(cx: &mut TestAppContext) {
		let (surface, visual) = open_factory(cx);
		surface.update(visual, |surface, cx| {
			surface.selection = FactorySelection::GpuiWork;
			surface.conversation = Some(ConversationTarget::GpuiWork);
			surface.mode = FactoryMode::Inspect;
			cx.notify();
		});
		assert_eq!(
			surface.read_with(visual, |surface, _| surface.conversation),
			Some(ConversationTarget::GpuiWork)
		);

		surface.update(visual, |surface, cx| {
			surface.gate = GateState::Approved;
			surface.selection = FactorySelection::ReleaseGate;
			surface.replay = ReplayMoment::Gate;
			cx.notify();
		});
		assert_eq!(surface.read_with(visual, |surface, _| surface.gate), GateState::Approved);
	}

	#[gpui::test]
	fn selected_design_viewport_draws_without_layout_failure(cx: &mut TestAppContext) {
		let (_surface, visual) = open_factory(cx);
		visual.update(|window, cx| {
			window.resize(size(px(1_490.0), px(1_092.0)));
			window.draw(cx).clear();
		});
	}

	#[test]
	fn application_icon_resolves_to_an_existing_asset() {
		assert!(app_icon_path().is_file());
	}

	#[test]
	fn program_timeline_uses_relative_compact_time() {
		assert_eq!(relative_timeline_time(0), "T+0µs");
		assert_eq!(relative_timeline_time(42), "T+42µs");
		assert_eq!(relative_timeline_time(42_000), "T+42ms");
		assert_eq!(relative_timeline_time(42_000_000), "T+42s");
	}
}
