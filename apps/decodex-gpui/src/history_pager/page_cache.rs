//! Disposable, owner-private filesystem cache for bounded history pages.

use std::{
	cmp::Ordering,
	collections::{BTreeMap, BTreeSet},
	ffi::{CStr, CString},
	fs::File,
	io,
	os::{
		fd::{AsRawFd as _, FromRawFd as _, IntoRawFd as _, RawFd},
		unix::ffi::OsStrExt as _,
	},
	path::{Component, Path},
};

use decodex_protocol::{ConversationHistoryPage, EntityId, HistoryCursorToken, ServerId};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const CACHE_DIRECTORY_NAME: &CStr = c"history-page-cache-v1";
const LOCK_NAME: &CStr = c"lock";
const INDEX_NAME: &CStr = c"index";
const INDEX_STAGE_NAME: &CStr = c".index.next";
const PAGES_DIRECTORY_NAME: &CStr = c"pages";
const PAGE_STAGE_NAME: &CStr = c".page.next";
const CACHE_SCHEMA_ID: &str = "decodex.gpui.history-page-cache/1";
const CACHE_SCHEMA_GENERATION: u32 = 1;

const PRIVATE_DIRECTORY_MODE: libc::mode_t = 0o700;
const PRIVATE_FILE_MODE: libc::mode_t = 0o600;
#[cfg(target_vendor = "apple")]
const ANCESTOR_DIRECTORY_ACCESS: libc::c_int = libc::O_SEARCH;
#[cfg(not(target_vendor = "apple"))]
const ANCESTOR_DIRECTORY_ACCESS: libc::c_int = libc::O_RDONLY;
const MAX_PAGE_ITEMS: usize = 8;
const MAX_PAGE_BYTES: usize = 256 * 1_024;
const MAX_CONVERSATION_PAGES: usize = 4;
const MAX_CONVERSATION_ITEMS: usize = 32;
const MAX_CONVERSATION_BYTES: usize = 1_024 * 1_024;
const MAX_CACHE_CONVERSATIONS: usize = 8;
const MAX_CACHE_PAGES: usize = 32;
const MAX_CACHE_ITEMS: usize = 256;
const MAX_CACHE_BYTES: usize = 8 * 1_024 * 1_024;
const MAX_INDEX_BYTES: usize = 64 * 1_024;
const MAX_PHYSICAL_BYTES: usize = 9 * 1_024 * 1_024;
const MAX_PHYSICAL_PAGE_NAMES: usize = MAX_CACHE_PAGES * 2 + 1;
const MAX_IDENTITY_BYTES: usize = 4_096;
const MAX_CURSOR_BYTES: usize = 128;
const FRESH_ELIGIBILITY_SECONDS: i64 = 15 * 60;
const SHA256_HEX_LENGTH: usize = 64;

