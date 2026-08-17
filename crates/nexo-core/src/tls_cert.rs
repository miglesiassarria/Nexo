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

/// Nombre del fichero donde queda constancia de a qué direcciones responde
/// el certificado guardado. Sin él no hay forma de saber si el certificado
/// que hay en disco sirve para la red actual, y un certificado que no nombra
/// la dirección por la que se conecta el cliente lo rechaza el cliente.
const SANS_FILE: &str = "sans.txt";

/// Certificado autofirmado listo para servir, ya generado o ya existente.
pub struct LanCertificate {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub fingerprint_sha256: String,
    /// La IP de red detectada ahora, que es también la que el certificado
    /// nombra: `ensure` lo rehace si dejaron de coincidir. `None` si no se
    /// detecta ninguna.
    pub address: Option<IpAddr>,
    /// `true` si este certificado se acaba de rehacer porque el guardado no
    /// cubría la dirección actual. La huella ha cambiado, así que los
    /// equipos que ya lo habían aceptado tienen que volver a aceptarlo — y
    /// eso hay que decírselo al usuario, no dejarlo en un fallo de TLS
    /// inexplicable.
    pub rotated: bool,
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
///
/// Si el certificado guardado es válido pero **no cubre la dirección de red
/// actual** — la máquina cambió de red, o lo generó una versión anterior que
/// no dejó constancia de qué direcciones cubría — se rehace. No hacerlo
/// dejaba a los clientes de la red local con un error de TLS imposible de
/// interpretar: el panel anunciaba `https://<IP de hoy>` y el certificado
/// nombraba la IP de otro día.
pub fn ensure(data_dir: &Path) -> Result<LanCertificate, TlsCertError> {
    let dir = data_dir.join("tls");
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    let sans_path = dir.join(SANS_FILE);
    let wanted = wanted_sans();

    if cert_path.exists() || key_path.exists() {
        // Validar antes que nada: un fichero corrupto se reporta, no se pisa.
        let existing = load(cert_path.clone(), key_path.clone())?;
        if recorded_sans(&sans_path).is_some_and(|r| covers(&r, &wanted)) {
            return Ok(existing);
        }
        tracing::warn!(
            sans = ?wanted,
            "el certificado guardado no cubre la dirección de red actual; se rehace"
        );
        return generate(dir, cert_path, key_path, sans_path, wanted, true);
    }

    generate(dir, cert_path, key_path, sans_path, wanted, false)
}

/// Los nombres y direcciones que el certificado tiene que cubrir hoy.
fn wanted_sans() -> Vec<String> {
    let mut sans = vec![
        "127.0.0.1".to_string(),
        "::1".to_string(),
        "localhost".to_string(),
    ];
    if let Some(ip) = detect_lan_ip() {
        sans.push(ip.to_string());
    }
    sans
}

/// Lo que dice el fichero de constancia, o `None` si no existe o no se lee.
/// Ausente significa «no se sabe qué cubre», que se trata igual que «no
/// cubre»: es lo único honesto que se puede hacer sin abrir el certificado.
fn recorded_sans(sans_path: &Path) -> Option<Vec<String>> {
    let raw = fs::read_to_string(sans_path).ok()?;
    Some(
        raw.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

fn covers(recorded: &[String], wanted: &[String]) -> bool {
    wanted.iter().all(|w| recorded.iter().any(|r| r == w))
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
        rotated: false,
    })
}

fn generate(
    dir: PathBuf,
    cert_path: PathBuf,
    key_path: PathBuf,
    sans_path: PathBuf,
    sans: Vec<String>,
    rotated: bool,
) -> Result<LanCertificate, TlsCertError> {
    let address = detect_lan_ip();

    let params =
        CertificateParams::new(sans.clone()).map_err(|e| TlsCertError::Generation(e.to_string()))?;
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
    // La constancia se escribe **después** del certificado: si algo falla en
    // medio, la próxima llamada ve un certificado sin constancia y lo rehace,
    // que es el lado seguro del error.
    fs::write(&sans_path, format!("{}\n", sans.join("\n"))).map_err(TlsCertError::Io)?;

    Ok(LanCertificate {
        cert_path,
        key_path,
        fingerprint_sha256: sha256_hex(cert.der().as_ref()),
        address,
        rotated,
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
        assert!(!first.rotated, "recién generado no es una rotación");
        assert!(!second.rotated, "reutilizar no es rotar");
    }

    #[test]
    fn a_certificate_that_no_longer_covers_this_network_is_remade() {
        // El fallo real: el certificado se generó en otra red (otra IP) y
        // seguía usándose tal cual. El cliente que se conectaba a la IP de
        // hoy lo rechazaba, y Nexo no se enteraba.
        let dir = temp_data_dir("red-cambiada");
        let first = ensure(&dir).expect("primera generación");
        let sans_path = dir.join("tls").join(SANS_FILE);
        std::fs::write(&sans_path, "127.0.0.1\n::1\nlocalhost\n192.0.2.7\n")
            .expect("simular el certificado de otra red");

        let second = ensure(&dir).expect("debe rehacerse");

        assert!(
            second.rotated,
            "un certificado que no cubre la red actual debe rehacerse"
        );
        assert_ne!(
            first.fingerprint_sha256, second.fingerprint_sha256,
            "rehacerlo cambia la huella"
        );
        let recorded = recorded_sans(&sans_path).expect("la constancia debe reescribirse");
        assert_eq!(recorded, wanted_sans());
    }

    #[test]
    fn a_certificate_without_a_record_of_what_it_covers_is_remade() {
        // Los certificados de versiones anteriores no dejaron constancia. No
        // se puede saber qué cubren, así que se rehacen una vez.
        let dir = temp_data_dir("sin-constancia");
        let first = ensure(&dir).expect("primera generación");
        std::fs::remove_file(dir.join("tls").join(SANS_FILE)).expect("borrar la constancia");

        let second = ensure(&dir).expect("debe rehacerse");

        assert!(second.rotated);
        assert_ne!(first.fingerprint_sha256, second.fingerprint_sha256);
    }

    #[test]
    fn covers_needs_every_wanted_name() {
        let recorded = vec!["127.0.0.1".to_string(), "localhost".to_string()];
        assert!(covers(&recorded, &["localhost".to_string()]));
        assert!(!covers(&recorded, &["192.168.1.5".to_string()]));
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
