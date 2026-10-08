//! A **connection card**: the single, non-secret string you share with another
//! node so it can find and identify you.
//!
//! A card carries only:
//! * the node's **identity** (its public key, which in iroh *is* its endpoint
//!   id), and
//! * how to **reach** it (relay URLs — the connective tissue; a node registers
//!   with these relays, which forward by node id regardless of the node's
//!   current IP/NAT).
//!
//! There is nothing secret in a card. Possessing one grants no access: iroh's
//! QUIC-TLS handshake proves a peer holds the private key for the identity in
//! the card (so it cannot be impersonated), and a connection is only authorized
//! if the peer's key is on the *other* side's allowlist. Trust is therefore a
//! mutual, public-key relationship — you exchange cards, and each side chooses
//! to accept the other. This is the WireGuard / SSH-known-hosts model.
//!
//! A card is versioned but deliberately **not** a capability token: it never
//! expires and confers no permission on its own.

use base64::Engine;
use iroh::{EndpointAddr, EndpointId, TransportAddr};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::RoamingError;
const CARD_VERSION: u32 = 1;
const CARD_SCHEME: &str = "bcaip+roam://";

/// Decode bounds, shared by the native and wasm decoders. A card is a tiny
/// identity+relay-list blob; anything near these limits is garbage or an
/// attack, and the caps stop allocation before it starts.
const MAX_CARD_TEXT_BYTES: usize = 8 * 1024;
const MAX_RELAY_URLS: usize = 16;
const MAX_RELAY_URL_BYTES: usize = 512;

/// A shareable, non-secret identity-plus-reachability card for a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionCard {
    pub version: u32,
    /// The node's public key (== its iroh endpoint id).
    pub endpoint_id: EndpointId,
    /// Relay URLs the node registers with; how peers reach it across network
    /// changes. May be empty for a LAN-only / directly-reachable node.
    pub relay_urls: Vec<String>,
}

impl ConnectionCard {
    pub fn new(endpoint_id: EndpointId, relay_urls: Vec<String>) -> Self {
        Self {
            version: CARD_VERSION,
            endpoint_id,
            relay_urls,
        }
    }

    /// A short, human-comparable fingerprint of the identity, for out-of-band
    /// verification ("does the code you added end in `…3f9a`?"). Derived from
    /// the public key, so it is stable and needs no secret.
    pub fn fingerprint(&self) -> String {
        let digest = Sha256::digest(self.endpoint_id.as_bytes());
        // First 16 bytes (128 bits) -> eight 4-hex-char groups. 48 bits was
        // brute-forceable for a second-preimage against a human comparing
        // out of band; 128 bits is not.
        digest[..16]
            .chunks(2)
            .map(|pair| format!("{:02x}{:02x}", pair[0], pair[1]))
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Encode to a compact, URL-safe string with the `bcaip+roam://` scheme.
    pub fn encode(&self) -> Result<String, RoamingError> {
        let json = serde_json::to_vec(self)
            .map_err(|e| RoamingError::Card(format!("encode card: {e}")))?;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json);
        Ok(format!("{CARD_SCHEME}{b64}"))
    }

    /// Decode a card produced by [`Self::encode`].
    ///
    /// This is the card-decoding contract, applied identically by the native
    /// and wasm decoders: bounded input (text/relay count/URL length caps
    /// before allocation), required `version == 1`, and http(s)-only relay
    /// URLs validated at decode time — a malformed card is rejected here, not
    /// when dialing.
    pub fn decode(text: &str) -> Result<Self, RoamingError> {
        let text = text.trim();
        if text.len() > MAX_CARD_TEXT_BYTES {
            return Err(RoamingError::Card("card too large".into()));
        }
        let b64 = text
            .strip_prefix(CARD_SCHEME)
            .ok_or_else(|| RoamingError::Card(format!("missing {CARD_SCHEME} scheme")))?;
        let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(b64)
            .map_err(|e| RoamingError::Card(format!("x base64: {e}")))?;
        let card: ConnectionCard = serde_json::from_slice(&json)
            .map_err(|e| RoamingError::Card(format!("x card: {e}")))?;
        if card.version != CARD_VERSION {
            return Err(RoamingError::Card(format!(
                "unsupported card version {}",
                card.version
            )));
        }
        if card.relay_urls.len() > MAX_RELAY_URLS {
            return Err(RoamingError::Card("too many relay urls".into()));
        }
        for url in &card.relay_urls {
            if url.len() > MAX_RELAY_URL_BYTES {
                return Err(RoamingError::Card("relay url too long".into()));
            }
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err(RoamingError::Card(format!(
                    "relay url must be http(s): {url}"
                )));
            }
        }
        Ok(card)
    }

    /// Build a dialable [`EndpointAddr`] from the card (id + relay URLs).
    pub(crate) fn endpoint_addr(&self) -> Result<EndpointAddr, RoamingError> {
        let mut addr = EndpointAddr::new(self.endpoint_id);
        for url in &self.relay_urls {
            // Relay URLs come from an untrusted card. Constrain the scheme to
            // http(s) so a malicious card can't smuggle some other URL scheme
            // into the dialer. (iroh relays are HTTP(S) endpoints.)
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err(RoamingError::Card(format!(
                    "relay url must be http(s): {url}"
                )));
            }
            let parsed = url
                .parse()
                .map_err(|_| RoamingError::Card(format!("x relay url {url}")))?;
            addr.addrs.insert(TransportAddr::Relay(parsed));
        }
        Ok(addr)
    }
}
