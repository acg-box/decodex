//! Account approval configuration observed for one native pending request.
use serde::{Deserialize, Serialize};

/// Configuration readback, distinct from effective tool policy or live approval readiness.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ChiefAppSettingsResult {
	/// Native account identity and configuration remained bound to the same request and source.
	Available {
		/// Opaque native connector identity.
		connector_id: String,
		/// Opaque native connected account identity.
		link_id: String,
		/// Source-bound identity of the exact configuration shown for review.
		review_token: String,
		/// Account mode after config layering, before tool and managed-policy precedence.
		effective_mode: Option<String>,
		/// Account reviewer after config layering, before managed-policy precedence.
		effective_reviewer: Option<String>,
		/// Account mode stored in the writable user layer; None inherits.
		user_mode: Option<String>,
		/// Account reviewer stored in the writable user layer; None inherits.
		user_reviewer: Option<String>,
	},
	/// No current native request, verified source, or readable configuration.
	Unavailable,
}
