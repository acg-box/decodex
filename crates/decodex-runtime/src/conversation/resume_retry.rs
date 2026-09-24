//! Retry only exact, rejected native resumes; each attempt rechecks process ownership.
use super::{ConversationProcessError, ConversationRejectionReason};
use std::{future::Future, time::Duration};

pub(super) async fn retry<T, F, Fut>(mut attempt: F) -> Result<T, ConversationProcessError>
where
	F: FnMut() -> Fut,
	Fut: Future<Output = Result<T, ConversationProcessError>>,
{
	let mut result = attempt().await;
	for delay in [1, 2, 4, 8] {
		if !matches!(
			&result,
			Err(ConversationProcessError::Rejected {
				reason: ConversationRejectionReason::ClosingThread,
				..
			})
		) {
			break;
		}
		// No supervisor lock is held while waiting. The closure fences each send.
		tokio::time::sleep(Duration::from_secs(delay)).await;
		result = attempt().await;
	}
	result
}

#[cfg(test)]
mod tests {
	use super::{ConversationProcessError, ConversationRejectionReason, retry};
	use std::cell::Cell;

	fn closing() -> ConversationProcessError {
		ConversationProcessError::Rejected {
			witness_digest: "a".repeat(64),
			reason: ConversationRejectionReason::ClosingThread,
		}
	}

	#[tokio::test]
	async fn resumes_after_closing_but_preserves_final_failure_without_more_sends() {
		for end in [
			Ok("same-thread"),
			Err(ConversationProcessError::Unavailable),
			Err(ConversationProcessError::ControlLost),
			Err(ConversationProcessError::Ambiguous {
				request_id: 42,
				request_sha256: "b".repeat(64),
			}),
			Err(ConversationProcessError::Rejected {
				witness_digest: "c".repeat(64),
				reason: ConversationRejectionReason::Other,
			}),
		] {
			let count = Cell::new(0);
			let result = retry(|| {
				let n = count.get();
				count.set(n + 1);
				std::future::ready(if n == 0 { Err(closing()) } else { end.clone() })
			})
			.await;
			assert_eq!(result, end);
			assert_eq!(count.get(), 2);
		}
	}

	#[tokio::test]
	async fn persistent_closing_has_five_attempt_limit_and_retains_last_witness() {
		let count = Cell::new(0);
		let result: Result<(), _> = retry(|| {
			count.set(count.get() + 1);
			std::future::ready(Err(ConversationProcessError::Rejected {
				witness_digest: format!("{:064x}", count.get()),
				reason: ConversationRejectionReason::ClosingThread,
			}))
		})
		.await;
		assert_eq!(count.get(), 5);
		assert_eq!(
			result,
			Err(ConversationProcessError::Rejected {
				witness_digest: format!("{:064x}", 5),
				reason: ConversationRejectionReason::ClosingThread,
			})
		);
	}
}
