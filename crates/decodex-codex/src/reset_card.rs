use std::{
	collections::BTreeSet,
	error::Error,
	fmt::{Debug, Display, Formatter},
};

pub use decodex_core::MAX_RESET_CARD_ITEMS as MAX_RESET_CARDS_PER_INVENTORY;
use decodex_core::{
	ResetCardConsumeOutcome, ResetCardDescriptor, ResetCardError, ResetCardTimestamp,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use zeroize::Zeroizing;

use crate::{SchemaContract, protocol::MAX_APP_SERVER_FRAME_BYTES};

/// Exact app-server request method for reading reset-card inventory.
pub const RESET_CARD_READ_METHOD: &str = "account/rateLimits/read";
/// Exact app-server request method for consuming one reset card.
pub const RESET_CARD_CONSUME_METHOD: &str = "account/rateLimitResetCredit/consume";
/// Maximum UTF-8 bytes retained for one exact provider credit identifier.
pub const MAX_EXACT_RESET_CREDIT_ID_BYTES: usize = 1_024;
/// Maximum UTF-8 bytes retained for one provider idempotency key.
pub const MAX_RESET_CARD_IDEMPOTENCY_KEY_BYTES: usize = 256;
/// Exact provider reset-credit identifier.
///
/// This value is private effect material. It is not a public card identity and must not cross the
/// core, client, or user-interface boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct ExactResetCreditId(Zeroizing<String>);
impl ExactResetCreditId {
	/// Validate one exact provider identifier without trimming or normalization.
	pub fn new(value: impl Into<String>) -> Result<Self, ResetCardProtocolError> {
		Self::from_zeroizing(Zeroizing::new(value.into()))
	}

	fn from_zeroizing(value: Zeroizing<String>) -> Result<Self, ResetCardProtocolError> {
		if !is_bounded_scalar(value.as_str(), MAX_EXACT_RESET_CREDIT_ID_BYTES) {
			return Err(ResetCardProtocolError::InvalidCreditId);
		}

		Ok(Self(value))
	}

	/// Borrow the exact identifier for durable effect preparation or an app-server request.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ExactResetCreditId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ExactResetCreditId([REDACTED])")
	}
}
impl Serialize for ExactResetCreditId {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Exact bounded provider idempotency key.
#[derive(Clone, Eq, PartialEq)]
pub struct ResetCardIdempotencyKey(Zeroizing<String>);
impl ResetCardIdempotencyKey {
	/// Validate a stable scalar key.
	///
	/// UUID text is recommended for newly generated keys. Bounded existing keys remain accepted so
	/// a retry can preserve the exact key used by an earlier effect attempt.
	pub fn new(value: impl Into<String>) -> Result<Self, ResetCardProtocolError> {
		let value = Zeroizing::new(value.into());

		if !is_bounded_scalar(value.as_str(), MAX_RESET_CARD_IDEMPOTENCY_KEY_BYTES) {
			return Err(ResetCardProtocolError::InvalidIdempotencyKey);
		}

		Ok(Self(value))
	}

	/// Borrow the exact key for durable effect preparation or an app-server request.
	pub fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ResetCardIdempotencyKey {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ResetCardIdempotencyKey([REDACTED])")
	}
}
impl Serialize for ResetCardIdempotencyKey {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		serializer.serialize_str(self.as_str())
	}
}

/// Public observation of one complete available card.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvailableResetCardObservation {
	descriptor: ResetCardDescriptor,
}
impl AvailableResetCardObservation {
	/// Read the public grant/expiry descriptor.
	pub const fn descriptor(self) -> ResetCardDescriptor {
		self.descriptor
	}
}

