//! Bounded host-rendered projection for built-in Domain Packs.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::{EntityId, Sha256Digest, WireText};

/// Stable identifier of the built-in software-development Pack.
pub const DEVELOPMENT_DOMAIN_PACK_ID: &str = "decodex.dev";
/// Stable identifier of the built-in paper-investment research Pack.
pub const PAPER_INVESTMENT_DOMAIN_PACK_ID: &str = "decodex.paper-investment";

/// Maximum domain entities in one built-in Pack projection.
pub const MAX_DOMAIN_PACK_ENTITIES: usize = 16;
/// Maximum domain relations in one built-in Pack projection.
pub const MAX_DOMAIN_PACK_RELATIONS: usize = 32;
/// Maximum declared capabilities in one built-in Pack.
pub const MAX_DOMAIN_PACK_CAPABILITIES: usize = 16;

/// Closed built-in Domain Pack projection failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DomainPackContractError {
	/// The Pack descriptor has an invalid identity, vocabulary, or capability set.
	InvalidDescriptor,
	/// The derived entity or relation projection violates the Pack contract.
	InvalidProjection,
}

/// Host-owned visual primitive selected by one declarative Pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainPackViewKind {
	/// Render the projection with the host graph and inspector surface.
	GraphInspector,
}

/// Current host disposition for one capability declared by a Pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainPackCapabilityStatus {
	/// The exact Pack grants this capability.
	Granted,
	/// The host cannot use this declared capability.
	Unavailable,
}

/// One inspectable capability declaration. Capabilities absent from the list are denied.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DomainPackCapabilityDto {
	/// Stable namespaced capability identifier.
	pub id: WireText,
	/// Current host disposition for the capability.
	pub status: DomainPackCapabilityStatus,
}

/// Immutable identity and host contract for one built-in Pack.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DomainPackDescriptorDto {
	/// Stable namespaced Pack identifier.
	pub id: WireText,
	/// Exact semantic version of the Pack.
	pub version: WireText,
	/// Content digest of the immutable Pack definition.
	pub digest: Sha256Digest,
	/// User-visible Pack name.
	pub name: WireText,
	/// Namespace used by the Pack entity and relation vocabulary.
	pub namespace: WireText,
	/// Host-owned visual primitive for this Pack.
	pub view: DomainPackViewKind,
	/// Closed capabilities declared by the Pack.
	pub capabilities: Vec<DomainPackCapabilityDto>,
	/// Closed entity kinds declared by the Pack.
	pub entity_types: Vec<WireText>,
	/// Closed relation kinds declared by the Pack.
	pub relation_types: Vec<WireText>,
}

/// One small host-rendered field on a domain entity.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DomainEntityFieldDto {
	/// User-visible field label.
	pub label: WireText,
	/// Bounded field value.
	pub value: WireText,
}

/// One stable namespaced entity derived by a built-in Pack.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DomainEntityDto {
	/// Stable derived entity identity.
	pub id: EntityId,
	/// Namespaced entity kind declared by the Pack.
	pub kind: WireText,
	/// User-visible entity title.
	pub title: WireText,
	/// Bounded entity summary.
	pub summary: WireText,
	/// Closed Pack-defined entity state.
	pub state: WireText,
	/// Optional source label for the derived fact.
	pub source: Option<WireText>,
	/// Small host-rendered entity fields.
	pub fields: Vec<DomainEntityFieldDto>,
}

/// One namespaced relation between the Program root and domain entities.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DomainRelationDto {
	/// Source Program or entity identity.
	pub from: EntityId,
	/// Target entity identity.
	pub to: EntityId,
	/// Namespaced relation kind declared by the Pack.
	pub kind: WireText,
}

/// Complete bounded domain projection rendered only by GPUI host primitives.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DomainPackProjectionDto {
	/// Immutable Pack identity and host contract.
	pub descriptor: DomainPackDescriptorDto,
	/// Bounded derived entities.
	pub entities: Vec<DomainEntityDto>,
	/// Bounded derived relations.
	pub relations: Vec<DomainRelationDto>,
}

