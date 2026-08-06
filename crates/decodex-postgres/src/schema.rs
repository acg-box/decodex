//! Empty-target creation and exact current-schema verification.

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use decodex_core::ProcessExecutionAuthorization;
use serde_json::{Value, json};
use tokio_postgres::{Config, GenericClient, Transaction};

use crate::{
	BootstrapFailure, REQUIRED_POSTGRES_MAJOR, StoreError, apply_trusted_session_invariants,
	authority,
	authority::{
		BootstrapAuthorityEvidence, BootstrapAuthorityFailureClass, BootstrapAuthorityObservation,
		BootstrapAuthorityOperation, BootstrapAuthorityProgress, BootstrapDigestEvidence,
	},
	checkout,
	error::{BootstrapDatabaseDiagnostic, BootstrapFailureIdentity},
	validate_connection, verified_socket_connect,
};

pub(crate) const LATEST_SCHEMA_SQL: &str = include_str!("../schema.sql");
/// Fixed prefix for the hidden operator command's one credential-negative report line.
pub const BOOTSTRAP_AUTHORITY_REPORT_PREFIX: &str = "DECODEX_BOOTSTRAP_REPORT=";
const BOOTSTRAP_AUTHORITY_REPORT_SCHEMA: &str = "decodex/bootstrap-report/1";
const BOOTSTRAP_AUTHORITY_REPORT_MAX_BYTES: usize = 16 * 1024;
const BOOTSTRAP_PLATFORM_NAMES: [&str; 8] = [
	"postgres_major",
	"data_checksums",
	"trusted_search_path",
	"trusted_time_zone",
	"trusted_time_zone_offset",
	"database_envelope",
	"pgcrypto_present",
	"pgcrypto_version",
];

/// Failed latest-schema bootstrap with a bounded credential-negative transaction report.
pub struct LatestSchemaBootstrapFailure {
	_error: StoreError,
	report: BootstrapReport,
}

impl LatestSchemaBootstrapFailure {
	fn reported(error: StoreError, report: BootstrapReport) -> Self {
		Self { _error: error, report }
	}

	fn operation_failure(
		error: StoreError,
		operation: BootstrapOperation,
		statement: Option<&str>,
	) -> Self {
		let report = BootstrapReport::operation_failure(operation, &error, statement);
		Self::reported(error, report)
	}

	fn with_rollback_failure(mut self, rollback_error: &StoreError) -> Self {
		let rollback = rollback_error.bootstrap_failure_identity();
		self.report.rollback_failure =
			Some(BootstrapRollbackFailure { failed: true, category: rollback.category });
		self
	}

	fn fmt_closed(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"latest-schema bootstrap failed (classification={}, phase={})",
			bootstrap_failure_name(self.report.classification),
			self.report.failure.phase,
		)
	}

	/// Return the stable value-free command failure classification.
	pub fn bootstrap_failure(&self) -> BootstrapFailure {
		self.report.classification
	}

	/// Return the canonical credential-negative report for the hidden command.
	#[doc(hidden)]
	pub fn report_json(&self) -> String {
		self.report.canonical_json()
	}
}

impl std::fmt::Debug for LatestSchemaBootstrapFailure {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.fmt_closed(formatter)
	}
}

impl std::fmt::Display for LatestSchemaBootstrapFailure {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		self.fmt_closed(formatter)
	}
}

impl std::error::Error for LatestSchemaBootstrapFailure {}

impl From<StoreError> for LatestSchemaBootstrapFailure {
	fn from(error: StoreError) -> Self {
		Self::operation_failure(error, BootstrapOperation::BootstrapAdmission, None)
	}
}

impl From<deadpool_postgres::BuildError> for LatestSchemaBootstrapFailure {
	fn from(error: deadpool_postgres::BuildError) -> Self {
		Self::operation_failure(
			StoreError::from(error),
			BootstrapOperation::BootstrapAdmission,
			None,
		)
	}
}

