# 0005 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una.

El orden va de dentro afuera: primero la capa de datos (donde está el criterio 2
y el 3, los más arriesgados porque cambian una firma con muchas llamadas), luego
el comando Tauri que la envuelve, y al final la interfaz. Así cada tarea deja el
workspace compilando y los tests en verde, sin ningún estado a medias.

- [ ] **T1.** `recent_requests(&self, since_ms: i64, limit: i64)`: nuevo parámetro
      y `WHERE r.ts >= ?1` en la consulta. Actualizar los ~12 sitios que hoy la
      llaman con un solo argumento (`service.rs`, `gateway_e2e.rs`) a pasar `0`
      como primer argumento — mismo comportamiento que hoy, sin filtro. Prueba
      nueva: una fila con `ts` anterior a la ventana no aparece; una posterior sí.
  - Ficheros: `crates/nexo-core/src/db/stats.rs`, `crates/nexo-core/src/service.rs`, `crates/nexo-core/tests/gateway_e2e.rs`
  - Verificación: `cargo test -p nexo-core -- recent_requests`

- [ ] **T2.** `recent_requests` incorpora `r.input_tokens, r.output_tokens` al
      `SELECT` y `RequestRow` gana esos dos campos (`Option<i64>`), junto al
      `total_tokens` que ya tenía. Prueba: una fila con entrada=10/salida=20
      devuelve esos dos valores por separado, no solo el total.
  - Ficheros: `crates/nexo-core/src/db/stats.rs`
  - Verificación: `cargo test -p nexo-core -- recent_requests`

- [ ] **T3.** `usage_summary` bifurca por tamaño de ventana: ventanas de 24h o
      menos (`util::now_ms() - since_ms <= 86_400_000`) consultan `requests`
      directamente con la misma forma de fila; el resto sigue usando
      `usage_hourly` sin cambios. Pruebas: una ventana de 1h excluye una fila de
      hace 2h (criterio 2); los tests existentes con `since_ms = 0` siguen en
      verde sin tocarlos, prueba de que la rama larga no cambió.
  - Ficheros: `crates/nexo-core/src/db/stats.rs`
  - Verificación: `cargo test -p nexo-core -- usage_summary`

- [ ] **T4.** Comandos Tauri `usage_summary` y `recent_requests` pasan a recibir
      `minutes: i64` (sustituye a `days` en el primero, se añade en el segundo);
      `DAY_MS` se sustituye por `MINUTE_MS = 60_000`. Los dos calculan `since` a
      partir de `util::now_ms()` igual que hoy.
  - Ficheros: `src-tauri/src/commands.rs`
  - Verificación: `cargo clippy --workspace --all-targets` sin avisos

- [ ] **T5.** Tipos y envoltorios en la interfaz: `api.usageSummary(minutes,
      group, operation)`, `api.recentRequests(limit, minutes)`, y `RequestRow`
      con `input_tokens`/`output_tokens`.
  - Ficheros: `src/lib/api.ts`
  - Verificación: `npm run check`

- [ ] **T6.** `Dashboard.svelte`: el estado `days` pasa a `minutes`; el
      `<select>` de periodo gana 1/2/5/12 horas junto a las opciones ya
      existentes (ahora expresadas en minutos: 1440/10080/43200/129600); las dos
      llamadas (`usageSummary`, `recentRequests`) usan el mismo valor.
  - Ficheros: `src/lib/views/Dashboard.svelte`
  - Verificación: `npm run check`

- [ ] **T7.** `Dashboard.svelte`: en «Uso agregado» y en «Últimas peticiones», la
      columna «Tokens» se sustituye por «Entrada» y «Salida»; en «Últimas
      peticiones» las dos celdas nuevas llevan la misma insignia de
      estimado/no-disponible que hoy lleva «Tokens», usando el `usage_source` de
      la fila.
  - Ficheros: `src/lib/views/Dashboard.svelte`
  - Verificación: `npm run check`; recorrido manual con `npm run app:install` — elegir cada periodo nuevo y comprobar que ambos paneles cambian juntos

## Cierre

- [ ] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`
- [ ] Aplicación de macOS compilada **e instalada**: `npm run app:install`, con las dos horas
- [ ] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [ ] Documentación actualizada si lo aprendido contradice lo escrito
- [ ] `specs/README.md` actualizado
