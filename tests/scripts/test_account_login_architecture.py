"""Architecture gates for daemon-owned ephemeral account login."""

from pathlib import Path
import tomllib
import unittest


ROOT = Path(__file__).resolve().parents[2]


def read(path: str) -> str:
	return (ROOT / path).read_text(encoding="utf-8")


def toml(path: str) -> dict[str, object]:
	with (ROOT / path).open("rb") as source:
		return tomllib.load(source)


class AccountLoginArchitectureTests(unittest.TestCase):
	"""Protect one runtime owner and one transient protocol surface."""

	def test_plain_owner_crate_is_used_only_by_the_runtime(self) -> None:
		workspace = toml("Cargo.toml")["workspace"]
		self.assertIn("crates/decodex-account-login", workspace["members"])
		owner = toml("crates/decodex-account-login/Cargo.toml")
		self.assertEqual(owner["package"]["name"], "decodex-account-login")
		self.assertNotIn("crate-type", owner.get("lib", {}))
		self.assertIn(
			"decodex-account-login",
			toml("crates/decodex-runtime/Cargo.toml")["dependencies"],
		)
		for manifest in (
			"crates/decodex-app-client-ffi/Cargo.toml",
			"apps/decodex-gpui/Cargo.toml",
			"crates/decodex-protocol/Cargo.toml",
		):
			with self.subTest(manifest=manifest):
				self.assertNotIn(
					"decodex-account-login", toml(manifest)["dependencies"]
				)

	def test_provider_engine_and_temporary_home_left_the_ffi(self) -> None:
		ffi = ROOT / "crates/decodex-app-client-ffi"
		self.assertFalse((ffi / "src/source_login_adapter.rs").exists())
		self.assertFalse((ffi / "src/account_reauthentication.rs").exists())
		bridge = read("crates/decodex-app-client-ffi/src/lib.rs")
		for forbidden in (
			"source_login_adapter",
			"LoginHome",
			"auth.json",
			"source_descriptor",
			"credential_path",
			"EnrollAccountFromCredentialFile",
			"ReauthenticateAccountFromCredentialFile",
			"callback_ports",
			"std::thread",
		):
			with self.subTest(marker=forbidden):
				self.assertNotIn(forbidden, bridge)
		dependencies = toml("crates/decodex-app-client-ffi/Cargo.toml")["dependencies"]
		for provider_dependency in (
			"base64",
			"getrandom",
			"httparse",
			"reqwest",
			"sha2",
			"time",
			"url",
			"zeroize",
		):
			with self.subTest(dependency=provider_dependency):
				self.assertNotIn(provider_dependency, dependencies)

	def test_owner_pins_provenance_license_bounds_and_forbidden_surfaces(self) -> None:
		owner_root = ROOT / "crates/decodex-account-login"
		production = "\n".join(
			path.read_text(encoding="utf-8").split("\n#[cfg(test)]\nmod tests", 1)[0]
			for path in sorted((owner_root / "src").glob("*.rs"))
		)
		for required in (
			"9392c3fa5bcda342b5b96a1a04d67b2f781617c2",
			"login/src/pkce.rs: generate_pkce",
			"login/src/server.rs: build_authorize_url, exchange_code_for_tokens",
			"login/src/device_code_auth.rs: request_device_code",
			"login/src/auth/storage.rs: FileAuthStorage::save",
			"MAX_CALLBACK_REQUEST_BYTES",
			"MAX_CALLBACK_HEADERS",
			"cleanup_stale_login_homes",
		):
			with self.subTest(marker=required):
				self.assertIn(required, production)
		for forbidden in (
			"std::process",
			"Command::new",
			"codex_bin",
			"openpty",
			"PseudoTerminal",
			"--device-auth",
			"println!",
			"eprintln!",
			"dbg!",
			"tracing::",
		):
			with self.subTest(marker=forbidden):
				self.assertNotIn(forbidden, production)
		notice = read("crates/decodex-account-login/THIRD_PARTY_NOTICES.md")
		license_text = read(
			"crates/decodex-account-login/third_party/openai-codex-LICENSE-APACHE"
		)
		self.assertIn("rust-v0.148.0-alpha.9", notice)
		self.assertIn("9392c3fa5bcda342b5b96a1a04d67b2f781617c2", notice)
		self.assertIn("Apache License", license_text)

	def test_runtime_owns_one_singleton_manager_and_account_service_install(self) -> None:
		bootstrap = read("crates/decodex-runtime/src/bootstrap.rs")
		runtime = read("crates/decodex-runtime/src/account_login.rs")
		self.assertIn("AccountLoginManager", bootstrap)
		self.assertIn("AccountLoginManager", runtime)
		self.assertIn("AccountService", runtime)
		self.assertIn("begin_shutdown", runtime)
		self.assertIn("wait_for_shutdown", runtime)
		self.assertNotIn("pub trait", runtime)

	def test_protocol_login_surface_is_dedicated_and_not_durable(self) -> None:
		protocol = read("crates/decodex-protocol/src/account_login.rs")
		wire = read("crates/decodex-protocol/src/wire.rs")
		production_wire = wire.split("\n#[cfg(test)]\nmod tests", 1)[0]
		client = read("crates/decodex-protocol/src/client.rs")
		for required in (
			"AccountLoginStart",
			"AccountLoginStatus",
			"AccountLoginRequest::Start",
			"AccountLoginRequest::Status",
			"AccountLoginRequest::Cancel",
		):
			with self.subTest(marker=required):
				self.assertIn(required, protocol)
		self.assertIn("ClientMessage::AccountLogin", client)
		self.assertIn("close_one_shot_socket", client)
		for retired_ingress in (
			"EnrollAccountFromCredentialFile",
			"ReauthenticateAccountFromCredentialFile",
		):
			with self.subTest(retired_ingress=retired_ingress):
				self.assertNotIn(retired_ingress, production_wire)
				self.assertNotIn(retired_ingress, client)
		self.assertIn(
			"retired_credential_file_command_names_do_not_decode",
			wire,
		)
		for durable_surface in ("SnapshotItem", "EventPayload", "CommandPayload"):
			block_start = wire.index(f"pub enum {durable_surface}")
			block_end = wire.find("\npub ", block_start + 1)
			block = wire[block_start:] if block_end == -1 else wire[block_start:block_end]
			with self.subTest(surface=durable_surface):
				self.assertNotIn("AccountLogin", block)
		for path in (ROOT / "database").rglob("*"):
			if path.is_file():
				self.assertNotIn(
					"account_login_session",
					path.read_text(encoding="utf-8", errors="ignore"),
					str(path.relative_to(ROOT)),
				)

	def test_both_desktop_frontends_are_protocol_only(self) -> None:
		ffi = read("crates/decodex-app-client-ffi/src/lib.rs")
		gpui = read("apps/decodex-gpui/src/account_login.rs")
		for source in (ffi, gpui):
			self.assertIn("AccountLoginClient", source)
			for forbidden in (
				"reqwest",
				"TcpListener",
				"auth.json",
				"OAUTH_CLIENT_ID",
				"LoginHome",
			):
				with self.subTest(marker=forbidden):
					self.assertNotIn(forbidden, source)


if __name__ == "__main__":
	unittest.main()