/// Complete validated provider inventory.
///
/// Public iteration exposes only grant and expiry. Exact credit identifiers remain redacted and
/// can be resolved only by an exact public descriptor inside the adapter/runtime boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct ResetCardInventory {
	available_cards: Vec<AvailableResetCardObservation>,
	exact_ids: Vec<ExactResetCreditId>,
}
impl ResetCardInventory {
	fn from_wire(wire: ResetCardInventoryWire) -> Result<Self, ResetCardProtocolError> {
		let summary =
			wire.rate_limit_reset_credits.ok_or(ResetCardProtocolError::DetailsUnavailable)?;
		let credits = summary.credits.ok_or(ResetCardProtocolError::DetailsUnavailable)?;
		let available_count = usize::try_from(summary.available_count)
			.map_err(|_| ResetCardProtocolError::InvalidAvailableCount)?;

		if available_count > MAX_RESET_CARDS_PER_INVENTORY
			|| credits.len() > MAX_RESET_CARDS_PER_INVENTORY
		{
			return Err(ResetCardProtocolError::InventoryLimitExceeded);
		}

		let mut available_cards = Vec::with_capacity(credits.len());
		let mut exact_ids = Vec::<ExactResetCreditId>::with_capacity(credits.len());
		let mut descriptors = BTreeSet::new();

		for credit in credits {
			match credit.status.as_str() {
				"available" => {},
				"redeeming" | "redeemed" => continue,
				_ => return Err(ResetCardProtocolError::UnknownCreditStatus),
			}
			if credit.reset_type != "codexRateLimits" {
				return Err(ResetCardProtocolError::UnsupportedResetType);
			}

			let expires_at =
				credit.expires_at.ok_or(ResetCardProtocolError::IncompleteCreditDetails)?;
			let granted_at = ResetCardTimestamp::from_unix_seconds(credit.granted_at)
				.map_err(map_timestamp_error)?;
			let expires_at =
				ResetCardTimestamp::from_unix_seconds(expires_at).map_err(map_timestamp_error)?;
			let descriptor =
				ResetCardDescriptor::new(granted_at, expires_at).map_err(map_descriptor_error)?;
			let exact_id = ExactResetCreditId::from_zeroizing(credit.id.0)?;

			if exact_ids.iter().any(|existing| existing == &exact_id) {
				return Err(ResetCardProtocolError::DuplicateCreditId);
			}
			if !descriptors.insert(descriptor) {
				return Err(ResetCardProtocolError::DuplicatePublicDescriptor);
			}

			available_cards.push(AvailableResetCardObservation { descriptor });
			exact_ids.push(exact_id);
		}
		if available_count != available_cards.len() {
			return Err(ResetCardProtocolError::InventoryCountMismatch);
		}

		Ok(Self { available_cards, exact_ids })
	}

	/// Number of complete available cards in this exact observation.
	pub fn available_count(&self) -> usize {
		self.available_cards.len()
	}

	/// Public complete cards in provider order.
	pub fn available_cards(&self) -> &[AvailableResetCardObservation] {
		&self.available_cards
	}

	/// Test whether the complete private inventory still contains one persisted exact identifier.
	///
	/// This comparison stays inside the adapter/runtime authority. It does not expose the
	/// identifier through public observations or debug output.
	pub fn contains_exact_credit_id(&self, exact_id: &ExactResetCreditId) -> bool {
		self.exact_ids.iter().any(|candidate| candidate == exact_id)
	}

	/// Resolve one exact provider identifier from a unique public descriptor.
	pub fn resolve_exact_credit_id(
		&self,
		descriptor: ResetCardDescriptor,
	) -> Result<ExactResetCreditId, ResetCardResolutionError> {
		let mut matches = self
			.available_cards
			.iter()
			.zip(&self.exact_ids)
			.filter(|(card, _)| card.descriptor == descriptor)
			.map(|(_, exact_id)| exact_id);
		let Some(exact_id) = matches.next() else {
			return Err(ResetCardResolutionError::NotFound);
		};

		if matches.next().is_some() {
			return Err(ResetCardResolutionError::Ambiguous);
		}

		Ok(exact_id.clone())
	}
}
impl Debug for ResetCardInventory {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ResetCardInventory")
			.field("available_cards", &self.available_cards)
			.finish()
	}
}
impl<'de> Deserialize<'de> for ResetCardInventory {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::from_wire(ResetCardInventoryWire::deserialize(deserializer)?)
			.map_err(D::Error::custom)
	}
}

