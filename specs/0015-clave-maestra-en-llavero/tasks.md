# 0015 · Tareas: Clave maestra en el Llavero del sistema y almacenamiento cifrado de credenciales

- **Estado:** hecho
- **Fichero:** `specs/0015-clave-maestra-en-llavero/tasks.md`
- **Spec:** [`spec.md`](spec.md)
- **Diseño:** [`design.md`](design.md)

## Tareas

- [x] **1. Dependencia criptográfica y tabla SQLite**
  - Ficheros: `crates/nexo-core/Cargo.toml`, `crates/nexo-core/src/db/migrations.rs`, `crates/nexo-core/src/db/mod.rs`
  - Añadir `aes-gcm = "0.10.3"` en `Cargo.toml`.
  - Crear tabla `encrypted_secrets` en migración V4 y métodos CRUD correspondientes en `db/mod.rs`.
  - Verificación: `cargo test -p nexo-core -- db::tests`

- [x] **2. Implementación de `EncryptedVaultSecretStore` y pruebas unitarias de cifrado**
  - Ficheros: `crates/nexo-core/src/secrets.rs`
  - Implementar lógica de generación/recuperación de `master_key` y cifrado/descifrado AES-256-GCM.
  - Implementar `SecretStore` sobre la base de datos y la clave maestra.
  - Pruebas unitarias de roundtrip cifrado, detección de corrupción y no-texto plano en BD.
  - Verificación: `cargo test -p nexo-core -- secrets::tests`

- [x] **3. Migración transparente de credenciales legacy del Llavero**
  - Ficheros: `crates/nexo-core/src/secrets.rs`, `crates/nexo-core/src/service.rs`
  - Implementar rutina de migración en el arranque que traslade entradas existentes de `keyring` a `encrypted_secrets`.
  - Pruebas de migración de credenciales antiguas.
  - Verificación: `cargo test -p nexo-core -- secrets::tests`

- [x] **4. Integración en `service.rs`, `main.rs` y suite completa de tests e2e**
  - Ficheros: `crates/nexo-core/src/service.rs`, `src-tauri/src/main.rs`, `crates/nexo-core/tests/gateway_e2e.rs`
  - Conectar el almacén cifrado con el Llavero del sistema en producción.
  - Comprobar que los tests de integración e2e y de políticas pasan intactos.
  - Verificación: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`

- [x] **5. Compilación e instalación en macOS y validación en la app real**
  - Ficheros: `scripts/install-macos.sh`
  - Ejecutar `npm run app:install`.
  - Verificar que la app instalada en `/Applications/Nexo.app` arranca y conecta con los proveedores solicitando una única confirmación del Llavero.
  - Verificación: `npm run app:install` (compilado e instalado: Aug 20 11:12:01 2026)
