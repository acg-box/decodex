mod completion;
mod dispatch;
mod manual_attention;
mod progress_checkpoint;
mod review_checkpoint;
mod review_checkpoint_flow;
mod tool_specs;

pub(super) const REVIEW_COMPLETION_INTENT_EVENT_TYPE: &str = "review_completion_intent";

const COMMENT_KIND_MANUAL_ATTENTION: &str = "manual_attention";
const MANUAL_ATTENTION_TERMINAL_PATH: &str = "manual_attention";
const TERMINAL_FINALIZE_EVENT_TYPE: &str = "terminal_finalize";
