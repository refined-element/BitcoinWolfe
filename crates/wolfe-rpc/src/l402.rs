//! L402 Lightning-gated API access.
//!
//! Implements stateless HMAC-SHA256 tokens bound to Lightning payment hashes.
//! Flow:
//! 1. Client requests a gated endpoint without a token
//! 2. Server creates a Lightning invoice and an HMAC token, returns HTTP 402
//! 3. Client pays the invoice and re-requests with `Authorization: L402 <token>`
//! 4. Server verifies the HMAC and checks that the payment hash was claimed

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use bitcoin::hashes::hmac::{Hmac, HmacEngine};
use bitcoin::hashes::sha256;
use bitcoin::hashes::{Hash, HashEngine};
use tracing::{debug, warn};

use crate::server::NodeState;

/// Token layout: version(1) || payment_hash(32) || expiry(8) || hmac(32) = 73 bytes
const TOKEN_VERSION: u8 = 1;
const TOKEN_LEN: usize = 73;

/// Create an L402 token binding a payment hash to an expiry time.
pub fn create_token(secret: &[u8; 32], payment_hash: &[u8; 32], expiry: u64) -> String {
    let hmac = compute_hmac(secret, payment_hash, expiry);

    let mut buf = Vec::with_capacity(TOKEN_LEN);
    buf.push(TOKEN_VERSION);
    buf.extend_from_slice(payment_hash);
    buf.extend_from_slice(&expiry.to_be_bytes());
    buf.extend_from_slice(&hmac);

    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(&buf)
}

/// Verify an L402 token. Returns the payment hash if valid.
pub fn verify_token(secret: &[u8; 32], token_b64: &str) -> Result<[u8; 32], &'static str> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(token_b64)
        .map_err(|_| "invalid base64")?;

    if bytes.len() != TOKEN_LEN {
        return Err("invalid token length");
    }

    if bytes[0] != TOKEN_VERSION {
        return Err("unsupported token version");
    }

    let mut payment_hash = [0u8; 32];
    payment_hash.copy_from_slice(&bytes[1..33]);

    let mut expiry_bytes = [0u8; 8];
    expiry_bytes.copy_from_slice(&bytes[33..41]);
    let expiry = u64::from_be_bytes(expiry_bytes);

    let mut expected_hmac = [0u8; 32];
    expected_hmac.copy_from_slice(&bytes[41..73]);

    // Check expiry
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now > expiry {
        return Err("token expired");
    }

    // Verify HMAC
    let computed = compute_hmac(secret, &payment_hash, expiry);
    if computed != expected_hmac {
        return Err("invalid token signature");
    }

    Ok(payment_hash)
}

/// Derive the L402 token secret from the Lightning seed.
pub fn derive_l402_secret(ln_seed: &[u8; 32]) -> [u8; 32] {
    let mut engine = HmacEngine::<sha256::Hash>::new(ln_seed);
    engine.input(b"l402-token-key");
    let hmac = Hmac::<sha256::Hash>::from_engine(engine);
    let mut out = [0u8; 32];
    out.copy_from_slice(hmac.as_byte_array());
    out
}

fn compute_hmac(secret: &[u8; 32], payment_hash: &[u8; 32], expiry: u64) -> [u8; 32] {
    let mut engine = HmacEngine::<sha256::Hash>::new(secret);
    engine.input(payment_hash);
    engine.input(&expiry.to_be_bytes());
    let hmac = Hmac::<sha256::Hash>::from_engine(engine);
    let mut out = [0u8; 32];
    out.copy_from_slice(hmac.as_byte_array());
    out
}

/// Axum middleware that gates requests behind L402 Lightning payments.
pub async fn l402_middleware(state: Arc<NodeState>, req: Request, next: Next) -> Response {
    // If L402 is disabled, pass through
    if !state.l402_config.enabled {
        return next.run(req).await;
    }

    let secret = match state.l402_secret() {
        Some(s) => s,
        None => {
            warn!("L402 enabled but no secret configured");
            return next.run(req).await;
        }
    };

    // Check for Authorization: L402 <token> header
    let auth_header = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if let Some(ref header) = auth_header {
        if let Some(token) = header.strip_prefix("L402 ") {
            match verify_token(&secret, token) {
                Ok(payment_hash) => {
                    // Check if this payment hash was actually paid
                    if state.paid_invoices().contains_key(&payment_hash) {
                        debug!(
                            "L402 token verified for payment {}",
                            hex::encode(payment_hash)
                        );
                        return next.run(req).await;
                    } else {
                        return payment_required_response("invoice not yet paid");
                    }
                }
                Err(e) => {
                    return payment_required_response(e);
                }
            }
        }
    }

    // No valid token — create invoice and return 402
    let ln = match state.lightning() {
        Some(ln) => ln,
        None => {
            return Response::builder()
                .status(axum::http::StatusCode::SERVICE_UNAVAILABLE)
                .body(axum::body::Body::from("lightning not available"))
                .unwrap();
        }
    };

    let config = &state.l402_config;
    let amount_msat = config.price_sats * 1000;

    match ln.create_invoice(
        Some(amount_msat),
        &config.invoice_description,
        Some(config.invoice_expiry_secs),
    ) {
        Ok(invoice_str) => {
            // Extract payment hash from the BOLT11 invoice
            let payment_hash = extract_payment_hash(&invoice_str);

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let expiry = now + config.token_expiry_secs;

            let token = if let Some(ref ph) = payment_hash {
                create_token(&secret, ph, expiry)
            } else {
                String::new()
            };

            let www_auth = format!("L402 macaroon=\"{}\", invoice=\"{}\"", token, invoice_str);

            Response::builder()
                .status(axum::http::StatusCode::PAYMENT_REQUIRED)
                .header("WWW-Authenticate", www_auth)
                .header("Content-Type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({
                        "code": 402,
                        "message": "Payment Required",
                        "invoice": invoice_str,
                        "price_sats": config.price_sats,
                    })
                    .to_string(),
                ))
                .unwrap()
        }
        Err(e) => {
            warn!(?e, "failed to create L402 invoice");
            Response::builder()
                .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::from("failed to create invoice"))
                .unwrap()
        }
    }
}

fn payment_required_response(reason: &str) -> Response {
    Response::builder()
        .status(axum::http::StatusCode::PAYMENT_REQUIRED)
        .header("Content-Type", "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({
                "code": 402,
                "message": reason,
            })
            .to_string(),
        ))
        .unwrap()
}

/// Extract payment hash bytes from a BOLT11 invoice string.
fn extract_payment_hash(invoice_str: &str) -> Option<[u8; 32]> {
    let invoice: lightning_invoice::Bolt11Invoice = invoice_str.parse().ok()?;
    let hash = invoice.payment_hash();
    let mut out = [0u8; 32];
    out.copy_from_slice(hash.as_ref());
    Some(out)
}
