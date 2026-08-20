# ADR 0006: Clave maestra en el almacén seguro y credenciales cifradas localmente

- **Fecha:** 2026-08-20
- **Estado:** aceptada
- **Decide:** Manuel Iglesias

## Contexto

Nexo almacena cada credencial (API keys de OpenAI/OpenRouter/Gemini/Zen, tokens de acceso y refresh de ChatGPT, tokens recuperables de aplicación) como una entrada independiente en el almacén seguro del sistema operativo (Keychain en macOS).

En macOS, el control de acceso del Llavero (*Keychain Access Control*) vincula la autorización a la firma digital exacta del binario. En desarrollo y despliegues locales sucesivos (`npm run app:install`), cada nueva versión compilada tiene una firma ad-hoc distinta. Al arrancar, Nexo descubre el catálogo consultando concurrentemente todas las cuentas conectadas (`resolve_credential`), lo que provoca que macOS muestre de **4 a 6 diálogos independientes de confirmación de contraseña** en cada nuevo despliegue.

## Decisión

Nexo pasa a utilizar un esquema de **clave maestra de cifrado** (*Master Encryption Key*):

1. **Un único secreto en el Llavero del sistema**: Nexo guarda una clave simétrica de 256 bits (`com.nexo.gateway / master_key`). Si no existe en el primer arranque, genera 32 bytes aleatorios criptográficamente seguros y los guarda en el Llavero.
2. **Almacén cifrado en SQLite**: Todos los secretos individuales (`SecretRef`) se cifran mediante **AES-256-GCM** con un vector de inicialización / nonce aleatorio único por cada guardado, y se persisten en una tabla dedicada (`encrypted_secrets`) de `nexo.sqlite`.
3. **Migración automática y transparente**: En el primer arranque con este esquema, si existen secretos antiguos guardados como entradas individuales en el Llavero (`account/*`, `app/*`), Nexo los lee, los migra al almacén cifrado y limpia las entradas antiguas del Llavero para no dejar credenciales huérfanas.

### Alternativas descartadas

- **Mantener N entradas individuales en el Llavero.** Obliga a confirmar la contraseña N veces por cada compilación/despliegue local, degradando severamente la experiencia de desarrollo y actualización.
- **Guardar secretos en texto plano en SQLite.** Se descarta de forma categórica por violar la seguridad del producto: un volcado o copia accidental de `nexo.sqlite` expondría todas las claves y tokens.
- **Agrupar todas las credenciales en un único JSON en el Llavero.** Aunque reduce a 1 las peticiones, cualquier actualización parcial reescribe el blob entero en el Llavero del SO y acopla el ciclo de vida de todas las credenciales.

## Riesgos y garantías de seguridad

1. **Garantía de seguridad equivalente**:
   - Un volcado de `nexo.sqlite` sigue siendo completamente inútil para un atacante sin acceso al Llavero del sistema operativo (el descifrado de las credenciales requiere la clave maestra almacenada en el Llavero).
   - El Llavero del sistema sigue siendo la raíz de confianza (*Root of Trust*).
2. **Disminución de fricción**:
   - Al arrancar Nexo, solo se realiza **una única petición** al Llavero para recuperar la clave maestra. En cada nuevo despliegue, macOS solicitará la confirmación como máximo una sola vez.
3. **Persistencia en memoria del proceso**:
   - La clave maestra se mantiene en memoria durante la ejecución del proceso para que las operaciones cotidianas del gateway no consulten repetidamente el subsistema del SO.

## Consecuencias arquitectónicas

1. **Invariante 1 de `CLAUDE.md`**:
   Se actualiza para clarificar que los secretos se almacenan cifrados con una clave maestra custodiada por el almacén seguro del sistema, garantizando que ningún secreto exista en texto plano en SQLite.
2. **Desacoplamiento intacto**:
   La abstracción `SecretStore` (`set`, `get`, `delete`) y su envoltorio `ReportingSecretStore` se mantienen idénticos; únicamente la implementación respaldada por el sistema (`SystemSecretStore` / `EncryptedVaultSecretStore`) pasa a usar la clave maestra y la persistencia cifrada.
