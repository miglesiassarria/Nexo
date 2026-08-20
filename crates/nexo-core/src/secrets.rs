//! Almacén seguro de credenciales.
//!
//! Keychain en macOS, Credential Manager en Windows, Secret Service en Linux.
//! Ningún secreto entra en SQLite ni en un fichero en texto plano (ADR 0006).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use crate::db::Db;
use crate::error::{CoreError, Result};
use std::sync::{Arc, RwLock};

pub const SERVICE: &str = "com.nexo.gateway";
pub const MASTER_KEY_NAME: &str = "master_key";

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

/// Proveedor de la clave maestra de cifrado (32 bytes).
pub trait MasterKeyProvider: Send + Sync {
    fn get_or_create_master_key(&self) -> Result<[u8; 32]>;
}

/// Proveedor de clave maestra respaldado por el Llavero del sistema operativo
/// (Keychain en macOS, Credential Manager en Windows, Secret Service en Linux).
pub struct SystemMasterKeyProvider {
    cached: RwLock<Option<[u8; 32]>>,
}

impl Default for SystemMasterKeyProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemMasterKeyProvider {
    pub fn new() -> Self {
        Self {
            cached: RwLock::new(None),
        }
    }
}

impl MasterKeyProvider for SystemMasterKeyProvider {
    fn get_or_create_master_key(&self) -> Result<[u8; 32]> {
        if let Ok(guard) = self.cached.read() {
            if let Some(key) = *guard {
                return Ok(key);
            }
        }

        let entry = keyring::Entry::new(SERVICE, MASTER_KEY_NAME)?;
        let key_bytes = match entry.get_password() {
            Ok(hex_str) => {
                let bytes = hex_decode(&hex_str)?;
                if bytes.len() != 32 {
                    return Err(CoreError::Keyring(
                        "la clave maestra en el llavero no tiene 32 bytes".into(),
                    ));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            }
            Err(keyring::Error::NoEntry) => {
                // Generar 32 bytes aleatorios criptográficos
                let mut raw = [0u8; 32];
                rand::fill(&mut raw);
                let hex_str = hex_encode(&raw);
                entry.set_password(&hex_str)?;
                raw
            }
            Err(e) => return Err(CoreError::Keyring(e.to_string())),
        };

        if let Ok(mut guard) = self.cached.write() {
            *guard = Some(key_bytes);
        }

        Ok(key_bytes)
    }
}

/// Almacén de credenciales cifrado en reposo (AES-256-GCM) en SQLite,
/// autenticado mediante una clave maestra protegida por el Llavero del sistema (ADR 0006).
pub struct EncryptedVaultSecretStore {
    db: Db,
    master_key_provider: Arc<dyn MasterKeyProvider>,
}

impl EncryptedVaultSecretStore {
    pub fn new(db: Db, master_key_provider: Arc<dyn MasterKeyProvider>) -> Self {
        Self {
            db,
            master_key_provider,
        }
    }

    fn cipher(&self) -> Result<Aes256Gcm> {
        let key = self.master_key_provider.get_or_create_master_key()?;
        Aes256Gcm::new_from_slice(&key)
            .map_err(|e| CoreError::Config(format!("clave de cifrado inválida: {e}")))
    }
}

impl SecretStore for EncryptedVaultSecretStore {
    fn set(&self, key: &SecretRef, secret: &str) -> Result<()> {
        let cipher = self.cipher()?;
        let mut nonce_bytes = [0u8; 12];
        rand::fill(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, secret.as_bytes())
            .map_err(|e| CoreError::Config(format!("fallo al cifrar secreto: {e}")))?;

        self.db
            .upsert_encrypted_secret(key.as_str(), &nonce_bytes, &ciphertext)?;
        Ok(())
    }

    fn get(&self, key: &SecretRef) -> Result<Option<String>> {
        let cipher = self.cipher()?;
        let record = self.db.get_encrypted_secret(key.as_str())?;
        let Some((nonce_bytes, ciphertext)) = record else {
            return Ok(None);
        };

        if nonce_bytes.len() != 12 {
            return Err(CoreError::Config("nonce de secreto inválido".into()));
        }

        let nonce = Nonce::from_slice(&nonce_bytes);
        let plaintext_bytes = cipher
            .decrypt(nonce, ciphertext.as_slice())
            .map_err(|e| CoreError::Config(format!("fallo al descifrar secreto: {e}")))?;

        let plaintext = String::from_utf8(plaintext_bytes)
            .map_err(|e| CoreError::Config(format!("secreto no es UTF-8 válido: {e}")))?;

        Ok(Some(plaintext))
    }