impl From<tokio_postgres::Error> for LatestSchemaBootstrapFailure {
	fn from(error: tokio_postgres::Error) -> Self {
		Self::operation_failure(
			StoreError::Database(error),
			BootstrapOperation::BootstrapAdmission,
			None,
		)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BootstrapOperationFailure {
	phase: &'static str,
	operation: &'static str,
	category: &'static str,
	sqlstate: Option<String>,
	statement_byte_position: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BootstrapOperation {
	BootstrapAdmission,
	TargetVerification,
	RuntimeRoleBinding,
	SchemaBatch,
	TrustedSessionReset,
	Platform,
	InitialAuthorization,
	Authority(BootstrapAuthorityOperation),
	AuthorityVerification,
	TransactionCommit,
}

impl BootstrapOperation {
	const fn phase(self) -> &'static str {
		match self {
			Self::BootstrapAdmission | Self::TargetVerification | Self::RuntimeRoleBinding =>
				"pre_schema",
			Self::SchemaBatch => "schema_apply",
			Self::TrustedSessionReset
			| Self::Platform
			| Self::InitialAuthorization
			| Self::Authority(_)
			| Self::AuthorityVerification => "post_schema_verify",
			Self::TransactionCommit => "finalize",
		}
	}

	const fn as_str(self) -> &'static str {
		match self {
			Self::BootstrapAdmission => "bootstrap_admission",
			Self::TargetVerification => "target_verification",
			Self::RuntimeRoleBinding => "runtime_role_binding",
			Self::SchemaBatch => "schema_batch",
			Self::TrustedSessionReset => "trusted_session_reset",
			Self::Platform => "platform",
			Self::InitialAuthorization => "initial_authorization",
			Self::Authority(operation) => operation.as_str(),
			Self::AuthorityVerification => "authority_verification",
			Self::TransactionCommit => "transaction_commit",
		}
	}

	const fn completed_authority_components(self) -> usize {
		match self {
			Self::BootstrapAdmission
			| Self::TargetVerification
			| Self::RuntimeRoleBinding
			| Self::SchemaBatch
			| Self::TrustedSessionReset
			| Self::Platform
			| Self::InitialAuthorization => 0,
			Self::Authority(operation) => operation.completed_components_before(),
			Self::AuthorityVerification | Self::TransactionCommit => 4,
		}
	}

	const fn platform_complete(self) -> bool {
		matches!(
			self,
			Self::InitialAuthorization
				| Self::Authority(_)
				| Self::AuthorityVerification
				| Self::TransactionCommit
		)
	}
}

#[derive(Debug)]
struct BootstrapReport {
	complete: bool,
	classification: BootstrapFailure,
	platform: Vec<BootstrapAuthorityObservation>,
	authority: BootstrapAuthorityProgress,
	failure: BootstrapOperationFailure,
	rollback_failure: Option<BootstrapRollbackFailure>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BootstrapRollbackFailure {
	failed: bool,
	category: &'static str,
}

struct CompletedBootstrapVerification {
	platform: Vec<BootstrapAuthorityObservation>,
	authority: BootstrapAuthorityEvidence,
}

impl BootstrapReport {
	fn operation_failure(
		operation: BootstrapOperation,
		error: &StoreError,
		statement: Option<&str>,
	) -> Self {
		Self::partial_failure(
			Vec::new(),
			BootstrapAuthorityProgress::default(),
			operation,
			error,
			statement,
		)
	}

	fn complete_failure(
		platform: Vec<BootstrapAuthorityObservation>,
		authority: BootstrapAuthorityEvidence,
		operation: BootstrapOperation,
		error: &StoreError,
	) -> Self {
		assert_eq!(
			platform.iter().map(|observation| observation.name).collect::<Vec<_>>(),
			BOOTSTRAP_PLATFORM_NAMES
		);
		assert_eq!(authority.semantic.len(), authority::SEMANTIC_AUTHORITY_PREDICATE_COUNT);
		let BootstrapFailureIdentity { classification, category } =
			error.bootstrap_failure_identity();
		Self {
			complete: true,
			classification,
			platform,
			authority: authority.into(),
			failure: operation_failure(operation, error, None, category),
			rollback_failure: None,
		}
	}

	fn partial_failure(
		platform: Vec<BootstrapAuthorityObservation>,
		authority: BootstrapAuthorityProgress,
		operation: BootstrapOperation,
		error: &StoreError,
		statement: Option<&str>,
	) -> Self {
		if operation.platform_complete() {
			assert!(
				platform.iter().map(|observation| observation.name).eq(BOOTSTRAP_PLATFORM_NAMES)
			);
		} else {
			assert!(platform.is_empty());
		}
		assert_eq!(authority.completed_components(), operation.completed_authority_components());
		let BootstrapFailureIdentity { classification, category } =
			error.bootstrap_failure_identity();
		Self {
			complete: false,
			classification,
			platform,
			authority,
			failure: operation_failure(operation, error, statement, category),
			rollback_failure: None,
		}
	}