/// Decode one `account/rateLimits/read` result with typed fail-closed errors.
pub fn decode_reset_card_inventory(
	bytes: &[u8],
) -> Result<ResetCardInventory, ResetCardProtocolError> {
	if bytes.len() > MAX_APP_SERVER_FRAME_BYTES {
		return Err(ResetCardProtocolError::FrameLimitExceeded);
	}

	let wire =
		serde_json::from_slice(bytes).map_err(|_| ResetCardProtocolError::MalformedResponse)?;

	ResetCardInventory::from_wire(wire)
}

/// Exact typed parameters for `account/rateLimitResetCredit/consume`.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetCardConsumeParams {
	credit_id: ExactResetCreditId,
	idempotency_key: ResetCardIdempotencyKey,
}
impl ResetCardConsumeParams {
	/// Bind one already-resolved exact credit to its already-prepared idempotency key.
	pub const fn new(
		credit_id: ExactResetCreditId,
		idempotency_key: ResetCardIdempotencyKey,
	) -> Self {
		Self { credit_id, idempotency_key }
	}

	/// Exact provider credit selected before the external effect begins.
	pub const fn credit_id(&self) -> &ExactResetCreditId {
		&self.credit_id
	}

	/// Exact retry key selected before the external effect begins.
	pub const fn idempotency_key(&self) -> &ResetCardIdempotencyKey {
		&self.idempotency_key
	}
}
impl Debug for ResetCardConsumeParams {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ResetCardConsumeParams")
			.field("credit_id", &self.credit_id)
			.field("idempotency_key", &self.idempotency_key)
			.finish()
	}
}

/// Strict typed result from `account/rateLimitResetCredit/consume`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResetCardConsumeResult {
	outcome: ResetCardConsumeOutcome,
}
impl ResetCardConsumeResult {
	fn from_wire(wire: ResetCardConsumeWire) -> Result<Self, ResetCardProtocolError> {
		let outcome = match wire.outcome.as_str() {
			"reset" => ResetCardConsumeOutcome::Reset,
			"nothingToReset" => ResetCardConsumeOutcome::NothingToReset,
			"noCredit" => ResetCardConsumeOutcome::NoCredit,
			"alreadyRedeemed" => ResetCardConsumeOutcome::AlreadyRedeemed,
			_ => return Err(ResetCardProtocolError::UnknownConsumeOutcome),
		};

		Ok(Self { outcome })
	}

	/// Read the closed terminal provider outcome.
	pub const fn outcome(self) -> ResetCardConsumeOutcome {
		self.outcome
	}
}
impl<'de> Deserialize<'de> for ResetCardConsumeResult {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::from_wire(ResetCardConsumeWire::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Decode one consume result with typed fail-closed errors.
pub fn decode_reset_card_consume_result(
	bytes: &[u8],
) -> Result<ResetCardConsumeResult, ResetCardProtocolError> {
	if bytes.len() > MAX_APP_SERVER_FRAME_BYTES {
		return Err(ResetCardProtocolError::FrameLimitExceeded);
	}

	let wire =
		serde_json::from_slice(bytes).map_err(|_| ResetCardProtocolError::MalformedResponse)?;

	ResetCardConsumeResult::from_wire(wire)
}

/// Schema-only reset-card operation support.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetCardCapabilityState {
	/// Both required request methods are advertised by the exact generated schema.
	Supported,
	/// The inventory read method is absent.
	ReadMethodMissing,
	/// The consume method is absent.
	ConsumeMethodMissing,
	/// Both operation methods are absent.
	ReadAndConsumeMethodsMissing,
}

/// Separate schema capability profile for the manual reset-card operation.
///
/// This profile does not alter the frozen routing-capability union and grants no live routing or
/// automatic-selection authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResetCardCapabilityProfile {
	state: ResetCardCapabilityState,
}
impl ResetCardCapabilityProfile {
	/// Derive operation support from the exact generated app-server schema.
	pub fn from_schema(schema: &SchemaContract) -> Self {
		let read = schema.advertises_request(RESET_CARD_READ_METHOD);
		let consume = schema.advertises_request(RESET_CARD_CONSUME_METHOD);
		let state = match (read, consume) {
			(true, true) => ResetCardCapabilityState::Supported,
			(false, true) => ResetCardCapabilityState::ReadMethodMissing,
			(true, false) => ResetCardCapabilityState::ConsumeMethodMissing,
			(false, false) => ResetCardCapabilityState::ReadAndConsumeMethodsMissing,
		};

		Self { state }
	}