    fn delete(&self, key: &SecretRef) -> Result<()> {
        self.db.delete_encrypted_secret(key.as_str())?;
        Ok(())
    }
}

/// Almacén respaldado por el sistema operativo (EncryptedVaultSecretStore con SystemMasterKeyProvider).
pub struct SystemSecretStore {
    inner: EncryptedVaultSecretStore,
}

impl SystemSecretStore {
    pub fn new(db: Db) -> Self {
        Self {
            inner: EncryptedVaultSecretStore::new(db, Arc::new(SystemMasterKeyProvider::new())),
        }
    }
}

impl SecretStore for SystemSecretStore {
    fn set(&self, key: &SecretRef, secret: &str) -> Result<()> {
        self.inner.set(key, secret)
    }

    fn get(&self, key: &SecretRef) -> Result<Option<String>> {
        self.inner.get(key)
    }

    fn delete(&self, key: &SecretRef) -> Result<()> {
        self.inner.delete(key)
    }
}

/// Migración de secretos antiguos individuales del Llavero al nuevo almacén cifrado.
///
/// Se ejecuta una sola vez en el ciclo de vida de la base de datos.
/// Solo busca las claves correspondientes al `credential_kind` de cada cuenta,
/// traslada los secretos a `encrypted_secrets` y limpia las entradas obsoletas del Llavero.
pub fn migrate_legacy_keyring_entries(db: &Db, vault: &dyn SecretStore) -> usize {
    if db.is_legacy_keyring_migrated() {
        return 0;
    }

    let mut migrated = 0;

    // Migrar cuentas
    if let Ok(accounts) = db.accounts() {
        for account in accounts {
            let refs: Vec<SecretRef> = match account.credential_kind {
                crate::provider::CredentialKind::SubscriptionOauth => vec![
                    SecretRef::access(&account.id),
                    SecretRef::refresh(&account.id),
                ],
                crate::provider::CredentialKind::ApiKey => vec![SecretRef::api_key(&account.id)],
                crate::provider::CredentialKind::Local | crate::provider::CredentialKind::Mock => {
                    Vec::new()
                }
            };
            for r in refs {
                if let Ok(None) = vault.get(&r) {
                    if let Ok(entry) = keyring::Entry::new(SERVICE, r.as_str()) {
                        if let Ok(legacy_secret) = entry.get_password() {
                            if vault.set(&r, &legacy_secret).is_ok() {
                                let _ = entry.delete_credential();
                                migrated += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    // Migrar tokens recuperables de aplicaciones
    if let Ok(apps) = db.apps() {
        for app in apps {
            let r = SecretRef::app_token(&app.id);
            if let Ok(None) = vault.get(&r) {
                if let Ok(entry) = keyring::Entry::new(SERVICE, r.as_str()) {
                    if let Ok(legacy_secret) = entry.get_password() {
                        if vault.set(&r, &legacy_secret).is_ok() {
                            let _ = entry.delete_credential();
                            migrated += 1;
                        }
                    }
                }
            }
        }
    }

    let _ = db.set_legacy_keyring_migrated();
    migrated
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(CoreError::Config("cadena hexadecimal inválida".into()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|e| CoreError::Config(format!("byte hex inválido: {e}")))
        })
        .collect()
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
        Self {
            inner,
            last_error: std::sync::RwLock::new(None),
        }
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

/// Proveedor de clave maestra en memoria para pruebas.
pub struct MemoryMasterKeyProvider {
    key: [u8; 32],
}

impl MemoryMasterKeyProvider {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub fn random() -> Self {
        let mut key = [0u8; 32];
        rand::fill(&mut key);
        Self { key }
    }
}

impl MasterKeyProvider for MemoryMasterKeyProvider {
    fn get_or_create_master_key(&self) -> Result<[u8; 32]> {
        Ok(self.key)
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

    #[test]
    fn encrypted_vault_roundtrips_and_deletes() {
        let db = Db::open_in_memory().unwrap();
        let master_key_provider = Arc::new(MemoryMasterKeyProvider::random());
        let vault = EncryptedVaultSecretStore::new(db.clone(), master_key_provider);

        let key = SecretRef::api_key("acc1");
        assert_eq!(vault.get(&key).unwrap(), None);

        vault.set(&key, "sk-secret-key-12345").unwrap();
        assert_eq!(
            vault.get(&key).unwrap().as_deref(),
            Some("sk-secret-key-12345")
        );

        // Borrado
        vault.delete(&key).unwrap();
        assert_eq!(vault.get(&key).unwrap(), None);
    }

    #[test]
    fn sqlite_vault_contains_no_plaintext_secrets() {
        let db = Db::open_in_memory().unwrap();
        let master_key_provider = Arc::new(MemoryMasterKeyProvider::random());
        let vault = EncryptedVaultSecretStore::new(db.clone(), master_key_provider);

        let key = SecretRef::api_key("acc1");
        let secret_text = "sk-super-secret-key-no-plain-text-allowed";
        vault.set(&key, secret_text).unwrap();

        // Comprobamos directamente en SQLite
        let record = db.get_encrypted_secret(key.as_str()).unwrap().unwrap();
        let nonce = record.0;
        let ciphertext = record.1;

        assert_eq!(nonce.len(), 12);
        assert!(!ciphertext.is_empty());
        // El texto plano NO debe estar contenido en el ciphertext
        assert!(!ciphertext
            .windows(secret_text.len())
            .any(|window| window == secret_text.as_bytes()));
    }

    #[test]
    fn different_master_key_fails_to_decrypt() {
        let db = Db::open_in_memory().unwrap();
        let master_1 = Arc::new(MemoryMasterKeyProvider::random());
        let vault_1 = EncryptedVaultSecretStore::new(db.clone(), master_1);

        let key = SecretRef::api_key("acc1");
        vault_1.set(&key, "secret-token").unwrap();

        // Abrir con otra clave maestra
        let master_2 = Arc::new(MemoryMasterKeyProvider::random());
        let vault_2 = EncryptedVaultSecretStore::new(db.clone(), master_2);

        assert!(vault_2.get(&key).is_err(), "debe fallar al descifrar con clave distinta");
    }

    #[test]
    fn tampered_ciphertext_fails_gracefully() {
        let db = Db::open_in_memory().unwrap();
        let master = Arc::new(MemoryMasterKeyProvider::random());
        let vault = EncryptedVaultSecretStore::new(db.clone(), master);

        let key = SecretRef::api_key("acc1");
        vault.set(&key, "secret-token").unwrap();

        let mut record = db.get_encrypted_secret(key.as_str()).unwrap().unwrap();
        // Corromper el último byte del ciphertext (tag de autenticación)
        let last = record.1.len() - 1;
        record.1[last] ^= 0xff;
        db.upsert_encrypted_secret(key.as_str(), &record.0, &record.1).unwrap();

        assert!(vault.get(&key).is_err(), "AES-GCM debe rechazar ciphertext manipulado");
    }

    pub struct CountingMasterKeyProvider {
        key: [u8; 32],
        cached: RwLock<Option<[u8; 32]>>,
        keychain_reads: std::sync::atomic::AtomicUsize,
    }

    impl CountingMasterKeyProvider {
        pub fn new(key: [u8; 32]) -> Self {
            Self {
                key,
                cached: RwLock::new(None),
                keychain_reads: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        pub fn keychain_reads(&self) -> usize {
            self.keychain_reads.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl MasterKeyProvider for CountingMasterKeyProvider {
        fn get_or_create_master_key(&self) -> Result<[u8; 32]> {
            if let Ok(guard) = self.cached.read() {
                if let Some(key) = *guard {
                    return Ok(key);
                }
            }
            self.keychain_reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if let Ok(mut guard) = self.cached.write() {
                *guard = Some(self.key);
            }
            Ok(self.key)
        }
    }

    #[test]
    fn single_keychain_access_on_catalog_sync_and_many_secrets() {
        let db = Db::open_in_memory().unwrap();
        let counting_provider = Arc::new(CountingMasterKeyProvider::new([7u8; 32]));
        let vault = EncryptedVaultSecretStore::new(db.clone(), counting_provider.clone());

        // Múltiples escrituras y lecturas de proveedores
        for i in 1..=10 {
            vault
                .set(&SecretRef::api_key(&format!("account_{i}")), &format!("secret_{i}"))
                .unwrap();
        }

        for i in 1..=10 {
            assert_eq!(
                vault
                    .get(&SecretRef::api_key(&format!("account_{i}")))
                    .unwrap()
                    .as_deref(),
                Some(format!("secret_{i}").as_str())
            );
        }

        // Exactamente 1 acceso al Llavero
        assert_eq!(counting_provider.keychain_reads(), 1);
    }

    #[test]
    fn migration_runs_only_once_and_sets_flag() {
        let db = Db::open_in_memory().unwrap();
        let master_key_provider = Arc::new(MemoryMasterKeyProvider::random());
        let vault = EncryptedVaultSecretStore::new(db.clone(), master_key_provider);

        assert!(!db.is_legacy_keyring_migrated());

        let migrated_first = migrate_legacy_keyring_entries(&db, &vault);
        assert_eq!(migrated_first, 0);
        assert!(db.is_legacy_keyring_migrated());

        // Segunda llamada no hace nada
        let migrated_second = migrate_legacy_keyring_entries(&db, &vault);
        assert_eq!(migrated_second, 0);
    }
}
