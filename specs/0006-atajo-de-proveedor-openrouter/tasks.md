# 0006 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una.

- [x] **T1.** Añadir `OPENROUTER: ProviderPreset` (`base_url =
      "https://openrouter.ai/api/v1"`, `docs_url =
      "https://openrouter.ai/models"`) y meterlo en `presets()`. Prueba nueva
      que comprueba que el atajo se ofrece con esos datos exactos, y que
      convive con el de Zen sin identificadores repetidos.
  - Ficheros: `crates/nexo-core/src/provider/openai_compat.rs`
  - Verificación: `cargo test -p nexo-core -- openrouter_shortcut`

- [x] **T2.** Cambiar el ejemplo «OpenRouter» por «Groq» en el texto de la
      opción genérica «Otro servicio OpenAI-compatible». Prueba nueva que
      falla si ese texto vuelve a mencionar «OpenRouter».
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core -- the_generic_compatible_option`

- [x] **T3.** Verificación completa y recorrido manual: confirmar en la app
      instalada que el atajo aparece en «Añadir proveedor» con nombre y URL ya
      puestos. Si hay una API key real de OpenRouter disponible, conectarlo y
      confirmar que el catálogo trae precio y capacidades reales tras
      refrescarlo; si no la hay, se deja dicho explícitamente que ese punto no
      se pudo comprobar contra la realidad.
  - Ficheros: ninguno
  - Verificación: `npm run app:install` (hecho, compilado e instalado). **No
    verificado por clic real**: pedía navegar a Proveedores → Añadir proveedor,
    lo mismo que en la spec 0005 requería el acceso de accesibilidad que no se
    concedió. Cubierto en su lugar por el test de T1, que comprueba
    exactamente los datos que `Providers.svelte` va a pintar (el frontend lee
    `connect_options()` sin ningún nombre de proveedor hardcodeado). **Tampoco
    verificado**: el criterio 5 (catálogo real con clave de OpenRouter) — no
    se aportó ninguna clave, tal como la propia spec anticipaba en "Fuera de
    alcance".

## Cierre

- [x] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check` — 263 tests + 24 e2e en verde, clippy sin avisos, check sin errores
- [x] Aplicación de macOS compilada **e instalada**: `npm run app:install` — compilado e instalado `Aug 2 11:26:23 2026`
- [x] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [x] Documentación actualizada si lo aprendido contradice lo escrito — nada contradecía lo escrito, no hizo falta
- [x] `specs/README.md` actualizado
