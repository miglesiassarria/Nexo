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
