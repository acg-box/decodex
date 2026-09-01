//! Daemon-owned registry and deterministic projections for built-in Domain Packs.

use std::{
	collections::{HashMap, HashSet},
	sync::OnceLock,
};

use decodex_core::ConversationId;
use decodex_database::{DomainPackIdentity, ProgramCycleRecord, ProgramDomainPackBinding};
use decodex_protocol::{
	DEVELOPMENT_DOMAIN_PACK_ID, DomainEntityDto, DomainEntityFieldDto, DomainPackCapabilityDto,
	DomainPackCapabilityStatus, DomainPackDescriptorDto, DomainPackProjectionDto,
	DomainPackViewKind, DomainRelationDto, EntityId, PAPER_INVESTMENT_DOMAIN_PACK_ID,
	ProviderThreadId, Sha256Digest, WireText,
};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

pub(crate) const CONVERSATION_CAPABILITY: &str = "codex.quick_task";

const MANIFEST_SCHEMA: &str = "decodex/domain-pack/1";
const MANIFEST_DIGEST_DOMAIN: &[u8] = b"decodex-domain-pack-manifest-v1\0";
const DEVELOPMENT_MANIFEST: &str = include_str!("../domain_packs/decodex.dev-1.0.0.json");
const PAPER_INVESTMENT_MANIFEST: &str =
	include_str!("../domain_packs/decodex.paper-investment-1.0.0.json");
const TREASURY_FIXTURE: &str = include_str!("../fixtures/us_treasury_yield_curve_2025_06.csv");
const DEVELOPMENT_MANIFEST_DIGEST: &str =
	"cdecdff922ef1ec29fbe48cc5b72877fa70cce564bbb783272dd47ce614dc146";
const PAPER_INVESTMENT_MANIFEST_DIGEST: &str =
	"996a5133a30bc968d27a16835bdbdb34736777c9d11ca2a5ed87d221c957e9eb";
