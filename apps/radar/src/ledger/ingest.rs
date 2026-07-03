//! Artifact ingestion and schema-specific ledger extraction.

mod bundle;
mod signal;
mod subject;

pub(super) use self::{bundle::ingest_artifact_set, signal::record_signal_artifact};
