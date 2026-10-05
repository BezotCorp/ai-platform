//! Peer-to-peer roaming transport for goose agents.
//!
//! This crate lets a goose agent expose itself over the internet via
//! [iroh](https://iroh.computer) so that a remote ACP client (another goose, or
//! any other ACP client) can connect to and drive it through a
//! relay, with no open ports.
//!
//! # Building blocks
//!
//! * [`RoamingIdentity`] — a persisted ed25519 node key whose public half is
//!   the iroh endpoint id (self-certifying at the QUIC-TLS handshake).
//! * [`ConnectionCard`] — a non-secret, shareable string carrying a node's
//!   public key + relay URLs (plus a short fingerprint for out-of-band
//!   verification). It never expires and grants nothing on its own.
//! * [`TrustBook`] — the local, mutual allowlist: which peer keys this node
//!   accepts, plus revocations. Access exists only by accepting a key; there is
//!   no bearer token. An accepted peer gets goose's full ACP surface.
//! * [`RoamingNode`] — owns the iroh endpoint + router, hosts agents over the
//!   `goose-acp/1` ALPN, and dials remote agents.
//!
//! The crate deliberately knows nothing about goose's agent internals: hosting
//! is driven through the [`AcpStreamServer`] trait, which the integration layer
//! implements by calling goose's generic `acp::server::serve`. This keeps the
//! heavy iroh dependency out of the `goose` core crate entirely.

mod connection_card;
mod directory;
mod error;
mod frame;
mod handshake;
mod host_ack;
mod identity;
mod peerbook;
mod relay;
mod roaming_client_stream;
mod roaming_config;
mod roaming_node;
mod trust;

pub use connection_card::ConnectionCard;
pub use directory::{Direction, Directory, PeerEntry};
#[doc(inline)]
pub use iroh::EndpointId;

/// Parse an [`EndpointId`] (a peer's public key) from its string form.
pub fn parse_endpoint_id(s: &str) -> Result<EndpointId, RoamingError> {
    s.parse()
        .map_err(|e| RoamingError::Identity(format!("invalid endpoint id `{s}`: {e}")))
}
pub use error::RoamingError;
pub use identity::{RoamingIdentity, default_key_path};
pub use peerbook::{PeerBook, PeerRecord};
pub use relay::{RelayEntry, RelaySettings};
pub use roaming_client_stream::RoamingClientStream;
pub use roaming_config::RoamingConfig;
pub use roaming_node::{AcpStreamServer, ROAMING_ACP_ALPN, RoamingNode};
pub use trust::TrustBook;
