//! Kernel-backed acceptance tests for the local transport namespace.

#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(unused_crate_dependencies)]

use std::{
	fs,
	os::unix::{
		fs::{MetadataExt as _, PermissionsExt as _, symlink},
		net::UnixListener as StandardUnixListener,
	},
	path::PathBuf,
};

use decodex_core::{DecodexRoot, LocalTrustPolicy};
use decodex_protocol::{LocalTransportAuthority, LocalTransportRefusal};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

fn fixture() -> (TempDir, LocalTransportAuthority, PathBuf) {
	let temporary = TempDir::new().expect("create local transport fixture");
	let root = DecodexRoot::new(
		temporary.path().canonicalize().expect("canonicalize fixture").join(".decodex"),
	)
	.expect("create typed fixture root");
	let paths = root.paths();

	paths.ensure_layout().expect("create private fixture layout");

	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };
	let authority =
		LocalTransportAuthority::new(paths.clone(), LocalTrustPolicy::SameUid, Some(effective_uid))
			.expect("create same-UID authority");

	(temporary, authority, paths.local_transport_socket())
}

#[tokio::test]
async fn first_publication_creates_only_its_private_namespace_parent() {
	let temporary = TempDir::new().expect("create first-publication fixture");
	let root = DecodexRoot::new(
		temporary.path().canonicalize().expect("canonicalize fixture").join(".decodex"),
	)
	.expect("create typed fixture root");
	let paths = root.paths();
	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };
	let authority =
		LocalTransportAuthority::new(paths.clone(), LocalTrustPolicy::SameUid, Some(effective_uid))
			.expect("create same-UID authority");

	assert!(!root.as_path().exists());

	let listener = authority.bind().await.expect("create first transport publication");

	assert_eq!(
		fs::metadata(root.as_path()).expect("read created root").permissions().mode() & 0o777,
		0o700,
	);
	assert_eq!(
		fs::metadata(paths.server_dir())
			.expect("read created server directory")
			.permissions()
			.mode() & 0o777,
		0o700,
	);
	assert!(!paths.server_identity_file().exists());
	assert!(!paths.logs_dir().exists());
	assert!(!paths.cache_dir().exists());

	listener.cleanup().expect("clean first publication");
}

#[tokio::test]
async fn publication_is_private_exclusive_and_reusable_after_exact_cleanup() {
	let (_temporary, authority, socket_path) = fixture();
	let mut listener = authority.bind().await.expect("publish local endpoint");
	let server_dir = socket_path.parent().expect("socket has server directory");
	let lock_path = server_dir.join("decodex.lock");
	let directory = fs::metadata(server_dir).expect("read server directory metadata");
	let socket = fs::symlink_metadata(&socket_path).expect("read socket metadata");
	let lock = fs::metadata(&lock_path).expect("read namespace lock metadata");
	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };

	assert_eq!(directory.permissions().mode() & 0o777, 0o700);
	assert_eq!(socket.permissions().mode() & 0o777, 0o600);
	assert_eq!(lock.permissions().mode() & 0o777, 0o600);
	assert_eq!(directory.uid(), effective_uid);
	assert_eq!(socket.uid(), effective_uid);
	assert_eq!(lock.uid(), effective_uid);
	assert_eq!(socket.nlink(), 1);
	assert_eq!(lock.nlink(), 1);

	assert_eq!(
		authority.bind().await.expect_err("second daemon must not acquire namespace"),
		LocalTransportRefusal::EndpointInUse
	);

	let client = authority.connect();
	let server = listener.accept();
	let (client, server) = tokio::join!(client, server);
	let mut client = client.expect("admit kernel-authenticated server peer");
	let mut server = server.expect("admit kernel-authenticated client peer");

	client.write_all(b"same-uid").await.expect("write through local stream");
	let mut message = [0_u8; 8];
	server.read_exact(&mut message).await.expect("read through local stream");
	assert_eq!(&message, b"same-uid");

	drop(client);
	drop(server);
	listener.cleanup().expect("clean exact publication");

	assert!(!socket_path.exists());
	assert!(lock_path.is_file(), "persistent namespace lock must remain");

	let replacement = authority.bind().await.expect("reuse namespace after cleanup");
	replacement.cleanup().expect("clean replacement publication");
}

