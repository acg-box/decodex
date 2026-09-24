use std::{io, process::Stdio, time::Duration};

use tokio::{
	io::{AsyncBufReadExt as _, BufReader},
	process::Command,
	time::timeout,
};

use super::signal_group;

#[tokio::test]
async fn denied_group_signal_cleans_members_and_keeps_escalation_available() {
	let mut child = Command::new("/bin/sh")
		.args(["-c", "trap '' TERM; /bin/sleep 10 & echo $!; wait; read line"])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.kill_on_drop(true)
		.process_group(0)
		.spawn()
		.unwrap();
	let group = i32::try_from(child.id().unwrap()).unwrap();
	let line = timeout(
		Duration::from_secs(3),
		BufReader::new(child.stdout.take().unwrap()).lines().next_line(),
	)
	.await
	.unwrap()
	.unwrap()
	.unwrap();
	let member: i32 = line.parse().unwrap();
	let error =
		signal_group(group, libc::SIGTERM, |_, _| Err(io::Error::from_raw_os_error(libc::EPERM)))
			.unwrap_err();
	assert_eq!(error.raw_os_error(), Some(libc::EPERM));
	let mut outsider = Command::new("/bin/sleep").arg("10").kill_on_drop(true).spawn().unwrap();
	for signal in [libc::SIGTERM, libc::SIGKILL] {
		let mut targets = Vec::new();
		signal_group(group, signal, |pid, signal| {
			targets.push(pid);
			if pid == -group || pid == group {
				return Err(io::Error::from_raw_os_error(libc::EPERM));
			}
			assert_eq!(pid, member);
			// SAFETY: this PID is our fixture's current group member.
			if unsafe { libc::kill(pid, signal) } == 0 {
				Ok(())
			} else {
				Err(io::Error::last_os_error())
			}
		})
		.unwrap();
		assert_eq!(targets, [-group, member, group]);
		assert!(outsider.try_wait().unwrap().is_none());
		if signal == libc::SIGTERM {
			assert!(child.try_wait().unwrap().is_none());
			// SAFETY: signal zero only checks whether the resistant member still exists.
			assert_eq!(unsafe { libc::kill(member, 0) }, 0);
		}
	}
	drop(child.stdin.take());
	timeout(Duration::from_secs(3), child.wait()).await.unwrap().unwrap();
	outsider.kill().await.unwrap();
}

#[test]
fn absent_groups_and_non_permission_errors_do_not_trigger_member_signals() {
	for result in [Ok(()), Err(libc::ESRCH), Err(libc::EINVAL)] {
		let mut calls = Vec::new();
		let observed = signal_group(2, libc::SIGTERM, |pid, _| {
			calls.push(pid);
			result.map_err(io::Error::from_raw_os_error)
		});
		assert_eq!(observed.map_err(|error| error.raw_os_error().unwrap()), result);
		assert_eq!(calls, [-2]);
	}
	for group in [0, 1, u32::MAX] {
		assert!(super::super::signal_owned_process_group_id(group, libc::SIGTERM).is_err());
	}
}
