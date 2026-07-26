//! Process-level proof for the Unix local supervisor restart and shutdown contract.

#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(unused_crate_dependencies)]

use std::{
	fs::{self, File, OpenOptions},
	io::{Read as _, Write as _},
	os::{
		fd::AsRawFd as _,
		unix::{
			fs::{MetadataExt as _, PermissionsExt as _},
			net::UnixStream,
		},
	},
	path::{Path, PathBuf},
	process::{Child, Command, ExitStatus, Stdio},
	thread,
	time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;

const PORT: u16 = 55_431;
const ACCOUNT_ID: &str = "provider-account-one";
const EMAIL: &str = "first@example.test";

#[test]
fn account_pool_replacement_restarts_the_daemon_only_for_changed_credentials() {
	let fixture = SupervisorFixture::new();
	fixture.write_accounts("access-one");
	let mut supervisor = fixture.start();

	let first_generation = fixture.wait_for_daemon_generation(None, &mut supervisor);
	fixture.replace_accounts("access-one", 2);
	fixture.assert_daemon_generation_stable(first_generation, &mut supervisor);
	fixture.replace_accounts("access-two", 3);
	fixture.wait_for_daemon_generation(Some(first_generation), &mut supervisor);

	let status = signal_and_wait(&mut supervisor, libc::SIGTERM);
	let output = supervisor.wait_with_output().expect("collect supervisor output");

	assert!(status.success(), "supervisor must stop cleanly: {status}");
	assert!(!fixture.postgres_socket().exists());
	assert!(UnixStream::connect(fixture.daemon_socket()).is_err());
	for secret in ["access-one", "access-two", ACCOUNT_ID, EMAIL] {
		assert!(!output.stdout.windows(secret.len()).any(|window| window == secret.as_bytes()));
		assert!(!output.stderr.windows(secret.len()).any(|window| window == secret.as_bytes()));
	}
}

#[test]
fn postgres_exit_stops_the_daemon_and_fails_the_supervisor_generation() {
	let fixture = SupervisorFixture::new();
	fixture.write_accounts("access-one");
	let mut supervisor = fixture.start();

	fixture.wait_for_daemon_generation(None, &mut supervisor);
	fixture.stop_postgres();
	let status = wait_for_exit(&mut supervisor);
	let output = supervisor.wait_with_output().expect("collect supervisor output");

	assert!(!status.success(), "a lost PostgreSQL generation must fail the supervisor");
	assert!(!fixture.postgres_socket().exists());
	assert!(UnixStream::connect(fixture.daemon_socket()).is_err());
	for secret in ["access-one", ACCOUNT_ID, EMAIL] {
		assert!(!output.stdout.windows(secret.len()).any(|window| window == secret.as_bytes()));
		assert!(!output.stderr.windows(secret.len()).any(|window| window == secret.as_bytes()));
	}
}

struct SupervisorFixture {
	_home: TempDir,
	canonical_home: PathBuf,
	accounts: PathBuf,
	lock: PathBuf,
	mapping: PathBuf,
	postgres: PathBuf,
	pg_isready: PathBuf,
	data_directory: PathBuf,
	socket_directory: PathBuf,
	working_directory: PathBuf,
}

impl SupervisorFixture {
	#[allow(clippy::too_many_lines)] // One complete local-supervisor process fixture.
	fn new() -> Self {
		let home = TempDir::new().expect("create supervisor fixture");
		secure_directory(home.path());
		let canonical_home = home.path().canonicalize().expect("canonical fixture home");
		let decodex_root = canonical_home.join(".decodex");
		let server = decodex_root.join("server");
		let legacy_parent = canonical_home.join(".codex/decodex");
		let data_directory = canonical_home.join("data");
		let socket_directory = canonical_home.join("pg");
		let database_socket = canonical_home.join("db");
		let working_directory = canonical_home.join("repository");

		for directory in [
			&decodex_root,
			&server,
			&legacy_parent,
			&data_directory,
			&socket_directory,
			&database_socket,
			&working_directory,
		] {
			fs::create_dir_all(directory).expect("create fixture directory");
			secure_directory(directory);
		}
		let accounts = legacy_parent.join("accounts.jsonl");
		let lock = legacy_parent.join(".accounts.jsonl.lock");
		let mapping = decodex_root.join("reset-card-legacy-bridge.json");
		let postgres = canonical_home.join("fake-postgres.py");
		let pg_isready = canonical_home.join("fake-pg-isready.sh");

		secure_write(&lock, "");
		secure_write(
			&mapping,
			&format!(
				r#"{{"schema":"decodex/reset-card-legacy-bridge/1","accounts":[{{"slot":1,"provider_account_id_sha256":"{}"}}]}}"#,
				hex_sha256(ACCOUNT_ID.as_bytes())
			),
		);
		secure_write(
			&decodex_root.join("config.toml"),
			&format!(
				r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {}

[server_host.repositories.fixture]
host_path = "{}"

[server_host.reset_card_accounts."10000000-0000-4000-8000-000000000001"]
display_label = "Fixture"
initial_state = "available"
access_token_env_var = "DECODEX_RESET_CARD_SLOT_01_ACCESS_TOKEN"
provider_account_id_env_var = "DECODEX_RESET_CARD_SLOT_01_ACCOUNT_ID"
expected_email_env_var = "DECODEX_RESET_CARD_SLOT_01_EMAIL"
plan_type = "pro"

[postgres]
socket_directory = "{}"
expected_peer_uid = {}
port = 5432
database = "decodex"

[postgres.migration]
user = "decodex_migration"

[postgres.runtime]
user = "decodex_runtime"

[cache]
max_entries = 16
max_bytes = 1048576
max_entry_bytes = 65536
"#,
				fs::metadata(&decodex_root).expect("inspect root").uid(),
				working_directory.display(),
				database_socket.display(),
				fs::metadata(&decodex_root).expect("inspect root").uid(),
			),
		);
		write_executable(
			&postgres,
			r#"#!/usr/bin/env python3
import os
import signal
import socket
import sys
import time

args = sys.argv[1:]
data = args[args.index("-D") + 1]
socket_dir = args[args.index("-k") + 1]
port = args[args.index("-p") + 1]
socket_path = os.path.join(socket_dir, ".s.PGSQL." + port)
pid_path = os.path.join(data, "postmaster.pid")
with open(pid_path, "w", encoding="ascii") as handle:
    handle.write(str(os.getpid()) + "\n")
os.chmod(pid_path, 0o600)
server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(socket_path)
running = True
def stop(_signal, _frame):
    global running
    running = False
signal.signal(signal.SIGTERM, stop)
signal.signal(signal.SIGINT, stop)
while running:
    time.sleep(0.02)
server.close()
for path in (socket_path, pid_path):
    try:
        os.unlink(path)
    except FileNotFoundError:
        pass
"#,
		);
		write_executable(
			&pg_isready,
			r#"#!/bin/sh
set -eu
host=
port=
while [ "$#" -gt 0 ]; do
	case "$1" in
		-h) host=$2; shift 2 ;;
		-p) port=$2; shift 2 ;;
		*) shift ;;
	esac
done
[ -S "$host/.s.PGSQL.$port" ]
"#,
		);
		Self {
			_home: home,
			canonical_home,
			accounts,
			lock,
			mapping,
			postgres,
			pg_isready,
			data_directory,
			socket_directory,
			working_directory,
		}
	}

	fn write_accounts(&self, access_token: &str) {
		secure_write(&self.accounts, &account_record(access_token, 1));
	}

	fn replace_accounts(&self, access_token: &str, generation: u64) {
		let lock = OpenOptions::new().read(true).write(true).open(&self.lock).expect("open lock");
		// SAFETY: the descriptor remains valid until after the matching unlock.
		assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) }, 0);
		let temporary = self.accounts.with_file_name(".accounts.jsonl.integration.tmp");

		secure_write(&temporary, &account_record(access_token, generation));
		fs::rename(&temporary, &self.accounts).expect("atomically replace accounts");
		// SAFETY: the descriptor still owns the acquired lock.
		assert_eq!(unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_UN) }, 0);
	}

	fn start(&self) -> Child {
		Command::new(env!("CARGO_BIN_EXE_decodexd"))
			.arg("supervise-local")
			.arg("--postgres")
			.arg(&self.postgres)
			.arg("--pg-isready")
			.arg(&self.pg_isready)
			.arg("--data-directory")
			.arg(&self.data_directory)
			.arg("--socket-directory")
			.arg(&self.socket_directory)
			.arg("--port")
			.arg(PORT.to_string())
			.arg("--legacy-accounts")
			.arg(&self.accounts)
			.arg("--legacy-mapping")
			.arg(&self.mapping)
			.arg("--working-directory")
			.arg(&self.working_directory)
			.env("HOME", &self.canonical_home)
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.expect("start local supervisor")
	}

	fn wait_for_daemon_generation(
		&self,
		previous_inode: Option<u64>,
		supervisor: &mut Child,
	) -> u64 {
		let deadline = Instant::now() + Duration::from_secs(20);
		loop {
			if let Some(status) = supervisor.try_wait().expect("poll supervisor") {
				let mut stderr = String::new();
				supervisor
					.stderr
					.as_mut()
					.expect("supervisor stderr")
					.read_to_string(&mut stderr)
					.expect("read supervisor stderr");
				panic!("supervisor exited before daemon generation: {status}: {stderr}");
			}
			if let Ok(metadata) = fs::symlink_metadata(self.daemon_socket()) {
				let inode = metadata.ino();
				if previous_inode != Some(inode) {
					return inode;
				}
			}
			if Instant::now() >= deadline {
				panic!("new daemon generation did not start");
			}
			thread::sleep(Duration::from_millis(20));
		}
	}

	fn assert_daemon_generation_stable(&self, expected_inode: u64, supervisor: &mut Child) {
		let deadline = Instant::now() + Duration::from_millis(1_500);

		loop {
			if let Some(status) = supervisor.try_wait().expect("poll supervisor") {
				let mut stderr = String::new();
				supervisor
					.stderr
					.as_mut()
					.expect("supervisor stderr")
					.read_to_string(&mut stderr)
					.expect("read supervisor stderr");
				panic!(
					"supervisor exited while daemon generation should remain stable: {status}: {stderr}"
				);
			}
			let metadata = fs::symlink_metadata(self.daemon_socket())
				.expect("stable daemon socket should remain published");

			assert_eq!(metadata.ino(), expected_inode, "daemon generation changed unexpectedly");
			if Instant::now() >= deadline {
				return;
			}
			thread::sleep(Duration::from_millis(20));
		}
	}

	fn postgres_socket(&self) -> PathBuf {
		self.socket_directory.join(format!(".s.PGSQL.{PORT}"))
	}

	fn stop_postgres(&self) {
		let process_id = fs::read_to_string(self.data_directory.join("postmaster.pid"))
			.expect("read fake PostgreSQL generation")
			.lines()
			.next()
			.expect("PostgreSQL PID line")
			.parse::<libc::pid_t>()
			.expect("PostgreSQL PID");
		// SAFETY: the PID is written by the live supervised PostgreSQL fixture.
		assert_eq!(unsafe { libc::kill(process_id, libc::SIGTERM) }, 0);
	}

	fn daemon_socket(&self) -> PathBuf {
		self.canonical_home.join(".decodex/server/decodex.sock")
	}
}