	fn canonical_json(&self) -> String {
		let namespace: Vec<Value> = self
			.authority
			.namespace
			.as_ref()
			.map(|evidence| evidence.iter().map(observation_json).collect())
			.unwrap_or_default();
		let semantic: Vec<Value> = self
			.authority
			.semantic
			.as_ref()
			.map(|evidence| evidence.iter().map(observation_json).collect())
			.unwrap_or_default();
		let configured_authority = self.authority.configured_authority.as_ref().map(digest_json);
		let schema_contract = self.authority.schema_contract.as_ref().map(digest_json);
		let failure = json!({
			"category": self.failure.category,
			"operation": self.failure.operation,
			"phase": self.failure.phase,
			"sqlstate": self.failure.sqlstate.as_deref(),
			"statement_byte_position": self.failure.statement_byte_position,
		});
		let rollback_failure = self.rollback_failure.map(|failure| {
			json!({
				"category": failure.category,
				"failed": failure.failed,
			})
		});
		let value = json!({
			"classification": bootstrap_failure_name(self.classification),
			"complete": self.complete,
			"configured_authority": configured_authority,
			"failure": failure,
			"namespace": namespace,
			"platform": self.platform.iter().map(observation_json).collect::<Vec<_>>(),
			"rollback_failure": rollback_failure,
			"schema": BOOTSTRAP_AUTHORITY_REPORT_SCHEMA,
			"schema_contract": schema_contract,
			"semantic": semantic,
		});
		let encoded = serde_json::to_string(&value).expect("closed bootstrap report serializes");
		assert!(encoded.len() <= BOOTSTRAP_AUTHORITY_REPORT_MAX_BYTES);
		encoded
	}
}

fn operation_failure(
	operation: BootstrapOperation,
	error: &StoreError,
	statement: Option<&str>,
	category: &'static str,
) -> BootstrapOperationFailure {
	let BootstrapDatabaseDiagnostic { sqlstate, statement_byte_position } =
		error.bootstrap_database_diagnostic(statement);
	BootstrapOperationFailure {
		phase: operation.phase(),
		operation: operation.as_str(),
		category,
		sqlstate,
		statement_byte_position,
	}
}

fn observation_json(observation: &BootstrapAuthorityObservation) -> Value {
	json!({
		"class": observation.failure_class.as_str(),
		"name": observation.name,
		"pass": observation.passed,
	})
}

fn digest_json(evidence: &BootstrapDigestEvidence) -> Value {
	json!({
		"actual_sha256": evidence.actual_sha256.map(hex_digest),
		"class": evidence.failure_class().as_str(),
		"complete": evidence.complete,
		"expected_sha256": hex_digest(evidence.expected_sha256),
		"pass": evidence.passed(),
	})
}

fn hex_digest(digest: [u8; 32]) -> String {
	digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

const fn bootstrap_failure_name(failure: BootstrapFailure) -> &'static str {
	match failure {
		BootstrapFailure::Authentication => "authentication",
		BootstrapFailure::Unreachable => "unreachable",
		BootstrapFailure::Incompatible => "incompatible",
		BootstrapFailure::UnsafeAuthority => "unsafe_authority",
		BootstrapFailure::UnsafeHostPath => "unsafe_host_path",
	}
}

#[cfg(unix)]
pub(crate) async fn bootstrap_latest_schema(
	mut owner: Config,
	expected_peer_uid: u32,
	schema_owner_role: &str,
	runtime_role: &str,
	authorization: &ProcessExecutionAuthorization,
) -> Result<(), LatestSchemaBootstrapFailure> {
	validate_connection(&owner)?;
	let connector = verified_socket_connect(&owner, expected_peer_uid)?;
	apply_trusted_session_invariants(&mut owner);
	let manager = Manager::from_connect(
		owner,
		connector.clone(),
		ManagerConfig { recycling_method: RecyclingMethod::Fast },
	);
	let pool = Pool::builder(manager).max_size(1).build()?;
	let mut client = checkout(&pool, &connector).await?;
	let transaction = client.transaction().await?;
	let bootstrap_result =
		bootstrap_transaction(&transaction, schema_owner_role, runtime_role, authorization).await;
	let result = match bootstrap_result {
		Ok(verification) => match transaction.commit().await {
			Ok(()) => Ok(()),
			Err(error) => {
				let error = StoreError::Database(error);
				let report = BootstrapReport::complete_failure(
					verification.platform,
					verification.authority,
					BootstrapOperation::TransactionCommit,
					&error,
				);
				Err(LatestSchemaBootstrapFailure::reported(error, report))
			},
		},
		Err(error) => match transaction.rollback().await {
			Ok(()) => Err(error),
			Err(rollback_error) => {
				let rollback_error = StoreError::Database(rollback_error);
				Err(error.with_rollback_failure(&rollback_error))
			},
		},
	};
	drop(client);
	pool.close();
	result
}

async fn bootstrap_transaction(
	transaction: &Transaction<'_>,
	schema_owner_role: &str,
	runtime_role: &str,
	authorization: &ProcessExecutionAuthorization,
) -> Result<CompletedBootstrapVerification, LatestSchemaBootstrapFailure> {
	verify_clean_target(transaction, schema_owner_role, runtime_role).await.map_err(|error| {
		LatestSchemaBootstrapFailure::operation_failure(
			error,
			BootstrapOperation::TargetVerification,
			None,
		)
	})?;
	transaction
		.execute(
			"SELECT pg_catalog.set_config('decodex.bootstrap_runtime_role',$1,true)",
			&[&runtime_role],
		)
		.await
		.map_err(|error| {
			LatestSchemaBootstrapFailure::operation_failure(
				StoreError::Database(error),
				BootstrapOperation::RuntimeRoleBinding,
				None,
			)
		})?;
	transaction.batch_execute(LATEST_SCHEMA_SQL).await.map_err(|error| {
		LatestSchemaBootstrapFailure::operation_failure(
			StoreError::Database(error),
			BootstrapOperation::SchemaBatch,
			Some(LATEST_SCHEMA_SQL),
		)
	})?;
	transaction
		.execute(
			"SELECT pg_catalog.set_config('search_path','pg_catalog',false), \
			 pg_catalog.set_config('TimeZone','+05:00',false)",
			&[],
		)
		.await
		.map_err(|error| {
			LatestSchemaBootstrapFailure::operation_failure(
				StoreError::Database(error),
				BootstrapOperation::TrustedSessionReset,
				None,
			)
		})?;
	let platform = match platform_evidence(transaction).await {
		Ok(evidence) => evidence,
		Err(error) => {
			let report = BootstrapReport::partial_failure(
				Vec::new(),
				BootstrapAuthorityProgress::default(),
				BootstrapOperation::Platform,
				&error,
				None,
			);
			return Err(LatestSchemaBootstrapFailure::reported(error, report));
		},
	};
	if let Err(error) = transaction
		.execute(
			"INSERT INTO decodex.process_generation_execution_epochs(\
			 execution_epoch_id,authorization_digest,authorized_at,retired_at) \
			 VALUES($1::text::uuid,$2,pg_catalog.clock_timestamp(),NULL)",
			&[&authorization.epoch_id.as_str(), &authorization.authorization_digest],
		)
		.await
	{
		let error = StoreError::Database(error);
		let report = BootstrapReport::partial_failure(
			platform,
			BootstrapAuthorityProgress::default(),
			BootstrapOperation::InitialAuthorization,
			&error,
			None,
		);
		return Err(LatestSchemaBootstrapFailure::reported(error, report));
	}
	let authority = match authority::collect_bootstrap_authority_evidence(
		transaction,
		schema_owner_role,
		runtime_role,
	)
	.await
	{
		Ok(evidence) => evidence,
		Err(failure) => {
			let progress = failure.progress;
			let operation = failure.operation;
			let error = failure.error;
			let report = BootstrapReport::partial_failure(
				platform,
				progress,
				BootstrapOperation::Authority(operation),
				&error,
				None,
			);
			return Err(LatestSchemaBootstrapFailure::reported(error, report));
		},
	};
	if let Err(error) = enforce_bootstrap_verification(&platform, &authority) {
		let report = BootstrapReport::complete_failure(
			platform,
			authority,
			BootstrapOperation::AuthorityVerification,
			&error,
		);
		return Err(LatestSchemaBootstrapFailure::reported(error, report));
	}

	Ok(CompletedBootstrapVerification { platform, authority })
}

async fn verify_clean_target<C>(
	client: &C,
	schema_owner_role: &str,
	runtime_role: &str,
) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	verify_bootstrap_principals(client, schema_owner_role, runtime_role).await?;
	verify_empty_catalog(client).await
}

async fn verify_bootstrap_principals<C>(
	client: &C,
	schema_owner_role: &str,
	runtime_role: &str,
) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	let principals = client
		.query_one(
			"SELECT \
			 session_user=$1::pg_catalog.name AND current_user=$1::pg_catalog.name, \
			 database_owner.rolname=$1::pg_catalog.name, \
			 runtime.oid IS NOT NULL \
			 AND runtime.rolcanlogin AND NOT runtime.rolinherit \
			 AND NOT runtime.rolsuper AND NOT runtime.rolcreatedb \
			 AND NOT runtime.rolcreaterole AND NOT runtime.rolreplication \
			 AND NOT runtime.rolbypassrls AND runtime.rolconnlimit=-1 \
			 AND runtime.rolvaliduntil='infinity'::pg_catalog.timestamptz \
			 AND NOT pg_catalog.pg_has_role(runtime.oid,$1::pg_catalog.name,'MEMBER') \
			 AND NOT pg_catalog.pg_has_role($1::pg_catalog.name,runtime.oid,'MEMBER') \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_db_role_setting AS setting \
			   WHERE setting.setrole IN (runtime.oid,database_owner.oid) \
			 ) \
			 FROM pg_catalog.pg_database AS database \
			 JOIN pg_catalog.pg_roles AS database_owner ON database_owner.oid=database.datdba \
			 LEFT JOIN pg_catalog.pg_roles AS runtime ON runtime.rolname=$2::pg_catalog.name \
			 WHERE database.datname=current_database()",
			&[&schema_owner_role, &runtime_role],
		)
		.await?;
	if !principals.get::<_, bool>(0) || !principals.get::<_, bool>(1) {
		return Err(StoreError::UnsafeAuthority(
			"bootstrap identity does not exclusively own the target database",
		));
	}
	if !principals.get::<_, bool>(2) {
		return Err(StoreError::UnsafeAuthority(
			"configured runtime PostgreSQL identity is absent or unsafe",
		));
	}
	Ok(())
}

