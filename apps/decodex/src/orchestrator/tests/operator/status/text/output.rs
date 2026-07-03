mod operator_status_output_tests {
	use crate::orchestrator;
	use std::io::{Error, ErrorKind, Result, Write};

	struct BrokenPipeWriter;

	struct FlushBrokenPipeWriter;

	impl Write for BrokenPipeWriter {
		fn write(&mut self, _buffer: &[u8]) -> Result<usize> {
			Err(Error::from(ErrorKind::BrokenPipe))
		}

		fn flush(&mut self) -> Result<()> {
			Ok(())
		}
	}

	impl Write for FlushBrokenPipeWriter {
		fn write(&mut self, buffer: &[u8]) -> Result<usize> {
			Ok(buffer.len())
		}

		fn flush(&mut self) -> Result<()> {
			Err(Error::from(ErrorKind::BrokenPipe))
		}
	}

	#[test]
	fn operator_status_output_accepts_closed_downstream_pipe() {
		let mut writer = BrokenPipeWriter;

		orchestrator::write_cli_output(&mut writer, "partial status output\n")
			.expect("broken stdout pipe should be accepted");

		let mut writer = FlushBrokenPipeWriter;

		orchestrator::write_cli_output(&mut writer, "buffered status output\n")
			.expect("broken stdout flush should be accepted");
	}
}
