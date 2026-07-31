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

/// Convierte un nombre elegido por el usuario en un identificador estable.
///
/// Minúsculas, sin acentos (solo el rango ASCII: es una aproximación deliberada,
/// no una normalización Unicode completa), separadores por guiones, recortado a
/// 48 caracteres. Es la misma regla que usa `scripts/new-spec.sh` para las
/// carpetas de especificación, para que el criterio sea el mismo en todo el
/// repositorio.
pub fn slugify(name: &str) -> String {
    // Se pasa a minúsculas ANTES de sustituir acentos: si no, «Ámbar» no coincide
    // con ningún patrón (son todos en minúscula) y la Á se cuela sin traducir.
    let lower = name.to_lowercase();
    let ascii: String = lower
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c => c,
        })
        .collect();

    let mut slug = String::new();
    let mut last_was_sep = true; // evita un guion al principio
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            last_was_sep = false;
        } else if !last_was_sep {
            slug.push('-');
            last_was_sep = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug.truncate(48);
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
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

    #[test]
    fn slugify_handles_spaces_punctuation_and_accents() {
        assert_eq!(slugify("OpenCode Zen"), "opencode-zen");
        assert_eq!(slugify("  espacios   raros!!  "), "espacios-raros");
        assert_eq!(slugify("ñandú Ámbar"), "nandu-ambar");
        assert_eq!(slugify("Mi Proveedor"), "mi-proveedor");
    }

    #[test]
    fn slugify_of_only_punctuation_is_empty() {
        assert_eq!(slugify("···!!!"), "");
    }

    #[test]
    fn slugify_never_starts_or_ends_with_a_hyphen() {
        let s = slugify("--hola--mundo--");
        assert!(!s.starts_with('-'));
        assert!(!s.ends_with('-'));
    }

    #[test]
    fn slugify_two_different_names_do_not_collide_by_accident() {
        assert_ne!(slugify("Mi Proveedor"), slugify("mi-proveedor-2"));
    }
}
