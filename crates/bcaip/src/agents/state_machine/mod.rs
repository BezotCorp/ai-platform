//! Runs an ordered, re-entrant pipeline over persisted conversation state.
//!
//! Callers persist incoming messages, construct `Step`s from their own operations,
//! and choose whether to call `StateMachine::step`, `StateMachine::apply`, or
//! `StateMachine::run`. Bcaip's concrete operations remain internal because their
//! configuration is part of `Agent::reply`, not the state-machine protocol.
//mod.rs need to have only module declarations and public exports. So review and extract
mod effects;
mod inference_preparation;
mod ops_bang_shell;
mod ops_compaction;
mod ops_doctor;
mod ops_entry_hook;
mod ops_exit_on_error;
mod ops_llm;
mod ops_maxturns;
mod ops_project;
mod ops_recipe;
mod ops_retry;
mod ops_skills;
mod ops_slash_command;
mod ops_status;
mod ops_steer;
mod ops_stop_hook;
mod ops_tool_approval;
mod ops_tool_pair_compaction;
mod ops_toolcalling;
mod ops_unknown_tool;
mod session;
mod tool_confirmation;
mod usage;

pub use effects::BcaipEffect;
pub(crate) use tool_confirmation::{
    has_unapplied_tool_confirmation_response, pending_tool_confirmations,
    persist_tool_confirmation_decision,
};

pub(crate) use inference_preparation::BcaipInferenceRequestPreparer;
pub(crate) use ops_bang_shell::BangShellOperation;
pub(crate) use ops_compaction::CompactionOperation;
pub(crate) use ops_doctor::DoctorOperation;
pub(crate) use ops_entry_hook::EntryHookOperation;
pub(crate) use ops_exit_on_error::ExitOnErrorOperation;
pub(crate) use ops_llm::BcaipInferenceProvider;
pub(crate) use ops_maxturns::{MAX_TURNS_MESSAGE, MaxTurnsOperation};
pub(crate) use ops_project::ProjectOperation;
pub(crate) use ops_recipe::RecipeOperation;
pub(crate) use ops_retry::RetryOperation;
pub(crate) use ops_skills::SkillOperation;
pub(crate) use ops_slash_command::SlashCommandOperation;
pub(crate) use ops_status::StatusOperation;
pub(crate) use ops_steer::{SteerOperation, SteerQueue};
pub(crate) use ops_stop_hook::StopHookOperation;
pub(crate) use ops_tool_approval::ToolApprovalOperation;
pub(crate) use ops_tool_pair_compaction::ToolPairCompactionOperation;
pub(crate) use ops_toolcalling::ToolExecutionOperation;
pub(crate) use ops_unknown_tool::UnknownToolOperation;
pub(crate) use session::run_bcaip;

pub fn enabled() -> bool {
    std::env::var("BCAIP_STATE_MACHINE")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
        .unwrap_or(false)
}