impl DomainPackProjectionDto {
	/// Validate and construct one complete Pack projection for a Program.
	pub fn new(
		descriptor: DomainPackDescriptorDto,
		entities: Vec<DomainEntityDto>,
		relations: Vec<DomainRelationDto>,
		program_id: &EntityId,
	) -> Result<Self, DomainPackContractError> {
		let namespace = descriptor.namespace.as_str();
		let prefix = format!("{namespace}.");
		if !is_namespaced_symbol(descriptor.id.as_str())
			|| !is_symbol_segment(namespace)
			|| !is_semver_triplet(descriptor.version.as_str())
			|| descriptor.name.as_str().is_empty()
			|| descriptor.capabilities.is_empty()
			|| descriptor.capabilities.len() > MAX_DOMAIN_PACK_CAPABILITIES
			|| descriptor
				.capabilities
				.iter()
				.any(|capability| !is_namespaced_symbol(capability.id.as_str()))
			|| descriptor
				.capabilities
				.iter()
				.map(|capability| capability.id.as_str())
				.collect::<HashSet<_>>()
				.len() != descriptor.capabilities.len()
			|| descriptor.entity_types.is_empty()
			|| descriptor.relation_types.is_empty()
			|| descriptor.entity_types.len() > MAX_DOMAIN_PACK_ENTITIES
			|| descriptor.relation_types.len() > MAX_DOMAIN_PACK_RELATIONS
			|| descriptor.entity_types.iter().any(|kind| {
				!kind.as_str().starts_with(&prefix) || !is_namespaced_symbol(kind.as_str())
			}) || descriptor
			.relation_types
			.iter()
			.any(|kind| !kind.as_str().starts_with(&prefix) || !is_namespaced_symbol(kind.as_str()))
			|| descriptor.entity_types.iter().map(WireText::as_str).collect::<HashSet<_>>().len()
				!= descriptor.entity_types.len()
			|| descriptor.relation_types.iter().map(WireText::as_str).collect::<HashSet<_>>().len()
				!= descriptor.relation_types.len()
		{
			return Err(DomainPackContractError::InvalidDescriptor);
		}
		if entities.is_empty()
			|| entities.len() > MAX_DOMAIN_PACK_ENTITIES
			|| relations.is_empty()
			|| relations.len() > MAX_DOMAIN_PACK_RELATIONS
		{
			return Err(DomainPackContractError::InvalidProjection);
		}
		let entity_ids = entities.iter().map(|entity| entity.id.as_str()).collect::<HashSet<_>>();
		if entity_ids.len() != entities.len()
			|| entities.iter().any(|entity| {
				!entity.kind.as_str().starts_with(&prefix)
					|| !is_namespaced_symbol(entity.kind.as_str())
					|| !descriptor.entity_types.iter().any(|kind| kind == &entity.kind)
					|| entity.title.as_str().is_empty()
					|| entity.summary.as_str().is_empty()
					|| entity.state.as_str().is_empty()
					|| entity.fields.len() > 8
					|| entity.fields.iter().any(|field| {
						field.label.as_str().is_empty() || field.value.as_str().is_empty()
					})
			}) || relations.iter().any(|relation| {
			!relation.kind.as_str().starts_with(&prefix)
				|| !is_namespaced_symbol(relation.kind.as_str())
				|| !descriptor.relation_types.iter().any(|kind| kind == &relation.kind)
				|| relation.from == relation.to
				|| (!entity_ids.contains(relation.from.as_str()) && relation.from != *program_id)
				|| !entity_ids.contains(relation.to.as_str())
		}) {
			return Err(DomainPackContractError::InvalidProjection);
		}
		Ok(Self { descriptor, entities, relations })
	}
}

pub(crate) fn is_namespaced_symbol(value: &str) -> bool {
	let mut segments = value.split('.');
	let Some(first) = segments.next() else {
		return false;
	};
	if !is_symbol_segment(first) {
		return false;
	}
	let rest = segments.collect::<Vec<_>>();
	!rest.is_empty() && rest.iter().all(|segment| is_symbol_segment(segment))
}

fn is_symbol_segment(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= 64
		&& value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
		&& value.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
		&& value.bytes().all(|byte| {
			byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
		})
}

fn is_semver_triplet(value: &str) -> bool {
	let parts = value.split('.').collect::<Vec<_>>();
	parts.len() == 3
		&& parts.iter().all(|part| {
			!part.is_empty()
				&& part.bytes().all(|byte| byte.is_ascii_digit())
				&& (*part == "0" || !part.starts_with('0'))
		})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn text(value: &str) -> WireText {
		WireText::new(value).expect("bounded text")
	}

	#[test]
	fn projection_requires_namespaced_types_and_declared_capabilities() {
		let program_id =
			EntityId::new("81000000-0000-4000-8000-000000000001").expect("Program identity");
		let entity_id =
			EntityId::new("81000000-0000-4000-8000-000000000002").expect("domain identity");
		let descriptor = DomainPackDescriptorDto {
			id: text("decodex.dev"),
			version: text("1.0.0"),
			digest: Sha256Digest::new(
				"1111111111111111111111111111111111111111111111111111111111111111",
			)
			.expect("digest"),
			name: text("Development"),
			namespace: text("dev"),
			view: DomainPackViewKind::GraphInspector,
			capabilities: vec![DomainPackCapabilityDto {
				id: text("codex.quick_task"),
				status: DomainPackCapabilityStatus::Granted,
			}],
			entity_types: vec![text("dev.repository")],
			relation_types: vec![text("dev.contains")],
		};
		let entity = DomainEntityDto {
			id: entity_id.clone(),
			kind: text("dev.repository"),
			title: text("Decodex"),
			summary: text("Current repository"),
			state: text("current"),
			source: None,
			fields: Vec::new(),
		};
		let relation = DomainRelationDto {
			from: program_id.clone(),
			to: entity_id,
			kind: text("dev.contains"),
		};
		assert!(
			DomainPackProjectionDto::new(
				descriptor.clone(),
				vec![entity.clone()],
				vec![relation.clone()],
				&program_id,
			)
			.is_ok()
		);
		let mut invalid_namespace = descriptor.clone();
		invalid_namespace.namespace = text("Finance");
		assert_eq!(
			DomainPackProjectionDto::new(
				invalid_namespace,
				vec![entity.clone()],
				vec![relation.clone()],
				&program_id,
			),
			Err(DomainPackContractError::InvalidDescriptor),
		);
		let mut undeclared_capabilities = descriptor;
		undeclared_capabilities.capabilities.clear();
		assert_eq!(
			DomainPackProjectionDto::new(
				undeclared_capabilities,
				vec![entity],
				vec![relation],
				&program_id,
			),
			Err(DomainPackContractError::InvalidDescriptor),
		);
	}
}
