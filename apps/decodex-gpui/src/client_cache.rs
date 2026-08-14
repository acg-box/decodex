//! Private, disposable client cache with immutable, content-attested generations.
//!
//! The cache is never product-state authority. Its only mutable publication fact is
//! the atomically replaced `current` pointer; published generation directories are
//! immutable and may be discarded wholesale.

use std::{
	collections::{BTreeMap, BTreeSet},
	fmt::{Display, Formatter},
	fs::{self, DirBuilder, File, Metadata, OpenOptions},
	io::{self, ErrorKind},
	path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use sha2::{Digest, Sha256};

use decodex_protocol::{EntityId, EntityRevision, ProtocolVersion, ServerId, Sha256Digest};

const MANIFEST_VERSION: u32 = 1;
const CURRENT_VERSION: u32 = 1;
const MANIFEST_FILE: &str = "manifest.json";
const OBJECTS_DIRECTORY: &str = "objects";
const GENERATIONS_DIRECTORY: &str = "generations";
const CURRENT_FILE: &str = "current";
const CURRENT_NEXT_FILE: &str = ".current.next";
const STAGING_DIRECTORY: &str = ".staging";
const WRITER_LOCK_FILE: &str = ".writer.lock";
const MAX_MANIFEST_BYTES: usize = 4 * 1_024 * 1_024;
const MAX_GENERATIONS: usize = 128;
const MAX_OBJECTS: usize = 16_384;
const MAX_BYTES: u64 = 512 * 1_024 * 1_024;
const COPY_BUFFER_BYTES: usize = 64 * 1_024;

/// Deterministic fault hook used only at explicit durability boundaries.
pub(crate) trait FaultInjector {
	/// Return an injected crash at a selected boundary.
	fn check(&mut self, point: FaultPoint) -> Result<(), CacheError>;
}

/// Explicit physical limits for the disposable cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CacheLimits {
	max_bytes: u64,
	max_objects: usize,
	max_generations: usize,
}
impl CacheLimits {
	/// Construct nonzero limits bounded by fixed process ceilings.
	pub(crate) fn new(
		max_bytes: u64,
		max_objects: usize,
		max_generations: usize,
	) -> Result<Self, CacheError> {
		if max_bytes == 0
			|| max_bytes > MAX_BYTES
			|| max_objects == 0
			|| max_objects > MAX_OBJECTS
			|| max_generations == 0
			|| max_generations > MAX_GENERATIONS
		{
			return Err(CacheError::InvalidLimits);
		}

		Ok(Self { max_bytes, max_objects, max_generations })
	}

	fn contains(self, usage: Usage) -> bool {
		usage.bytes <= self.max_bytes
			&& usage.objects <= self.max_objects
			&& usage.generations <= self.max_generations
	}
}

/// Exact server and protocol/schema authority for one cache generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CacheAuthority {
	server_id: String,
	protocol_major: u16,
	protocol_minor: u16,
	schema_generation: u64,
}
impl CacheAuthority {
	/// Bind a cache authority to validated wire identity and an explicit schema generation.
	pub(crate) fn new(
		server_id: &ServerId,
		protocol: ProtocolVersion,
		schema_generation: u64,
	) -> Result<Self, CacheError> {
		if schema_generation == 0 {
			return Err(CacheError::InvalidAuthority);
		}

		Ok(Self {
			server_id: server_id.as_str().to_owned(),
			protocol_major: protocol.major,
			protocol_minor: protocol.minor,
			schema_generation,
		})
	}
}
impl<'de> Deserialize<'de> for CacheAuthority {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		#[derive(Deserialize)]
		#[serde(deny_unknown_fields)]
		struct RawAuthority {
			server_id: String,
			protocol_major: u16,
			protocol_minor: u16,
			schema_generation: u64,
		}

		let raw = RawAuthority::deserialize(deserializer)?;
		let server_id = ServerId::new(raw.server_id).map_err(D::Error::custom)?;

		Self::new(
			&server_id,
			ProtocolVersion { major: raw.protocol_major, minor: raw.protocol_minor },
			raw.schema_generation,
		)
		.map_err(D::Error::custom)
	}
}
impl Serialize for CacheAuthority {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		#[derive(Serialize)]
		struct RawAuthority<'a> {
			server_id: &'a str,
			protocol_major: u16,
			protocol_minor: u16,
			schema_generation: u64,
		}

		RawAuthority {
			server_id: &self.server_id,
			protocol_major: self.protocol_major,
			protocol_minor: self.protocol_minor,
			schema_generation: self.schema_generation,
		}
		.serialize(serializer)
	}
}

/// Whether an object is authoritative or must survive until explicitly resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ObjectCertainty {
	/// The server authoritatively classified this object.
	Authoritative,
	/// The client cannot yet classify this object and must retain it.
	Uncertain,
}

/// One object supplied for a new immutable generation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ObjectInput<'a> {
	entity_id: &'a EntityId,
	revision: EntityRevision,
	bytes: &'a [u8],
	certainty: ObjectCertainty,
}
impl<'a> ObjectInput<'a> {
	/// Construct one entity/revision/content observation.
	pub(crate) const fn new(
		entity_id: &'a EntityId,
		revision: EntityRevision,
		bytes: &'a [u8],
		certainty: ObjectCertainty,
	) -> Self {
		Self { entity_id, revision, bytes, certainty }
	}
}

/// Explicit authoritative resolution of a previously uncertain entity revision.
#[derive(Clone, Copy, Debug)]
pub(crate) struct UncertainResolution<'a> {
	entity_id: &'a EntityId,
	revision: EntityRevision,
}
impl<'a> UncertainResolution<'a> {
	/// Name one uncertain identity that the caller has now authoritatively resolved.
	pub(crate) const fn new(entity_id: &'a EntityId, revision: EntityRevision) -> Self {
		Self { entity_id, revision }
	}
}

/// Filesystem ordering points exposed to deterministic crash tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaultPoint {
	/// The exclusive writer marker is durable.
	WriterLocked,
	/// The unpublished staging directory is durable.
	StagingCreated,
	/// Every object file and the object directory are durable.
	ObjectsDurable,
	/// The closed manifest and staging directory are durable.
	ManifestDurable,
	/// The generation rename and generations directory sync completed.
	GenerationPublished,
	/// The replacement current pointer file is durable but not renamed.
	CurrentPointerDurable,
	/// The current pointer rename occurred but its parent sync did not.
	CurrentPointerRenamed,
	/// The current pointer rename and parent directory sync completed.
	CurrentPointerPublished,
	/// Removing the writer marker is about to begin.
	WriterMarkerRemove,
	/// The parent-directory sync required after marker removal is about to begin.
	WriterDirectorySync,
}

