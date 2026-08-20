# 0015 · Diseño: Clave maestra en el Llavero del sistema y almacenamiento cifrado de credenciales

- **Estado:** hecho
- **Fichero:** `specs/0015-clave-maestra-en-llavero/design.md`
- **Spec:** [`spec.md`](spec.md)
- **ADR asociado:** [`docs/adr/0006-clave-maestra-en-llavero-y-almacen-cifrado.md`](../../docs/adr/0006-clave-maestra-en-llavero-y-almacen-cifrado.md)

## Componentes y contratos afectados

1. **`crates/nexo-core/Cargo.toml`**:
   - Se añade `aes-gcm = "0.10.3"` para cifrado autenticado AEAD (AES-256-GCM).
2. **`crates/nexo-core/src/db/schema.sql` y `crates/nexo-core/src/db/mod.rs`**:
   - Nueva tabla `encrypted_secrets`:
     ```sql
     CREATE TABLE IF NOT EXISTS encrypted_secrets (
         key TEXT PRIMARY KEY,
         nonce BLOB NOT NULL,
         ciphertext BLOB NOT NULL,
         updated_at INTEGER NOT NULL
     );
     ```
   - Métodos en `Db` para insertar/actualizar, leer y borrar secretos cifrados.
3. **`crates/nexo-core/src/secrets.rs`**:
   - `EncryptedVaultSecretStore`:
     - Posee una referencia a `Db` y un `Arc<MasterKeyStore>`.
     - En el primer acceso o al arrancar, obtiene la clave maestra del Llavero del sistema (`keyring::Entry::new("com.nexo.gateway", "master_key")`).
     - Si la entrada no existe, genera 32 bytes aleatorios criptográficos (`rand::rng().fill_bytes(...)` o similar con `rand`), la guarda en el Llavero y la usa en memoria.
     - `set(key, secret)`: Genera un nonce aleatorio de 96 bits (12 bytes), cifra el secreto con AES-256-GCM y guarda `(key, nonce, ciphertext)` en `encrypted_secrets`.
     - `get(key)`: Lee el registro de `encrypted_secrets` y descifra el texto con AES-256-GCM usando la clave maestra y el nonce.
     - `delete(key)`: Elimina la fila de `encrypted_secrets`.
   - `SystemSecretStore`: pasa a ser una instancia de `EncryptedVaultSecretStore` respaldada por `keyring` para la clave maestra y SQLite para los blobs cifrados.
4. **Migración de credenciales existentes**:
   - Función `migrate_legacy_keyring(db, secret_store)`:
     - Itera sobre las cuentas activas en `db.accounts()` y las apps en `db.apps()`.
     - Para cada cuenta, intenta leer la API key, token de acceso o refresh del `keyring` antiguo (`account/<id>/...`, `app/<id>/token`).
     - Si existe en el Llavero antiguo y no existe en `encrypted_secrets`, lo escribe en el almacén cifrado.
     - Tras guardarse exitosamente, borra la entrada antigua del Llavero.

## Decisiones y alternativas descartadas

| Decisión | Alternativa descartada | Motivo |
| --- | --- | --- |
| **AES-256-GCM (AEAD)** | Cifrado simétrico sin autenticación (AES-CBC) o ChaCha20 | AES-GCM es el estándar indiscutible de la industria para cifrado en reposo con verificación de integridad; soporte nativo acelerado por hardware en ARM (Apple Silicon) y x86_64. |
| **Nonce único de 12 bytes generado por cada escritura** | Nonce fijo o predecible | La reutilización de nonces en AES-GCM destruiría la confidencialidad. 12 bytes aleatorios por operación garantizan seguridad criptográfica estándar. |
| **Caché en memoria de la clave maestra (`Arc<KeyBytes>`)** | Consultar el Llavero en cada llamada a `get()` | Evita overhead de IPC con el demonio del Llavero de macOS en cada petición HTTP atendida por el gateway. |
| **Migración perezosa pero ejecutada al arrancar** | Forzar al usuario a reintroducir todas las cuentas | Destruiría la experiencia de usuario y obligaría a reconectar ChatGPT, OpenRouter, Gemini, etc. |

## Detección de fallos

- Si el Llavero del sistema está bloqueado o denegado por el usuario, el intento de recuperar/guardar `master_key` falla inmediatamente con `CoreError::Keyring`.
- `ReportingSecretStore` intercepta el fallo y `gateway_status` lo refleja de inmediato en el banner de advertencia del frontend.