	/// Read the exact missing-method classification.
	pub const fn state(self) -> ResetCardCapabilityState {
		self.state
	}

	/// Return true only when both exact operation methods are advertised.
	pub const fn is_supported(self) -> bool {
		matches!(self.state, ResetCardCapabilityState::Supported)
	}
}

/// Fail-closed provider decoding or scalar-validation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetCardProtocolError {
	/// The app-server frame exceeded its mechanical byte limit.
	FrameLimitExceeded,
	/// JSON, field types, or the strict provider result shape were invalid.
	MalformedResponse,
	/// The inventory summary or its full details were absent.
	DetailsUnavailable,
	/// `availableCount` was negative or not representable.
	InvalidAvailableCount,
	/// The provider count differed from the complete detail count.
	InventoryCountMismatch,
	/// The provider inventory exceeded the accepted collection bound.
	InventoryLimitExceeded,
	/// One provider credit omitted a required public timestamp.
	IncompleteCreditDetails,
	/// One provider credit timestamp was before the Unix epoch.
	InvalidCreditTimestamp,
	/// One provider credit expired no later than it was granted.
	InvalidCreditChronology,
	/// One exact provider credit identifier was empty, oversized, or not a safe scalar.
	InvalidCreditId,
	/// The provider repeated an exact credit identifier.
	DuplicateCreditId,
	/// The provider repeated a public grant/expiry descriptor.
	DuplicatePublicDescriptor,
	/// A credit used a reset type other than `codexRateLimits`.
	UnsupportedResetType,
	/// A credit used an unknown status.
	UnknownCreditStatus,
	/// An idempotency key was empty, oversized, or not a safe scalar.
	InvalidIdempotencyKey,
	/// The consume result used an unknown outcome.
	UnknownConsumeOutcome,
}
impl Error for ResetCardProtocolError {}

impl Display for ResetCardProtocolError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::FrameLimitExceeded => "reset-card app-server result exceeds the frame limit",
			Self::MalformedResponse => "reset-card app-server result is malformed",
			Self::DetailsUnavailable => "complete reset-card details are unavailable",
			Self::InvalidAvailableCount => "reset-card available count is invalid",
			Self::InventoryCountMismatch =>
				"reset-card available count does not match complete details",
			Self::InventoryLimitExceeded => "reset-card inventory exceeds its collection limit",
			Self::IncompleteCreditDetails => "reset-card public details are incomplete",
			Self::InvalidCreditTimestamp => "reset-card provider timestamp is invalid",
			Self::InvalidCreditChronology => "reset-card provider chronology is invalid",
			Self::InvalidCreditId => "reset-card provider credit identity is invalid",
			Self::DuplicateCreditId => "reset-card provider credit identity is duplicated",
			Self::DuplicatePublicDescriptor => "reset-card public descriptor is duplicated",
			Self::UnsupportedResetType => "reset-card provider type is unsupported",
			Self::UnknownCreditStatus => "reset-card provider status is unknown",
			Self::InvalidIdempotencyKey => "reset-card idempotency key is invalid",
			Self::UnknownConsumeOutcome => "reset-card consume outcome is unknown",
		})
	}
}

/// Exact-descriptor resolution failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetCardResolutionError {
	/// The current complete inventory did not contain the descriptor.
	NotFound,
	/// More than one current credit had the descriptor.
	Ambiguous,
}
impl Error for ResetCardResolutionError {}

