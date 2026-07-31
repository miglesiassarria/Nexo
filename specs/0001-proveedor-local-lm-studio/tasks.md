# 0001 · Tareas

El repositorio queda funcionando después de cada tarea.

- [x] **T0. Averiguar lo que falta antes de diseñar sobre suposiciones.**
  Preguntar a LM Studio de verdad: si `/v1/chat/completions` devuelve `usage`, si
  respeta `stream_options.include_usage`, y cuánto tarda la primera petición a un
  modelo `not-loaded`. Anotar los resultados en `design.md`.
  - Ficheros: `specs/0001-.../design.md`
  - Verificación: `curl` contra `127.0.0.1:1234` con y sin `stream`, y los hallazgos
    escritos en la sección «Qué queda pendiente de descubrir»

- [x] **T1. Extraer la traducción de `chat/completions` a un módulo compartido.**
  Sacar `build_chat_completions` y `translate_chunk` de `openai_apikey.rs` a
  `translate/chat_completions.rs`, junto con el helper que convierte una respuesta
  HTTP en `EventStream`. Sin cambios de comportamiento.
  - Ficheros: `crates/nexo-core/src/translate/chat_completions.rs`,
    `crates/nexo-core/src/translate/mod.rs`,
    `crates/nexo-core/src/provider/openai_apikey.rs`
  - Verificación: `cargo test -p nexo-core openai_apikey` sigue en verde con las
    mismas pruebas, ahora sobre el módulo compartido

- [x] **T2. El adaptador de LM Studio.**
  `catalog()` por `/api/v0/models` con respaldo a `/v1/models`, `stream()` por la
  superficie compatible reutilizando T1, `health()` comprobando que el servidor
  responde. Mapeo de `type` a capacidades según D2.
  - Ficheros: `crates/nexo-core/src/provider/lmstudio.rs`,
    `crates/nexo-core/src/provider/mod.rs`
  - Verificación: `cargo test -p nexo-core lmstudio` con la muestra real capturada;
    cubre criterios 1, 2 y 3

- [x] **T3. Ajuste de dirección y registro del adaptador.**
  `lmstudio_base_url` en configuración, persistido, y el adaptador registrado en el
  servicio con esa dirección.
  - Ficheros: `crates/nexo-core/src/config.rs`, `crates/nexo-core/src/db/mod.rs`,
    `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core settings` comprueba el ida y vuelta del
    ajuste

- [x] **T4. Detección y conexión.**
  `detect_lmstudio()` que confirma la forma de la respuesta (D5), crea la cuenta
  local y refresca su catálogo. Se llama al arrancar, sin bloquear, y a demanda.
  - Ficheros: `crates/nexo-core/src/service.rs`, `src-tauri/src/commands.rs`,
    `src-tauri/src/main.rs`
  - Verificación: `cargo test -p nexo-core detect` con una respuesta con forma
    equivocada, que debe rechazarse

- [x] **T5. Prueba de extremo a extremo contra LM Studio real.**
  Chat con y sin streaming, texto reensamblado idéntico, evento registrado con
  `credential_kind = "local"`, latencia y tiempo al primer token.
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs` (marcada `#[ignore]` para que
    no rompa el CI sin LM Studio), más ejecución manual
  - Verificación: `cargo test -p nexo-core --test gateway_e2e -- --ignored lmstudio`
    con LM Studio abierto; cubre criterios 4, 5, 6 y 9

- [x] **T6. Interfaz: proveedor, catálogo y coste.**
  Sección de LM Studio en *Proveedores* con estado y dirección editable; columnas de
  cuantización y estado de carga en *Modelos*; etiqueta «Local» en el coste del panel
  (D4).
  - Ficheros: `src/lib/api.ts`, `src/lib/views/Providers.svelte`,
    `src/lib/views/Models.svelte`, `src/lib/views/Dashboard.svelte`
  - Verificación: `npm run check` sin errores, y comprobación visual con la app
    compilada

- [x] **T7. Comportamiento con LM Studio apagado.**
  Mensaje de error que nombra la dirección y dice qué hacer; estado reflejado en la
  interfaz.
  - Ficheros: `crates/nexo-core/src/provider/lmstudio.rs`
  - Verificación: cerrar LM Studio, pedir chat y comprobar el cuerpo del error;
    cubre criterio 7

- [x] **T8. Documentación.**
  `docs/producto.md` (los modelos locales ya no son futuro), `docs/contrato-proveedor.md`
  (un tercer adaptador entró sin tocar el núcleo), `README.md` y `ROADMAP.md`.
  - Ficheros: los citados
  - Verificación: los enlaces resuelven y nada afirma lo contrario de lo medido

## Cierre

- [x] Verificación del repositorio: 194 pruebas, 0 avisos de clippy, 0 errores de svelte
- [x] Artefacto de macOS: `Nexo.app` y `Nexo_0.1.0_aarch64.dmg`, 2026-07-31 15:27
- [x] Criterios de aceptación repasados uno por uno: los 10 cumplidos
- [x] Documentación actualizada: producto, contrato, README, ROADMAP y el propio diseño
- [x] `specs/README.md` actualizado
