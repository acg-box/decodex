//! Operator HTTP request, route, operator WebSocket session, and event types.

pub(super) mod dashboard_events;
pub(super) mod requests;
pub(super) mod routes;
pub(super) mod websocket;

pub(super) use self::dashboard_events::DashboardClientSubscription;