fn account_record(access_token: &str, generation: u64) -> String {
	let identity_token = identity_token(ACCOUNT_ID, EMAIL, "pro");

	format!(
		r#"{{"email":"{EMAIL}","last_selected_at_unix_epoch":{generation},"tokens":{{"id_token":"{identity_token}","access_token":"{access_token}","refresh_token":"not-read","account_id":"{ACCOUNT_ID}"}}}}
"#
	)
}

fn signal_and_wait(child: &mut Child, signal: libc::c_int) -> ExitStatus {
	// SAFETY: the PID came from `Child`; the test passes a defined Unix signal.
	assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, signal) }, 0);
	wait_for_exit(child)
}

fn wait_for_exit(child: &mut Child) -> ExitStatus {
	let deadline = Instant::now() + Duration::from_secs(20);
	loop {
		if let Some(status) = child.try_wait().expect("poll supervisor exit") {
			return status;
		}
		if Instant::now() >= deadline {
			let _ = child.kill();
			panic!("supervisor did not stop before timeout");
		}
		thread::sleep(Duration::from_millis(20));
	}
}

fn hex_sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn identity_token(account_id: &str, email: &str, plan_type: &str) -> String {
	let payload = format!(
		r#"{{"email":"{email}","https://api.openai.com/auth":{{"chatgpt_account_id":"{account_id}","chatgpt_plan_type":"{plan_type}"}}}}"#
	);

	format!("header.{}.signature", URL_SAFE_NO_PAD.encode(payload))
}

fn secure_directory(path: &Path) {
	fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("secure fixture directory");
}

fn secure_write(path: &Path, body: &str) {
	let mut file = File::create(path).expect("create fixture file");
	file.write_all(body.as_bytes()).expect("write fixture file");
	file.sync_all().expect("sync fixture file");
	fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("secure fixture file");
}

fn write_executable(path: &Path, body: &str) {
	fs::write(path, body).expect("write fixture executable");
	fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("make fixture executable");
}
