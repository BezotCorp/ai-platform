use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for the load_tutorial tool
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct LoadTutorialParams {
    /// Name of the tutorial to load, e.g. 'getting-started' or 'developer-mcp'
    pub name: String,
}
