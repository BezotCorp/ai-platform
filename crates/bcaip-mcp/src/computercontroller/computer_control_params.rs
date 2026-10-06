use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the computer_control tool (macOS — Peekaboo CLI passthrough)
#[cfg(target_os = "macos")]
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ComputerControlParams {
    /// The peekaboo subcommand and arguments as a single string.
    /// Examples:
    ///   "see --app Safari --annotate"
    ///   "click --on B1"
    ///   "type \"hello\" --return"
    ///   "hotkey --keys cmd,c"
    ///   "app launch Safari --open https://example.com"
    ///   "window list --app Safari --json"
    ///   "press tab --count 3"
    ///   "clipboard --action get"
    pub command: String,
    /// Whether to capture and return a screenshot as part of the result.
    /// Useful after click/type actions to see the updated UI state.
    #[serde(default)]
    pub capture_screenshot: bool,
}
