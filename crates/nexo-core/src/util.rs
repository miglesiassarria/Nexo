use base64::prelude::{Engine as _, BASE64_URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Instante actual en milisegundos desde epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Trunca un instante en milisegundos al comienzo de su hora.
pub fn hour_floor_ms(ts_ms: i64) -> i64 {
    const HOUR: i64 = 3_600_000;
    ts_ms - ts_ms.rem_euclid(HOUR)
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    (0..n).map(|_| rand::random::<u8>()).collect()
}

pub fn b64url(bytes: &[u8]) -> String {
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}

pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    BASE64_URL_SAFE_NO_PAD.decode(s.as_bytes()).ok()
}

pub fn sha256(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    sha256(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Decodifica el payload de un JWT sin validar la firma.
///
/// Nexo no es el destinatario de estos tokens: solo necesita leer claims
/// informativas (el identificador de cuenta). La validación corresponde al
/// proveedor que los emitió y los recibe.
pub fn jwt_claims(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = b64url_decode(payload)?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hour_floor_truncates() {
        assert_eq!(hour_floor_ms(3_600_000 + 1234), 3_600_000);
        assert_eq!(hour_floor_ms(0), 0);
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_ne!(sha256_hex(b"nexo"), sha256_hex(b"nexo "));
    }

    #[test]
    fn jwt_claims_reads_payload_without_verifying() {
        let payload = b64url(br#"{"chatgpt_account_id":"acc-1"}"#);
        let token = format!("header.{payload}.signature");
        let claims = jwt_claims(&token).expect("claims");
        assert_eq!(claims["chatgpt_account_id"], "acc-1");
    }

    #[test]
    fn jwt_claims_rejects_malformed() {
        assert!(jwt_claims("not-a-jwt").is_none());
    }
}