const TREASURY_FIXTURE_DIGEST: &str =
	"1736087dfc077c238d8ab206629c4ccf9a2cb127e21b0cd91a53e5d0d4b0daf7";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DomainPackError {
	RegistryInvalid,
	UnknownPack,
	BindingMissing,
	BindingMismatch,
	CapabilityDenied,
	ProjectionInvalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuiltInPackKind {
	Development,
	PaperInvestment,
}

#[derive(Clone)]
struct BuiltInDomainPack {
	kind: BuiltInPackKind,
	descriptor: DomainPackDescriptorDto,
}

struct DomainPackRegistry {
	packs: Vec<BuiltInDomainPack>,
	treasury: TreasuryFixture,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainPackManifest {
	schema: String,
	id: String,
	version: String,
	name: String,
	namespace: String,
	capabilities: Vec<String>,
	entity_types: Vec<String>,
	relation_types: Vec<String>,
	view: DomainPackViewKind,
	dataset: Option<DatasetManifest>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DatasetManifest {
	source: String,
	period: String,
	fields: Vec<String>,
	sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TreasuryObservation {
	date: String,
	two_year_basis_points: i64,
	ten_year_basis_points: i64,
}

struct TreasuryFixture {
	source: String,
	period: String,
	observations: Vec<TreasuryObservation>,
}

static REGISTRY: OnceLock<Result<DomainPackRegistry, DomainPackError>> = OnceLock::new();

fn registry() -> Result<&'static DomainPackRegistry, DomainPackError> {
	REGISTRY.get_or_init(DomainPackRegistry::load).as_ref().map_err(|error| *error)
}

impl DomainPackRegistry {
	fn load() -> Result<Self, DomainPackError> {
		let development = parse_pack(
			BuiltInPackKind::Development,
			DEVELOPMENT_DOMAIN_PACK_ID,
			DEVELOPMENT_MANIFEST,
			DEVELOPMENT_MANIFEST_DIGEST,
		)?;
		let paper = parse_pack(
			BuiltInPackKind::PaperInvestment,
			PAPER_INVESTMENT_DOMAIN_PACK_ID,
			PAPER_INVESTMENT_MANIFEST,
			PAPER_INVESTMENT_MANIFEST_DIGEST,
		)?;
		let manifest: DomainPackManifest = serde_json::from_str(PAPER_INVESTMENT_MANIFEST)
			.map_err(|_| DomainPackError::RegistryInvalid)?;
		let dataset = manifest.dataset.ok_or(DomainPackError::RegistryInvalid)?;
		if dataset.sha256 != TREASURY_FIXTURE_DIGEST
			|| digest_hex(TREASURY_FIXTURE.as_bytes()) != TREASURY_FIXTURE_DIGEST
			|| dataset.period != "2025-06"
			|| dataset.fields != ["date", "2_year", "10_year"]
		{
			return Err(DomainPackError::RegistryInvalid);
		}
		let treasury = TreasuryFixture {
			source: dataset.source,
			period: dataset.period,
			observations: parse_treasury_fixture(TREASURY_FIXTURE)?,
		};
		Ok(Self { packs: vec![development, paper], treasury })
	}

	fn pack(&self, pack_id: &str) -> Result<&BuiltInDomainPack, DomainPackError> {
		self.packs
			.iter()
			.find(|pack| pack.descriptor.id.as_str() == pack_id)
			.ok_or(DomainPackError::UnknownPack)
	}
}

pub(crate) fn resolve_identity(pack_id: &str) -> Result<DomainPackIdentity, DomainPackError> {
	let pack = registry()?.pack(pack_id)?;
	Ok(DomainPackIdentity {
		pack_id: pack.descriptor.id.as_str().to_owned(),
		pack_version: pack.descriptor.version.as_str().to_owned(),
		pack_digest: pack.descriptor.digest.as_str().to_owned(),
	})
}

pub(crate) fn authorize(
	binding: Option<&ProgramDomainPackBinding>,
	capability: &str,
) -> Result<(), DomainPackError> {
	let pack = validated_pack(binding)?;
	if pack.descriptor.capabilities.iter().any(|declaration| {
		declaration.id.as_str() == capability
			&& declaration.status == DomainPackCapabilityStatus::Granted
	}) {
		Ok(())
	} else {
		Err(DomainPackError::CapabilityDenied)
	}
}

pub(crate) fn projection(
	record: &ProgramCycleRecord,
	provider_threads: &HashMap<ConversationId, ProviderThreadId>,
) -> Result<Option<DomainPackProjectionDto>, DomainPackError> {
	let Some(binding) = record.domain_pack.as_ref() else {
		return Ok(None);
	};
	let pack = validated_pack(Some(binding))?;
	let projection = match pack.kind {
		BuiltInPackKind::Development => development_projection(pack, record, provider_threads),
		BuiltInPackKind::PaperInvestment => paper_projection(pack, record, &registry()?.treasury),
	}?;
	Ok(Some(projection))
}

fn validated_pack(
	binding: Option<&ProgramDomainPackBinding>,
) -> Result<&'static BuiltInDomainPack, DomainPackError> {
	let binding = binding.ok_or(DomainPackError::BindingMissing)?;
	let pack = registry()?.pack(&binding.pack_id)?;
	if pack.descriptor.version.as_str() != binding.pack_version
		|| pack.descriptor.digest.as_str() != binding.pack_digest
	{
		return Err(DomainPackError::BindingMismatch);
	}
	Ok(pack)
}

fn parse_pack(
	kind: BuiltInPackKind,
	expected_id: &str,
	raw: &str,
	expected_digest: &str,
) -> Result<BuiltInDomainPack, DomainPackError> {
	let manifest: DomainPackManifest =
		serde_json::from_str(raw).map_err(|_| DomainPackError::RegistryInvalid)?;
	let digest = manifest_digest(raw.as_bytes());
	if manifest.schema != MANIFEST_SCHEMA
		|| manifest.id != expected_id
		|| manifest.version != "1.0.0"
		|| digest != expected_digest
		|| manifest.capabilities.is_empty()
		|| manifest.entity_types.is_empty()
		|| manifest.relation_types.is_empty()
		|| manifest.capabilities.iter().collect::<HashSet<_>>().len() != manifest.capabilities.len()
		|| manifest.entity_types.iter().collect::<HashSet<_>>().len() != manifest.entity_types.len()
		|| manifest.relation_types.iter().collect::<HashSet<_>>().len()
			!= manifest.relation_types.len()
	{
		return Err(DomainPackError::RegistryInvalid);
	}
	if kind == BuiltInPackKind::Development && manifest.dataset.is_some()
		|| kind == BuiltInPackKind::PaperInvestment && manifest.dataset.is_none()
	{
		return Err(DomainPackError::RegistryInvalid);
	}
	let descriptor = DomainPackDescriptorDto {
		id: text(manifest.id)?,
		version: text(manifest.version)?,
		digest: Sha256Digest::new(digest).map_err(|_| DomainPackError::RegistryInvalid)?,
		name: text(manifest.name)?,
		namespace: text(manifest.namespace)?,
		view: manifest.view,
		capabilities: manifest
			.capabilities
			.into_iter()
			.map(|id| {
				Ok(DomainPackCapabilityDto {
					id: text(id)?,
					status: DomainPackCapabilityStatus::Granted,
				})
			})
			.collect::<Result<_, DomainPackError>>()?,
		entity_types: manifest.entity_types.into_iter().map(text).collect::<Result<_, _>>()?,
		relation_types: manifest.relation_types.into_iter().map(text).collect::<Result<_, _>>()?,
	};
	validate_descriptor(&descriptor)?;
	Ok(BuiltInDomainPack { kind, descriptor })
}

fn validate_descriptor(descriptor: &DomainPackDescriptorDto) -> Result<(), DomainPackError> {
	let program_id = EntityId::new("00000000-0000-4000-8000-000000000001")
		.map_err(|_| DomainPackError::RegistryInvalid)?;
	let entity_id = EntityId::new("00000000-0000-4000-8000-000000000002")
		.map_err(|_| DomainPackError::RegistryInvalid)?;
	let entity = DomainEntityDto {
		id: entity_id.clone(),
		kind: descriptor.entity_types[0].clone(),
		title: text("Registry validation")?,
		summary: text("Built-in Domain Pack structural validation")?,
		state: text("valid")?,
		source: None,
		fields: Vec::new(),
	};
	let relation = DomainRelationDto {
		from: program_id.clone(),
		to: entity_id,
		kind: descriptor.relation_types[0].clone(),
	};
	DomainPackProjectionDto::new(descriptor.clone(), vec![entity], vec![relation], &program_id)
		.map(|_| ())
		.map_err(|_| DomainPackError::RegistryInvalid)
}

fn development_projection(
	pack: &BuiltInDomainPack,
	record: &ProgramCycleRecord,
	provider_threads: &HashMap<ConversationId, ProviderThreadId>,
) -> Result<DomainPackProjectionDto, DomainPackError> {
	let work_item = record.work_items.last().ok_or(DomainPackError::ProjectionInvalid)?;
	let repository_id = stable_entity_id(record, pack, "repository")?;
	let change_id = stable_entity_id(record, pack, "change")?;
	let validation_id = stable_entity_id(record, pack, "validation")?;
	let cycle_count = record.work_items.len().to_string();
	let repository = DomainEntityDto {
		id: repository_id.clone(),
		kind: text("dev.repository")?,
		title: text(repository_title(&work_item.working_directory))?,
		summary: text(record.program.purpose.clone())?,
		state: text("active")?,
		source: Some(text(work_item.working_directory.clone())?),
		fields: vec![field("Program cycles", cycle_count)?],
	};
	let change = DomainEntityDto {
		id: change_id.clone(),
		kind: text("dev.change")?,
		title: text(work_item.title.clone())?,
		summary: text(work_item.instructions.clone())?,
		state: text(work_item.state.as_str())?,
		source: work_item
			.conversation_id
			.as_ref()
			.and_then(|id| provider_threads.get(id))
			.map(|id| {
				id.codex_url()
					.map_err(|_| DomainPackError::ProjectionInvalid)
					.and_then(|url| text(url.to_string()))
			})
			.transpose()?,
		fields: vec![field("Work item", work_item.work_item_id.as_str())?],
	};
	let (validation_title, validation_summary, validation_state, validation_source, fields) =
		if let Some(review) = record.reviews.last() {
			let evidence = record
				.evidence
				.iter()
				.filter(|item| item.work_item_id == review.work_item_id)
				.collect::<Vec<_>>();
			let source = evidence
				.iter()
				.find(|item| item.evidence_id == review.external_evidence_id)
				.map(|item| item.source.clone());
			(
				"Latest Program review".to_owned(),
				review.rationale.clone(),
				review.classification.as_str().to_owned(),
				source,
				vec![field("Evidence records", evidence.len().to_string())?],
			)
		} else {
			(
				"Validation pending".to_owned(),
				record.program.review_policy.clone(),
				"pending".to_owned(),
				None,
				vec![field("Required", "deterministic and external evidence")?],
			)
		};
	let validation = DomainEntityDto {
		id: validation_id.clone(),
		kind: text("dev.validation")?,
		title: text(validation_title)?,
		summary: text(validation_summary)?,
		state: text(validation_state)?,
		source: validation_source.map(text).transpose()?,
		fields,
	};
	DomainPackProjectionDto::new(
		pack.descriptor.clone(),
		vec![repository, change, validation],
		vec![
			DomainRelationDto {
				from: repository_id,
				to: change_id.clone(),
				kind: text("dev.contains")?,
			},
			DomainRelationDto {
				from: change_id,
				to: validation_id,
				kind: text("dev.validated_by")?,
			},
		],
		&entity(record.program.program_id.as_str())?,
	)
	.map_err(|_| DomainPackError::ProjectionInvalid)
}

fn paper_projection(
	pack: &BuiltInDomainPack,
	record: &ProgramCycleRecord,
	fixture: &TreasuryFixture,
) -> Result<DomainPackProjectionDto, DomainPackError> {
	let first = fixture.observations.first().ok_or(DomainPackError::RegistryInvalid)?;
	let last = fixture.observations.last().ok_or(DomainPackError::RegistryInvalid)?;
	let spreads = fixture
		.observations
		.iter()
		.map(|item| item.ten_year_basis_points - item.two_year_basis_points)
		.collect::<Vec<_>>();
	let minimum = spreads.iter().min().copied().ok_or(DomainPackError::RegistryInvalid)?;
	let maximum = spreads.iter().max().copied().ok_or(DomainPackError::RegistryInvalid)?;
	let two_year_id = stable_entity_id(record, pack, "two-year")?;
	let ten_year_id = stable_entity_id(record, pack, "ten-year")?;
	let thesis_id = stable_entity_id(record, pack, "thesis")?;
	let scenario_id = stable_entity_id(record, pack, "scenario")?;
	let source = Some(text(fixture.source.clone())?);
	let two_year = DomainEntityDto {
		id: two_year_id.clone(),
		kind: text("finance.asset")?,
		title: text("U.S. Treasury 2-Year")?,
		summary: text(format!(
			"June 2025 month-end par yield was {}.",
			format_yield(last.two_year_basis_points)
		))?,
		state: text("observed")?,
		source: source.clone(),
		fields: vec![
			field("Period", fixture.period.clone())?,
			field("First", format_yield(first.two_year_basis_points))?,
			field("Last", format_yield(last.two_year_basis_points))?,
		],
	};
	let ten_year = DomainEntityDto {
		id: ten_year_id.clone(),
		kind: text("finance.asset")?,
		title: text("U.S. Treasury 10-Year")?,
		summary: text(format!(
			"June 2025 month-end par yield was {}.",
			format_yield(last.ten_year_basis_points)
		))?,
		state: text("observed")?,
		source: source.clone(),
		fields: vec![
			field("Period", fixture.period.clone())?,
			field("First", format_yield(first.ten_year_basis_points))?,
			field("Last", format_yield(last.ten_year_basis_points))?,
		],
	};
	let first_spread = first.ten_year_basis_points - first.two_year_basis_points;
	let last_spread = last.ten_year_basis_points - last.two_year_basis_points;
	let thesis = DomainEntityDto {
		id: thesis_id.clone(),
		kind: text("finance.thesis")?,
		title: text("Positive 2s10s slope")?,
		summary: text("The 10-year par yield stayed above the 2-year yield in the frozen sample.")?,
		state: text("supported")?,
		source: source.clone(),
		fields: vec![
			field("First spread", format!("{first_spread} bp"))?,
			field("Last spread", format!("{last_spread} bp"))?,
		],
	};
	let scenario = DomainEntityDto {
		id: scenario_id.clone(),
		kind: text("finance.scenario")?,
		title: text("June spread range")?,
		summary: text(format!(
			"The 2s10s spread stayed within {minimum}-{maximum} basis points across {} observations.",
			fixture.observations.len()
		))?,
		state: text("bounded")?,
		source,
		fields: vec![
			field("Minimum", format!("{minimum} bp"))?,
			field("Maximum", format!("{maximum} bp"))?,
			field("Range", format!("{} bp", maximum - minimum))?,
			field("Dataset SHA-256", TREASURY_FIXTURE_DIGEST)?,
		],
	};
	DomainPackProjectionDto::new(
		pack.descriptor.clone(),
		vec![two_year, ten_year, thesis, scenario],
		vec![
			DomainRelationDto {
				from: two_year_id.clone(),
				to: ten_year_id,
				kind: text("finance.compared_with")?,
			},
			DomainRelationDto {
				from: two_year_id,
				to: thesis_id.clone(),
				kind: text("finance.informs")?,
			},
			DomainRelationDto { from: thesis_id, to: scenario_id, kind: text("finance.tests")? },
		],
		&entity(record.program.program_id.as_str())?,
	)
	.map_err(|_| DomainPackError::ProjectionInvalid)
}

fn parse_treasury_fixture(raw: &str) -> Result<Vec<TreasuryObservation>, DomainPackError> {
	let mut lines = raw.lines();
	if lines.next() != Some("date,2_year,10_year") {
		return Err(DomainPackError::RegistryInvalid);
	}
	let observations = lines
		.map(|line| {
			let values = line.split(',').collect::<Vec<_>>();
			if values.len() != 3 || values[0].len() != 10 {
				return Err(DomainPackError::RegistryInvalid);
			}
			Ok(TreasuryObservation {
				date: values[0].to_owned(),
				two_year_basis_points: parse_yield(values[1])?,
				ten_year_basis_points: parse_yield(values[2])?,
			})
		})
		.collect::<Result<Vec<_>, _>>()?;
	if observations.len() != 20 || observations.windows(2).any(|pair| pair[0].date >= pair[1].date)
	{
		return Err(DomainPackError::RegistryInvalid);
	}
	Ok(observations)
}

fn parse_yield(value: &str) -> Result<i64, DomainPackError> {
	let (whole, fraction) = value.split_once('.').ok_or(DomainPackError::RegistryInvalid)?;
	if fraction.len() != 2
		|| !whole.bytes().all(|byte| byte.is_ascii_digit())
		|| !fraction.bytes().all(|byte| byte.is_ascii_digit())
	{
		return Err(DomainPackError::RegistryInvalid);
	}
	let whole = whole.parse::<i64>().map_err(|_| DomainPackError::RegistryInvalid)?;
	let fraction = fraction.parse::<i64>().map_err(|_| DomainPackError::RegistryInvalid)?;
	Ok(whole * 100 + fraction)
}

fn format_yield(basis_points: i64) -> String {
	format!("{}.{:02}%", basis_points / 100, basis_points % 100)
}

fn stable_entity_id(
	record: &ProgramCycleRecord,
	pack: &BuiltInDomainPack,
	local_key: &str,
) -> Result<EntityId, DomainPackError> {
	let mut hasher = Sha256::new();
	hasher.update(b"decodex-domain-entity-v1\0");
	hasher.update(record.program.program_id.as_str().as_bytes());
	hasher.update([0]);
	hasher.update(pack.descriptor.digest.as_str().as_bytes());
	hasher.update([0]);
	hasher.update(local_key.as_bytes());
	let digest = hasher.finalize();
	let mut bytes = [0_u8; 16];
	bytes.copy_from_slice(&digest[..16]);
	bytes[6] = (bytes[6] & 0x0f) | 0x40;
	bytes[8] = (bytes[8] & 0x3f) | 0x80;
	entity(format!(
		"{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
		bytes[0],
		bytes[1],
		bytes[2],
		bytes[3],
		bytes[4],
		bytes[5],
		bytes[6],
		bytes[7],
		bytes[8],
		bytes[9],
		bytes[10],
		bytes[11],
		bytes[12],
		bytes[13],
		bytes[14],
		bytes[15]
	))
}

fn manifest_digest(raw: &[u8]) -> String {
	let mut hasher = Sha256::new();
	hasher.update(MANIFEST_DIGEST_DOMAIN);
	hasher.update(raw);
	hex(hasher.finalize().as_slice())
}

fn digest_hex(raw: &[u8]) -> String {
	hex(Sha256::digest(raw).as_slice())
}

fn hex(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";
	let mut output = String::with_capacity(bytes.len() * 2);
	for byte in bytes {
		output.push(char::from(HEX[usize::from(byte >> 4)]));
		output.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}
	output
}

fn repository_title(path: &str) -> String {
	std::path::Path::new(path)
		.file_name()
		.and_then(|name| name.to_str())
		.filter(|name| !name.is_empty())
		.unwrap_or(path)
		.to_owned()
}

fn text(value: impl Into<String>) -> Result<WireText, DomainPackError> {
	WireText::new(value).map_err(|_| DomainPackError::ProjectionInvalid)
}

fn entity(value: impl Into<String>) -> Result<EntityId, DomainPackError> {
	EntityId::new(value).map_err(|_| DomainPackError::ProjectionInvalid)
}

fn field(
	label: impl Into<String>,
	value: impl Into<String>,
) -> Result<DomainEntityFieldDto, DomainPackError> {
	Ok(DomainEntityFieldDto { label: text(label)?, value: text(value)? })
}

#[cfg(test)]
mod tests {
	use decodex_core::{
		ObjectiveId, ObjectiveState, ProgramId, ProgramState, WorkItemId, WorkItemState,
	};
	use decodex_database::{ProgramCharterRecord, ProgramObjectiveRecord, ProgramWorkItemRecord};

	use super::*;

	fn program(pack_id: &str) -> ProgramCycleRecord {
		let identity = resolve_identity(pack_id).expect("built-in identity");
		let program_id =
			ProgramId::new("81000000-0000-4000-8000-000000000001").expect("Program ID");
		ProgramCycleRecord {
			program: ProgramCharterRecord {
				program_id: program_id.clone(),
				name: "Pressure test".to_owned(),
				purpose: "Prove one shared Program kernel".to_owned(),
				non_goals: vec!["No external actions".to_owned()],
				review_policy: "Require deterministic and external evidence".to_owned(),
				state: ProgramState::Active,
				revision: 1,
				created_at_micros: 1,
				updated_at_micros: 1,
			},
			domain_pack: Some(ProgramDomainPackBinding {
				pack_id: identity.pack_id,
				pack_version: identity.pack_version,
				pack_digest: identity.pack_digest,
				bound_at_micros: 1,
			}),
			signals: Vec::new(),
			claims: Vec::new(),
			proposals: Vec::new(),
			objectives: vec![ProgramObjectiveRecord {
				objective_id: ObjectiveId::new("81000000-0000-4000-8000-000000000002")
					.expect("Objective ID"),
				program_id: program_id.clone(),
				proposal_id: decodex_core::ProgramProposalId::new(
					"81000000-0000-4000-8000-000000000003",
				)
				.expect("Proposal ID"),
				outcome: "One bounded result".to_owned(),
				acceptance_criteria: vec!["Projection is stable".to_owned()],
				validation_criteria: vec!["Tests pass".to_owned()],
				state: ObjectiveState::Active,
				revision: 1,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
			work_items: vec![ProgramWorkItemRecord {
				work_item_id: WorkItemId::new("81000000-0000-4000-8000-000000000004")
					.expect("WorkItem ID"),
				program_id,
				objective_id: ObjectiveId::new("81000000-0000-4000-8000-000000000002")
					.expect("Objective ID"),
				title: "Inspect the pressure test".to_owned(),
				instructions: "Read the deterministic projection".to_owned(),
				working_directory: "/tmp/decodex".to_owned(),
				state: WorkItemState::Ready,
				revision: 1,
				conversation_id: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			}],
			evidence: Vec::new(),
			reviews: Vec::new(),
		}
	}

	#[test]
	fn built_in_manifests_have_stable_distinct_identities() {
		let development = resolve_identity(DEVELOPMENT_DOMAIN_PACK_ID).expect("development Pack");
		let paper = resolve_identity(PAPER_INVESTMENT_DOMAIN_PACK_ID).expect("paper Pack");
		assert_eq!(development.pack_digest, DEVELOPMENT_MANIFEST_DIGEST);
		assert_eq!(paper.pack_digest, PAPER_INVESTMENT_MANIFEST_DIGEST);
		assert_ne!(development.pack_digest, paper.pack_digest);
		assert_eq!(registry().expect("registry").packs.len(), 2);
	}

	#[test]
	fn treasury_fixture_has_expected_curve_metrics() {
		let observations = &registry().expect("registry").treasury.observations;
		let spreads = observations
			.iter()
			.map(|item| item.ten_year_basis_points - item.two_year_basis_points)
			.collect::<Vec<_>>();
		assert_eq!(spreads.first(), Some(&52));
		assert_eq!(spreads.last(), Some(&52));
		assert_eq!(spreads.iter().min(), Some(&44));
		assert_eq!(spreads.iter().max(), Some(&56));
		assert!(spreads.iter().all(|spread| *spread > 0));
	}

	#[test]
	fn projections_are_stable_and_domain_distinct() {
		let development = program(DEVELOPMENT_DOMAIN_PACK_ID);
		let provider_threads = HashMap::new();
		let first = projection(&development, &provider_threads)
			.expect("projection")
			.expect("bound projection");
		let second = projection(&development, &provider_threads)
			.expect("projection")
			.expect("bound projection");
		assert_eq!(first, second);
		assert_eq!(first.entities.len(), 3);
		let paper = projection(&program(PAPER_INVESTMENT_DOMAIN_PACK_ID), &provider_threads)
			.expect("projection")
			.expect("bound projection");
		assert_eq!(paper.entities.len(), 4);
		assert_ne!(first.entities[0].id, paper.entities[0].id);
	}

	#[test]
	fn development_projection_uses_only_the_authoritative_provider_thread_url() {
		let mut development = program(DEVELOPMENT_DOMAIN_PACK_ID);
		let conversation_id = ConversationId::new("36000000-0000-4000-8000-000000000001")
			.expect("fixture Conversation identity");
		development.work_items[0].conversation_id = Some(conversation_id.clone());
		let without_binding = projection(&development, &HashMap::new())
			.expect("projection")
			.expect("bound projection");
		let change = without_binding
			.entities
			.iter()
			.find(|entity| entity.kind.as_str() == "dev.change")
			.expect("development change");
		assert!(change.source.is_none());

		let mut provider_threads = HashMap::new();
		provider_threads.insert(
			conversation_id,
			ProviderThreadId::new("provider-thread:opaque-1").expect("provider thread identity"),
		);
		let with_binding = projection(&development, &provider_threads)
			.expect("projection")
			.expect("bound projection");
		let change = with_binding
			.entities
			.iter()
			.find(|entity| entity.kind.as_str() == "dev.change")
			.expect("development change");
		assert_eq!(
			change.source.as_ref().map(WireText::as_str),
			Some("codex://threads/provider-thread:opaque-1")
		);
	}

	#[test]
	fn binding_and_capabilities_are_closed_before_execution() {
		let mut record = program(DEVELOPMENT_DOMAIN_PACK_ID);
		let binding = record.domain_pack.as_ref().expect("binding");
		assert_eq!(authorize(Some(binding), CONVERSATION_CAPABILITY), Ok(()));
		assert_eq!(resolve_identity("decodex.unknown"), Err(DomainPackError::UnknownPack));
		assert_eq!(
			authorize(Some(binding), "finance.place_order"),
			Err(DomainPackError::CapabilityDenied)
		);
		assert_eq!(authorize(None, CONVERSATION_CAPABILITY), Err(DomainPackError::BindingMissing));
		record.domain_pack.as_mut().expect("binding").pack_digest.replace_range(0..1, "0");
		assert_eq!(projection(&record, &HashMap::new()), Err(DomainPackError::BindingMismatch));
		let paper = resolve_identity(PAPER_INVESTMENT_DOMAIN_PACK_ID).expect("paper Pack");
		assert!(
			!registry()
				.expect("registry")
				.pack(&paper.pack_id)
				.expect("paper Pack")
				.descriptor
				.capabilities
				.iter()
				.any(|capability| capability.id.as_str().contains("order"))
		);
	}
}
