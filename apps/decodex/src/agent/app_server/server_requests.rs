//! App-server server request routing and non-interactive response handling.

mod dispatch;
mod recording;
mod rejection;

#[cfg(test)]
pub(super) use self::recording::{record_interactive_request_state, record_server_request};
pub(super) use self::{
	dispatch::{handle_server_request_during_turn_execution, handle_server_request_while_waiting},
	recording::{
		apply_protocol_message_side_effects, interactive_flag_for_request,
		record_server_request_response,
	},
};
