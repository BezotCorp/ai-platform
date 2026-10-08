use crate::{Directory, RelaySettings, RoamingIdentity, TrustBook};

/// Configuration for binding a roaming node.
///
/// For the common case use [`RoamingConfig::new`] and the `with_*` chainers,
/// which default to iroh's public relays, an empty allowlist (accepts no
/// one), and an in-memory directory:
///
/// ```no_run
/// use bcaip_roaming::{RoamingConfig, RoamingIdentity, RoamingNode};
/// # async fn f() -> anyhow::Result<()> {
/// let node = RoamingNode::bind(RoamingConfig::new(RoamingIdentity::generate())).await?;
/// # Ok(()) }
/// ```
pub struct RoamingConfig {
    pub identity: RoamingIdentity,
    pub relay: RelaySettings,
    pub trust: TrustBook,
    /// Optional path to the persisted trust allowlist. When set, it is
    /// re-read on every inbound connection so `peers accept`/`revoke` from a
    /// separate process take effect against a running `share` without a
    /// restart. When `None` only the in-memory `trust` is consulted.
    pub trust_path: Option<std::path::PathBuf>,
    /// Directory used to track observed connections. Defaults to an in-memory
    /// directory; pass [`Directory::persistent`] to make `roam list` work from
    /// a separate process.
    pub directory: Directory,
    /// Optional explicit socket address to bind the QUIC endpoint to. When set
    /// with relays disabled, the default IP transports are cleared first so a
    /// single-family local path is used — iroh's multipath negotiation
    /// otherwise stalls (`MultipathNotNegotiated`) when both the specified IPv4
    /// and a default `[::]` IPv6 socket are candidates with no relay fallback.
    pub bind_addr: Option<std::net::SocketAddr>,
    /// Override the CA trust used for relay TLS. `None` (the default) uses
    /// the system roots. Tests pass `CaTlsConfig::insecure_skip_verify()`
    /// (available under iroh's `test-utils` feature) to run against a local
    /// self-signed relay.
    pub relay_tls: Option<iroh::tls::CaTlsConfig>,
}

impl RoamingConfig {
    /// A config for `identity` with sensible defaults: iroh's public relays,
    /// an empty trust allowlist (accepts no one until a peer key is accepted),
    /// an in-memory directory, and no explicit bind address.
    pub fn new(identity: RoamingIdentity) -> Self {
        Self {
            identity,
            relay: RelaySettings::N0Default,
            trust: TrustBook::new(),
            trust_path: None,
            directory: Directory::new(),
            bind_addr: None,
            relay_tls: None,
        }
    }

    /// Use a specific relay configuration (default: iroh's public relays).
    pub fn with_relay(mut self, relay: RelaySettings) -> Self {
        self.relay = relay;
        self
    }

    /// Bind the QUIC endpoint to a specific socket address.
    pub fn with_bind_addr(mut self, addr: std::net::SocketAddr) -> Self {
        self.bind_addr = Some(addr);
        self
    }
}
