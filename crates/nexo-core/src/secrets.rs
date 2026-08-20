//! Almacén seguro de credenciales.
//!
//! Keychain en macOS, Credential Manager en Windows, Secret Service en Linux.
//! Ningún secreto entra en SQLite ni en un fichero de texto plano.

use crate::error::{CoreError, Result};

const SERVICE: &str = "com.nexo.gateway";

/// Referencia lógica a un secreto. Es lo único que se persiste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretRef(String);

impl SecretRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn from_stored(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Access token de una cuenta.
    pub fn access(account_id: &str) -> Self {
        Self(format!("account/{account_id}/access"))
    }

    /// Refresh token de una cuenta.
    pub fn refresh(account_id: &str) -> Self {
        Self(format!("account/{account_id}/refresh"))
    }

    /// API key de una cuenta.
    pub fn api_key(account_id: &str) -> Self {
        Self(format!("account/{account_id}/api_key"))
    }

    /// Token emitido a una aplicación cliente. Solo se guarda aquí si el
    /// usuario quiere poder volver a verlo; la autenticación usa el hash.
    pub fn app_token(app_id: &str) -> Self {
        Self(format!("app/{app_id}/token"))
    }
}

pub trait SecretStore: Send + Sync {
    fn set(&self, key: &SecretRef, secret: &str) -> Result<()>;
    fn get(&self, key: &SecretRef) -> Result<Option<String>>;
    fn delete(&self, key: &SecretRef) -> Result<()>;
}

/// Almacén respaldado por el sistema operativo.
pub struct SystemSecretStore;

impl SecretStore for SystemSecretStore {
    fn set(&self, key: &SecretRef, secret: &str) -> Result<()> {
        keyring::Entry::new(SERVICE, key.as_str())?.set_password(secret)?;
        Ok(())
    }

    fn get(&self, key: &SecretRef) -> Result<Option<String>> {
        match keyring::Entry::new(SERVICE, key.as_str())?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CoreError::Keyring(e.to_string())),
        }
    }

    fn delete(&self, key: &SecretRef) -> Result<()> {
        match keyring::Entry::new(SERVICE, key.as_str())?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CoreError::Keyring(e.to_string())),
        }
    }
}

/// Envoltorio que deja constancia del último fallo del almacén de verdad.
///
/// Existe por un incidente real (2026-08-20): el llavero de inicio de sesión de
/// macOS dejó de autenticarse —problema del sistema, no de Nexo—, y Nexo se lo
/// comió con un `warn` en un log que nadie mira. El usuario vio dos síntomas sin
/// relación aparente: «la clave de la aplicación solo copia el prefijo» y «me
/// faltan modelos de la suscripción». La causa era una sola, y estaba escrita en
/// ninguna parte visible.
///
/// Envolver el almacén, en lugar de comprobarlo en cada sitio que lo usa,
/// garantiza que **cualquier** fallo quede registrado: leer una API key,
/// guardar el token de una aplicación o borrarlo al revocarla. Un acierto
/// posterior lo limpia, para que el aviso no se quede pegado cuando el problema
/// se arregla.
pub struct ReportingSecretStore {
    inner: std::sync::Arc<dyn SecretStore>,
    last_error: std::sync::RwLock<Option<String>>,
}

impl ReportingSecretStore {
    pub fn new(inner: std::sync::Arc<dyn SecretStore>) -> Self {
        Self { inner, last_error: std::sync::RwLock::new(None) }
    }

    /// El último fallo, o `None` si la última operación fue bien.
    pub fn last_error(&self) -> Option<String> {
        self.last_error.read().ok().and_then(|e| e.clone())
    }

    fn record<T>(&self, result: Result<T>) -> Result<T> {
        if let Ok(mut slot) = self.last_error.write() {
            *slot = match &result {
                Err(e) => Some(e.to_string()),
                Ok(_) => None,
            };
        }
        result
    }
}

impl SecretStore for ReportingSecretStore {
    fn set(&self, key: &SecretRef, secret: &str) -> Result<()> {
        self.record(self.inner.set(key, secret))
    }

    fn get(&self, key: &SecretRef) -> Result<Option<String>> {
        self.record(self.inner.get(key))
    }

    fn delete(&self, key: &SecretRef) -> Result<()> {
        self.record(self.inner.delete(key))
    }
}

/// Almacén en memoria, solo para pruebas.
#[derive(Default)]
pub struct MemorySecretStore {
    inner: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn set(&self, key: &SecretRef, secret: &str) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .insert(key.as_str().to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, key: &SecretRef) -> Result<Option<String>> {
        Ok(self.inner.lock().unwrap().get(key.as_str()).cloned())
    }

    fn delete(&self, key: &SecretRef) -> Result<()> {
        self.inner.lock().unwrap().remove(key.as_str());
        Ok(())
    }
}

/// Almacén que siempre falla, para reproducir el llavero inaccesible del
/// incidente del 2026-08-20 sin depender del estado del sistema.
#[cfg(test)]
pub struct FailingSecretStore;

#[cfg(test)]
impl SecretStore for FailingSecretStore {
    fn set(&self, _key: &SecretRef, _secret: &str) -> Result<()> {
        Err(Self::error())
    }

    fn get(&self, _key: &SecretRef) -> Result<Option<String>> {
        Err(Self::error())
    }

    fn delete(&self, _key: &SecretRef) -> Result<()> {
        Err(Self::error())
    }
}

#[cfg(test)]
impl FailingSecretStore {
    fn error() -> CoreError {
        // El mensaje literal que devolvió macOS en el incidente.
        CoreError::Keyring(
            "Platform failure: The user name or passphrase you entered is not correct.".into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_are_namespaced_per_account_and_purpose() {
        assert_eq!(SecretRef::access("a1").as_str(), "account/a1/access");
        assert_eq!(SecretRef::refresh("a1").as_str(), "account/a1/refresh");
        assert_ne!(SecretRef::access("a1"), SecretRef::refresh("a1"));
        assert_ne!(SecretRef::access("a1"), SecretRef::access("a2"));
    }

    #[test]
    fn memory_store_roundtrips_and_deletes() {
        let store = MemorySecretStore::default();
        let key = SecretRef::access("a1");
        assert_eq!(store.get(&key).unwrap(), None);
        store.set(&key, "tok").unwrap();
        assert_eq!(store.get(&key).unwrap().as_deref(), Some("tok"));
        store.delete(&key).unwrap();
        assert_eq!(store.get(&key).unwrap(), None);
    }

    #[test]
    fn deleting_absent_secret_is_not_an_error() {
        let store = MemorySecretStore::default();
        assert!(store.delete(&SecretRef::access("nope")).is_ok());
    }
}
