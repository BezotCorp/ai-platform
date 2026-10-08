/// Host's response to a [`ClientHello`] from the client.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HostAck {
    /// Connection accepted; the client gets BCAIP's full ACP surface.
    Accepted { agent_id: String },
    /// Connection refused with a coarse reason code.
    Rejected { code: String },
}
