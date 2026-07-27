//! Apply the real immutable migration ledger through V26 for the XY-1422 boundary gate.

use std::{env, error::Error, path::PathBuf};

use decodex_postgres::PostgresStore;
use tokio_postgres::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let mut arguments = env::args().skip(1);
	let socket_directory = required_path(&mut arguments, "--socket-directory")?;
	let port = required_value(&mut arguments, "--port")?.parse::<u16>()?;
	let database = required_value(&mut arguments, "--database")?;
	let user = required_value(&mut arguments, "--user")?;
	let attempt_v27_without_handoff = match arguments.next().as_deref() {
		None => false,
		Some("--attempt-v27-without-handoff") => true,
		Some(_) => return Err("unexpected migration transition fixture argument".into()),
	};
	if arguments.next().is_some() {
		return Err("unexpected migration transition fixture argument".into());
	}

	let mut config = Config::new();
	config.host_path(socket_directory).port(port).dbname(&database).user(&user);
	let expected_peer_uid = unsafe { libc::geteuid() };
	if attempt_v27_without_handoff {
		PostgresStore::migrate(config, expected_peer_uid).await?;
	} else {
		PostgresStore::migrate_fixture_through_v26(config, expected_peer_uid).await?;
	}
	Ok(())
}

fn required_path(
	arguments: &mut impl Iterator<Item = String>,
	name: &str,
) -> Result<PathBuf, Box<dyn Error>> {
	required_value(arguments, name).map(PathBuf::from)
}

fn required_value(
	arguments: &mut impl Iterator<Item = String>,
	name: &str,
) -> Result<String, Box<dyn Error>> {
	if arguments.next().as_deref() != Some(name) {
		return Err(format!("missing {name}").into());
	}
	arguments.next().ok_or_else(|| format!("missing {name} value").into())
}
