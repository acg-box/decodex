use std::{fs::File, io, thread, time::Duration};

use anyhow::{Context, Result};
use reqwest_webrtc::{StatusCode, blocking};

pub(crate) fn archive(url: &str, output: &mut File) -> Result<()> {
	for attempt in 1..=3 {
		let response = blocking::get(url);
		let retry = match &response {
			Ok(response) =>
				response.status().is_server_error()
					|| response.status() == StatusCode::TOO_MANY_REQUESTS,
			Err(_) => true,
		};

		if retry && attempt < 3 {
			println!(
				"cargo:warning=WebRTC download attempt {attempt} failed; retrying in 5 seconds"
			);

			thread::sleep(Duration::from_secs(5));

			continue;
		}

		let mut response = response.context("WebRTC archive request failed")?;

		if response.status() != StatusCode::OK {
			anyhow::bail!("WebRTC archive request returned {}", response.status());
		}

		io::copy(&mut response, output).context("cannot write WebRTC archive")?;

		return Ok(());
	}

	anyhow::bail!("WebRTC archive request failed")
}