/// Fault injector that permits every operation.
pub(crate) struct NoFault;
impl FaultInjector for NoFault {
	fn check(&mut self, _point: FaultPoint) -> Result<(), CacheError> {
		Ok(())
	}
}

/// Bounded summary of one verified immutable generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GenerationInspection {
	/// Content digest naming the generation directory.
	pub(crate) generation: String,
	/// Monotonic local publication sequence.
	pub(crate) sequence: u64,
	/// Exact authority bound into the manifest.
	pub(crate) authority: CacheAuthority,
	/// Number of entity/revision records.
	pub(crate) records: usize,
	/// Number of physically distinct content objects.
	pub(crate) physical_objects: usize,
	/// Number of unresolved uncertain records.
	pub(crate) uncertain_records: usize,
	/// Payload bytes referenced by the manifest.
	pub(crate) payload_bytes: u64,
}

/// Fail-closed cache error classes. Paths, raw identifiers, and content are omitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CacheError {
	/// Configured limits are zero or exceed fixed ceilings.
	InvalidLimits,
	/// Schema or wire authority is not a valid cache authority.
	InvalidAuthority,
	/// The cache root is not an absolute normalized path.
	UnsafeRoot,
	/// A symlink or unexpected filesystem object was observed.
	UnsafeArtifact,
	/// An unexpected name or directory shape was observed.
	ForeignArtifact,
	/// A writer/staging/pointer remnant proves an incomplete operation.
	CrashRemnant,
	/// Structured metadata was corrupt, truncated, oversized, or unknown-versioned.
	InvalidMetadata,
	/// Content bytes did not match their recorded length or SHA-256 digest.
	IntegrityMismatch,
	/// The current generation belongs to another server or protocol/schema authority.
	AuthorityMismatch,
	/// Existing or proposed physical usage exceeds configured limits.
	BoundsExceeded,
	/// Entity/revision input was duplicated or contradicted immutable content identity.
	ConflictingObject,
	/// An explicit uncertain resolution did not name a retained uncertain record.
	UnknownResolution,
	/// The requested generation does not exist.
	GenerationNotFound,
	/// The current valid generation cannot be disposed individually.
	CurrentGeneration,
	/// A generation was published without a matching current pointer.
	OrphanGeneration,
	/// Monotonic generation sequence capacity is exhausted.
	SequenceExhausted,
	/// A deterministic test fault simulated process loss.
	InjectedCrash(FaultPoint),
	/// A filesystem operation failed; only its non-sensitive error class is retained.
	Io(ErrorKind),
}
impl Display for CacheError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "client cache operation failed: {self:?}")
	}
}

impl std::error::Error for CacheError {}

impl From<io::Error> for CacheError {
	fn from(error: io::Error) -> Self {
		Self::Io(error.kind())
	}
}

/// Private cache owner bound to one exact server and protocol/schema authority.
pub(crate) struct ClientCache {
	root: PathBuf,
	limits: CacheLimits,
	authority: CacheAuthority,
}
impl ClientCache {
	/// Open a cache for exact-authority use and validate every reachable artifact.
	pub(crate) fn open(
		root: impl Into<PathBuf>,
		limits: CacheLimits,
		authority: CacheAuthority,
	) -> Result<Self, CacheError> {
		Self::open_inner(root.into(), limits, authority, false)
	}

	/// Open a fully valid cache while explicitly preparing an authority switch.
	///
	/// The old current generation remains inspectable but is never inherited into the
	/// first generation published under the new authority.
	pub(crate) fn prepare_authority_switch(
		root: impl Into<PathBuf>,
		limits: CacheLimits,
		authority: CacheAuthority,
	) -> Result<Self, CacheError> {
		Self::open_inner(root.into(), limits, authority, true)
	}

	fn open_inner(
		root: PathBuf,
		limits: CacheLimits,
		authority: CacheAuthority,
		allow_switch: bool,
	) -> Result<Self, CacheError> {
		ensure_absolute_normalized(&root)?;
		ensure_directory_chain(&root)?;
		validate_private_directory(&root)?;

		let generations = root.join(GENERATIONS_DIRECTORY);

		ensure_child_directory(&generations)?;
		validate_private_directory(&generations)?;
		validate_root_shape(&root, false)?;

		let cache = Self { root, limits, authority };
		let inventory = cache.read_inventory()?;

		if !allow_switch
			&& let Some(current) = inventory.current_generation()
			&& current.manifest.authority != cache.authority
		{
			return Err(CacheError::AuthorityMismatch);
		}

		cache.ensure_within_limits(&inventory)?;

		Ok(cache)
	}

	/// Verify and inspect the current generation without server access.
	pub(crate) fn inspect_current(&self) -> Result<Option<GenerationInspection>, CacheError> {
		let inventory = self.read_inventory()?;
		let Some(current) = inventory.current_generation() else {
			return Ok(None);
		};

		if current.manifest.authority != self.authority {
			return Err(CacheError::AuthorityMismatch);
		}

		Ok(Some(current.inspection()))
	}

	/// Verify and inspect one named generation without server access.
	pub(crate) fn inspect_generation(
		&self,
		generation: &str,
	) -> Result<GenerationInspection, CacheError> {
		validate_digest(generation)?;

		self.read_inventory()?
			.generations
			.iter()
			.find(|candidate| candidate.id == generation)
			.map(VerifiedGeneration::inspection)
			.ok_or(CacheError::GenerationNotFound)
	}

