# 0012 · Tareas

Orden pensado para que el repositorio compile y pase pruebas después de cada
una. Se quita el uso antes que la pieza, y la pieza antes que la dependencia.

- [x] **T1 · `net::listening_addresses()`**
  `crates/nexo-core/src/net.rs`. Enumera con `if-addrs`, filtra loopback e
  IPv6, marca la de `detect_lan_ip()` como preferida y la pone primera.
  Verificación: `cargo test -p nexo-core net::`

- [x] **T2 · `LanAccessInfo` cambia de forma y `prepare_gateway_bind` se
  simplifica**
  `crates/nexo-core/src/service.rs`. `GatewayBindPlan` vuelve a `{ addr }`;
  `prepare_gateway_bind` con `allow_lan` publica las direcciones y no toca
  ningún certificado. Aquí desaparece el camino de error del certificado.
  Verificación: `cargo test -p nexo-core prepare_gateway_bind lan_info`

- [x] **T3 · `main.rs` vuelve a un solo listener**
  `src-tauri/src/main.rs`. Un `bind` + `serve_on`, sin rama de TLS ni segundo
  listener.
  Verificación: `cargo clippy -p nexo --all-targets`

- [x] **T4 · Fuera `serve_on_tls` y el módulo `tls_cert`**
  `crates/nexo-core/src/gateway/mod.rs`, `crates/nexo-core/src/lib.rs`, y
  borrar `crates/nexo-core/src/tls_cert.rs`. Fuera también el módulo de prueba
  `tls_from_reserved_listener` y los tres tests de TLS de
  `crates/nexo-core/tests/gateway_e2e.rs`.
  Verificación: `test ! -f crates/nexo-core/src/tls_cert.rs && cargo test --workspace`

- [x] **T5 · Fuera `rcgen` y `axum-server`**
  `crates/nexo-core/Cargo.toml`.
  Verificación: `! grep -rn "rcgen\|axum.server\|axum_server" crates/ src-tauri/src src/ Cargo.lock --include='*.rs' --include='*.toml' && cargo build --workspace`

- [x] **T6 · e2e: se sirve por la red en HTTP plano, y sigue exigiendo token**
  `crates/nexo-core/tests/gateway_e2e.rs`.
  Verificación: `cargo test -p nexo-core --test gateway_e2e lan_mode`

- [x] **T7 · El aviso dice que no va cifrado**
  `src-tauri/src/commands.rs` + su prueba.
  Verificación: `cargo test -p nexo lan_risk_notice`

- [x] **T8 · Interfaz: lista de direcciones, aviso, y fuera lo del certificado**
  `src/lib/api.ts`, `src/lib/views/Settings.svelte`.
  Verificación: `npm run check`

- [x] **T9 · Cierre**
  Verificación completa del repositorio, `npm run app:install`, y comprobación
  contra la realidad por la IP de red con `curl`. Repasar los 8 criterios de
  `spec.md` uno por uno e informar del resultado real.
  Verificación: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check && npm run app:install`

## Cierre (2026-08-17)

- `cargo test --workspace`: 1 + 292 + 30, 0 fallos, 16 ignoradas (las de red real
  con clave de proveedor). `cargo clippy --workspace --all-targets` y
  `npm run check` limpios.
- Compilado e instalado: `Aug 17 23:06:34 2026`, las dos horas iguales.
- Los 8 criterios comprobados uno por uno, todos cumplidos. El 8, contra la
  instalación real:
  - `http://192.168.11.230:8787/v1/chat/completions` con la clave de «Msty GO
    Macbook 16» y el modelo `opencode-go/deepseek-v4-flash` responde «por la
    red», 145 tokens. Sin certificado y sin aceptar nada.
  - Sin token, por la red: `401`.
  - `https://192.168.11.230:8787` no responde (curl exit 35, fallo de
    handshake): ya no hay nadie escuchando por TLS.
  - `http://127.0.0.1:8787/v1/models`: `200`. El modo local no cambió.
  - `lsof` confirma **un** solo listener: `*:8787`.
- Lo que quedó fuera y sigue fuera: los ficheros del certificado antiguo en
  `~/Library/Application Support/Nexo/tls/`. Ya no se usan; se borran a mano.