async fn verify_empty_catalog<C>(client: &C) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	let clean: bool = client
		.query_one(
			"SELECT NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_namespace \
			   WHERE nspname NOT IN ('pg_catalog','information_schema','pg_toast','public') \
			     AND nspname !~ '^pg_(toast_)?temp_[0-9]+$' \
			 ) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_extension WHERE extname<>'plpgsql') \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_class AS relation \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=relation.relnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_proc AS routine \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=routine.pronamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_type AS type \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=type.typnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_collation AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.collnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_conversion AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.connamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_operator AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.oprnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_opclass AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.opcnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_opfamily AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.opfnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_statistic_ext AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.stxnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_ts_config AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.cfgnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_ts_dict AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.dictnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_ts_parser AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.prsnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_ts_template AS object \
			   JOIN pg_catalog.pg_namespace AS namespace ON namespace.oid=object.tmplnamespace \
			   WHERE namespace.nspname='public' \
			 ) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_event_trigger) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_largeobject_metadata) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_publication) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_subscription) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_foreign_data_wrapper) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_foreign_server) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_transform) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_language \
			   WHERE lanname NOT IN ('internal','c','sql','plpgsql') \
			 )",
			&[],
		)
		.await?
		.get(0);
	if !clean {
		return Err(StoreError::Incompatible(
			"latest-schema bootstrap requires an exact empty PostgreSQL target".into(),
		));
	}

	Ok(())
}