#[tokio::test]
async fn executable_stale_stage_and_canonical_sockets_are_recovered() {
	for name in ["decodex.sock.stage", "decodex.sock"] {
		let (_temporary, authority, socket_path) = fixture();
		let stale_path = socket_path.parent().expect("socket has parent").join(name);
		let stale = StandardUnixListener::bind(&stale_path).expect("bind stale fixture socket");

		fs::set_permissions(&stale_path, fs::Permissions::from_mode(0o600))
			.expect("scope stale fixture socket");
		drop(stale);

		assert!(stale_path.exists());

		let listener = authority.bind().await.expect("recover provably stale socket");

		assert!(!socket_path.with_file_name("decodex.sock.stage").exists());
		assert!(socket_path.exists());

		listener.cleanup().expect("clean recovered publication");
	}
}

#[tokio::test]
async fn endpoint_replacement_is_reported_and_never_unlinked_by_cleanup() {
	let (_temporary, authority, socket_path) = fixture();
	let listener = authority.bind().await.expect("publish retained endpoint");
	let retained_copy = socket_path.with_file_name("retained.sock");

	fs::rename(&socket_path, &retained_copy).expect("move retained endpoint aside");

	let replacement =
		StandardUnixListener::bind(&socket_path).expect("publish replacement endpoint");

	fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
		.expect("scope replacement endpoint");

	assert_eq!(listener.revalidate(), Err(LocalTransportRefusal::EndpointReplaced));
	assert_eq!(listener.cleanup(), Err(LocalTransportRefusal::EndpointReplaced));
	assert!(socket_path.exists(), "cleanup must preserve an unowned replacement");

	drop(replacement);
	fs::remove_file(&socket_path).expect("remove temporary replacement");
	fs::remove_file(&retained_copy).expect("remove moved retained endpoint");
}

#[tokio::test]
async fn unsafe_namespace_entries_fail_closed() {
	let (_temporary, authority, socket_path) = fixture();
	let target = socket_path.with_file_name("not-a-socket");

	fs::write(&target, b"fixture").expect("write symlink target");
	symlink(&target, &socket_path).expect("create unsafe endpoint link");

	assert_eq!(
		authority.bind().await.expect_err("linked endpoint must be refused"),
		LocalTransportRefusal::UnsafeEndpoint
	);
	assert!(socket_path.is_symlink(), "refusal must preserve the unsafe entry");
}

#[tokio::test]
async fn missing_and_overlong_endpoints_fail_before_connect_or_publish() {
	let (_temporary, authority, _socket_path) = fixture();

	assert_eq!(
		authority.connect().await.expect_err("missing publication must be explicit"),
		LocalTransportRefusal::EndpointUnavailable
	);

	let temporary = TempDir::new().expect("create long-path fixture");
	let root = DecodexRoot::new(
		temporary
			.path()
			.canonicalize()
			.expect("canonicalize long-path fixture")
			.join("x".repeat(160)),
	)
	.expect("long root remains within product root bound");
	let paths = root.paths();

	paths.ensure_layout().expect("create long-path private layout");

	// SAFETY: `geteuid` has no arguments or failure return.
	let effective_uid = unsafe { libc::geteuid() };
	let authority =
		LocalTransportAuthority::new(paths, LocalTrustPolicy::SameUid, Some(effective_uid))
			.expect("construct long-path authority");

	assert_eq!(
		authority.bind().await.expect_err("overlong sockaddr path must fail closed"),
		LocalTransportRefusal::UnsafeEndpoint
	);
}