	/// Publish a new immutable generation using the no-fault filesystem path.
	pub(crate) fn publish(
		&self,
		objects: &[ObjectInput<'_>],
		resolved_uncertain: &[UncertainResolution<'_>],
	) -> Result<GenerationInspection, CacheError> {
		self.publish_with_faults(objects, resolved_uncertain, &mut NoFault)
	}

	/// Publish with deterministic crash injection at explicit durability boundaries.
	pub(crate) fn publish_with_faults(
		&self,
		objects: &[ObjectInput<'_>],
		resolved_uncertain: &[UncertainResolution<'_>],
		faults: &mut impl FaultInjector,
	) -> Result<GenerationInspection, CacheError> {
		let mut writer = WriterGuard::acquire(&self.root)?;

		if let Err(error) = faults.check(FaultPoint::WriterLocked) {
			writer.preserve();

			return Err(error);
		}

		let mut persistent_mutation = false;
		let result =
			self.publish_locked(objects, resolved_uncertain, faults, &mut persistent_mutation);

		match result {
			Ok(inspection) => {
				writer.release(faults)?;

				Ok(inspection)
			},
			Err(error) => {
				if persistent_mutation || matches!(error, CacheError::InjectedCrash(_)) {
					writer.preserve();
				}

				Err(error)
			},
		}
	}

	fn publish_locked(
		&self,
		objects: &[ObjectInput<'_>],
		resolved_uncertain: &[UncertainResolution<'_>],
		faults: &mut impl FaultInjector,
		persistent_mutation: &mut bool,
	) -> Result<GenerationInspection, CacheError> {
		validate_root_shape(&self.root, true)?;

		let mut inventory = self.read_inventory_locked()?;
		let sequence = inventory
			.generations
			.iter()
			.map(|generation| generation.manifest.sequence)
			.max()
			.unwrap_or(0)
			.checked_add(1)
			.ok_or(CacheError::SequenceExhausted)?;
		let plan = self.build_plan(&inventory, sequence, objects, resolved_uncertain)?;
		let manifest_bytes =
			serde_json::to_vec(&plan.manifest).map_err(|_| CacheError::InvalidMetadata)?;

		if manifest_bytes.len() > MAX_MANIFEST_BYTES {
			return Err(CacheError::BoundsExceeded);
		}

		let generation = sha256_hex(&manifest_bytes);
		let current_bytes = serialize_current(&generation)?;
		let peak = plan.peak_usage(&manifest_bytes, &current_bytes)?;

		self.evict_for_peak(&mut inventory, peak, persistent_mutation)?;

		*persistent_mutation = true;

		self.write_staging(&plan, &manifest_bytes, faults)?;
		self.publish_generation(&generation, faults)?;
		self.publish_current(&generation, &current_bytes, faults)?;

		let final_inventory = self.read_inventory_locked()?;

		self.ensure_within_limits(&final_inventory)?;

		let current = final_inventory.current_generation().ok_or(CacheError::InvalidMetadata)?;

		Ok(current.inspection())
	}

	fn build_plan<'a>(
		&self,
		inventory: &Inventory,
		sequence: u64,
		objects: &'a [ObjectInput<'a>],
		resolved_uncertain: &[UncertainResolution<'_>],
	) -> Result<PublicationPlan<'a>, CacheError> {
		let same_authority = inventory
			.current_generation()
			.filter(|generation| generation.manifest.authority == self.authority);
		let mut records = BTreeMap::<RecordKey, PlannedObject<'a>>::new();

		if let Some(current) = same_authority {
			for record in &current.manifest.objects {
				if record.certainty == ObjectCertainty::Uncertain {
					let key = RecordKey::new(&record.entity_id, record.revision);
					let source = current.object_path(&record.sha256);

					records.insert(key, PlannedObject::existing(record.clone(), source));
				}
			}
		}

		let mut resolution_keys = BTreeSet::new();

		for resolution in resolved_uncertain {
			let key = RecordKey::new(resolution.entity_id.as_str(), resolution.revision.0);

			if !resolution_keys
				.insert(RecordKey::new(resolution.entity_id.as_str(), resolution.revision.0))
			{
				return Err(CacheError::UnknownResolution);
			}

			match records.remove(&key) {
				Some(record) if record.record.certainty == ObjectCertainty::Uncertain => {},
				Some(record) => {
					records.insert(key, record);

					return Err(CacheError::UnknownResolution);
				},
				None => return Err(CacheError::UnknownResolution),
			}
		}

		let mut input_keys = BTreeSet::new();

		for input in objects {
			let key = RecordKey::new(input.entity_id.as_str(), input.revision.0);

			if resolution_keys.contains(&key)
				|| !input_keys.insert(RecordKey::new(input.entity_id.as_str(), input.revision.0))
			{
				return Err(CacheError::ConflictingObject);
			}

			let byte_length =
				u64::try_from(input.bytes.len()).map_err(|_| CacheError::BoundsExceeded)?;

			if byte_length > self.limits.max_bytes {
				return Err(CacheError::BoundsExceeded);
			}

			let digest = sha256_hex(input.bytes);
			let record = ObjectRecord {
				entity_id: input.entity_id.as_str().to_owned(),
				revision: input.revision.0,
				sha256: digest,
				byte_length,
				certainty: input.certainty,
			};

			if let Some(existing) = records.get(&key)
				&& (existing.record.sha256 != record.sha256
					|| existing.record.byte_length != record.byte_length)
			{
				return Err(CacheError::ConflictingObject);
			}

			records.insert(key, PlannedObject::input(record, input.bytes));
		}

		if records.len() > MAX_OBJECTS {
			return Err(CacheError::BoundsExceeded);
		}

		let objects = records.into_values().collect::<Vec<_>>();
		let physical_objects =
			objects.iter().map(|object| &object.record.sha256).collect::<BTreeSet<_>>();

		if physical_objects.len() > self.limits.max_objects {
			return Err(CacheError::BoundsExceeded);
		}

		let manifest = Manifest {
			version: MANIFEST_VERSION,
			sequence,
			authority: self.authority.clone(),
			objects: objects.iter().map(|object| object.record.clone()).collect(),
		};

		Ok(PublicationPlan { manifest, objects })
	}

	fn write_staging(
		&self,
		plan: &PublicationPlan<'_>,
		manifest_bytes: &[u8],
		faults: &mut impl FaultInjector,
	) -> Result<(), CacheError> {
		let staging = self.root.join(STAGING_DIRECTORY);

		create_new_directory(&staging)?;
		sync_directory(&self.root)?;

		faults.check(FaultPoint::StagingCreated)?;

		let object_directory = staging.join(OBJECTS_DIRECTORY);

		create_new_directory(&object_directory)?;

		let mut written = BTreeSet::new();

		for object in &plan.objects {
			if written.insert(object.record.sha256.clone()) {
				let destination = object_directory.join(&object.record.sha256);

				object.write_verified(&destination, self.limits.max_bytes)?;
			}
		}

		sync_directory(&object_directory)?;

		faults.check(FaultPoint::ObjectsDurable)?;

		write_new_synced(&staging.join(MANIFEST_FILE), manifest_bytes)?;
		sync_directory(&staging)?;

		faults.check(FaultPoint::ManifestDurable)
	}

	fn publish_generation(
		&self,
		generation: &str,
		faults: &mut impl FaultInjector,
	) -> Result<(), CacheError> {
		let source = self.root.join(STAGING_DIRECTORY);
		let target = self.root.join(GENERATIONS_DIRECTORY).join(generation);

		if fs::symlink_metadata(&target).is_ok() {
			return Err(CacheError::ForeignArtifact);
		}

		fs::rename(source, target)?;

		sync_directory(&self.root.join(GENERATIONS_DIRECTORY))?;

		faults.check(FaultPoint::GenerationPublished)
	}

	fn publish_current(
		&self,
		generation: &str,
		bytes: &[u8],
		faults: &mut impl FaultInjector,
	) -> Result<(), CacheError> {
		let next = self.root.join(CURRENT_NEXT_FILE);

		write_new_synced(&next, bytes)?;

		faults.check(FaultPoint::CurrentPointerDurable)?;

		let current = self.root.join(CURRENT_FILE);

		validate_optional_regular_file(&current)?;

		fs::rename(next, current)?;

		faults.check(FaultPoint::CurrentPointerRenamed)?;

		sync_directory(&self.root)?;

		faults.check(FaultPoint::CurrentPointerPublished)?;

		let pointer = read_current(&self.root, self.limits.max_bytes)?;

		if pointer.generation != generation {
			return Err(CacheError::InvalidMetadata);
		}

		Ok(())
	}

	fn evict_for_peak(
		&self,
		inventory: &mut Inventory,
		peak: Usage,
		persistent_mutation: &mut bool,
	) -> Result<(), CacheError> {
		let current = inventory.current.as_ref().map(|pointer| pointer.generation.as_str());
		let mut projected = inventory.usage.checked_add(peak)?;
		let mut candidates = inventory
			.generations
			.iter()
			.filter(|generation| Some(generation.id.as_str()) != current)
			.map(|generation| (generation.manifest.sequence, generation.id.clone()))
			.collect::<Vec<_>>();

		candidates.sort();

		for (_, id) in candidates {
			if self.limits.contains(projected) {
				break;
			}

			let index = inventory
				.generations
				.iter()
				.position(|generation| generation.id == id)
				.ok_or(CacheError::InvalidMetadata)?;
			let generation = inventory.generations.remove(index);

			*persistent_mutation = true;

			remove_verified_generation(&generation)?;
			sync_directory(&self.root.join(GENERATIONS_DIRECTORY))?;

			inventory.usage = inventory.usage.checked_sub(generation.usage)?;
			projected = projected.checked_sub(generation.usage)?;
		}

		if !self.limits.contains(projected) {
			return Err(CacheError::BoundsExceeded);
		}

		Ok(())
	}

	/// Dispose one fully verified non-current generation without server access.
	pub(crate) fn dispose_generation(&self, generation: &str) -> Result<(), CacheError> {
		self.dispose_generation_with_faults(generation, &mut NoFault)
	}

	fn dispose_generation_with_faults(
		&self,
		generation: &str,
		faults: &mut impl FaultInjector,
	) -> Result<(), CacheError> {
		validate_digest(generation)?;

		let mut writer = WriterGuard::acquire(&self.root)?;
		let mut persistent_mutation = false;
		let result = (|| {
			let inventory = self.read_inventory_locked()?;

			if inventory.current.as_ref().is_some_and(|current| current.generation == generation) {
				return Err(CacheError::CurrentGeneration);
			}

			let target = inventory
				.generations
				.iter()
				.find(|candidate| candidate.id == generation)
				.ok_or(CacheError::GenerationNotFound)?;

			persistent_mutation = true;

			remove_verified_generation(target)?;
			sync_directory(&self.root.join(GENERATIONS_DIRECTORY))
		})();

		match result {
			Ok(()) => writer.release(faults),
			Err(error) => {
				if persistent_mutation {
					writer.preserve();
				}

				Err(error)
			},
		}
	}

	/// Delete the complete disposable cache tree without following symlinks.
	pub(crate) fn dispose_all(root: &Path) -> Result<(), CacheError> {
		ensure_absolute_normalized(root)?;

		match fs::symlink_metadata(root) {
			Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
				Err(CacheError::UnsafeRoot)
			},
			Ok(_) => {
				validate_existing_directory_chain(root)?;

				remove_tree_without_following(root)
			},
			Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
			Err(error) => Err(error.into()),
		}
	}

	fn read_inventory(&self) -> Result<Inventory, CacheError> {
		self.read_inventory_inner(false)
	}

	fn read_inventory_locked(&self) -> Result<Inventory, CacheError> {
		self.read_inventory_inner(true)
	}

	fn read_inventory_inner(&self, allow_writer: bool) -> Result<Inventory, CacheError> {
		validate_root_shape(&self.root, allow_writer)?;

		let current = read_optional_current(&self.root, self.limits.max_bytes)?;
		let generations_root = self.root.join(GENERATIONS_DIRECTORY);
		let entry_limit =
			self.limits.max_generations.checked_add(1).ok_or(CacheError::BoundsExceeded)?;
		let entries = bounded_entries(&generations_root, entry_limit)?;

		if entries.len() > self.limits.max_generations {
			return Err(CacheError::BoundsExceeded);
		}

		let mut preflight = Vec::new();
		let mut usage = current.as_ref().map_or(Usage::default(), |pointer| pointer.usage);

		if !self.limits.contains(usage) {
			return Err(CacheError::BoundsExceeded);
		}

		for entry in entries {
			let name = entry.file_name().into_string().map_err(|_| CacheError::ForeignArtifact)?;

			validate_digest(&name)?;

			let metadata = fs::symlink_metadata(entry.path())?;

			if metadata.file_type().is_symlink() || !metadata.is_dir() {
				return Err(CacheError::UnsafeArtifact);
			}

			let generation = preflight_generation(entry.path(), name, self.limits)?;

			usage = usage.checked_add(generation.usage)?;

			if !self.limits.contains(usage) {
				return Err(CacheError::BoundsExceeded);
			}

			preflight.push(generation);
		}

		preflight.sort_by(|left, right| {
			left.manifest
				.sequence
				.cmp(&right.manifest.sequence)
				.then_with(|| left.id.cmp(&right.id))
		});

		validate_current_relation(current.as_ref(), &preflight)?;

		let generations = preflight
			.into_iter()
			.map(|generation| generation.verify(self.limits.max_bytes))
			.collect::<Result<Vec<_>, _>>()?;

		Ok(Inventory { current, generations, usage })
	}

	fn ensure_within_limits(&self, inventory: &Inventory) -> Result<(), CacheError> {
		if self.limits.contains(inventory.usage) { Ok(()) } else { Err(CacheError::BoundsExceeded) }
	}
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
	version: u32,
	sequence: u64,
	authority: CacheAuthority,
	objects: Vec<ObjectRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ObjectRecord {
	entity_id: String,
	revision: u64,
	sha256: String,
	byte_length: u64,
	certainty: ObjectCertainty,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CurrentPointer {
	version: u32,
	generation: String,
	#[serde(skip)]
	usage: Usage,
}

struct Inventory {
	current: Option<CurrentPointer>,
	generations: Vec<VerifiedGeneration>,
	usage: Usage,
}
impl Inventory {
	fn current_generation(&self) -> Option<&VerifiedGeneration> {
		let current = self.current.as_ref()?;

		self.generations.iter().find(|generation| generation.id == current.generation)
	}
}

struct VerifiedGeneration {
	id: String,
	path: PathBuf,
	manifest: Manifest,
	usage: Usage,
	payload_bytes: u64,
	physical_objects: usize,
}
impl VerifiedGeneration {
	fn inspection(&self) -> GenerationInspection {
		GenerationInspection {
			generation: self.id.clone(),
			sequence: self.manifest.sequence,
			authority: self.manifest.authority.clone(),
			records: self.manifest.objects.len(),
			physical_objects: self.physical_objects,
			uncertain_records: self
				.manifest
				.objects
				.iter()
				.filter(|object| object.certainty == ObjectCertainty::Uncertain)
				.count(),
			payload_bytes: self.payload_bytes,
		}
	}

	fn object_path(&self, digest: &str) -> PathBuf {
		self.path.join(OBJECTS_DIRECTORY).join(digest)
	}
}

struct PreflightObject {
	path: PathBuf,
	digest: String,
	expected_length: u64,
}

struct PreflightGeneration {
	id: String,
	path: PathBuf,
	manifest: Manifest,
	usage: Usage,
	payload_bytes: u64,
	objects: Vec<PreflightObject>,
}
impl PreflightGeneration {
	fn verify(self, max_bytes: u64) -> Result<VerifiedGeneration, CacheError> {
		for object in &self.objects {
			verify_file(&object.path, &object.digest, object.expected_length, max_bytes)?;
		}

		Ok(VerifiedGeneration {
			id: self.id,
			path: self.path,
			manifest: self.manifest,
			usage: self.usage,
			payload_bytes: self.payload_bytes,
			physical_objects: self.objects.len(),
		})
	}
}

#[derive(Clone, Copy, Default)]
struct Usage {
	bytes: u64,
	objects: usize,
	generations: usize,
}
impl Usage {
	fn checked_add(self, other: Self) -> Result<Self, CacheError> {
		Ok(Self {
			bytes: self.bytes.checked_add(other.bytes).ok_or(CacheError::BoundsExceeded)?,
			objects: self.objects.checked_add(other.objects).ok_or(CacheError::BoundsExceeded)?,
			generations: self
				.generations
				.checked_add(other.generations)
				.ok_or(CacheError::BoundsExceeded)?,
		})
	}

	fn checked_sub(self, other: Self) -> Result<Self, CacheError> {
		Ok(Self {
			bytes: self.bytes.checked_sub(other.bytes).ok_or(CacheError::InvalidMetadata)?,
			objects: self.objects.checked_sub(other.objects).ok_or(CacheError::InvalidMetadata)?,
			generations: self
				.generations
				.checked_sub(other.generations)
				.ok_or(CacheError::InvalidMetadata)?,
		})
	}
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
struct RecordKey {
	entity_id: String,
	revision: u64,
}
impl RecordKey {
	fn new(entity_id: &str, revision: u64) -> Self {
		Self { entity_id: entity_id.to_owned(), revision }
	}
}

enum ObjectSource<'a> {
	Input(&'a [u8]),
	Existing(PathBuf),
}

struct PlannedObject<'a> {
	record: ObjectRecord,
	source: ObjectSource<'a>,
}
impl<'a> PlannedObject<'a> {
	fn input(record: ObjectRecord, bytes: &'a [u8]) -> Self {
		Self { record, source: ObjectSource::Input(bytes) }
	}

	fn existing(record: ObjectRecord, path: PathBuf) -> Self {
		Self { record, source: ObjectSource::Existing(path) }
	}

	fn write_verified(&self, destination: &Path, max_bytes: u64) -> Result<(), CacheError> {
		match &self.source {
			ObjectSource::Input(bytes) => {
				if self.record.byte_length > max_bytes
					|| u64::try_from(bytes.len()).ok() != Some(self.record.byte_length)
					|| sha256_hex(bytes) != self.record.sha256
				{
					return Err(CacheError::IntegrityMismatch);
				}

				write_new_synced(destination, bytes)
			},
			ObjectSource::Existing(source) => copy_verified(
				source,
				destination,
				&self.record.sha256,
				self.record.byte_length,
				max_bytes,
			),
		}
	}
}

struct PublicationPlan<'a> {
	manifest: Manifest,
	objects: Vec<PlannedObject<'a>>,
}
impl PublicationPlan<'_> {
	fn peak_usage(&self, manifest: &[u8], current: &[u8]) -> Result<Usage, CacheError> {
		let unique = self
			.objects
			.iter()
			.map(|object| (&object.record.sha256, object.record.byte_length))
			.collect::<BTreeMap<_, _>>();
		let object_bytes = unique.values().try_fold(0_u64, |total, bytes| {
			total.checked_add(*bytes).ok_or(CacheError::BoundsExceeded)
		})?;
		let manifest_bytes =
			u64::try_from(manifest.len()).map_err(|_| CacheError::BoundsExceeded)?;
		let current_bytes = u64::try_from(current.len()).map_err(|_| CacheError::BoundsExceeded)?;

		Ok(Usage {
			bytes: object_bytes
				.checked_add(manifest_bytes)
				.and_then(|bytes| bytes.checked_add(current_bytes))
				.ok_or(CacheError::BoundsExceeded)?,
			objects: unique.len(),
			generations: 1,
		})
	}
}

struct WriterGuard {
	root: PathBuf,
	preserve: bool,
}
impl WriterGuard {
	fn acquire(root: &Path) -> Result<Self, CacheError> {
		validate_root_shape(root, false)?;

		let path = root.join(WRITER_LOCK_FILE);
		let file = open_new_private_file(&path)?;

		file.sync_all()?;

		sync_directory(root)?;

		Ok(Self { root: root.to_owned(), preserve: false })
	}

	fn preserve(&mut self) {
		self.preserve = true;
	}

	fn release(&mut self, faults: &mut impl FaultInjector) -> Result<(), CacheError> {
		if let Err(error) = faults.check(FaultPoint::WriterMarkerRemove) {
			self.preserve();

			return Err(error);
		}
		if let Err(error) = fs::remove_file(self.root.join(WRITER_LOCK_FILE)) {
			self.preserve();

			return Err(error.into());
		}

		self.preserve();

		if let Err(error) = faults.check(FaultPoint::WriterDirectorySync) {
			self.restore_marker()?;

			return Err(error);
		}

		if let Err(error) = sync_directory(&self.root) {
			self.restore_marker()?;

			return Err(error);
		}

		Ok(())
	}

	fn restore_marker(&self) -> Result<(), CacheError> {
		let file = open_new_private_file(&self.root.join(WRITER_LOCK_FILE))?;

		file.sync_all()?;
		sync_directory(&self.root)
	}
}

impl Drop for WriterGuard {
	fn drop(&mut self) {
		if !self.preserve {
			let _result = fs::remove_file(self.root.join(WRITER_LOCK_FILE));
			let _result = sync_directory(&self.root);
		}
	}
}

fn validate_root_shape(root: &Path, allow_writer: bool) -> Result<(), CacheError> {
	for entry in bounded_entries(root, 6)? {
		let name = entry.file_name().into_string().map_err(|_| CacheError::ForeignArtifact)?;
		let allowed = matches!(
			name.as_str(),
			GENERATIONS_DIRECTORY
				| CURRENT_FILE
				| CURRENT_NEXT_FILE
				| STAGING_DIRECTORY
				| WRITER_LOCK_FILE
		);

		if !allowed {
			return Err(CacheError::ForeignArtifact);
		}

		let metadata = fs::symlink_metadata(entry.path())?;

		if metadata.file_type().is_symlink() {
			return Err(CacheError::UnsafeArtifact);
		}
		if !has_private_permissions(&metadata) {
			return Err(CacheError::UnsafeArtifact);
		}
		if matches!(name.as_str(), CURRENT_NEXT_FILE | STAGING_DIRECTORY)
			|| name == WRITER_LOCK_FILE && !allow_writer
		{
			return Err(CacheError::CrashRemnant);
		}
		if name == GENERATIONS_DIRECTORY && !metadata.is_dir()
			|| name == CURRENT_FILE && !metadata.is_file()
		{
			return Err(CacheError::UnsafeArtifact);
		}
	}

	Ok(())
}

fn read_optional_current(
	root: &Path,
	max_bytes: u64,
) -> Result<Option<CurrentPointer>, CacheError> {
	let path = root.join(CURRENT_FILE);

	match fs::symlink_metadata(&path) {
		Ok(metadata) => {
			if metadata.file_type().is_symlink() || !metadata.is_file() {
				return Err(CacheError::UnsafeArtifact);
			}
			if metadata.len() > max_bytes {
				return Err(CacheError::BoundsExceeded);
			}

			let bytes = read_bounded(&path, MAX_MANIFEST_BYTES)?;
			let mut pointer = serde_json::from_slice::<CurrentPointer>(&bytes)
				.map_err(|_| CacheError::InvalidMetadata)?;

			if pointer.version != CURRENT_VERSION {
				return Err(CacheError::InvalidMetadata);
			}

			validate_digest(&pointer.generation)?;

			pointer.usage = Usage {
				bytes: u64::try_from(bytes.len()).map_err(|_| CacheError::BoundsExceeded)?,
				objects: 0,
				generations: 0,
			};

			Ok(Some(pointer))
		},
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.into()),
	}
}

fn read_current(root: &Path, max_bytes: u64) -> Result<CurrentPointer, CacheError> {
	read_optional_current(root, max_bytes)?.ok_or(CacheError::InvalidMetadata)
}

fn serialize_current(generation: &str) -> Result<Vec<u8>, CacheError> {
	serde_json::to_vec(&CurrentPointer {
		version: CURRENT_VERSION,
		generation: generation.to_owned(),
		usage: Usage::default(),
	})
	.map_err(|_| CacheError::InvalidMetadata)
}

fn validate_current_relation(
	current: Option<&CurrentPointer>,
	generations: &[PreflightGeneration],
) -> Result<(), CacheError> {
	match current {
		None if generations.is_empty() => Ok(()),
		None => Err(CacheError::OrphanGeneration),
		Some(pointer) => {
			let current_generation = generations
				.iter()
				.find(|generation| generation.id == pointer.generation)
				.ok_or(CacheError::InvalidMetadata)?;

			if generations.iter().any(|generation| {
				generation.manifest.sequence > current_generation.manifest.sequence
			}) {
				return Err(CacheError::OrphanGeneration);
			}

			Ok(())
		},
	}
}

fn preflight_generation(
	path: PathBuf,
	id: String,
	limits: CacheLimits,
) -> Result<PreflightGeneration, CacheError> {
	let entries = bounded_entries(&path, 3)?;

	if entries.len() != 2 {
		return Err(CacheError::ForeignArtifact);
	}

	let names = entries
		.iter()
		.map(|entry| entry.file_name().into_string().map_err(|_| CacheError::ForeignArtifact))
		.collect::<Result<BTreeSet<_>, _>>()?;

	if names != BTreeSet::from([MANIFEST_FILE.to_owned(), OBJECTS_DIRECTORY.to_owned()]) {
		return Err(CacheError::ForeignArtifact);
	}

	validate_private_directory(&path)?;
	validate_regular_file(&path.join(MANIFEST_FILE))?;
	validate_private_directory(&path.join(OBJECTS_DIRECTORY))?;

	if fs::symlink_metadata(path.join(MANIFEST_FILE))?.len() > limits.max_bytes {
		return Err(CacheError::BoundsExceeded);
	}

	let manifest_bytes = read_bounded(&path.join(MANIFEST_FILE), MAX_MANIFEST_BYTES)?;

	if sha256_hex(&manifest_bytes) != id {
		return Err(CacheError::IntegrityMismatch);
	}

	let manifest = serde_json::from_slice::<Manifest>(&manifest_bytes)
		.map_err(|_| CacheError::InvalidMetadata)?;

	if manifest.version != MANIFEST_VERSION {
		return Err(CacheError::InvalidMetadata);
	}

	let (expected, recorded_bytes) = validate_manifest_records(&manifest, limits)?;

	let object_directory = path.join(OBJECTS_DIRECTORY);
	let entry_limit = limits.max_objects.checked_add(1).ok_or(CacheError::BoundsExceeded)?;
	let entries = bounded_entries(&object_directory, entry_limit)?;

	if entries.len() > limits.max_objects {
		return Err(CacheError::BoundsExceeded);
	}

	if entries.len() != expected.len() {
		return Err(CacheError::IntegrityMismatch);
	}

	let mut payload_bytes = 0_u64;
	let mut objects = Vec::with_capacity(entries.len());

	for entry in entries {
		let name = entry.file_name().into_string().map_err(|_| CacheError::ForeignArtifact)?;

		let expected_length = *expected.get(&name).ok_or(CacheError::ForeignArtifact)?;
		let object_path = entry.path();

		validate_regular_file(&object_path)?;

		let physical_length = fs::symlink_metadata(&object_path)?.len();

		if physical_length > limits.max_bytes {
			return Err(CacheError::BoundsExceeded);
		}
		if physical_length != expected_length {
			return Err(CacheError::IntegrityMismatch);
		}

		payload_bytes =
			payload_bytes.checked_add(physical_length).ok_or(CacheError::BoundsExceeded)?;

		if payload_bytes > limits.max_bytes {
			return Err(CacheError::BoundsExceeded);
		}

		objects.push(PreflightObject { path: object_path, digest: name, expected_length });
	}

	if payload_bytes != recorded_bytes {
		return Err(CacheError::IntegrityMismatch);
	}

	let manifest_length =
		u64::try_from(manifest_bytes.len()).map_err(|_| CacheError::BoundsExceeded)?;
	let usage = Usage {
		bytes: payload_bytes.checked_add(manifest_length).ok_or(CacheError::BoundsExceeded)?,
		objects: expected.len(),
		generations: 1,
	};

	Ok(PreflightGeneration { id, path, manifest, usage, payload_bytes, objects })
}

fn validate_manifest_records(
	manifest: &Manifest,
	limits: CacheLimits,
) -> Result<(BTreeMap<String, u64>, u64), CacheError> {
	if manifest.sequence == 0 {
		return Err(CacheError::InvalidMetadata);
	}
	if manifest.objects.len() > MAX_OBJECTS {
		return Err(CacheError::BoundsExceeded);
	}

	let mut keys = BTreeSet::new();
	let mut content_lengths = BTreeMap::new();
	let mut recorded_bytes = 0_u64;

	for object in &manifest.objects {
		EntityId::new(object.entity_id.clone()).map_err(|_| CacheError::InvalidMetadata)?;

		validate_digest(&object.sha256)?;

		if object.byte_length > limits.max_bytes {
			return Err(CacheError::BoundsExceeded);
		}

		if !keys.insert((&object.entity_id, object.revision)) {
			return Err(CacheError::InvalidMetadata);
		}

		match content_lengths.get(&object.sha256) {
			Some(length) if *length != object.byte_length => {
				return Err(CacheError::InvalidMetadata);
			},
			Some(_) => {},
			None => {
				recorded_bytes = recorded_bytes
					.checked_add(object.byte_length)
					.ok_or(CacheError::BoundsExceeded)?;

				if recorded_bytes > limits.max_bytes {
					return Err(CacheError::BoundsExceeded);
				}

				content_lengths.insert(object.sha256.clone(), object.byte_length);
			},
		}
	}

	if content_lengths.len() > limits.max_objects {
		return Err(CacheError::BoundsExceeded);
	}

	Ok((content_lengths, recorded_bytes))
}

fn remove_verified_generation(generation: &VerifiedGeneration) -> Result<(), CacheError> {
	for entry in
		bounded_entries(&generation.path.join(OBJECTS_DIRECTORY), generation.physical_objects + 1)?
	{
		validate_regular_file(&entry.path())?;

		fs::remove_file(entry.path())?;
	}

	fs::remove_dir(generation.path.join(OBJECTS_DIRECTORY))?;

	validate_regular_file(&generation.path.join(MANIFEST_FILE))?;

	fs::remove_file(generation.path.join(MANIFEST_FILE))?;
	fs::remove_dir(&generation.path)?;

	Ok(())
}

fn copy_verified(
	source: &Path,
	destination: &Path,
	expected_digest: &str,
	expected_length: u64,
	max_bytes: u64,
) -> Result<(), CacheError> {
	if expected_length > max_bytes {
		return Err(CacheError::BoundsExceeded);
	}

	validate_regular_file(source)?;

	let source_length = fs::symlink_metadata(source)?.len();

	if source_length > max_bytes {
		return Err(CacheError::BoundsExceeded);
	}
	if source_length != expected_length {
		return Err(CacheError::IntegrityMismatch);
	}

	let read_limit = expected_length.checked_add(1).ok_or(CacheError::BoundsExceeded)?;
	let mut input = std::io::Read::take(File::open(source)?, read_limit);
	let mut output = open_new_private_file(destination)?;
	let mut hasher = Sha256::new();
	let mut length = 0_u64;
	let mut buffer = [0_u8; COPY_BUFFER_BYTES];

	loop {
		let read = std::io::Read::read(&mut input, &mut buffer)?;

		if read == 0 {
			break;
		}

		let read_u64 = u64::try_from(read).map_err(|_| CacheError::IntegrityMismatch)?;

		length = length.checked_add(read_u64).ok_or(CacheError::IntegrityMismatch)?;

		if length > expected_length {
			return Err(CacheError::IntegrityMismatch);
		}

		hasher.update(&buffer[..read]);
		std::io::Write::write_all(&mut output, &buffer[..read])?;
	}

	if length != expected_length || encode_digest(hasher.finalize().as_slice()) != expected_digest {
		return Err(CacheError::IntegrityMismatch);
	}

	output.sync_all()?;

	Ok(())
}

fn verify_file(
	path: &Path,
	digest: &str,
	expected_length: u64,
	max_bytes: u64,
) -> Result<(), CacheError> {
	if expected_length > max_bytes {
		return Err(CacheError::BoundsExceeded);
	}

	validate_regular_file(path)?;

	let physical_length = fs::symlink_metadata(path)?.len();

	if physical_length > max_bytes {
		return Err(CacheError::BoundsExceeded);
	}
	if physical_length != expected_length {
		return Err(CacheError::IntegrityMismatch);
	}

	let read_limit = expected_length.checked_add(1).ok_or(CacheError::BoundsExceeded)?;
	let mut file = std::io::Read::take(File::open(path)?, read_limit);
	let mut hasher = Sha256::new();
	let mut length = 0_u64;
	let mut buffer = [0_u8; COPY_BUFFER_BYTES];

	loop {
		let read = std::io::Read::read(&mut file, &mut buffer)?;

		if read == 0 {
			break;
		}

		length = length
			.checked_add(u64::try_from(read).map_err(|_| CacheError::IntegrityMismatch)?)
			.ok_or(CacheError::IntegrityMismatch)?;

		if length > expected_length {
			return Err(CacheError::IntegrityMismatch);
		}

		hasher.update(&buffer[..read]);
	}

	if length != expected_length || encode_digest(hasher.finalize().as_slice()) != digest {
		return Err(CacheError::IntegrityMismatch);
	}

	Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
	encode_digest(Sha256::digest(bytes).as_slice())
}

fn encode_digest(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";

	let mut output = String::with_capacity(bytes.len() * 2);

	for byte in bytes {
		output.push(char::from(HEX[usize::from(byte >> 4)]));
		output.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}

	output
}

fn validate_digest(value: &str) -> Result<(), CacheError> {
	Sha256Digest::new(value.to_owned()).map(|_| ()).map_err(|_| CacheError::InvalidMetadata)
}

fn ensure_absolute_normalized(path: &Path) -> Result<(), CacheError> {
	if !path.is_absolute()
		|| path
			.components()
			.any(|component| matches!(component, Component::ParentDir | Component::CurDir))
	{
		return Err(CacheError::UnsafeRoot);
	}

	Ok(())
}

fn ensure_directory_chain(path: &Path) -> Result<(), CacheError> {
	let mut current = PathBuf::new();

	for component in path.components() {
		current.push(component.as_os_str());

		if matches!(component, Component::RootDir | Component::Prefix(_)) {
			continue;
		}

		match fs::symlink_metadata(&current) {
			Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
				return Err(CacheError::UnsafeRoot);
			},
			Ok(_) => {},
			Err(error) if error.kind() == ErrorKind::NotFound => {
				create_private_directory(&current)?;
				validate_private_directory(&current)?;

				if let Some(parent) = current.parent() {
					sync_directory(parent)?;
				}
			},
			Err(error) => return Err(error.into()),
		}
	}

	Ok(())
}

fn validate_existing_directory_chain(path: &Path) -> Result<(), CacheError> {
	let mut current = PathBuf::new();

	for component in path.components() {
		current.push(component.as_os_str());

		if matches!(component, Component::RootDir | Component::Prefix(_)) {
			continue;
		}

		let metadata = fs::symlink_metadata(&current)?;

		if metadata.file_type().is_symlink() || !metadata.is_dir() {
			return Err(CacheError::UnsafeRoot);
		}
	}

	Ok(())
}

fn ensure_child_directory(path: &Path) -> Result<(), CacheError> {
	match fs::symlink_metadata(path) {
		Ok(_) => validate_directory(path),
		Err(error) if error.kind() == ErrorKind::NotFound => {
			create_private_directory(path)?;
			validate_private_directory(path)?;

			let parent = path.parent().ok_or(CacheError::UnsafeRoot)?;

			sync_directory(parent)
		},
		Err(error) => Err(error.into()),
	}
}

fn create_new_directory(path: &Path) -> Result<(), CacheError> {
	create_private_directory(path)?;

	validate_private_directory(path)
}

fn validate_directory(path: &Path) -> Result<(), CacheError> {
	let metadata = fs::symlink_metadata(path)?;

	if metadata.file_type().is_symlink() || !metadata.is_dir() {
		return Err(CacheError::UnsafeArtifact);
	}

	Ok(())
}

fn validate_private_directory(path: &Path) -> Result<(), CacheError> {
	validate_directory(path)?;

	if !has_private_permissions(&fs::symlink_metadata(path)?) {
		return Err(CacheError::UnsafeArtifact);
	}

	Ok(())
}

fn validate_regular_file(path: &Path) -> Result<(), CacheError> {
	let metadata = fs::symlink_metadata(path)?;

	if metadata.file_type().is_symlink()
		|| !metadata.is_file()
		|| !has_private_permissions(&metadata)
	{
		return Err(CacheError::UnsafeArtifact);
	}

	Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), CacheError> {
	let builder = private_directory_builder();

	builder.create(path)?;

	Ok(())
}

#[cfg(unix)]
fn private_directory_builder() -> DirBuilder {
	let mut builder = DirBuilder::new();

	std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);