async fn platform_baseline_evidence<C>(
	client: &C,
) -> Result<Vec<BootstrapAuthorityObservation>, StoreError>
where
	C: GenericClient + Sync,
{
	let row = client
		.query_one(
			"SELECT pg_catalog.current_setting('server_version_num')::integer / 10000, \
			 pg_catalog.current_setting('data_checksums'), \
			 pg_catalog.current_setting('search_path'), \
			 pg_catalog.current_setting('TimeZone'), \
			 EXTRACT(timezone FROM CURRENT_TIMESTAMP)::pg_catalog.int8",
			&[],
		)
		.await?;
	let major: i32 = row.get(0);
	let checksums: String = row.get(1);
	let search_path: String = row.get(2);
	let time_zone: String = row.get(3);
	let time_zone_offset_seconds: i64 = row.get(4);

	Ok(vec![
		BootstrapAuthorityObservation {
			name: "postgres_major",
			failure_class: BootstrapAuthorityFailureClass::Incompatible,
			passed: major == i32::try_from(REQUIRED_POSTGRES_MAJOR).expect("major fits i32"),
		},
		BootstrapAuthorityObservation {
			name: "data_checksums",
			failure_class: BootstrapAuthorityFailureClass::Incompatible,
			passed: checksums == "on",
		},
		BootstrapAuthorityObservation {
			name: "trusted_search_path",
			failure_class: BootstrapAuthorityFailureClass::Unsafe,
			passed: search_path == "pg_catalog",
		},
		BootstrapAuthorityObservation {
			name: "trusted_time_zone",
			failure_class: BootstrapAuthorityFailureClass::Unsafe,
			passed: time_zone == "+05:00",
		},
		BootstrapAuthorityObservation {
			name: "trusted_time_zone_offset",
			failure_class: BootstrapAuthorityFailureClass::Unsafe,
			passed: time_zone_offset_seconds == -18_000,
		},
	])
}

pub(crate) async fn verify_platform<C>(client: &C) -> Result<(), StoreError>
where
	C: GenericClient + Sync,
{
	let evidence = platform_evidence(client).await?;
	enforce_platform_evidence(&evidence)
}

