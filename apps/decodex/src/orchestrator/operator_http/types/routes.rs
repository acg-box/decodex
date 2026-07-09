#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::orchestrator::operator_http) enum OperatorRequestRoute {
	DashboardWs,
	Live,
	AppSnapshot,
	LinearScan,
	LaneInspect,
	LaneInterrupt,
	LaneSteer,
	AccountList { force_refresh: bool },
	AccountSelect,
	AccountClear,
	AccountLogout,
	AccountImport,
	AccountUse,
	AccountRerollName,
}
