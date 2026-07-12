//! Trusted authority mutation broker boundary.

mod identity;

pub(crate) struct AuthorityBrokerSeal(());
impl AuthorityBrokerSeal {
	fn new() -> Self {
		Self(())
	}
}

#[cfg(test)]
pub(crate) use identity::test_invocation_identity;