impl Display for ResetCardResolutionError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::NotFound => "reset-card descriptor was not found",
			Self::Ambiguous => "reset-card descriptor is ambiguous",
		})
	}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResetCardInventoryWire {
	#[serde(default)]
	rate_limit_reset_credits: Option<ResetCardSummaryWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResetCardSummaryWire {
	available_count: i64,
	#[serde(default)]
	credits: Option<Vec<ResetCardCreditWire>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResetCardCreditWire {
	id: ZeroizingWireText,
	granted_at: i64,
	#[serde(default)]
	expires_at: Option<i64>,
	reset_type: String,
	status: String,
	#[serde(default, rename = "title")]
	_title: Option<ZeroizingWireText>,
	#[serde(default, rename = "description")]
	_description: Option<ZeroizingWireText>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResetCardConsumeWire {
	outcome: String,
}

struct ZeroizingWireText(Zeroizing<String>);
impl<'de> Deserialize<'de> for ZeroizingWireText {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Ok(Self(Zeroizing::new(String::deserialize(deserializer)?)))
	}
}

fn is_bounded_scalar(value: &str, max_bytes: usize) -> bool {
	!value.is_empty()
		&& value.len() <= max_bytes
		&& value.trim() == value
		&& !value.chars().any(char::is_control)
}

fn map_timestamp_error(error: ResetCardError) -> ResetCardProtocolError {
	match error {
		ResetCardError::NegativeTimestamp => ResetCardProtocolError::InvalidCreditTimestamp,
		ResetCardError::ExpirationNotAfterGrant => ResetCardProtocolError::InvalidCreditChronology,
	}
}

fn map_descriptor_error(error: ResetCardError) -> ResetCardProtocolError {
	match error {
		ResetCardError::NegativeTimestamp => ResetCardProtocolError::InvalidCreditTimestamp,
		ResetCardError::ExpirationNotAfterGrant => ResetCardProtocolError::InvalidCreditChronology,
	}
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeSet;

	use decodex_core::{ResetCardConsumeOutcome, ResetCardDescriptor, ResetCardTimestamp};
	use serde_json::{Value, json};

	use crate::{
		MAX_EXACT_RESET_CREDIT_ID_BYTES, MAX_RESET_CARD_IDEMPOTENCY_KEY_BYTES,
		REQUIRED_NOTIFICATION_METHODS, REQUIRED_REQUEST_METHODS, RESET_CARD_CONSUME_METHOD,
		RESET_CARD_READ_METHOD, ResetCardCapabilityProfile, ResetCardCapabilityState,
		ResetCardConsumeParams, ResetCardConsumeResult, ResetCardIdempotencyKey,
		ResetCardInventory, ResetCardProtocolError, ResetCardResolutionError, SchemaContract,
		SchemaMarker, decode_reset_card_consume_result, decode_reset_card_inventory,
		reset_card::ExactResetCreditId,
	};

	fn descriptor(granted_at: i64, expires_at: i64) -> ResetCardDescriptor {
		ResetCardDescriptor::new(
			ResetCardTimestamp::from_unix_seconds(granted_at).unwrap(),
			ResetCardTimestamp::from_unix_seconds(expires_at).unwrap(),
		)
		.unwrap()
	}

	fn inventory_json(credits: Value, available_count: i64) -> Vec<u8> {
		serde_json::to_vec(&json!({
			"rateLimits": {},
			"rateLimitResetCredits": {
				"availableCount": available_count,
				"credits": credits
			}
		}))
		.unwrap()
	}

	fn credit(id: &str, granted_at: i64, expires_at: Value) -> Value {
		json!({
			"id": id,
			"grantedAt": granted_at,
			"expiresAt": expires_at,
			"resetType": "codexRateLimits",
			"status": "available",
			"title": "provider display title",
			"description": null
		})
	}

	#[test]
	fn complete_inventory_exposes_public_cards_and_resolves_only_an_exact_descriptor() {
		let bytes = inventory_json(
			json!([
				credit("private-credit-a", 100, json!(200)),
				credit("private-credit-b", 300, json!(400))
			]),
			2,
		);
		let inventory = decode_reset_card_inventory(&bytes).unwrap();

		assert_eq!(inventory.available_count(), 2);
		assert_eq!(
			inventory.available_cards().iter().map(|card| card.descriptor()).collect::<Vec<_>>(),
			vec![descriptor(100, 200), descriptor(300, 400)]
		);
		assert_eq!(
			inventory.resolve_exact_credit_id(descriptor(300, 400)).unwrap().as_str(),
			"private-credit-b"
		);
		assert_eq!(
			inventory.resolve_exact_credit_id(descriptor(500, 600)),
			Err(ResetCardResolutionError::NotFound)
		);

		let debug = format!("{inventory:?}");

		assert!(!debug.contains("private-credit-a"));
		assert!(!debug.contains("private-credit-b"));
		assert!(!debug.contains("provider display title"));
	}

	#[test]
	fn inventory_implements_strict_deserialization_for_the_runtime_request_boundary() {
		let bytes = inventory_json(json!([credit("private-credit", 100, json!(200))]), 1);
		let inventory: ResetCardInventory = serde_json::from_slice(&bytes).unwrap();

		assert_eq!(inventory.available_count(), 1);
		assert_eq!(
			inventory.resolve_exact_credit_id(descriptor(100, 200)).unwrap().as_str(),
			"private-credit"
		);
	}

	#[test]
	fn exact_id_and_idempotency_key_are_exact_bounded_and_redacted() {
		let exact_id = ExactResetCreditId::new("credit:\"Case-Sensitive\"").unwrap();
		let uuid_key =
			ResetCardIdempotencyKey::new("10000000-0000-4000-8000-000000000001").unwrap();
		let existing_key = ResetCardIdempotencyKey::new("retry-key.v1").unwrap();
		let maximum_id = "x".repeat(MAX_EXACT_RESET_CREDIT_ID_BYTES);
		let maximum_key = "x".repeat(MAX_RESET_CARD_IDEMPOTENCY_KEY_BYTES);

		assert_eq!(exact_id.as_str(), "credit:\"Case-Sensitive\"");
		assert_eq!(serde_json::to_string(&exact_id).unwrap(), r#""credit:\"Case-Sensitive\"""#);
		assert_eq!(existing_key.as_str(), "retry-key.v1");
		assert_eq!(ExactResetCreditId::new(&maximum_id).unwrap().as_str(), maximum_id);
		assert_eq!(ResetCardIdempotencyKey::new(&maximum_key).unwrap().as_str(), maximum_key);
		assert_eq!(format!("{exact_id:?}"), "ExactResetCreditId([REDACTED])");
		assert_eq!(format!("{uuid_key:?}"), "ResetCardIdempotencyKey([REDACTED])");
		assert!(!format!("{uuid_key:?}").contains(uuid_key.as_str()));

		for invalid in [
			"",
			" leading",
			"trailing ",
			"line\nbreak",
			&"x".repeat(MAX_EXACT_RESET_CREDIT_ID_BYTES + 1),
		] {
			assert_eq!(
				ExactResetCreditId::new(invalid),
				Err(ResetCardProtocolError::InvalidCreditId)
			);
		}
		for invalid in [
			"",
			" leading",
			"trailing ",
			"line\nbreak",
			&"x".repeat(MAX_RESET_CARD_IDEMPOTENCY_KEY_BYTES + 1),
		] {
			assert_eq!(
				ResetCardIdempotencyKey::new(invalid),
				Err(ResetCardProtocolError::InvalidIdempotencyKey)
			);
		}
	}

	#[test]
	fn consume_params_use_exact_camel_case_provider_fields_without_leaking_debug_values() {
		let params = ResetCardConsumeParams::new(
			ExactResetCreditId::new("credit-private").unwrap(),
			ResetCardIdempotencyKey::new("attempt-private").unwrap(),
		);
		let value = serde_json::to_value(&params).unwrap();

		assert_eq!(
			value,
			json!({"creditId": "credit-private", "idempotencyKey": "attempt-private"})
		);
		assert_eq!(params.credit_id().as_str(), "credit-private");
		assert_eq!(params.idempotency_key().as_str(), "attempt-private");
		assert!(!format!("{params:?}").contains("credit-private"));
		assert!(!format!("{params:?}").contains("attempt-private"));
	}

	#[test]
	fn inventory_rejects_missing_or_capped_details() {
		assert_eq!(
			decode_reset_card_inventory(br#"{"rateLimits":{}}"#),
			Err(ResetCardProtocolError::DetailsUnavailable)
		);
		assert_eq!(
			decode_reset_card_inventory(
				br#"{"rateLimitResetCredits":{"availableCount":1,"credits":null}}"#
			),
			Err(ResetCardProtocolError::DetailsUnavailable)
		);
		assert_eq!(
			decode_reset_card_inventory(&inventory_json(
				json!([credit("credit-a", 100, json!(200))]),
				2
			)),
			Err(ResetCardProtocolError::InventoryCountMismatch)
		);
		assert_eq!(
			decode_reset_card_inventory(
				br#"{"rateLimitResetCredits":{"availableCount":-1,"credits":[]}}"#
			),
			Err(ResetCardProtocolError::InvalidAvailableCount)
		);

		let capped = (0..super::MAX_RESET_CARDS_PER_INVENTORY)
			.map(|index| credit(&format!("credit-{index}"), index as i64, json!(index + 1)))
			.collect::<Vec<_>>();
		let inventory = decode_reset_card_inventory(&inventory_json(
			json!(capped),
			super::MAX_RESET_CARDS_PER_INVENTORY as i64,
		))
		.unwrap();

		assert_eq!(super::MAX_RESET_CARDS_PER_INVENTORY, 64);
		assert_eq!(inventory.available_count(), 64);

		let oversized = (0..=super::MAX_RESET_CARDS_PER_INVENTORY)
			.map(|index| credit(&format!("credit-{index}"), index as i64, json!(index + 1)))
			.collect::<Vec<_>>();

		assert_eq!(
			decode_reset_card_inventory(&inventory_json(
				json!(oversized),
				(super::MAX_RESET_CARDS_PER_INVENTORY + 1) as i64
			)),
			Err(ResetCardProtocolError::InventoryLimitExceeded)
		);
	}

	#[test]
	fn inventory_rejects_duplicate_public_or_private_identity() {
		assert_eq!(
			decode_reset_card_inventory(&inventory_json(
				json!([credit("credit-a", 100, json!(200)), credit("credit-b", 100, json!(200))]),
				2
			)),
			Err(ResetCardProtocolError::DuplicatePublicDescriptor)
		);
		assert_eq!(
			decode_reset_card_inventory(&inventory_json(
				json!([credit("credit-a", 100, json!(200)), credit("credit-a", 300, json!(400))]),
				2
			)),
			Err(ResetCardProtocolError::DuplicateCreditId)
		);
	}

	#[test]
	fn inventory_filters_known_non_available_credits_and_counts_available_cards() {
		let mut redeeming = credit("credit-redeeming", 300, Value::Null);
		redeeming["status"] = json!("redeeming");
		let mut redeemed = credit("credit-redeemed", 500, Value::Null);
		redeemed["status"] = json!("redeemed");
		let credits = json!([credit("credit-available", 100, json!(200)), redeeming, redeemed]);
		let inventory = decode_reset_card_inventory(&inventory_json(credits.clone(), 1)).unwrap();

		assert_eq!(inventory.available_count(), 1);
		assert_eq!(
			inventory.resolve_exact_credit_id(descriptor(100, 200)).unwrap().as_str(),
			"credit-available"
		);
		assert!(
			inventory
				.contains_exact_credit_id(&ExactResetCreditId::new("credit-available").unwrap())
		);
		assert!(
			!inventory
				.contains_exact_credit_id(&ExactResetCreditId::new("credit-redeemed").unwrap())
		);
		assert_eq!(
			decode_reset_card_inventory(&inventory_json(credits, 2)),
			Err(ResetCardProtocolError::InventoryCountMismatch)
		);
	}

	#[test]
	fn inventory_rejects_incomplete_invalid_or_unknown_card_fields() {
		let cases = [
			(credit("credit-a", 100, Value::Null), ResetCardProtocolError::IncompleteCreditDetails),
			(credit("credit-a", -1, json!(200)), ResetCardProtocolError::InvalidCreditTimestamp),
			(credit("credit-a", 200, json!(200)), ResetCardProtocolError::InvalidCreditChronology),
			(
				{
					let mut value = credit("credit-a", 100, json!(200));
					value["resetType"] = json!("unknown");
					value
				},
				ResetCardProtocolError::UnsupportedResetType,
			),
			(
				{
					let mut value = credit("credit-a", 100, json!(200));
					value["status"] = json!("unknown");
					value
				},
				ResetCardProtocolError::UnknownCreditStatus,
			),
		];

		for (credit, expected) in cases {
			assert_eq!(
				decode_reset_card_inventory(&inventory_json(json!([credit]), 1)),
				Err(expected)
			);
		}

		let mut unknown_field = credit("credit-a", 100, json!(200));
		unknown_field["providerExtension"] = json!(true);

		assert_eq!(
			decode_reset_card_inventory(&inventory_json(json!([unknown_field]), 1)),
			Err(ResetCardProtocolError::MalformedResponse)
		);
	}

	#[test]
	fn consume_outcomes_are_strict_and_closed() {
		for (wire, outcome) in [
			("reset", ResetCardConsumeOutcome::Reset),
			("nothingToReset", ResetCardConsumeOutcome::NothingToReset),
			("noCredit", ResetCardConsumeOutcome::NoCredit),
			("alreadyRedeemed", ResetCardConsumeOutcome::AlreadyRedeemed),
		] {
			let bytes = serde_json::to_vec(&json!({"outcome": wire})).unwrap();
			let decoded = decode_reset_card_consume_result(&bytes).unwrap();
			let generic: ResetCardConsumeResult = serde_json::from_slice(&bytes).unwrap();

			assert_eq!(decoded.outcome(), outcome);
			assert_eq!(generic.outcome(), outcome);
		}

		assert_eq!(
			decode_reset_card_consume_result(br#"{"outcome":"futureOutcome"}"#),
			Err(ResetCardProtocolError::UnknownConsumeOutcome)
		);
		assert_eq!(
			decode_reset_card_consume_result(br#"{"outcome":"reset","extra":true}"#),
			Err(ResetCardProtocolError::MalformedResponse)
		);
	}

	#[test]
	fn reset_card_capability_is_optional_and_requires_both_exact_methods() {
		let accepted = SchemaContract::validate(SchemaMarker::accepted()).unwrap();
		let accepted_profile = ResetCardCapabilityProfile::from_schema(&accepted);

		assert_eq!(accepted_profile.state(), ResetCardCapabilityState::ConsumeMethodMissing);
		assert!(!accepted_profile.is_supported());
		assert!(!REQUIRED_REQUEST_METHODS.contains(&RESET_CARD_CONSUME_METHOD));

		let requests = REQUIRED_REQUEST_METHODS
			.iter()
			.copied()
			.chain([RESET_CARD_READ_METHOD, RESET_CARD_CONSUME_METHOD])
			.map(str::to_owned)
			.collect::<BTreeSet<_>>();
		let notifications =
			REQUIRED_NOTIFICATION_METHODS.iter().map(|method| (*method).to_owned()).collect();
		let supported =
			SchemaContract::from_generated(requests, notifications, true, true).unwrap();
		let profile = ResetCardCapabilityProfile::from_schema(&supported);

		assert_eq!(profile.state(), ResetCardCapabilityState::Supported);
		assert!(profile.is_supported());
	}
}