async fn platform_evidence<C>(client: &C) -> Result<Vec<BootstrapAuthorityObservation>, StoreError>
where
	C: GenericClient + Sync,
{
	let mut evidence = platform_baseline_evidence(client).await?;
	evidence.push(BootstrapAuthorityObservation {
		name: "database_envelope",
		failure_class: BootstrapAuthorityFailureClass::Incompatible,
		passed: database_envelope_matches(client).await?,
	});
	let pgcrypto = client
		.query_opt("SELECT extversion FROM pg_catalog.pg_extension WHERE extname='pgcrypto'", &[])
		.await?
		.map(|row| row.get::<_, String>(0));
	evidence.push(BootstrapAuthorityObservation {
		name: "pgcrypto_present",
		failure_class: BootstrapAuthorityFailureClass::Incompatible,
		passed: pgcrypto.is_some(),
	});
	evidence.push(BootstrapAuthorityObservation {
		name: "pgcrypto_version",
		failure_class: BootstrapAuthorityFailureClass::Incompatible,
		passed: pgcrypto.as_deref() == Some("1.4"),
	});

	Ok(evidence)
}

fn enforce_platform_evidence(evidence: &[BootstrapAuthorityObservation]) -> Result<(), StoreError> {
	let Some(failed) = evidence.iter().find(|observation| !observation.passed) else {
		return Ok(());
	};
	match failed.failure_class {
		BootstrapAuthorityFailureClass::Unsafe => Err(StoreError::UnsafeAuthority(
			"PostgreSQL trusted platform authority differs from the shipped contract",
		)),
		BootstrapAuthorityFailureClass::Incompatible => Err(StoreError::Incompatible(
			"PostgreSQL platform differs from the shipped contract".into(),
		)),
	}
}

fn enforce_bootstrap_verification(
	platform: &[BootstrapAuthorityObservation],
	authority: &BootstrapAuthorityEvidence,
) -> Result<(), StoreError> {
	enforce_platform_evidence(platform)?;
	authority::enforce_bootstrap_authority(authority)
}

