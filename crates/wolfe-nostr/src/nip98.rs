//! NIP-98 HTTP Auth verification.
//!
//! Verifies `Authorization: Nostr <base64-encoded-event>` headers per
//! <https://github.com/nostr-protocol/nips/blob/master/98.md>.
//!
//! The event must:
//! - Have kind 27235
//! - Have a valid signature
//! - Have a `u` tag matching the request URL
//! - Have a `method` tag matching the HTTP method
//! - Have `created_at` within the allowed time window (default 60s)

use nostr_sdk::prelude::*;
use tracing::debug;

use crate::error::NostrError;

/// Maximum allowed age (in seconds) of a NIP-98 auth event.
const MAX_AUTH_AGE_SECS: u64 = 60;

/// NIP-98 kind constant.
const NIP98_KIND: u16 = 27235;

/// Verify a NIP-98 authorization header value.
///
/// `auth_value` is the raw header value after "Nostr " prefix.
/// `url` is the full request URL.
/// `method` is the HTTP method (GET, POST, etc.).
/// `allowed_pubkeys` is the set of pubkeys authorized to use RPC.
/// If empty, any valid NIP-98 event is accepted.
pub fn verify_nip98(
    auth_value: &str,
    url: &str,
    method: &str,
    allowed_pubkeys: &[PublicKey],
) -> Result<PublicKey, NostrError> {
    // Decode base64 to get event JSON
    use base64::Engine as _;
    let event_json = base64::engine::general_purpose::STANDARD
        .decode(auth_value.trim())
        .map_err(|e| NostrError::Nip98(format!("invalid base64: {}", e)))?;

    let event_str = String::from_utf8(event_json)
        .map_err(|e| NostrError::Nip98(format!("invalid UTF-8: {}", e)))?;

    // Parse the event
    let event: Event = Event::from_json(&event_str)
        .map_err(|e| NostrError::Nip98(format!("invalid event JSON: {}", e)))?;

    // Verify signature
    event
        .verify()
        .map_err(|e| NostrError::Nip98(format!("invalid signature: {}", e)))?;

    // Check kind
    if event.kind != Kind::Custom(NIP98_KIND) {
        return Err(NostrError::Nip98(format!(
            "wrong kind: expected {}, got {}",
            NIP98_KIND,
            event.kind.as_u16()
        )));
    }

    // Check timestamp (within MAX_AUTH_AGE_SECS)
    let now = Timestamp::now();
    let created = event.created_at;
    let diff = if now >= created {
        (now - created).as_u64()
    } else {
        (created - now).as_u64()
    };
    if diff > MAX_AUTH_AGE_SECS {
        return Err(NostrError::Nip98(format!(
            "event too old or in future: {}s drift (max {}s)",
            diff, MAX_AUTH_AGE_SECS
        )));
    }

    // Extract tag values by searching through the tag list.
    // In nostr 0.39, `event.tags` is a `Tags` struct with `.iter()`.
    let mut url_value: Option<String> = None;
    let mut method_value: Option<String> = None;

    for tag in event.tags.iter() {
        let parts = tag.as_slice();
        if parts.len() >= 2 {
            match parts[0].as_str() {
                "u" => url_value = Some(parts[1].clone()),
                "method" => method_value = Some(parts[1].clone()),
                _ => {}
            }
        }
    }

    // Check `u` tag matches URL
    match url_value {
        Some(ref u) if u == url => {}
        Some(ref u) => {
            return Err(NostrError::Nip98(format!(
                "URL mismatch: event has '{}', request is '{}'",
                u, url
            )));
        }
        None => {
            return Err(NostrError::Nip98("missing 'u' tag".to_string()));
        }
    }

    // Check `method` tag matches HTTP method
    match method_value {
        Some(ref m) if m.eq_ignore_ascii_case(method) => {}
        Some(ref m) => {
            return Err(NostrError::Nip98(format!(
                "method mismatch: event has '{}', request is '{}'",
                m, method
            )));
        }
        None => {
            return Err(NostrError::Nip98("missing 'method' tag".to_string()));
        }
    }

    let pubkey = event.pubkey;

    // Check pubkey is in the allowed list (if not empty)
    if !allowed_pubkeys.is_empty() && !allowed_pubkeys.contains(&pubkey) {
        return Err(NostrError::Nip98(format!(
            "pubkey {} not in allowed list",
            pubkey.to_bech32().unwrap_or_else(|_| pubkey.to_hex())
        )));
    }

    debug!(
        pubkey = %pubkey.to_bech32().unwrap_or_else(|_| pubkey.to_hex()),
        "NIP-98 auth verified"
    );

    Ok(pubkey)
}