#[derive(Clone, Copy)]
enum DirectoryShape {
	Root,
	Pages,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct AuthorityIdentity {
	stable_server_id: ServerId,
	protocol_major: u16,
	protocol_minor: u16,
	cache_schema_generation: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CacheRequestKey {
	Head,
	After(HistoryCursorToken),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct PageIdentity {
	authority: AuthorityIdentity,
	conversation_id: EntityId,
	request_key: CacheRequestKey,
	page_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct IndexEntry {
	identity: PageIdentity,
	fresh_received_at_unix_seconds: i64,
	recency: u64,
	item_count: u8,
	byte_length: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct CacheIndex {
	schema_id: String,
	entries: Vec<IndexEntry>,
}

impl CacheIndex {
	fn empty() -> Self {
		Self { schema_id: CACHE_SCHEMA_ID.to_owned(), entries: Vec::new() }
	}
}

#[derive(Clone)]
struct ValidatedPageFile {
	byte_length: usize,
	item_count: usize,
}

struct CacheInventory {
	page_files: BTreeMap<String, ValidatedPageFile>,
	physical_bytes: usize,
	index_stage_present: bool,
	page_stage_present: bool,
}

struct ValidatedCacheState {
	index: CacheIndex,
	inventory: CacheInventory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CacheAuthority {
	identity: AuthorityIdentity,
}

impl CacheAuthority {
	pub(super) fn new(
		stable_server_id: ServerId,
		protocol_major: u16,
		protocol_minor: u16,
		cache_schema_generation: u32,
	) -> Result<Self, CacheFailure> {
		let identity = AuthorityIdentity {
			stable_server_id,
			protocol_major,
			protocol_minor,
			cache_schema_generation,
		};
		validate_authority(&identity)?;

		Ok(Self { identity })
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CacheRequest {
	authority: AuthorityIdentity,
	conversation_id: EntityId,
	request_key: CacheRequestKey,
}

impl CacheRequest {
	pub(super) fn head(
		authority: &CacheAuthority,
		conversation_id: EntityId,
	) -> Result<Self, CacheFailure> {
		Self::new(authority, conversation_id, CacheRequestKey::Head)
	}

	pub(super) fn after(
		authority: &CacheAuthority,
		conversation_id: EntityId,
		after: HistoryCursorToken,
	) -> Result<Self, CacheFailure> {
		Self::new(authority, conversation_id, CacheRequestKey::After(after))
	}

	fn new(
		authority: &CacheAuthority,
		conversation_id: EntityId,
		request_key: CacheRequestKey,
	) -> Result<Self, CacheFailure> {
		let request = Self { authority: authority.identity.clone(), conversation_id, request_key };
		validate_request(&request)?;

		Ok(request)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CacheHit {
	identity: PageIdentity,
	page: ConversationHistoryPage,
	fresh_received_at_unix_seconds: i64,
}

impl CacheHit {
	pub(super) const fn page(&self) -> &ConversationHistoryPage {
		&self.page
	}

	pub(super) fn into_page(self) -> ConversationHistoryPage {
		self.page
	}

	pub(super) const fn fresh_received_at_unix_seconds(&self) -> i64 {
		self.fresh_received_at_unix_seconds
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CacheLookup {
	Hit(CacheHit),
	Miss(CacheDiagnostic),
	Failure(CacheFailure),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CachePublishResult {
	Published,
	Reinitialized,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CacheDiagnostic {
	InvalidInput,
	IncompatibleSchema,
	NotFound,
	Ineligible,
	Integrity,
	UnsafeShape,
	Bounds,
	Filesystem,
	RecencyExhausted,
	DurabilityFault,
}

impl CacheDiagnostic {
	pub(super) const fn as_str(self) -> &'static str {
		match self {
			Self::InvalidInput => "history page cache input is invalid",
			Self::IncompatibleSchema => "history page cache schema is incompatible",
			Self::NotFound => "history page cache entry was not found",
			Self::Ineligible => "history page cache entry is ineligible",
			Self::Integrity => "history page cache integrity validation failed",
			Self::UnsafeShape => "history page cache filesystem shape is unsafe",
			Self::Bounds => "history page cache bounds were exceeded",
			Self::Filesystem => "history page cache filesystem operation failed",
			Self::RecencyExhausted => "history page cache recency is exhausted",
			Self::DurabilityFault => "history page cache durability fault was injected",
		}
	}
}

#[derive(Debug)]
pub(super) struct HistoryPageCache {
	root: File,
	pages: File,
	lock: File,
	index: CacheIndex,
	hit_recency: Vec<(PageIdentity, u64)>,
	next_recency: Option<u64>,
}

pub(super) struct PreparedCachePublication {
	candidate: CacheIndex,
	following_recency: u64,
	created_page_digest: Option<String>,
	reinitialized: bool,
}

pub(super) struct CommittedCachePublication {
	reinitialized: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct CacheFailure {
	diagnostic: CacheDiagnostic,
}

impl CacheFailure {
	pub(super) fn new(diagnostic: CacheDiagnostic) -> Self {
		Self { diagnostic }
	}

	pub(super) const fn diagnostic(&self) -> &'static str {
		self.diagnostic.as_str()
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DurabilityEdge {
	PageStageSync,
	PagePublish,
	PagesSync,
	IndexStageSync,
	IndexPublish,
	RootSync,
	CleanupSync,
}

trait FaultInjector {
	fn check(&self, edge: DurabilityEdge) -> Result<(), CacheFailure>;
}

struct NoFaults;

impl FaultInjector for NoFaults {
	fn check(&self, _edge: DurabilityEdge) -> Result<(), CacheFailure> {
		Ok(())
	}
}

impl HistoryPageCache {
	pub(super) fn open(parent: &Path, cache_schema_generation: u32) -> Result<Self, CacheFailure> {
		if cache_schema_generation != CACHE_SCHEMA_GENERATION {
			return Err(CacheFailure::new(CacheDiagnostic::IncompatibleSchema));
		}

		let parent = open_or_create_absolute_parent(parent)?;
		let root = open_or_create_directory_at(&parent, CACHE_DIRECTORY_NAME)?;
		let lock = open_or_create_file_at(&root, LOCK_NAME)?;
		lock_exclusive(&lock)?;
		validate_directory(&root)?;
		validate_regular_file(&lock, None)?;
		validate_directory_entries(&root, DirectoryShape::Root)?;
		let pages = open_or_create_directory_at(&root, PAGES_DIRECTORY_NAME)?;

		validate_directory(&root)?;
		validate_directory(&pages)?;
		validate_regular_file(&lock, None)?;
		validate_known_shape(&root, &pages)?;

		let state = load_validated_cache_state(&root, &pages, &lock)?;
		let next_recency =
			state.index.entries.iter().map(|entry| entry.recency).max().unwrap_or(0).checked_add(1);

		Ok(Self { root, pages, lock, index: state.index, hit_recency: Vec::new(), next_recency })
	}

	pub(super) fn lookup(&self, request: &CacheRequest, now_unix_seconds: i64) -> CacheLookup {
		if let Err(failure) = validate_request(request) {
			return CacheLookup::Failure(failure);
		}
		if now_unix_seconds < 0 {
			return CacheLookup::Failure(CacheFailure::new(CacheDiagnostic::InvalidInput));
		}

		match self.lookup_inner(request, now_unix_seconds) {
			Ok(lookup) => lookup,
			Err(failure) => CacheLookup::Failure(failure),
		}
	}

	pub(super) fn record_hit_recency(&mut self, hit: &CacheHit) -> Result<(), CacheFailure> {
		if !self.index.entries.iter().any(|entry| entry.identity == hit.identity) {
			return Err(CacheFailure::new(CacheDiagnostic::Integrity));
		}
		let (recency, next_recency) = self
			.next_recency
			.and_then(|recency| recency.checked_add(1).map(|next| (recency, next)))
			.ok_or_else(|| CacheFailure::new(CacheDiagnostic::RecencyExhausted))?;

		self.next_recency = Some(next_recency);
		if let Some(position) =
			self.hit_recency.iter().position(|(identity, _)| identity == &hit.identity)
		{
			self.hit_recency[position].1 = recency;
		} else {
			self.hit_recency.push((hit.identity.clone(), recency));
		}

		Ok(())
	}

	pub(super) fn prepare_publication(
		&mut self,
		request: &CacheRequest,
		page: &ConversationHistoryPage,
		fresh_received_at_unix_seconds: i64,
	) -> Result<PreparedCachePublication, CacheFailure> {
		validate_request(request)?;
		if fresh_received_at_unix_seconds < 0 {
			return Err(CacheFailure::new(CacheDiagnostic::InvalidInput));
		}
		let (page_bytes, page_sha256) = page_bytes_and_digest(page)?;

		self.prepare_publication_with_faults(
			request,
			page,
			fresh_received_at_unix_seconds,
			&page_bytes,
			&page_sha256,
			&NoFaults,
		)
	}

	pub(super) fn commit_publication(
		&mut self,
		prepared: PreparedCachePublication,
	) -> Result<CommittedCachePublication, (PreparedCachePublication, CacheFailure)> {
		self.commit_publication_with_faults(prepared, &NoFaults)
	}

	pub(super) fn finish_publication(
		&self,
		committed: CommittedCachePublication,
	) -> Result<CachePublishResult, CacheFailure> {
		self.finish_publication_with_faults(committed, &NoFaults)
	}

	pub(super) fn discard_prepared_publication(
		&self,
		prepared: PreparedCachePublication,
	) -> Result<(), CacheFailure> {
		self.discard_prepared_publication_with_faults(prepared, &NoFaults)
	}

	fn lookup_inner(
		&self,
		request: &CacheRequest,
		now_unix_seconds: i64,
	) -> Result<CacheLookup, CacheFailure> {
		validate_regular_file(&self.lock, None)?;
		validate_directory(&self.root)?;
		validate_directory(&self.pages)?;
		validate_known_shape(&self.root, &self.pages)?;

		let Some(entry) =
			self.index.entries.iter().find(|entry| entry_matches_request(entry, request)).cloned()
		else {
			return Ok(CacheLookup::Miss(CacheDiagnostic::NotFound));
		};
		if !is_fresh_eligible(entry.fresh_received_at_unix_seconds, now_unix_seconds) {
			return Ok(CacheLookup::Miss(CacheDiagnostic::Ineligible));
		}

		let (page, metadata) = read_validated_page(&self.pages, &entry.identity.page_sha256)?;
		if metadata.item_count != usize::from(entry.item_count)
			|| metadata.byte_length
				!= usize::try_from(entry.byte_length)
					.map_err(|_| CacheFailure::new(CacheDiagnostic::Bounds))?
		{
			return Err(CacheFailure::new(CacheDiagnostic::Integrity));
		}

		Ok(CacheLookup::Hit(CacheHit {
			identity: entry.identity,
			page,
			fresh_received_at_unix_seconds: entry.fresh_received_at_unix_seconds,
		}))
	}

	fn prepare_publication_with_faults(
		&mut self,
		request: &CacheRequest,
		page: &ConversationHistoryPage,
		fresh_received_at_unix_seconds: i64,
		page_bytes: &[u8],
		page_sha256: &str,
		faults: &impl FaultInjector,
	) -> Result<PreparedCachePublication, CacheFailure> {
		let mut state = load_validated_cache_state(&self.root, &self.pages, &self.lock)?;
		if state.index != self.index {
			return Err(CacheFailure::new(CacheDiagnostic::Integrity));
		}
		state = clean_known_remnants(&self.root, &self.pages, &self.lock, state, faults)?;

		let reinitialized = self.next_recency.and_then(|recency| recency.checked_add(1)).is_none();
		let (mut candidate, new_recency, following_recency) = if reinitialized {
			(CacheIndex::empty(), 1, 2)
		} else {
			let new_recency = self
				.next_recency
				.ok_or_else(|| CacheFailure::new(CacheDiagnostic::RecencyExhausted))?;
			let following_recency = new_recency
				.checked_add(1)
				.ok_or_else(|| CacheFailure::new(CacheDiagnostic::RecencyExhausted))?;
			let mut candidate = state.index.clone();
			merge_hit_recencies(&mut candidate, &self.hit_recency);
			(candidate, new_recency, following_recency)
		};
		candidate.entries.retain(|entry| !entry_matches_request(entry, request));
		candidate.entries.push(IndexEntry {
			identity: PageIdentity {
				authority: request.authority.clone(),
				conversation_id: request.conversation_id.clone(),
				request_key: request.request_key.clone(),
				page_sha256: page_sha256.to_owned(),
			},
			fresh_received_at_unix_seconds,
			recency: new_recency,
			item_count: u8::try_from(page.items.len())
				.map_err(|_| CacheFailure::new(CacheDiagnostic::Bounds))?,
			byte_length: u32::try_from(page_bytes.len())
				.map_err(|_| CacheFailure::new(CacheDiagnostic::Bounds))?,
		});
		evict_to_bounds(&mut candidate, fresh_received_at_unix_seconds)?;
		if !candidate.entries.iter().any(|entry| {
			entry_matches_request(entry, request)
				&& entry.identity.page_sha256 == page_sha256
				&& entry.fresh_received_at_unix_seconds == fresh_received_at_unix_seconds
		}) {
			return Err(CacheFailure::new(CacheDiagnostic::Bounds));
		}

		let mut candidate_pages = state.inventory.page_files.clone();
		candidate_pages.insert(
			page_sha256.to_owned(),
			ValidatedPageFile { byte_length: page_bytes.len(), item_count: page.items.len() },
		);
		validate_index(&candidate, &candidate_pages)?;
		let index_bytes = serialize_index(&candidate)?;

		let page_exists = state.inventory.page_files.contains_key(page_sha256);
		let after_page = if page_exists {
			state.inventory.physical_bytes
		} else {
			checked_add(state.inventory.physical_bytes, page_bytes.len())?
		};
		let page_publish_peak = if page_exists {
			state.inventory.physical_bytes
		} else {
			checked_add(after_page, page_bytes.len())?
		};
		let index_stage_peak = checked_add(after_page, index_bytes.len())?;
		if page_publish_peak > MAX_PHYSICAL_BYTES || index_stage_peak > MAX_PHYSICAL_BYTES {
			return Err(CacheFailure::new(CacheDiagnostic::Bounds));
		}

		let created_page = if page_exists {
			verify_page_target(&self.pages, page_sha256, page_bytes)?;
			false
		} else {
			publish_page(&self.pages, page_sha256, page_bytes, faults)?
		};
		stage_index(&self.root, &index_bytes, faults)?;

		Ok(PreparedCachePublication {
			candidate,
			following_recency,
			created_page_digest: created_page.then(|| page_sha256.to_owned()),
			reinitialized,
		})
	}

	fn commit_publication_with_faults(
		&mut self,
		prepared: PreparedCachePublication,
		faults: &impl FaultInjector,
	) -> Result<CommittedCachePublication, (PreparedCachePublication, CacheFailure)> {
		let commit_result = faults.check(DurabilityEdge::IndexPublish).and_then(|()| {
			rename_at(self.root.as_raw_fd(), INDEX_STAGE_NAME, self.root.as_raw_fd(), INDEX_NAME)
		});
		if let Err(failure) = commit_result {
			return Err((prepared, failure));
		}

		let PreparedCachePublication {
			candidate,
			following_recency,
			created_page_digest: _,
			reinitialized,
		} = prepared;
		self.index = candidate;
		self.hit_recency.clear();
		self.next_recency = Some(following_recency);

		Ok(CommittedCachePublication { reinitialized })
	}

	fn finish_publication_with_faults(
		&self,
		committed: CommittedCachePublication,
		faults: &impl FaultInjector,
	) -> Result<CachePublishResult, CacheFailure> {
		faults.check(DurabilityEdge::RootSync)?;
		sync_file(&self.root)?;
		let published = load_validated_cache_state(&self.root, &self.pages, &self.lock)?;
		if published.index != self.index {
			return Err(CacheFailure::new(CacheDiagnostic::Integrity));
		}
		let cleaned =
			clean_newly_unreferenced_pages(&self.root, &self.pages, &self.lock, published, faults)?;
		if cleaned.index != self.index {
			return Err(CacheFailure::new(CacheDiagnostic::Integrity));
		}

		Ok(if committed.reinitialized {
			CachePublishResult::Reinitialized
		} else {
			CachePublishResult::Published
		})
	}

	fn discard_prepared_publication_with_faults(
		&self,
		prepared: PreparedCachePublication,
		faults: &impl FaultInjector,
	) -> Result<(), CacheFailure> {
		unlink_at(self.root.as_raw_fd(), INDEX_STAGE_NAME)?;
		let mut removed_page = false;
		if let Some(digest) = prepared.created_page_digest.as_ref() {
			if referenced_digests(&self.index).contains(digest) {
				return Err(CacheFailure::new(CacheDiagnostic::Integrity));
			}
			let name = CString::new(digest.as_str())
				.map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
			unlink_at(self.pages.as_raw_fd(), &name)?;
			removed_page = true;
		}
		faults.check(DurabilityEdge::CleanupSync)?;
		sync_file(&self.root)?;
		if removed_page {
			faults.check(DurabilityEdge::CleanupSync)?;
			sync_file(&self.pages)?;
		}

		let discarded = load_validated_cache_state(&self.root, &self.pages, &self.lock)?;
		if discarded.index != self.index
			|| discarded.inventory.index_stage_present
			|| discarded.inventory.page_stage_present
			|| prepared
				.created_page_digest
				.as_ref()
				.is_some_and(|digest| discarded.inventory.page_files.contains_key(digest))
		{
			return Err(CacheFailure::new(CacheDiagnostic::Integrity));
		}

		Ok(())
	}
}

impl Drop for HistoryPageCache {
	fn drop(&mut self) {
		loop {
			if unsafe { libc::flock(self.lock.as_raw_fd(), libc::LOCK_UN) } == 0
				|| errno() != libc::EINTR
			{
				break;
			}
		}
	}
}

fn read_index(root: &File) -> Result<(CacheIndex, usize, bool), CacheFailure> {
	let Some(index_file) = open_optional_file_at(root, INDEX_NAME)? else {
		return Ok((CacheIndex::empty(), 0, false));
	};
	validate_regular_file(&index_file, Some(MAX_INDEX_BYTES))?;

	let bytes = read_bounded(&index_file, MAX_INDEX_BYTES)?;
	let index: CacheIndex = serde_json::from_slice(&bytes)
		.map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
	if index.schema_id != CACHE_SCHEMA_ID {
		return Err(CacheFailure::new(CacheDiagnostic::IncompatibleSchema));
	}
	if serialize_index(&index)? != bytes {
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}

	Ok((index, bytes.len(), true))
}

fn load_validated_cache_state(
	root: &File,
	pages: &File,
	lock: &File,
) -> Result<ValidatedCacheState, CacheFailure> {
	validate_directory(root)?;
	validate_directory(pages)?;
	validate_regular_file(lock, Some(0))?;
	let root_names = validated_directory_names(root, DirectoryShape::Root)?;
	let page_names = validated_directory_names(pages, DirectoryShape::Pages)?;
	if !root_names.iter().any(|name| name.as_slice() == LOCK_NAME.to_bytes())
		|| !root_names.iter().any(|name| name.as_slice() == PAGES_DIRECTORY_NAME.to_bytes())
	{
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}

	let (index, index_bytes, index_present) = read_index(root)?;
	if root_names.iter().any(|name| name.as_slice() == INDEX_NAME.to_bytes()) != index_present {
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}
	let index_stage_present =
		root_names.iter().any(|name| name.as_slice() == INDEX_STAGE_NAME.to_bytes());
	let page_stage_present =
		page_names.iter().any(|name| name.as_slice() == PAGE_STAGE_NAME.to_bytes());
	let mut physical_bytes = index_bytes;
	if index_stage_present {
		physical_bytes = checked_add(
			physical_bytes,
			validated_file_length(root, INDEX_STAGE_NAME, MAX_INDEX_BYTES)?,
		)?;
	}
	if page_stage_present {
		physical_bytes = checked_add(
			physical_bytes,
			validated_file_length(pages, PAGE_STAGE_NAME, MAX_PAGE_BYTES)?,
		)?;
	}

	let mut page_files = BTreeMap::new();
	for name in page_names {
		if name.as_slice() == PAGE_STAGE_NAME.to_bytes() {
			continue;
		}
		let digest = std::str::from_utf8(&name)
			.map_err(|_| CacheFailure::new(CacheDiagnostic::UnsafeShape))?
			.to_owned();
		let (_, metadata) = read_validated_page(pages, &digest)?;
		physical_bytes = checked_add(physical_bytes, metadata.byte_length)?;
		page_files.insert(digest, metadata);
	}
	if physical_bytes > MAX_PHYSICAL_BYTES {
		return Err(CacheFailure::new(CacheDiagnostic::Bounds));
	}
	validate_index(&index, &page_files)?;

	Ok(ValidatedCacheState {
		index,
		inventory: CacheInventory {
			page_files,
			physical_bytes,
			index_stage_present,
			page_stage_present,
		},
	})
}

fn validate_index(
	index: &CacheIndex,
	page_files: &BTreeMap<String, ValidatedPageFile>,
) -> Result<(), CacheFailure> {
	if index.schema_id != CACHE_SCHEMA_ID {
		return Err(CacheFailure::new(CacheDiagnostic::IncompatibleSchema));
	}
	if index.entries.len() > MAX_CACHE_PAGES {
		return Err(CacheFailure::new(CacheDiagnostic::Bounds));
	}

	let mut mappings = BTreeSet::new();
	let mut recencies = BTreeSet::new();
	let mut conversations = BTreeMap::<Vec<u8>, (usize, usize, usize)>::new();
	let mut total_items = 0_usize;
	let mut total_bytes = 0_usize;
	for entry in &index.entries {
		validate_page_identity(&entry.identity)?;
		if entry.fresh_received_at_unix_seconds < 0
			|| entry.recency == 0
			|| !recencies.insert(entry.recency)
			|| !mappings.insert(mapping_sort_key(&entry.identity))
		{
			return Err(CacheFailure::new(CacheDiagnostic::Integrity));
		}
		let item_count = usize::from(entry.item_count);
		let byte_length = usize::try_from(entry.byte_length)
			.map_err(|_| CacheFailure::new(CacheDiagnostic::Bounds))?;
		if item_count > MAX_PAGE_ITEMS || byte_length == 0 || byte_length > MAX_PAGE_BYTES {
			return Err(CacheFailure::new(CacheDiagnostic::Bounds));
		}
		let page_file = page_files
			.get(&entry.identity.page_sha256)
			.ok_or_else(|| CacheFailure::new(CacheDiagnostic::Integrity))?;
		if page_file.item_count != item_count || page_file.byte_length != byte_length {
			return Err(CacheFailure::new(CacheDiagnostic::Integrity));
		}

		let totals = conversations
			.entry(conversation_sort_key(
				&entry.identity.authority,
				&entry.identity.conversation_id,
			))
			.or_default();
		totals.0 = checked_add(totals.0, 1)?;
		totals.1 = checked_add(totals.1, item_count)?;
		totals.2 = checked_add(totals.2, byte_length)?;
		if totals.0 > MAX_CONVERSATION_PAGES
			|| totals.1 > MAX_CONVERSATION_ITEMS
			|| totals.2 > MAX_CONVERSATION_BYTES
		{
			return Err(CacheFailure::new(CacheDiagnostic::Bounds));
		}
		total_items = checked_add(total_items, item_count)?;
		total_bytes = checked_add(total_bytes, byte_length)?;
	}
	if conversations.len() > MAX_CACHE_CONVERSATIONS
		|| index.entries.len() > MAX_CACHE_PAGES
		|| total_items > MAX_CACHE_ITEMS
		|| total_bytes > MAX_CACHE_BYTES
	{
		return Err(CacheFailure::new(CacheDiagnostic::Bounds));
	}

	Ok(())
}

fn validate_authority(authority: &AuthorityIdentity) -> Result<(), CacheFailure> {
	if authority.cache_schema_generation != CACHE_SCHEMA_GENERATION
		|| authority.protocol_major == 0
		|| !bounded_identity(authority.stable_server_id.as_str(), MAX_IDENTITY_BYTES)
	{
		return Err(CacheFailure::new(CacheDiagnostic::InvalidInput));
	}

	Ok(())
}

fn validate_request(request: &CacheRequest) -> Result<(), CacheFailure> {
	validate_authority(&request.authority)?;
	if !bounded_identity(request.conversation_id.as_str(), MAX_IDENTITY_BYTES) {
		return Err(CacheFailure::new(CacheDiagnostic::InvalidInput));
	}
	validate_request_key(&request.request_key)
}

fn validate_page_identity(identity: &PageIdentity) -> Result<(), CacheFailure> {
	validate_authority(&identity.authority)
		.map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
	if !bounded_identity(identity.conversation_id.as_str(), MAX_IDENTITY_BYTES)
		|| validate_request_key(&identity.request_key).is_err()
		|| !is_digest_name(identity.page_sha256.as_bytes())
	{
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}

	Ok(())
}

fn validate_request_key(request_key: &CacheRequestKey) -> Result<(), CacheFailure> {
	match request_key {
		CacheRequestKey::Head => Ok(()),
		CacheRequestKey::After(after) if bounded_identity(after.as_str(), MAX_CURSOR_BYTES) =>
			Ok(()),
		CacheRequestKey::After(_) => Err(CacheFailure::new(CacheDiagnostic::InvalidInput)),
	}
}

fn bounded_identity(value: &str, maximum: usize) -> bool {
	!value.is_empty() && value.len() <= maximum
}

fn entry_matches_request(entry: &IndexEntry, request: &CacheRequest) -> bool {
	entry.identity.authority == request.authority
		&& entry.identity.conversation_id == request.conversation_id
		&& entry.identity.request_key == request.request_key
}

fn mapping_sort_key(identity: &PageIdentity) -> Vec<u8> {
	let mut key = conversation_sort_key(&identity.authority, &identity.conversation_id);
	match &identity.request_key {
		CacheRequestKey::Head => key.push(0),
		CacheRequestKey::After(after) => {
			key.push(1);
			append_sort_field(&mut key, after.as_str());
		},
	}
	key
}

fn page_identity_sort_key(identity: &PageIdentity) -> Vec<u8> {
	let mut key = mapping_sort_key(identity);
	append_sort_field(&mut key, &identity.page_sha256);
	key
}

fn conversation_sort_key(authority: &AuthorityIdentity, conversation_id: &EntityId) -> Vec<u8> {
	let mut key = Vec::new();
	append_sort_field(&mut key, authority.stable_server_id.as_str());
	key.extend_from_slice(&authority.protocol_major.to_be_bytes());
	key.extend_from_slice(&authority.protocol_minor.to_be_bytes());
	key.extend_from_slice(&authority.cache_schema_generation.to_be_bytes());
	append_sort_field(&mut key, conversation_id.as_str());
	key
}

fn append_sort_field(key: &mut Vec<u8>, value: &str) {
	key.extend_from_slice(&(value.len() as u64).to_be_bytes());
	key.extend_from_slice(value.as_bytes());
}

fn validated_file_length(
	parent: &File,
	name: &CStr,
	maximum: usize,
) -> Result<usize, CacheFailure> {
	let file = open_file_at(parent.as_raw_fd(), name, libc::O_RDONLY).map_err(|_| io_failure())?;
	validate_regular_file(&file, Some(maximum))?;
	let status = file_status(&file)?;

	usize::try_from(status.st_size).map_err(|_| CacheFailure::new(CacheDiagnostic::Bounds))
}

fn read_validated_page(
	pages: &File,
	digest: &str,
) -> Result<(ConversationHistoryPage, ValidatedPageFile), CacheFailure> {
	if !is_digest_name(digest.as_bytes()) {
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}
	let name = CString::new(digest).map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
	let file = open_file_at(pages.as_raw_fd(), &name, libc::O_RDONLY).map_err(|_| io_failure())?;
	validate_regular_file(&file, Some(MAX_PAGE_BYTES))?;
	let bytes = read_bounded(&file, MAX_PAGE_BYTES)?;
	if bytes.is_empty() || sha256_hex(&bytes) != digest {
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}
	let page: ConversationHistoryPage = serde_json::from_slice(&bytes)
		.map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
	let (canonical, canonical_digest) =
		page_bytes_and_digest(&page).map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
	if canonical != bytes || canonical_digest != digest {
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}
	let item_count = page.items.len();

	Ok((page, ValidatedPageFile { byte_length: bytes.len(), item_count }))
}

fn serialize_index(index: &CacheIndex) -> Result<Vec<u8>, CacheFailure> {
	let bytes =
		serde_json::to_vec(index).map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
	if bytes.is_empty() || bytes.len() > MAX_INDEX_BYTES {
		return Err(CacheFailure::new(CacheDiagnostic::Bounds));
	}

	Ok(bytes)
}

fn checked_add(left: usize, right: usize) -> Result<usize, CacheFailure> {
	left.checked_add(right).ok_or_else(|| CacheFailure::new(CacheDiagnostic::Bounds))
}

fn merge_hit_recencies(index: &mut CacheIndex, hit_recencies: &[(PageIdentity, u64)]) {
	for entry in &mut index.entries {
		for (identity, recency) in hit_recencies {
			if identity == &entry.identity {
				entry.recency = *recency;
				break;
			}
		}
	}
}

fn evict_to_bounds(index: &mut CacheIndex, now_unix_seconds: i64) -> Result<(), CacheFailure> {
	index
		.entries
		.retain(|entry| is_fresh_eligible(entry.fresh_received_at_unix_seconds, now_unix_seconds));

	let conversation_keys = index
		.entries
		.iter()
		.map(|entry| {
			conversation_sort_key(&entry.identity.authority, &entry.identity.conversation_id)
		})
		.collect::<BTreeSet<_>>();
	for conversation_key in conversation_keys {
		loop {
			let (pages, items, bytes) = conversation_totals(index, &conversation_key)?;
			if pages <= MAX_CONVERSATION_PAGES
				&& items <= MAX_CONVERSATION_ITEMS
				&& bytes <= MAX_CONVERSATION_BYTES
			{
				break;
			}
			remove_oldest_entry(index, |entry| {
				conversation_sort_key(&entry.identity.authority, &entry.identity.conversation_id)
					== conversation_key
			})?;
		}
	}

	while conversation_count(index) > MAX_CACHE_CONVERSATIONS {
		let mut newest_by_conversation = BTreeMap::<Vec<u8>, u64>::new();
		for entry in &index.entries {
			let key =
				conversation_sort_key(&entry.identity.authority, &entry.identity.conversation_id);
			newest_by_conversation
				.entry(key)
				.and_modify(|recency| *recency = (*recency).max(entry.recency))
				.or_insert(entry.recency);
		}
		let conversation = newest_by_conversation
			.into_iter()
			.min_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
			.map(|(key, _)| key)
			.ok_or_else(|| CacheFailure::new(CacheDiagnostic::Integrity))?;
		index.entries.retain(|entry| {
			conversation_sort_key(&entry.identity.authority, &entry.identity.conversation_id)
				!= conversation
		});
	}

	loop {
		let (items, bytes) = global_totals(index)?;
		let serialized_length = serde_json::to_vec(index)
			.map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?
			.len();
		if index.entries.len() <= MAX_CACHE_PAGES
			&& items <= MAX_CACHE_ITEMS
			&& bytes <= MAX_CACHE_BYTES
			&& serialized_length <= MAX_INDEX_BYTES
		{
			break;
		}
		remove_oldest_entry(index, |_| true)?;
	}
	index.entries.sort_by(|left, right| {
		page_identity_sort_key(&left.identity).cmp(&page_identity_sort_key(&right.identity))
	});

	Ok(())
}

fn conversation_totals(
	index: &CacheIndex,
	conversation_key: &[u8],
) -> Result<(usize, usize, usize), CacheFailure> {
	let mut pages = 0_usize;
	let mut items = 0_usize;
	let mut bytes = 0_usize;
	for entry in &index.entries {
		if conversation_sort_key(&entry.identity.authority, &entry.identity.conversation_id)
			!= conversation_key
		{
			continue;
		}
		pages = checked_add(pages, 1)?;
		items = checked_add(items, usize::from(entry.item_count))?;
		bytes = checked_add(
			bytes,
			usize::try_from(entry.byte_length)
				.map_err(|_| CacheFailure::new(CacheDiagnostic::Bounds))?,
		)?;
	}

	Ok((pages, items, bytes))
}

fn conversation_count(index: &CacheIndex) -> usize {
	index
		.entries
		.iter()
		.map(|entry| {
			conversation_sort_key(&entry.identity.authority, &entry.identity.conversation_id)
		})
		.collect::<BTreeSet<_>>()
		.len()
}

fn global_totals(index: &CacheIndex) -> Result<(usize, usize), CacheFailure> {
	let mut items = 0_usize;
	let mut bytes = 0_usize;
	for entry in &index.entries {
		items = checked_add(items, usize::from(entry.item_count))?;
		bytes = checked_add(
			bytes,
			usize::try_from(entry.byte_length)
				.map_err(|_| CacheFailure::new(CacheDiagnostic::Bounds))?,
		)?;
	}

	Ok((items, bytes))
}

fn remove_oldest_entry(
	index: &mut CacheIndex,
	matches: impl Fn(&IndexEntry) -> bool,
) -> Result<(), CacheFailure> {
	let oldest = index
		.entries
		.iter()
		.enumerate()
		.filter(|(_, entry)| matches(entry))
		.min_by(|(_, left), (_, right)| eviction_order(left, right))
		.map(|(position, _)| position)
		.ok_or_else(|| CacheFailure::new(CacheDiagnostic::Integrity))?;
	index.entries.remove(oldest);

	Ok(())
}

fn eviction_order(left: &IndexEntry, right: &IndexEntry) -> Ordering {
	left.recency.cmp(&right.recency).then_with(|| {
		page_identity_sort_key(&left.identity).cmp(&page_identity_sort_key(&right.identity))
	})
}

fn referenced_digests(index: &CacheIndex) -> BTreeSet<String> {
	index.entries.iter().map(|entry| entry.identity.page_sha256.clone()).collect()
}

fn clean_known_remnants(
	root: &File,
	pages: &File,
	lock: &File,
	state: ValidatedCacheState,
	faults: &impl FaultInjector,
) -> Result<ValidatedCacheState, CacheFailure> {
	let referenced = referenced_digests(&state.index);
	let mut root_changed = false;
	let mut pages_changed = false;
	if state.inventory.index_stage_present {
		unlink_at(root.as_raw_fd(), INDEX_STAGE_NAME)?;
		root_changed = true;
	}
	if state.inventory.page_stage_present {
		unlink_at(pages.as_raw_fd(), PAGE_STAGE_NAME)?;
		pages_changed = true;
	}
	for digest in state.inventory.page_files.keys() {
		if referenced.contains(digest) {
			continue;
		}
		let name = CString::new(digest.as_str())
			.map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
		unlink_at(pages.as_raw_fd(), &name)?;
		pages_changed = true;
	}
	if root_changed {
		faults.check(DurabilityEdge::CleanupSync)?;
		sync_file(root)?;
	}
	if pages_changed {
		faults.check(DurabilityEdge::CleanupSync)?;
		sync_file(pages)?;
	}
	let cleaned = load_validated_cache_state(root, pages, lock)?;
	if cleaned.index != state.index
		|| cleaned.inventory.index_stage_present
		|| cleaned.inventory.page_stage_present
		|| cleaned.inventory.page_files.keys().any(|digest| !referenced.contains(digest))
	{
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}

	Ok(cleaned)
}

fn clean_newly_unreferenced_pages(
	root: &File,
	pages: &File,
	lock: &File,
	state: ValidatedCacheState,
	faults: &impl FaultInjector,
) -> Result<ValidatedCacheState, CacheFailure> {
	let referenced = referenced_digests(&state.index);
	let mut removed = false;
	for digest in state.inventory.page_files.keys() {
		if referenced.contains(digest) {
			continue;
		}
		let name = CString::new(digest.as_str())
			.map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
		unlink_at(pages.as_raw_fd(), &name)?;
		removed = true;
	}
	if removed {
		faults.check(DurabilityEdge::CleanupSync)?;
		sync_file(pages)?;
	}
	let cleaned = load_validated_cache_state(root, pages, lock)?;
	if cleaned.index != state.index
		|| cleaned.inventory.page_files.keys().any(|digest| !referenced.contains(digest))
	{
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}

	Ok(cleaned)
}

fn publish_page(
	pages: &File,
	digest: &str,
	bytes: &[u8],
	faults: &impl FaultInjector,
) -> Result<bool, CacheFailure> {
	let stage = create_new_file_at(pages, PAGE_STAGE_NAME)?;
	write_all(&stage, bytes)?;
	validate_regular_file(&stage, Some(bytes.len()))?;
	if validated_length(&stage)? != bytes.len() {
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}
	faults.check(DurabilityEdge::PageStageSync)?;
	sync_file(&stage)?;

	let digest_name =
		CString::new(digest).map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
	faults.check(DurabilityEdge::PagePublish)?;
	let created =
		link_create_only(pages.as_raw_fd(), PAGE_STAGE_NAME, pages.as_raw_fd(), &digest_name)?;
	unlink_at(pages.as_raw_fd(), PAGE_STAGE_NAME)?;
	verify_page_target(pages, digest, bytes)?;
	faults.check(DurabilityEdge::PagesSync)?;
	sync_file(pages)?;

	Ok(created)
}

fn stage_index(root: &File, bytes: &[u8], faults: &impl FaultInjector) -> Result<(), CacheFailure> {
	let stage = create_new_file_at(root, INDEX_STAGE_NAME)?;
	write_all(&stage, bytes)?;
	validate_regular_file(&stage, Some(bytes.len()))?;
	if validated_length(&stage)? != bytes.len() {
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}
	faults.check(DurabilityEdge::IndexStageSync)?;
	sync_file(&stage)?;

	Ok(())
}

fn create_new_file_at(parent: &File, name: &CStr) -> Result<File, CacheFailure> {
	let file = create_file_at(parent.as_raw_fd(), name).map_err(|error| {
		if error.raw_os_error() == Some(libc::EEXIST) {
			CacheFailure::new(CacheDiagnostic::Integrity)
		} else {
			io_failure()
		}
	})?;
	validate_regular_file(&file, Some(0))?;

	Ok(file)
}

fn verify_page_target(pages: &File, digest: &str, expected: &[u8]) -> Result<(), CacheFailure> {
	let (page, metadata) = read_validated_page(pages, digest)?;
	let bytes =
		serde_json::to_vec(&page).map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
	if bytes != expected || metadata.byte_length != expected.len() {
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}

	Ok(())
}

fn validate_known_shape(root: &File, pages: &File) -> Result<(), CacheFailure> {
	validate_directory_entries(root, DirectoryShape::Root)?;
	validate_directory_entries(pages, DirectoryShape::Pages)?;

	Ok(())
}

fn validate_directory_entries(directory: &File, shape: DirectoryShape) -> Result<(), CacheFailure> {
	validated_directory_names(directory, shape).map(|_| ())
}

fn validated_directory_names(
	directory: &File,
	shape: DirectoryShape,
) -> Result<Vec<Vec<u8>>, CacheFailure> {
	let scan_directory =
		open_directory_at(directory.as_raw_fd(), c".").map_err(|_| io_failure())?;
	let scan_descriptor = scan_directory.into_raw_fd();
	let stream = unsafe { libc::fdopendir(scan_descriptor) };
	if stream.is_null() {
		let _ = unsafe { libc::close(scan_descriptor) };
		return Err(io_failure());
	}

	let mut result = Ok(());
	let mut names = Vec::new();
	loop {
		errno_clear();
		let entry = unsafe { libc::readdir(stream) };
		if entry.is_null() {
			let error = errno();
			if error == libc::EINTR {
				continue;
			}
			if error != 0 {
				result = Err(io_failure());
			}
			break;
		}
		let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
		if name == b"." || name == b".." {
			continue;
		}
		if !directory_name_is_known(shape, name) {
			result = Err(CacheFailure::new(CacheDiagnostic::UnsafeShape));
			break;
		}
		let maximum = match shape {
			DirectoryShape::Root => 4,
			DirectoryShape::Pages => MAX_PHYSICAL_PAGE_NAMES,
		};
		if names.len() == maximum {
			result = Err(CacheFailure::new(CacheDiagnostic::Bounds));
			break;
		}
		names.push(name.to_vec());
	}
	let _ = unsafe { libc::closedir(stream) };
	result?;
	names.sort();

	Ok(names)
}

fn directory_name_is_known(shape: DirectoryShape, name: &[u8]) -> bool {
	match shape {
		DirectoryShape::Root => [
			LOCK_NAME.to_bytes(),
			INDEX_NAME.to_bytes(),
			INDEX_STAGE_NAME.to_bytes(),
			PAGES_DIRECTORY_NAME.to_bytes(),
		]
		.contains(&name),
		DirectoryShape::Pages => name == PAGE_STAGE_NAME.to_bytes() || is_digest_name(name),
	}
}

fn is_digest_name(name: &[u8]) -> bool {
	name.len() == SHA256_HEX_LENGTH
		&& name.iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn open_or_create_absolute_parent(path: &Path) -> Result<File, CacheFailure> {
	if !path.is_absolute() {
		return Err(CacheFailure::new(CacheDiagnostic::InvalidInput));
	}
	let path_bytes = path.as_os_str().as_bytes();
	if path_bytes.len() <= 1
		|| path_bytes.last() == Some(&b'/')
		|| path_bytes[1..]
			.split(|byte| *byte == b'/')
			.any(|component| component.is_empty() || component == b"." || component == b"..")
	{
		return Err(CacheFailure::new(CacheDiagnostic::InvalidInput));
	}

	let mut lexical_components = path.components();
	if !matches!(lexical_components.next(), Some(Component::RootDir))
		|| lexical_components.any(|component| !matches!(component, Component::Normal(_)))
	{
		return Err(CacheFailure::new(CacheDiagnostic::InvalidInput));
	}
	let external_base =
		path.parent().ok_or_else(|| CacheFailure::new(CacheDiagnostic::InvalidInput))?;
	let cache_parent_leaf =
		path.file_name().ok_or_else(|| CacheFailure::new(CacheDiagnostic::InvalidInput))?;
	if !matches!(
		path.components().next_back(),
		Some(Component::Normal(name)) if name == cache_parent_leaf
	) {
		return Err(CacheFailure::new(CacheDiagnostic::InvalidInput));
	}
	let cache_parent_leaf = CString::new(cache_parent_leaf.as_bytes())
		.map_err(|_| CacheFailure::new(CacheDiagnostic::InvalidInput))?;

	let resolved_external_base = std::fs::canonicalize(external_base).map_err(|_| io_failure())?;
	let mut resolved_components = resolved_external_base.components();
	if !matches!(resolved_components.next(), Some(Component::RootDir)) {
		return Err(CacheFailure::new(CacheDiagnostic::UnsafeShape));
	}

	let mut components = resolved_components.peekable();
	let mut directory = if components.peek().is_some() {
		open_search_directory_at(libc::AT_FDCWD, c"/").map_err(|_| io_failure())?
	} else {
		open_directory_at(libc::AT_FDCWD, c"/").map_err(|_| io_failure())?
	};
	validate_ancestor_directory(&directory)?;
	while let Some(component) = components.next() {
		let Component::Normal(name) = component else {
			return Err(CacheFailure::new(CacheDiagnostic::UnsafeShape));
		};
		let name = CString::new(name.as_bytes())
			.map_err(|_| CacheFailure::new(CacheDiagnostic::UnsafeShape))?;
		directory = if components.peek().is_some() {
			open_search_directory_at(directory.as_raw_fd(), &name).map_err(|_| io_failure())?
		} else {
			open_directory_at(directory.as_raw_fd(), &name).map_err(|_| io_failure())?
		};
		validate_ancestor_directory(&directory)?;
	}

	open_or_create_directory_at(&directory, &cache_parent_leaf)
}

fn open_or_create_directory_at(parent: &File, name: &CStr) -> Result<File, CacheFailure> {
	let (directory, created) = match open_directory_at(parent.as_raw_fd(), name) {
		Ok(directory) => (directory, false),
		Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
			create_directory_at(parent.as_raw_fd(), name)?;
			(open_directory_at(parent.as_raw_fd(), name).map_err(|_| io_failure())?, true)
		},
		Err(_) => return Err(io_failure()),
	};
	validate_directory(&directory)?;
	if created {
		sync_file(parent)?;
	}

	Ok(directory)
}

fn open_or_create_file_at(parent: &File, name: &CStr) -> Result<File, CacheFailure> {
	let file = match create_file_at(parent.as_raw_fd(), name) {
		Ok(file) => file,
		Err(error) if error.raw_os_error() == Some(libc::EEXIST) =>
			open_file_at(parent.as_raw_fd(), name, libc::O_RDWR).map_err(|_| io_failure())?,
		Err(_) => return Err(io_failure()),
	};
	validate_regular_file(&file, Some(0))?;
	sync_file(parent)?;

	Ok(file)
}

fn open_optional_file_at(parent: &File, name: &CStr) -> Result<Option<File>, CacheFailure> {
	match open_file_at(parent.as_raw_fd(), name, libc::O_RDONLY) {
		Ok(file) => Ok(Some(file)),
		Err(error) if error.raw_os_error() == Some(libc::ENOENT) => Ok(None),
		Err(_) => Err(io_failure()),
	}
}

fn open_directory_at(parent: RawFd, name: &CStr) -> io::Result<File> {
	open_directory_with_access_at(parent, name, libc::O_RDONLY)
}

fn open_search_directory_at(parent: RawFd, name: &CStr) -> io::Result<File> {
	open_directory_with_access_at(parent, name, ANCESTOR_DIRECTORY_ACCESS)
}

fn open_directory_with_access_at(
	parent: RawFd,
	name: &CStr,
	access: libc::c_int,
) -> io::Result<File> {
	loop {
		let descriptor = unsafe {
			libc::openat(
				parent,
				name.as_ptr(),
				access | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
			)
		};
		if descriptor != -1 {
			return Ok(unsafe { File::from_raw_fd(descriptor) });
		}
		let error = io::Error::last_os_error();
		if error.raw_os_error() != Some(libc::EINTR) {
			return Err(error);
		}
	}
}

fn create_directory_at(parent: RawFd, name: &CStr) -> Result<(), CacheFailure> {
	loop {
		if unsafe { libc::mkdirat(parent, name.as_ptr(), PRIVATE_DIRECTORY_MODE) } == 0 {
			return Ok(());
		}
		let error = io::Error::last_os_error();
		match error.raw_os_error() {
			Some(libc::EINTR) => {},
			Some(libc::EEXIST) => return Ok(()),
			_ => return Err(io_failure()),
		}
	}
}

fn open_file_at(parent: RawFd, name: &CStr, access: libc::c_int) -> io::Result<File> {
	loop {
		let descriptor = unsafe {
			libc::openat(
				parent,
				name.as_ptr(),
				access | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
			)
		};
		if descriptor != -1 {
			return Ok(unsafe { File::from_raw_fd(descriptor) });
		}
		let error = io::Error::last_os_error();
		if error.raw_os_error() != Some(libc::EINTR) {
			return Err(error);
		}
	}
}

fn create_file_at(parent: RawFd, name: &CStr) -> io::Result<File> {
	let mut interrupted = false;
	loop {
		let descriptor = unsafe {
			libc::openat(
				parent,
				name.as_ptr(),
				libc::O_RDWR
					| libc::O_CREAT | libc::O_EXCL
					| libc::O_NOFOLLOW
					| libc::O_CLOEXEC
					| libc::O_NONBLOCK,
				PRIVATE_FILE_MODE as libc::c_uint,
			)
		};
		if descriptor != -1 {
			return Ok(unsafe { File::from_raw_fd(descriptor) });
		}
		let error = io::Error::last_os_error();
		match error.raw_os_error() {
			Some(libc::EINTR) => interrupted = true,
			Some(libc::EEXIST) if interrupted => return open_file_at(parent, name, libc::O_RDWR),
			_ => return Err(error),
		}
	}
}

fn validated_length(file: &File) -> Result<usize, CacheFailure> {
	let status = file_status(file)?;
	usize::try_from(status.st_size).map_err(|_| CacheFailure::new(CacheDiagnostic::Bounds))
}

fn write_all(file: &File, bytes: &[u8]) -> Result<(), CacheFailure> {
	let mut offset = 0_usize;
	while offset < bytes.len() {
		let written = unsafe {
			libc::pwrite(
				file.as_raw_fd(),
				bytes[offset..].as_ptr().cast(),
				bytes.len() - offset,
				offset as libc::off_t,
			)
		};
		if written == -1 && errno() == libc::EINTR {
			continue;
		}
		if written <= 0 {
			return Err(io_failure());
		}
		offset = checked_add(offset, written as usize)?;
	}

	Ok(())
}

fn sync_file(file: &File) -> Result<(), CacheFailure> {
	loop {
		if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
			return Ok(());
		}
		if errno() != libc::EINTR {
			return Err(io_failure());
		}
	}
}

fn unlink_at(parent: RawFd, name: &CStr) -> Result<(), CacheFailure> {
	let mut interrupted = false;
	loop {
		if unsafe { libc::unlinkat(parent, name.as_ptr(), 0) } == 0 {
			return Ok(());
		}
		match errno() {
			libc::EINTR => interrupted = true,
			libc::ENOENT if interrupted => return Ok(()),
			_ => return Err(io_failure()),
		}
	}
}

fn link_create_only(
	source_parent: RawFd,
	source_name: &CStr,
	target_parent: RawFd,
	target_name: &CStr,
) -> Result<bool, CacheFailure> {
	loop {
		if unsafe {
			libc::linkat(
				source_parent,
				source_name.as_ptr(),
				target_parent,
				target_name.as_ptr(),
				0,
			)
		} == 0
		{
			return Ok(true);
		}
		match errno() {
			libc::EINTR => {},
			libc::EEXIST => return Ok(false),
			_ => return Err(io_failure()),
		}
	}
}

fn rename_at(
	source_parent: RawFd,
	source_name: &CStr,
	target_parent: RawFd,
	target_name: &CStr,
) -> Result<(), CacheFailure> {
	let mut interrupted = false;
	loop {
		if unsafe {
			libc::renameat(source_parent, source_name.as_ptr(), target_parent, target_name.as_ptr())
		} == 0
		{
			return Ok(());
		}
		match errno() {
			libc::EINTR => interrupted = true,
			libc::ENOENT if interrupted => return Ok(()),
			_ => return Err(io_failure()),
		}
	}
}

fn validate_ancestor_directory(directory: &File) -> Result<(), CacheFailure> {
	let status = file_status(directory)?;
	let mode = status.st_mode & 0o7777;
	let owner_is_allowed = status.st_uid == 0 || status.st_uid == effective_uid();
	let root_owned_sticky = status.st_uid == 0 && mode & libc::S_ISVTX != 0;
	if status.st_mode & libc::S_IFMT != libc::S_IFDIR
		|| !owner_is_allowed
		|| (mode & 0o022 != 0 && !root_owned_sticky)
	{
		return Err(CacheFailure::new(CacheDiagnostic::UnsafeShape));
	}

	Ok(())
}

fn validate_directory(directory: &File) -> Result<(), CacheFailure> {
	let status = file_status(directory)?;
	if status.st_uid != effective_uid()
		|| status.st_mode & libc::S_IFMT != libc::S_IFDIR
		|| status.st_mode & 0o7777 != PRIVATE_DIRECTORY_MODE
	{
		return Err(CacheFailure::new(CacheDiagnostic::UnsafeShape));
	}

	Ok(())
}

fn validate_regular_file(file: &File, max_length: Option<usize>) -> Result<(), CacheFailure> {
	let status = file_status(file)?;
	if status.st_uid != effective_uid()
		|| status.st_mode & libc::S_IFMT != libc::S_IFREG
		|| status.st_mode & 0o7777 != PRIVATE_FILE_MODE
		|| status.st_nlink != 1
		|| status.st_size < 0
		|| max_length.is_some_and(|maximum| status.st_size as u64 > maximum as u64)
	{
		return Err(CacheFailure::new(CacheDiagnostic::UnsafeShape));
	}

	Ok(())
}

fn file_status(file: &File) -> Result<libc::stat, CacheFailure> {
	loop {
		let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
		if unsafe { libc::fstat(file.as_raw_fd(), status.as_mut_ptr()) } == 0 {
			return Ok(unsafe { status.assume_init() });
		}
		if errno() != libc::EINTR {
			return Err(io_failure());
		}
	}
}

fn read_bounded(file: &File, maximum: usize) -> Result<Vec<u8>, CacheFailure> {
	let status = file_status(file)?;
	let length =
		usize::try_from(status.st_size).map_err(|_| CacheFailure::new(CacheDiagnostic::Bounds))?;
	if length > maximum {
		return Err(CacheFailure::new(CacheDiagnostic::Bounds));
	}
	let mut bytes = vec![0; length];
	let mut offset = 0;
	while offset < bytes.len() {
		let read = unsafe {
			libc::pread(
				file.as_raw_fd(),
				bytes[offset..].as_mut_ptr().cast(),
				bytes.len() - offset,
				offset as libc::off_t,
			)
		};
		if read == -1 && errno() == libc::EINTR {
			continue;
		}
		if read <= 0 {
			return Err(io_failure());
		}
		offset = checked_add(offset, read as usize)?;
	}
	if validated_length(file)? != length {
		return Err(CacheFailure::new(CacheDiagnostic::Integrity));
	}

	Ok(bytes)
}

fn page_bytes_and_digest(
	page: &ConversationHistoryPage,
) -> Result<(Vec<u8>, String), CacheFailure> {
	let bytes =
		serde_json::to_vec(page).map_err(|_| CacheFailure::new(CacheDiagnostic::Integrity))?;
	if page.items.len() > MAX_PAGE_ITEMS || bytes.len() > MAX_PAGE_BYTES {
		return Err(CacheFailure::new(CacheDiagnostic::Bounds));
	}
	let digest = sha256_hex(&bytes);

	Ok((bytes, digest))
}

fn sha256_hex(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";

	let digest = Sha256::digest(bytes);
	let mut output = String::with_capacity(SHA256_HEX_LENGTH);
	for byte in digest {
		output.push(char::from(HEX[usize::from(byte >> 4)]));
		output.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}

	output
}

fn is_fresh_eligible(fresh_received_at: i64, now: i64) -> bool {
	now >= fresh_received_at
		&& now.checked_sub(fresh_received_at).is_some_and(|age| age < FRESH_ELIGIBILITY_SECONDS)
}

fn lock_exclusive(lock: &File) -> Result<(), CacheFailure> {
	loop {
		if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
			return Ok(());
		}
		let error = errno();
		if error == libc::EINTR {
			continue;
		}
		if error == libc::EWOULDBLOCK || error == libc::EAGAIN {
			return Err(io_failure());
		}
		return Err(io_failure());
	}
}

fn effective_uid() -> libc::uid_t {
	unsafe { libc::geteuid() }
}

fn errno() -> i32 {
	io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn errno_clear() {
	#[cfg(target_os = "macos")]
	unsafe {
		*libc::__error() = 0;
	}
	#[cfg(target_os = "linux")]
	unsafe {
		*libc::__errno_location() = 0;
	}
}

fn io_failure() -> CacheFailure {
	CacheFailure::new(CacheDiagnostic::Filesystem)
}

#[allow(dead_code)]
fn closed_limits() -> (usize, usize, usize, usize, usize, usize, usize, usize, usize) {
	(
		MAX_CONVERSATION_PAGES,
		MAX_CONVERSATION_ITEMS,
		MAX_CONVERSATION_BYTES,
		MAX_CACHE_CONVERSATIONS,
		MAX_CACHE_PAGES,
		MAX_CACHE_ITEMS,
		MAX_CACHE_BYTES,
		MAX_INDEX_BYTES,
		MAX_PHYSICAL_BYTES,
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{
		fs,
		os::unix::fs::{PermissionsExt as _, symlink},
		path::PathBuf,
	};

	use decodex_protocol::CURRENT_VERSION;
	use tempfile::TempDir;

	fn host_temp_fixture() -> TempDir {
		TempDir::new_in(std::env::temp_dir())
			.expect("host temporary directory accepts an isolated fixture")
	}

	fn authority() -> CacheAuthority {
		CacheAuthority::new(
			ServerId::new("history-cache-server").expect("server identity is bounded"),
			CURRENT_VERSION.major,
			CURRENT_VERSION.minor,
			CACHE_SCHEMA_GENERATION,
		)
		.expect("cache authority is valid")
	}

	fn request() -> CacheRequest {
		CacheRequest::head(
			&authority(),
			EntityId::new("history-cache-conversation").expect("conversation identity is bounded"),
		)
		.expect("cache request is valid")
	}

	fn page(next_cursor: &str) -> ConversationHistoryPage {
		ConversationHistoryPage {
			items: Vec::new(),
			next_cursor: Some(
				HistoryCursorToken::new(next_cursor).expect("history cursor is bounded"),
			),
		}
	}

	fn publish(
		cache: &mut HistoryPageCache,
		request: &CacheRequest,
		page: &ConversationHistoryPage,
		fresh_received_at: i64,
	) {
		let prepared = cache
			.prepare_publication(request, page, fresh_received_at)
			.expect("publication prepares");
		let committed = match cache.commit_publication(prepared) {
			Ok(committed) => committed,
			Err(_) => panic!("publication commits"),
		};

		cache.finish_publication(committed).expect("publication finishes");
	}

	#[test]
	fn eviction_limits_and_identity_ties_are_deterministic() {
		const NOW: i64 = 10_000;

		let authority = authority().identity;
		let entry = |conversation_id: &str,
		             request_key: CacheRequestKey,
		             digest: u8,
		             recency: u64| IndexEntry {
			identity: PageIdentity {
				authority: authority.clone(),
				conversation_id: EntityId::new(conversation_id)
					.expect("conversation identity is bounded"),
				request_key,
				page_sha256: format!("{digest:064x}"),
			},
			fresh_received_at_unix_seconds: NOW,
			recency,
			item_count: 0,
			byte_length: 1,
		};

		let mut pages = CacheIndex::empty();
		for number in 1_u8..=5 {
			let request_key = if number == 1 {
				CacheRequestKey::Head
			} else {
				CacheRequestKey::After(
					HistoryCursorToken::new(format!("page-{number}"))
						.expect("history cursor is bounded"),
				)
			};
			pages.entries.push(entry("bounded-pages", request_key, number, u64::from(number)));
		}

		evict_to_bounds(&mut pages, NOW).expect("page bounds evict deterministically");
		let mut retained_recencies =
			pages.entries.iter().map(|entry| entry.recency).collect::<Vec<_>>();
		retained_recencies.sort_unstable();

		assert_eq!(pages.entries.len(), 4);
		assert_eq!(retained_recencies, [2, 3, 4, 5]);

		let mut conversations = CacheIndex::empty();
		conversations.entries.extend([
			entry("oldest-conversation", CacheRequestKey::Head, 10, 1),
			entry(
				"oldest-conversation",
				CacheRequestKey::After(
					HistoryCursorToken::new("oldest-next").expect("history cursor is bounded"),
				),
				11,
				2,
			),
		]);
		for number in 1_u8..=8 {
			conversations.entries.push(entry(
				&format!("retained-conversation-{number}"),
				CacheRequestKey::Head,
				number + 20,
				u64::from(number) + 2,
			));
		}

		assert_eq!(conversation_count(&conversations), 9);
		evict_to_bounds(&mut conversations, NOW)
			.expect("conversation bounds evict deterministically");

		assert_eq!(conversation_count(&conversations), 8);
		assert_eq!(conversations.entries.len(), 8);
		assert!(
			conversations
				.entries
				.iter()
				.all(|entry| { entry.identity.conversation_id.as_str() != "oldest-conversation" })
		);

		let lower_identity = entry("identity-tie", CacheRequestKey::Head, 40, 20);
		let higher_identity = entry("identity-tie", CacheRequestKey::Head, 41, 20);

		assert_eq!(
			lower_identity
				.identity
				.page_sha256
				.as_bytes()
				.cmp(higher_identity.identity.page_sha256.as_bytes()),
			Ordering::Less,
		);
		assert_eq!(eviction_order(&lower_identity, &higher_identity), Ordering::Less,);
		assert_eq!(eviction_order(&higher_identity, &lower_identity), Ordering::Greater,);
	}

	#[derive(Clone, Copy, Debug)]
	enum ParentExpectation {
		Opens,
		Refuses,
	}

	#[test]
	fn absolute_parent_boundary_accepts_host_alias_and_refuses_missing_base() {
		let temporary = host_temp_fixture();
		let accepted_parent = temporary.path().join("accepted-parent");
		let absent_parent = temporary.path().join("absent-base").join("cache-parent");

		let cases = [
			("host temporary path", accepted_parent.clone(), ParentExpectation::Opens),
			("absent external base", absent_parent, ParentExpectation::Refuses),
		];
		for (name, parent, expectation) in cases {
			let result = HistoryPageCache::open(&parent, CACHE_SCHEMA_GENERATION);

			match (expectation, result) {
				(ParentExpectation::Opens, Ok(cache)) => {
					drop(cache);
					assert!(parent.is_dir(), "{name} creates the unchanged final leaf");
				},
				(ParentExpectation::Refuses, Err(_)) => {},
				(ParentExpectation::Opens, Err(failure)) => {
					panic!("{name} must open: {}", failure.diagnostic())
				},
				(ParentExpectation::Refuses, Ok(_)) => panic!("{name} must be refused"),
			}
		}

		#[cfg(target_os = "macos")]
		if temporary.path().starts_with("/var") {
			assert!(
				fs::canonicalize(temporary.path())
					.expect("temporary fixture canonicalizes")
					.starts_with("/private/var"),
				"the host /var alias resolves through the external-base boundary",
			);
		}
	}

	#[derive(Clone, Copy, Debug)]
	enum NoFollowBoundary {
		FinalParentLeaf,
		CacheRoot,
		PagesDirectory,
		LockFile,
	}

	#[test]
	fn shared_no_follow_boundaries_refuse_representative_symlinks() {
		let cases = [
			("final parent leaf", NoFollowBoundary::FinalParentLeaf),
			("cache root", NoFollowBoundary::CacheRoot),
			("pages directory", NoFollowBoundary::PagesDirectory),
			("lock file", NoFollowBoundary::LockFile),
		];

		for (name, boundary) in cases {
			let temporary = host_temp_fixture();
			let parent = temporary.path().join("cache-parent");
			let root = parent.join("history-page-cache-v1");
			let target = temporary.path().join("link-target");
			let linked_path = match boundary {
				NoFollowBoundary::FinalParentLeaf => {
					fs::create_dir(&target).expect("directory link target is created");
					fs::set_permissions(&target, fs::Permissions::from_mode(0o700))
						.expect("directory link target is owner-private");
					symlink(&target, &parent).expect("final parent leaf link is created");

					parent.clone()
				},
				NoFollowBoundary::CacheRoot => {
					fs::create_dir(&parent).expect("cache parent is created");
					fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
						.expect("cache parent is owner-private");
					fs::create_dir(&target).expect("directory link target is created");
					fs::set_permissions(&target, fs::Permissions::from_mode(0o700))
						.expect("directory link target is owner-private");
					symlink(&target, &root).expect("cache root link is created");

					root.clone()
				},
				NoFollowBoundary::PagesDirectory => {
					drop(
						HistoryPageCache::open(&parent, CACHE_SCHEMA_GENERATION)
							.expect("baseline cache opens"),
					);
					fs::create_dir(&target).expect("directory link target is created");
					fs::set_permissions(&target, fs::Permissions::from_mode(0o700))
						.expect("directory link target is owner-private");
					let pages = root.join("pages");

					fs::remove_dir(&pages).expect("baseline pages directory is empty");
					symlink(&target, &pages).expect("pages directory link is created");

					pages
				},
				NoFollowBoundary::LockFile => {
					drop(
						HistoryPageCache::open(&parent, CACHE_SCHEMA_GENERATION)
							.expect("baseline cache opens"),
					);
					fs::write(&target, b"").expect("file link target is created");
					fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
						.expect("file link target is owner-private");
					let lock = root.join("lock");

					fs::remove_file(&lock).expect("baseline lock file is removed");
					symlink(&target, &lock).expect("lock file link is created");

					lock
				},
			};

			assert!(
				HistoryPageCache::open(&parent, CACHE_SCHEMA_GENERATION).is_err(),
				"{name} symlink must be refused",
			);
			assert!(
				fs::symlink_metadata(&linked_path)
					.expect("linked boundary remains present")
					.file_type()
					.is_symlink(),
				"{name} remains a symbolic link",
			);
			match boundary {
				NoFollowBoundary::LockFile => assert!(
					fs::read(&target).expect("file link target remains readable").is_empty(),
					"{name} target must not be followed",
				),
				NoFollowBoundary::FinalParentLeaf
				| NoFollowBoundary::CacheRoot
				| NoFollowBoundary::PagesDirectory => assert!(
					fs::read_dir(&target)
						.expect("directory link target remains readable")
						.next()
						.is_none(),
					"{name} target must not be followed",
				),
			}
		}
	}

	#[test]
	fn round_trip_restart_ttl_digest_and_integrity_contract() {
		let temporary = host_temp_fixture();
		let parent = temporary.path().join("cache-parent");
		let request = request();
		let page = page("cached-next");
		let fresh_received_at = 10_000;
		let expected_bytes: &[u8] = br#"{"items":[],"next_cursor":"cached-next"}"#;
		let expected_digest = "72908abdf71e80cf521b7dd94147f08d64e73d9d5a81544fea93391fed46fbcf";
		let serialized_bytes = serde_json::to_vec(&page).expect("page serializes");
		let page_path = parent.join("history-page-cache-v1").join("pages").join(expected_digest);
		let mut cache =
			HistoryPageCache::open(&parent, CACHE_SCHEMA_GENERATION).expect("cache opens");

		assert_eq!(serialized_bytes.as_slice(), expected_bytes);
		assert_eq!(sha256_hex(expected_bytes), expected_digest);
		publish(&mut cache, &request, &page, fresh_received_at);

		assert_eq!(cache.index.entries.len(), 1);
		assert_eq!(cache.index.entries[0].identity.page_sha256.as_str(), expected_digest);
		let persisted_bytes = fs::read(&page_path).expect("published page is readable");

		assert_eq!(persisted_bytes.as_slice(), expected_bytes);
		drop(cache);

		let mut reopened =
			HistoryPageCache::open(&parent, CACHE_SCHEMA_GENERATION).expect("cache reopens");
		let hit = match reopened.lookup(&request, fresh_received_at) {
			CacheLookup::Hit(hit) => hit,
			other => panic!("persisted page must be eligible after restart: {other:?}"),
		};

		assert_eq!(hit.page(), &page);
		assert_eq!(hit.fresh_received_at_unix_seconds(), fresh_received_at);
		reopened.record_hit_recency(&hit).expect("eligible hit records bounded recency");
		assert!(matches!(
			reopened.lookup(&request, fresh_received_at + FRESH_ELIGIBILITY_SECONDS - 1),
			CacheLookup::Hit(_)
		));
		for now in [fresh_received_at - 1, fresh_received_at + FRESH_ELIGIBILITY_SECONDS] {
			assert_eq!(
				reopened.lookup(&request, now),
				CacheLookup::Miss(CacheDiagnostic::Ineligible),
				"a hit cannot extend the immutable eligibility interval",
			);
		}
		drop(reopened);

		fs::write(&page_path, b"tampered").expect("fixture corrupts the page bytes");
		let failure = HistoryPageCache::open(&parent, CACHE_SCHEMA_GENERATION)
			.expect_err("digest mismatch is refused");

		assert_eq!(failure.diagnostic, CacheDiagnostic::Integrity);
	}

	struct FailAt(DurabilityEdge);

	impl FaultInjector for FailAt {
		fn check(&self, edge: DurabilityEdge) -> Result<(), CacheFailure> {
			if edge == self.0 {
				Err(CacheFailure::new(CacheDiagnostic::DurabilityFault))
			} else {
				Ok(())
			}
		}
	}

	#[derive(Clone, Copy)]
	enum DurabilityCase {
		PreIndex,
		PostIndexCleanup,
	}

	#[test]
	fn representative_durability_faults_preserve_index_authority() {
		let cases = [
			("pre-index", DurabilityCase::PreIndex),
			("post-index-cleanup", DurabilityCase::PostIndexCleanup),
		];
		for (name, case) in cases {
			let temporary = host_temp_fixture();
			let parent: PathBuf = temporary.path().join(name);
			let request = request();
			let baseline = page("baseline-next");
			let candidate = page("candidate-next");
			let fresh_received_at = 20_000;
			let (candidate_bytes, candidate_digest) =
				page_bytes_and_digest(&candidate).expect("candidate page is bounded");
			let mut cache =
				HistoryPageCache::open(&parent, CACHE_SCHEMA_GENERATION).expect("cache opens");

			match case {
				DurabilityCase::PreIndex => {
					let failure = cache
						.prepare_publication_with_faults(
							&request,
							&candidate,
							fresh_received_at,
							&candidate_bytes,
							&candidate_digest,
							&FailAt(DurabilityEdge::IndexStageSync),
						)
						.err()
						.expect("pre-index fault is injected");

					assert_eq!(failure.diagnostic, CacheDiagnostic::DurabilityFault);
				},
				DurabilityCase::PostIndexCleanup => {
					publish(&mut cache, &request, &baseline, fresh_received_at - 1);
					let prepared = cache
						.prepare_publication_with_faults(
							&request,
							&candidate,
							fresh_received_at,
							&candidate_bytes,
							&candidate_digest,
							&NoFaults,
						)
						.expect("replacement prepares");
					let committed = match cache.commit_publication_with_faults(prepared, &NoFaults)
					{
						Ok(committed) => committed,
						Err(_) => panic!("replacement index commits"),
					};
					let failure = cache
						.finish_publication_with_faults(
							committed,
							&FailAt(DurabilityEdge::CleanupSync),
						)
						.expect_err("post-index cleanup fault is injected");

					assert_eq!(failure.diagnostic, CacheDiagnostic::DurabilityFault);
				},
			}
			drop(cache);

			let reopened =
				HistoryPageCache::open(&parent, CACHE_SCHEMA_GENERATION).expect("cache reopens");
			match case {
				DurabilityCase::PreIndex => assert_eq!(
					reopened.lookup(&request, fresh_received_at),
					CacheLookup::Miss(CacheDiagnostic::NotFound),
					"an uncommitted mapping never becomes authoritative",
				),
				DurabilityCase::PostIndexCleanup => {
					match reopened.lookup(&request, fresh_received_at) {
						CacheLookup::Hit(hit) => assert_eq!(hit.page(), &candidate),
						other => panic!("the committed mapping must remain recoverable: {other:?}"),
					}
				},
			}
		}
	}
}
