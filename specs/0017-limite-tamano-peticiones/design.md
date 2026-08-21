# 0017 · Diseño: Límite de tamaño de peticiones de chat y archivos

- **Estado:** hecho
- **Fichero:** `specs/0017-limite-tamano-peticiones/design.md`
- **Spec:** [`spec.md`](spec.md)
- **ADR asociado:** [`docs/adr/0007-limite-de-tamano-e-ingestion-de-cuerpos-grandes.md`](../../docs/adr/0007-limite-de-tamano-e-ingestion-de-cuerpos-grandes.md)

## Componentes Afectados

### 1. Base de datos y configuración (`crates/nexo-core/src/db/`)
- En `Db`:
  - `max_request_body_bytes(&self) -> Result<Option<u64>>`: lee la clave `max_request_body_bytes` de `settings`.
    - Si no existe en `settings`: devuelve `Some(32 * 1024 * 1024)` (32 MiB por defecto).
    - Si el valor es `"null"` o `""`: devuelve `None` (sin límite impuesto por Nexo).
    - Si contiene un entero: devuelve `Some(bytes)`.
  - `set_max_request_body_bytes(&self, bytes: Option<u64>) -> Result<()>`:
    - Valida que si `Some(v)` está presente, cumpla `1 MiB <= v <= 5 GiB`.
    - Persiste en `settings` como cadena de entero o `"null"`.

### 2. Receptor de cuerpo HTTP seguro (`crates/nexo-core/src/gateway/body.rs` / `routes.rs`)
- Extractor / Receptor `RequestBodyReceiver`:
  - Lee el límite actual desde `nexo.db().max_request_body_bytes()`.
  - Consume el stream del cuerpo (`axum::body::Body`) chunk a chunk midiendo los bytes recibidos.
  - Si los bytes superan el límite configurado (`max_bytes`), aborta inmediatamente el stream y devuelve `HTTP 413` con el JSON de error OpenAI:
    ```json
    {
      "error": {
        "message": "La petición supera el tamaño máximo permitido por Nexo.",
        "type": "invalid_request_error",
        "code": "request_too_large",
        "nexo": {
          "kind": "request_too_large",
          "max_bytes": 33554432
        }
      }
    }
    ```
  - **Umbral de memoria de 4 MiB**:
    - Hasta 4 MiB se mantiene en un buffer `Vec<u8>`.
    - Si supera 4 MiB, crea un archivo temporal seguro en el directorio de datos de Nexo (`temp/req_XXXXXX.tmp` con permisos `0600`) y continúa transmitiendo el stream al archivo.
    - Devuelve una abstracción `BufferedPayload` (en memoria o respaldada en archivo temporal con implementación de `Read` y `Drop` limpiador).
  - Al deserializar el JSON (`serde_json::from_reader`), se lee directamente de `BufferedPayload`.

### 3. Pre-autenticación y orden en el Handler
- En `crates/nexo-core/src/gateway/routes.rs`:
  - Antes de leer el cuerpo, verificar que la cabecera `Authorization: Bearer <token>` esté presente y corresponda a una aplicación activa en la base de datos (y comprobar si Nexo está pausado).
  - Si la autenticación falla, responder `401 Unauthorized` de inmediato sin descargar el cuerpo.

### 4. Limpieza de temporales huérfanos (`crates/nexo-core/src/service.rs`)
- Al iniciar `Nexo::new(...)`:
  - Revisar el directorio de temporales de Nexo y purgar cualquier archivo residual `req_*.tmp` dejado por un apagado forzoso previo.

### 5. Comandos Tauri e Interfaz Svelte (`src-tauri/src/commands/` y `src/`)
- Comandos Tauri:
  - `get_max_request_body_bytes() -> Result<Option<u64>>`
  - `set_max_request_body_bytes(bytes: Option<u64>) -> Result<()>`
- En la vista de Configuración:
  - Nueva tarjeta **Peticiones y archivos**.
  - Campo numérico, selector de unidades (MiB / GiB).
  - Checkbox para «Sin límite impuesto por Nexo» con modal de confirmación.
  - Botón «Restaurar predeterminado (32 MiB)».
  - Texto con equivalencia en bytes y advertencias contextuales (> 512 MiB, base64 +33%, red local).
