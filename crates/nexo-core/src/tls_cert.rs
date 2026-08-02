//! Certificado autofirmado para el acceso desde la red local.
//!
//! Pieza frágil, aislada en este único módulo (invariante 7 de `CLAUDE.md`):
//! si algo de esto se rompe, se rompe aquí y en ningún otro sitio. Genera el
//! certificado una sola vez y lo persiste como fichero en el directorio de
//! datos — no en SQLite, no en el almacén de credenciales de proveedores: no
//! es una credencial de proveedor, es la identidad TLS del propio servidor.
//!
//! Ver [ADR 0003](../../../docs/adr/0003-acceso-desde-la-red-local.md) y
//! `specs/0007-acceso-red-local/design.md`.

use base64::prelude::{Engine as _, BASE64_STANDARD};
use rcgen::{CertificateParams, KeyPair};
use std::fmt;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use crate::net::detect_lan_ip;
use crate::util::sha256_hex;

/// Certificado autofirmado listo para servir, ya generado o ya existente.
pub struct LanCertificate {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub fingerprint_sha256: String,
    /// La IP de red detectada al generar el certificado (no necesariamente
    /// la actual, si la máquina cambió de red después). `None` si no se
    /// detectó ninguna en su momento.
    pub address: Option<IpAddr>,
}

#[derive(Debug)]
pub enum TlsCertError {
    Io(std::io::Error),
    Generation(String),
    Corrupt(String),
}

impl fmt::Display for TlsCertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TlsCertError::Io(e) => write!(f, "error de E/S con el certificado: {e}"),
            TlsCertError::Generation(e) => write!(f, "no se pudo generar el certificado: {e}"),
            TlsCertError::Corrupt(e) => write!(f, "el certificado guardado no es válido: {e}"),
        }
    }
}

impl std::error::Error for TlsCertError {}

/// Si `<data_dir>/tls/cert.pem` y `key.pem` ya existen, los lee y valida. Si
/// no existen, los genera. Si existen pero no se pueden leer o no son un
/// certificado válido, devuelve `Err` **sin regenerar por encima** — un
/// fichero que no se entiende podría ser algo que el usuario necesita
/// diagnosticar, no algo que pisar en silencio.
pub fn ensure(data_dir: &Path) -> Result<LanCertificate, TlsCertError> {
    let dir = data_dir.join("tls");
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");

    if cert_path.exists() || key_path.exists() {
        load(cert_path, key_path)
    } else {
        generate(dir, cert_path, key_path)
    }
}

fn load(cert_path: PathBuf, key_path: PathBuf) -> Result<LanCertificate, TlsCertError> {
    let cert_pem = fs::read_to_string(&cert_path).map_err(TlsCertError::Io)?;
    let key_pem = fs::read_to_string(&key_path).map_err(TlsCertError::Io)?;
    if key_pem.trim().is_empty() {
        return Err(TlsCertError::Corrupt(
            "el fichero de la clave privada está vacío".into(),
        ));
    }
    let der = pem_to_der(&cert_pem)
        .ok_or_else(|| TlsCertError::Corrupt("el certificado no es un PEM válido".into()))?;

    Ok(LanCertificate {
        cert_path,
        key_path,
        fingerprint_sha256: sha256_hex(&der),
        address: detect_lan_ip(),
    })
}

fn generate(
    dir: PathBuf,
    cert_path: PathBuf,
    key_path: PathBuf,
) -> Result<LanCertificate, TlsCertError> {
    let address = detect_lan_ip();
    let mut sans = vec![
        "127.0.0.1".to_string(),
        "::1".to_string(),
        "localhost".to_string(),
    ];
    if let Some(ip) = address {
        sans.push(ip.to_string());
    }

    let params =
        CertificateParams::new(sans).map_err(|e| TlsCertError::Generation(e.to_string()))?;
    let key_pair = KeyPair::generate().map_err(|e| TlsCertError::Generation(e.to_string()))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| TlsCertError::Generation(e.to_string()))?;

    fs::create_dir_all(&dir).map_err(TlsCertError::Io)?;
    #[cfg(unix)]
    restrict_permissions(&dir, 0o700)?;

    fs::write(&cert_path, cert.pem()).map_err(TlsCertError::Io)?;
    fs::write(&key_path, key_pair.serialize_pem()).map_err(TlsCertError::Io)?;
    #[cfg(unix)]
    restrict_permissions(&key_path, 0o600)?;

    Ok(LanCertificate {
        cert_path,
        key_path,
        fingerprint_sha256: sha256_hex(cert.der().as_ref()),
        address,
    })
}

#[cfg(unix)]
fn restrict_permissions(path: &Path, mode: u32) -> Result<(), TlsCertError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(TlsCertError::Io)
}

/// Decodifica el cuerpo base64 de un bloque PEM de un solo certificado a DER.
/// No es un parser PEM general: basta con lo que `rcgen` escribe.
fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let body: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect();
    BASE64_STANDARD.decode(body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_data_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nexo-tls-cert-test-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).expect("crear directorio temporal de prueba");
        dir
    }

    #[test]
    fn generates_the_certificate_files_with_a_fingerprint() {
        let dir = temp_data_dir("generates");
        let cert = ensure(&dir).expect("debe generar el certificado");
        assert!(cert.cert_path.exists());
        assert!(cert.key_path.exists());
        assert!(!cert.fingerprint_sha256.is_empty());
    }

    #[test]
    fn reuses_the_same_certificate_and_fingerprint_across_calls() {
        let dir = temp_data_dir("reuses");
        let first = ensure(&dir).expect("primera generación");
        let second = ensure(&dir).expect("segunda llamada debe reutilizar");
        assert_eq!(first.fingerprint_sha256, second.fingerprint_sha256);
    }

    #[test]
    fn a_corrupt_certificate_file_is_reported_and_not_silently_regenerated() {
        let dir = temp_data_dir("corrupt");
        let tls_dir = dir.join("tls");
        std::fs::create_dir_all(&tls_dir).expect("crear tls/");
        std::fs::write(tls_dir.join("cert.pem"), "esto no es un certificado").unwrap();
        std::fs::write(tls_dir.join("key.pem"), "esto no es una clave").unwrap();

        let result = ensure(&dir);
        assert!(
            matches!(result, Err(TlsCertError::Corrupt(_))),
            "un fichero corrupto debe reportarse como error, no regenerarse en silencio"
        );
        // Y de verdad no lo regeneró por encima: el contenido corrupto sigue ahí.
        let untouched = std::fs::read_to_string(tls_dir.join("cert.pem")).unwrap();
        assert_eq!(untouched, "esto no es un certificado");
    }

    #[test]
    fn the_key_file_has_restrictive_permissions() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir = temp_data_dir("permissions");
            let cert = ensure(&dir).expect("debe generar el certificado");
            let mode = std::fs::metadata(&cert.key_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn certificate_params_do_not_expire_for_at_least_ten_years() {
        // Mismos parámetros que construye `generate()`, comprobados antes de
        // firmar: si una actualización de `rcgen` cambiara su valor por
        // defecto de `not_after` (hoy año 4096), esta prueba lo nota en
        // `cargo test`, no en un fallo de TLS meses después de activar el
        // modo red.
        let params = CertificateParams::new(vec!["127.0.0.1".to_string()])
            .expect("los parámetros deben construirse");
        assert!(
            params.not_after.year() >= 2035,
            "el certificado no debe caducar en menos de diez años, caduca en {}",
            params.not_after.year()
        );
    }
}
