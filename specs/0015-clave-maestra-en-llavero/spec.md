# 0015 · Clave maestra en el Llavero del sistema y almacenamiento cifrado de credenciales

- **Estado:** hecho
- **Creada:** 2026-08-20
- **Pedida por:** el usuario, al detectar que en cada nuevo despliegue e instalación (`npm run app:install`), macOS solicita confirmar la contraseña de 4 a 6 veces debido a la consulta concurrente de cada secreto individual en el Llavero con un binario de firma ad-hoc nueva

## Problema

Cada vez que se compila e instala Nexo (`npm run app:install`), el binario generado recibe una firma ad-hoc nueva. En macOS, el subsistema de seguridad (*Keychain Access Control*) comprueba la firma exacta del ejecutable al acceder a cada entrada del Llavero.

Al arrancar la aplicación, Nexo ejecuta el descubrimiento de modelos consultando todas las cuentas conectadas (`resolve_credential`), lo que dispara entre 4 y 6 peticiones independientes al Llavero del sistema (tokens de acceso y refresh de ChatGPT, API keys de OpenAI, OpenRouter, Gemini, proveedores personalizados y tokens de aplicación). Esto obliga al usuario a introducir o autorizar su contraseña 4-6 veces consecutivas por cada despliegue.

## Comportamiento esperado

1. **Una única petición de contraseña por despliegue**: Al arrancar Nexo tras un despliegue, el sistema operativo solicita confirmación de acceso al Llavero como máximo **una sola vez** (para acceder a la clave maestra `master_key`).
2. **Cifrado seguro en local**: Todos los secretos individuales (`SecretRef`) se cifran mediante **AES-256-GCM** (con nonce único y etiqueta de autenticación) y se almacenan en una tabla dedicada (`encrypted_secrets`) en `nexo.sqlite`.
3. **Generación automática de la clave maestra**: Si no existe una clave maestra en el Llavero, Nexo genera 32 bytes aleatorios criptográficamente seguros y los almacena en el Llavero bajo el servicio `com.nexo.gateway`.
4. **Migración transparente sin pérdida de datos**: En el primer arranque, si existen credenciales en el Llavero antiguo (`account/*`, `app/*`), Nexo las lee, las migra al nuevo almacén cifrado y limpia las entradas obsoletas del Llavero. El usuario no pierde ninguna cuenta conectada ni tiene que reconfigurar nada.
5. **Comportamiento sin cambios en operaciones cotidianas**: La API pública del gateway, la interfaz gráfica, la revocación y creación de tokens y la gestión de cuentas funcionan exactamente igual sin degradación.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | `EncryptedVaultSecretStore` genera o recupera la clave maestra del Llavero y almacena/recupera secretos cifrados con AES-256-GCM | `cargo test -p nexo-core -- encrypted_vault_` (suite de pruebas unitarias de cifrado, descifrado y borrado) |
| 2 | La base de datos SQLite almacena únicamente blobs cifrados y nonces, nunca valores en texto plano | `cargo test -p nexo-core -- sqlite_vault_contains_no_plaintext_secrets` |
| 3 | Los secretos existentes en el Llavero antiguo se migran automáticamente al almacén cifrado en el primer arranque | `cargo test -p nexo-core -- migration_from_legacy_keychain_entries` |
| 4 | Al arrancar y descubrir modelos con múltiples proveedores configurados, solo se realiza una lectura al Llavero del SO | `cargo test -p nexo-core -- single_keychain_access_on_catalog_sync` |
| 5 | Toda la batería de pruebas y linters pasan sin errores | `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check` |
| 6 | La aplicación compila, se instala en `/Applications` y arranca correctamente | `npm run app:install` ejecutado con éxito |

## Fuera de alcance

- **Pedir una contraseña maestra al usuario en la interfaz.** La clave maestra es gestionada de forma totalmente transparente por el Llavero del sistema operativo; el usuario no tiene que inventar, recordar ni escribir una contraseña propia de Nexo.
- **Sincronización de secretos en la nube.** Los secretos y la base de datos permanecen estrictamente locales en la máquina del usuario.

## Supuestos asumidos

- El Llavero del sistema operativo (Keychain en macOS) sigue siendo la raíz de confianza (*Root of Trust*).
- Se utiliza el estándar criptográfico `AES-256-GCM` (mediante el crate `aes-gcm` estándar en el ecosistema Rust).
- Si el Llavero del sistema está bloqueado o inaccesible, `ReportingSecretStore` continuará registrando el fallo de forma visible en el panel como hasta ahora.

## Riesgos

- **Corrupción de datos durante la migración**: Mitigado validando que cada secreto se pueda descifrar correctamente tras escribirse antes de eliminar la entrada antigua del Llavero.

## Invariantes que esto no puede romper

- **1. Ningún secreto en texto plano en SQLite.** Se cumple estrictamente: SQLite almacena únicamente datos cifrados mediante AES-256-GCM autenticado con clave maestra externa.
- **2. Nunca degradar en silencio.** Si la clave maestra no se puede obtener o descifrar, se devuelve el error correspondiente y se refleja en el panel.
