//! Domain, application, configuration, and owned local-storage foundations for Decodex vNext.

mod account;
mod blob;
mod cache;
mod config;
mod conversation;
mod identity;
#[cfg(unix)] mod path_unix;
mod paths;
mod storage;

pub use self::{
	account::{AccountError, AccountId, AccountState},
	blob::{
		BlobHash, BlobInventoryCursor, BlobInventoryEntry, BlobInventoryPage, BlobStore,
		MAX_BLOB_BYTES,
	},
	cache::{
		BoundedCache, CacheLimits, CacheUsage, MAX_CACHE_BYTES, MAX_CACHE_ENTRIES,
		MAX_CACHE_ENTRY_BYTES,
	},
	config::{
		CacheConfig, ConfigError, DecodexClientConfig, DecodexConfig, LocalProfile,
		MAX_CONFIG_BYTES, PostgresConnectionConfig, PostgresIdentityConfig, ProfileName,
		RemoteProfile, RepositoryName, ServerHostConfig, ServerProfile, ServerRepositoryPath,
	},
	conversation::{
		AccountSnapshot, ArtifactId, ArtifactReference, ArtifactStatus, ContextPack,
		ContextPackInput, ContextPackPolicy, ContextPackSource, ContextSourceDisposition,
		ContextSourceKind, ContextSourceManifest, Conversation, ConversationError, ConversationId,
		ConversationStatus, HistoryItem, HistoryItemId, HistoryItemKind, HistoryMediaType,
		HistoryMetadata, HistoryMetadataValue, ItemStatus, MAX_CONTEXT_PACK_BYTES,
		MAX_CONTEXT_RECENT_ITEMS, MAX_CONTEXT_SOURCE_INPUT_BYTES, MAX_CONTEXT_SOURCES,
		MAX_CONVERSATION_TITLE_BYTES, MAX_HISTORY_METADATA_FIELDS, MAX_HISTORY_METADATA_KEY_BYTES,
		MAX_HISTORY_METADATA_VALUE_BYTES, MAX_INLINE_HISTORY_BYTES, MIN_CONTEXT_PACK_BYTES,
		NormalizedPayload, PinnedContextSource, PossibleSideEffects, ProfileSnapshot,
		ProposedTransition, ProposedTransitionKind, RuntimeSession, RuntimeSessionId,
		RuntimeSessionState, Turn, TurnId, TurnRole, TurnStatus, compile_context_pack,
		contains_credential_material, is_canonical_media_type, is_credential_metadata_key,
	},
	identity::ServerIdentity,
	paths::{DecodexPaths, DecodexRoot, PathError},
	storage::StorageError,
};

#[cfg(test)] use tempfile as _;

/// Application-facing product-state port.
pub trait ProductState {
	/// Report whether the adapter can currently serve product-state requests.
	fn availability(&self) -> Availability;
}

/// Application-facing conversation execution port.
pub trait ConversationRuntime {
	/// Report whether the adapter can currently serve conversation requests.
	fn availability(&self) -> Availability;
}

/// Whether an owned subsystem can currently serve requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Availability {
	/// The subsystem can serve its owned application contract.
	Available,
	/// The subsystem is intentionally unable to serve its owned application contract.
	Unavailable {
		/// Stable human-readable explanation of the unavailable boundary.
		reason: &'static str,
	},
}

/// Validated status of the two authority-bearing vNext foundations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FoundationStatus {
	product_state: Availability,
	conversation_runtime: Availability,
}
impl FoundationStatus {
	/// Assemble the application status through its owned ports.
	pub fn assemble(
		product_state: &impl ProductState,
		conversation_runtime: &impl ConversationRuntime,
	) -> Self {
		Self {
			product_state: product_state.availability(),
			conversation_runtime: conversation_runtime.availability(),
		}
	}

	/// Report product-state adapter availability.
	pub const fn product_state(self) -> Availability {
		self.product_state
	}

	/// Report conversation runtime availability.
	pub const fn conversation_runtime(self) -> Availability {
		self.conversation_runtime
	}

	/// Return true only when both required authority-bearing adapters are available.
	pub const fn is_operational(self) -> bool {
		matches!(self.product_state, Availability::Available)
			&& matches!(self.conversation_runtime, Availability::Available)
	}
}

#[cfg(test)]
mod tests {
	use crate::{Availability, ConversationRuntime, FoundationStatus, ProductState};

	struct Store;

	impl ProductState for Store {
		fn availability(&self) -> Availability {
			Availability::Unavailable { reason: "not wired" }
		}
	}

	struct Conversation;

	impl ConversationRuntime for Conversation {
		fn availability(&self) -> Availability {
			Availability::Unavailable { reason: "not wired" }
		}
	}

	#[test]
	fn foundation_is_not_operational_until_both_owned_ports_are_available() {
		let status = FoundationStatus::assemble(&Store, &Conversation);

		assert!(!status.is_operational());
	}
}