	builder
}

#[cfg(not(unix))]
fn private_directory_builder() -> DirBuilder {
	DirBuilder::new()
}

fn open_new_private_file(path: &Path) -> Result<File, CacheError> {
	let mut options = OpenOptions::new();

	options.write(true).create_new(true);
	#[cfg(unix)]
	std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);

	options.open(path).map_err(Into::into)
}

#[cfg(unix)]
fn has_private_permissions(metadata: &Metadata) -> bool {
	std::os::unix::fs::PermissionsExt::mode(&metadata.permissions()) & 0o077 == 0
}

#[cfg(not(unix))]
fn has_private_permissions(_metadata: &Metadata) -> bool {
	true
}

fn validate_optional_regular_file(path: &Path) -> Result<(), CacheError> {
	match fs::symlink_metadata(path) {
		Ok(_) => validate_regular_file(path),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

fn bounded_entries(path: &Path, limit: usize) -> Result<Vec<fs::DirEntry>, CacheError> {
	validate_directory(path)?;

	let mut entries = Vec::new();

	for entry in fs::read_dir(path)? {
		if entries.len() == limit {
			return Err(CacheError::BoundsExceeded);
		}

		entries.push(entry?);
	}

	entries.sort_by_key(fs::DirEntry::file_name);

	Ok(entries)
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, CacheError> {
	validate_regular_file(path)?;

	let length = fs::symlink_metadata(path)?.len();

	if length > u64::try_from(limit).map_err(|_| CacheError::BoundsExceeded)? {
		return Err(CacheError::InvalidMetadata);
	}

	let mut bytes =
		Vec::with_capacity(usize::try_from(length).map_err(|_| CacheError::InvalidMetadata)?);

	let mut reader = std::io::Read::take(
		File::open(path)?,
		u64::try_from(limit + 1).map_err(|_| CacheError::InvalidMetadata)?,
	);

	std::io::Read::read_to_end(&mut reader, &mut bytes)?;

	if bytes.len() > limit {
		return Err(CacheError::InvalidMetadata);
	}

	Ok(bytes)
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), CacheError> {
	let mut file = open_new_private_file(path)?;

	std::io::Write::write_all(&mut file, bytes)?;
	file.sync_all()?;

	Ok(())
}

fn sync_directory(path: &Path) -> Result<(), CacheError> {
	validate_directory(path)?;

	File::open(path)?.sync_all()?;

	Ok(())
}

fn remove_tree_without_following(path: &Path) -> Result<(), CacheError> {
	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let metadata = fs::symlink_metadata(entry.path())?;

		if metadata.is_dir() && !metadata.file_type().is_symlink() {
			remove_tree_without_following(&entry.path())?;
		} else {
			fs::remove_file(entry.path())?;
		}
	}

	fs::remove_dir(path)?;

	Ok(())
}

#[cfg(test)]
#[path = "client_cache/tests.rs"]
mod tests;