async fn database_envelope_matches<C>(client: &C) -> Result<bool, StoreError>
where
	C: GenericClient + Sync,
{
	let exact = client
		.query_one(
			"WITH pgcrypto AS ( \
			   SELECT oid FROM pg_catalog.pg_extension WHERE extname='pgcrypto' \
			 ), public_namespace AS ( \
			   SELECT oid FROM pg_catalog.pg_namespace WHERE nspname='public' \
			 ), public_objects(classid,objid) AS ( \
			   SELECT 'pg_catalog.pg_class'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_class WHERE relnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_proc'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_proc WHERE pronamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_type'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_type WHERE typnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_collation'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_collation WHERE collnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_conversion'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_conversion WHERE connamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_operator'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_operator WHERE oprnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_opclass'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_opclass WHERE opcnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_opfamily'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_opfamily WHERE opfnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_statistic_ext'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_statistic_ext WHERE stxnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_ts_config'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_ts_config WHERE cfgnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_ts_dict'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_ts_dict WHERE dictnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_ts_parser'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_ts_parser WHERE prsnamespace IN (SELECT oid FROM public_namespace) \
			   UNION ALL SELECT 'pg_catalog.pg_ts_template'::pg_catalog.regclass,oid \
			   FROM pg_catalog.pg_ts_template WHERE tmplnamespace IN (SELECT oid FROM public_namespace) \
			 ) \
			 SELECT COALESCE(( \
			   SELECT pg_catalog.array_agg(extname::pg_catalog.text ORDER BY extname COLLATE pg_catalog.\"C\") \
			   FROM pg_catalog.pg_extension \
			 ),ARRAY[]::pg_catalog.text[])=ARRAY['pgcrypto','plpgsql']::pg_catalog.text[] \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_namespace \
			   WHERE nspname NOT IN ('pg_catalog','information_schema','pg_toast','public','decodex') \
			     AND nspname !~ '^pg_(toast_)?temp_[0-9]+$' \
			 ) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM public_objects AS object \
			   WHERE NOT EXISTS ( \
			     SELECT 1 FROM pg_catalog.pg_depend AS dependency \
			     WHERE dependency.classid=object.classid \
			       AND dependency.objid=object.objid AND dependency.objsubid=0 \
			       AND dependency.refclassid='pg_catalog.pg_extension'::pg_catalog.regclass \
			       AND dependency.refobjid=(SELECT oid FROM pgcrypto) \
			       AND dependency.refobjsubid=0 AND dependency.deptype='e' \
			   ) \
			 ) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_event_trigger) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_largeobject_metadata) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_publication) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_subscription) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_foreign_data_wrapper) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_foreign_server) \
			 AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_transform) \
			 AND NOT EXISTS ( \
			   SELECT 1 FROM pg_catalog.pg_language \
			   WHERE lanname NOT IN ('internal','c','sql','plpgsql') \
			 )",
			&[],
		)
		.await?
		.get(0);

	Ok(exact)
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use serde_json::Value;

	use super::{
		BOOTSTRAP_AUTHORITY_REPORT_MAX_BYTES, BOOTSTRAP_AUTHORITY_REPORT_SCHEMA,
		BOOTSTRAP_PLATFORM_NAMES, BootstrapAuthorityFailureClass, BootstrapAuthorityObservation,
		BootstrapAuthorityOperation, BootstrapAuthorityProgress, BootstrapOperation,
		BootstrapReport, LatestSchemaBootstrapFailure, StoreError,
	};
	use crate::authority;

	fn passing_platform() -> Vec<BootstrapAuthorityObservation> {
		BOOTSTRAP_PLATFORM_NAMES
			.into_iter()
			.map(|name| BootstrapAuthorityObservation {
				name,
				failure_class: if name.starts_with("trusted_") {
					BootstrapAuthorityFailureClass::Unsafe
				} else {
					BootstrapAuthorityFailureClass::Incompatible
				},
				passed: true,
			})
			.collect()
	}

	fn authority_progress(completed_components: usize) -> BootstrapAuthorityProgress {
		assert!(completed_components <= 4);
		let mut progress: BootstrapAuthorityProgress =
			authority::passing_bootstrap_authority_evidence_fixture().into();
		if completed_components < 4 {
			progress.schema_contract = None;
		}
		if completed_components < 3 {
			progress.configured_authority = None;
		}
		if completed_components < 2 {
			progress.semantic = None;
		}
		if completed_components < 1 {
			progress.namespace = None;
		}
		progress
	}

	fn assert_bounded_strings(value: &Value) {
		match value {
			Value::Array(values) => values.iter().for_each(assert_bounded_strings),
			Value::Object(values) =>
				for (key, value) in values {
					assert!(key.len() <= 64);
					assert_bounded_strings(value);
				},
			Value::String(value) => assert!(value.len() <= 128),
			_ => {},
		}
	}

	#[test]
	fn complete_bootstrap_report_is_canonical_closed_unique_and_credential_negative() {
		let mut authority = authority::passing_bootstrap_authority_evidence_fixture();
		authority.configured_authority.actual_sha256 = Some([0; 32]);
		let error = StoreError::UnsafeAuthority("test authority mismatch");
		let report = BootstrapReport::complete_failure(
			passing_platform(),
			authority,
			BootstrapOperation::AuthorityVerification,
			&error,
		);
		let encoded = report.canonical_json();
		assert!(encoded.len() <= BOOTSTRAP_AUTHORITY_REPORT_MAX_BYTES);
		assert!(!encoded.contains('\n'));
		let value: Value = serde_json::from_str(&encoded).expect("report is JSON");
		assert_eq!(encoded, serde_json::to_string(&value).expect("canonical report reserializes"));
		assert_eq!(value["schema"], BOOTSTRAP_AUTHORITY_REPORT_SCHEMA);
		assert_eq!(value["complete"], true);
		assert_eq!(value["failure"]["phase"], "post_schema_verify");
		assert_eq!(value["failure"]["operation"], "authority_verification");
		assert!(value["rollback_failure"].is_null());
		assert_eq!(value["semantic"].as_array().expect("semantic array").len(), 37);
		let names = value["semantic"]
			.as_array()
			.expect("semantic array")
			.iter()
			.map(|item| item["name"].as_str().expect("closed name"))
			.collect::<HashSet<_>>();
		assert_eq!(names.len(), 37);
		for forbidden in [
			"credential",
			"database_message",
			"detail",
			"hint",
			"os_status",
			"role_name",
			"sql_text",
			"token",
		] {
			assert!(!encoded.contains(forbidden));
		}
		assert_bounded_strings(&value);
	}

	#[test]
	fn operation_failure_reports_retain_only_completed_authority_prefixes() {
		let private_detail = concat!(
			"credential_secret database_message_secret detail_secret hint_secret path_secret ",
			"role_name_secret sql_text_secret token_secret",
		);
		let error = StoreError::Incompatible(private_detail.into());
		for (operation, operation_name, phase, completed_components, platform_complete) in [
			(BootstrapOperation::Platform, "platform", "post_schema_verify", 0, false),
			(
				BootstrapOperation::InitialAuthorization,
				"initial_authorization",
				"post_schema_verify",
				0,
				true,
			),
			(
				BootstrapOperation::Authority(BootstrapAuthorityOperation::Namespace),
				"namespace",
				"post_schema_verify",
				0,
				true,
			),
			(
				BootstrapOperation::Authority(BootstrapAuthorityOperation::Semantic),
				"semantic",
				"post_schema_verify",
				1,
				true,
			),
			(
				BootstrapOperation::Authority(BootstrapAuthorityOperation::ConfiguredAuthority),
				"configured_authority",
				"post_schema_verify",
				2,
				true,
			),
			(
				BootstrapOperation::Authority(BootstrapAuthorityOperation::SchemaContract),
				"schema_contract",
				"post_schema_verify",
				3,
				true,
			),
		] {
			let platform = if platform_complete { passing_platform() } else { Vec::new() };
			let report = BootstrapReport::partial_failure(
				platform,
				authority_progress(completed_components),
				operation,
				&error,
				None,
			);
			let encoded = report.canonical_json();
			let value: Value = serde_json::from_str(&encoded).expect("report is JSON");
			let failure = value["failure"].as_object().expect("operation failure object");
			assert_eq!(value["complete"], false);
			assert_eq!(
				value["platform"].as_array().expect("platform array").len(),
				if platform_complete { 8 } else { 0 }
			);
			assert_eq!(
				value["namespace"].as_array().expect("namespace array").len(),
				if completed_components >= 1 { 2 } else { 0 }
			);
			assert_eq!(
				value["semantic"].as_array().expect("semantic array").len(),
				if completed_components >= 2 { 37 } else { 0 }
			);
			assert_eq!(value["configured_authority"].is_object(), completed_components >= 3);
			assert!(value["schema_contract"].is_null());
			assert_eq!(failure.len(), 5);
			assert_eq!(failure["phase"], phase);
			assert_eq!(failure["operation"], operation_name);
			assert_eq!(failure["category"], "evidence");
			assert!(failure["sqlstate"].is_null());
			assert!(failure["statement_byte_position"].is_null());
			for forbidden in [
				"credential_secret",
				"database_message_secret",
				"detail_secret",
				"hint_secret",
				"path_secret",
				"role_name_secret",
				"sql_text_secret",
				"token_secret",
			] {
				assert!(!encoded.contains(forbidden));
			}
			for forbidden_field in
				["credential", "detail", "hint", "message", "path", "role", "sql"]
			{
				assert!(!failure.contains_key(forbidden_field));
			}
			assert_bounded_strings(&value);
		}
	}

	#[test]
	fn schema_apply_report_preserves_primary_identity_when_rollback_also_fails() {
		let primary = StoreError::Incompatible("private primary detail".into());
		let mut report = BootstrapReport::operation_failure(
			BootstrapOperation::SchemaBatch,
			&primary,
			Some("SELECT 'secret';"),
		);
		report.failure.sqlstate = Some("42601".into());
		report.failure.statement_byte_position = Some(8);
		let failure = LatestSchemaBootstrapFailure::reported(primary, report)
			.with_rollback_failure(&StoreError::Incompatible("private rollback detail".into()));
		let encoded = failure.report_json();
		let value: Value = serde_json::from_str(&encoded).expect("report is JSON");

		assert_eq!(value["schema"], BOOTSTRAP_AUTHORITY_REPORT_SCHEMA);
		assert_eq!(value["classification"], "incompatible");
		assert_eq!(value["complete"], false);
		assert_eq!(value["failure"]["phase"], "schema_apply");
		assert_eq!(value["failure"]["operation"], "schema_batch");
		assert_eq!(value["failure"]["category"], "evidence");
		assert_eq!(value["failure"]["sqlstate"], "42601");
		assert_eq!(value["failure"]["statement_byte_position"], 8);
		assert_eq!(value["rollback_failure"]["failed"], true);
		assert_eq!(value["rollback_failure"]["category"], "evidence");
		assert!(!encoded.contains("private primary detail"));
		assert!(!encoded.contains("private rollback detail"));
		assert!(!encoded.contains("SELECT"));
	}

	#[test]
	fn public_bootstrap_failure_formatting_never_exposes_the_private_store_error() {
		let secret = "credential_secret database_message_secret path_secret token_secret";
		let failure = LatestSchemaBootstrapFailure::operation_failure(
			StoreError::Incompatible(secret.into()),
			BootstrapOperation::TargetVerification,
			None,
		);
		let expected =
			"latest-schema bootstrap failed (classification=incompatible, phase=pre_schema)";

		assert_eq!(failure.to_string(), expected);
		assert_eq!(format!("{failure:?}"), expected);
		assert!(!failure.to_string().contains(secret));
		assert!(!format!("{failure:?}").contains(secret));
	}
}
