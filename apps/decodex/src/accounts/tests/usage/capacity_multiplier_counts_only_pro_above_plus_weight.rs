use crate::accounts::{self};

#[test]
fn capacity_multiplier_counts_only_pro_above_plus_weight() {
	assert_eq!(accounts::account_capacity_multiplier(Some("pro")), 20);
	assert_eq!(accounts::account_capacity_multiplier(Some("plus")), 1);
	assert_eq!(accounts::account_capacity_multiplier(Some("team")), 1);
	assert_eq!(accounts::account_capacity_multiplier(None), 1);
}
