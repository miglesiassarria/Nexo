# 0017 · Límite de tamaño de peticiones de chat y archivos

- **Estado:** hecho
- **Creada:** 2026-08-21
- **Pedida por:** el usuario al reproducir errores HTTP 413 (`Failed to buffer the request body: length limit exceeded`) desde clientes como Msty Go al enviar imágenes multimodales en base64 (~4,3 MB) contra el gateway de Nexo.

## Problema

1. **Límite rígido e invisible de 2 MiB**:
   En `POST /v1/chat/completions`, Axum utiliza por defecto un límite de 2 MiB (`2.097.152 bytes`) al extraer `axum::body::Bytes`. Cualquier payload multimodal (capturas de pantalla, fotos o documentos codificados en base64) que supere los 2 MiB es rechazado inmediatamente con `HTTP 413` en texto plano antes de llegar a la lógica de Nexo, antes de comprobar autenticación y sin quedar registrado en el panel ni en las métricas.
2. **Riesgo de denegación de servicio (DoS) y consumo de memoria**:
   Elevar ingenuamente `DefaultBodyLimit` a varios gigabytes provocaría que peticiones grandes se carguen íntegramente en memoria RAM y se dupliquen durante la deserialización y traducción, pudiendo provocar cierres abruptos por falta de memoria (OOM). Además, clientes no autenticados podrían obligar al servidor a absorber flujos gigantescos.

## Solución propuesta

1. **Límite configurable y dinámico**:
   - Límite predeterminado: **32 MiB** (`33.554.432 bytes`).
   - Rango permitido en la interfaz: **1 MiB a 5 GiB**.
   - Opción avanzada: **«Sin límite impuesto por Nexo»** (representado como `NULL` en la base de datos, no como cero).
   - Persistido en la tabla `settings` (clave `max_request_body_bytes`) en bytes enteros y aplicado en caliente a nuevas peticiones sin necesidad de reiniciar la aplicación.
2. **Seguridad y pre-autenticación**:
   - Inspeccionar y validar la cabecera `Authorization: Bearer ...` (y el estado de pausa de Nexo) antes de consumir o derivar el cuerpo de la petición. Los clientes no autenticados reciben `HTTP 401` inmediato sin descargar el cuerpo.
3. **Ingestión híbrida protegida (Memoria / Disco)**:
   - Los cuerpos de hasta un umbral razonable (p. ej. 4 MiB) se gestionan directamente en buffer de memoria.
   - Los cuerpos que superen el umbral de memoria se canalizan mediante streaming hacia un archivo temporal seguro en disco (con permisos restrictivos `0600` en macOS/Linux y directorio aislado).
   - Los archivos temporales se limpian mediante RAII (en `Drop`), en caso de error, cancelación del cliente o desconexión, y se incluye una rutina de limpieza al inicio de Nexo para residuos de cierres inesperados.
4. **Respuestas de error OpenAI-compatibles**:
   - Si una petición supera el límite configurado por Nexo, se devuelve `HTTP 413` con formato JSON estándar:
     ```json
     {
       "error": {
         "message": "La petición supera el tamaño máximo permitido por Nexo (32 MiB).",
         "type": "invalid_request_error",
         "code": "request_too_large",
         "nexo": {
           "kind": "request_too_large",
           "max_bytes": 33554432
         }
       }
     }
     ```
   - Si está activa la opción «Sin límite impuesto por Nexo» y la petición falla por disco lleno o rechazo del proveedor upstream, el error identifica la causa real en lugar de simular un límite superado.
5. **Interfaz de usuario**:
   - Nueva sección en *Configuración*: **Peticiones y archivos**.
   - Control numérico con selector de unidad (MiB / GiB).
   - Botón para restaurar el valor predeterminado de 32 MiB.
   - Opción avanzada con diálogo de confirmación para «Sin límite impuesto por Nexo».
   - Advertencias explícitas sobre la inflación de base64 (~+33%), valores superiores a 512 MiB y exposición en red local (LAN).

## Criterios de Aceptación

1. **Prueba de regresión (TDD)**:
   - Una prueba contra el router con un payload de ~3-4 MB falla con el 413 de Axum antes del arreglo y pasa limpiamente tras la corrección.
2. **Valor predeterminado y rango**:
   - En una instalación limpia, el valor por defecto es 32 MiB y permite procesar imágenes de 4–5 MB (como la captura de Msty Go) tanto con `stream: true` como con `stream: false`.
   - Al configurar 1 MiB, una petición de 2 MiB es rechazada con `HTTP 413` y el JSON estructurado de error.
   - Al ampliar el límite (p. ej. a 64 MiB), la misma petición de 2 MiB pasa inmediatamente sin reiniciar Nexo.
3. **Persistencia y semántica**:
   - Valores hasta 5 GiB se persisten en bytes sin desbordamiento entero.
   - «Sin límite impuesto por Nexo» se persiste como `NULL` y no se confunde con cero ni bloquea el tráfico.
4. **Seguridad y Recursos**:
   - Peticiones sin token o con token revocado son rechazadas con 401 antes de recibir cuerpos grandes.
   - Cuerpos que superan el límite no se transmiten al proveedor upstream ni computan como coste/uso.
   - Cuerpos grandes se procesan sin mantener múltiples copias duplicadas completas en RAM.
   - Los archivos temporales se eliminan siempre tras completarse la petición o ante fallos/cancelaciones.
   - Al iniciar Nexo se eliminan archivos temporales huérfanos anteriores.
5. **Alcance de rutas**:
   - El límite se aplica a `/v1/chat/completions` sin modificar el comportamiento de `/healthz` ni `/v1/models`.
6. **Interfaz de usuario**:
   - La vista de Configuración permite editar, guardar y restaurar el límite, mostrando advertencias en valores altos (> 512 MiB) y exigiendo confirmación para la opción sin límite.
7. **Verificación integral**:
   - `cargo test --workspace`, `cargo clippy --workspace --all-targets` y `npm run check` pasan con 0 errores y 0 advertencias.
   - La aplicación instalada en `/Applications/Nexo.app` funciona correctamente con Msty Go y la captura de pantalla que provocaba el fallo.

## Fuera de Alcance

- Modificar el cliente Msty Go o exigirle el uso de endpoints propietarios.
- Prometer que todos los proveedores externos aceptarán archivos de 5 GiB si el proveedor upstream impone límites inferiores.
- Implementar un sistema de almacenamiento permanente de archivos o bucket externo.
