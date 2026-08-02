# 0007 · Tareas

Cada tarea cabe en una sesión, dice qué toca y **cómo se comprueba**. El
repositorio queda funcionando después de cada una. Orden pensado para
verificar primero el punto más incierto del diseño (T1) antes de construir
nada encima.

- [ ] **T1.** Añadir `axum-server` (`tls-rustls`) y `rcgen` a
      `crates/nexo-core/Cargo.toml`. Antes de tocar nada más, un test mínimo
      que sirve un `Router` de axum trivial sobre TLS a partir de un
      `tokio::net::TcpListener` **ya reservado** (convertido con
      `into_std()`), con un certificado autofirmado generado in-line con
      `rcgen` solo para esa prueba, y hace una petición HTTPS real contra él
      aceptando ese certificado concreto. Es la comprobación de la decisión 5
      del diseño: si `axum-server` 0.8.0 no ofrece esa forma exacta, se
      descubre aquí, no después de construir `tls_cert.rs` y `serve_on_tls`
      encima de una suposición.
  - Ficheros: `crates/nexo-core/Cargo.toml`, `crates/nexo-core/src/gateway/mod.rs` (test temporal, puede quedarse como base de T4)
  - Verificación: `cargo test -p nexo-core -- serves_https_from_an_already_bound_listener`

- [ ] **T2.** `net::detect_lan_ip()` con el truco del socket UDP sin conectar
      de verdad. Devuelve `Option<IpAddr>`, nunca entra en pánico, nunca
      manda tráfico.
  - Ficheros: `crates/nexo-core/src/net.rs` (nuevo), `crates/nexo-core/src/lib.rs`
  - Verificación: `cargo test -p nexo-core -- detect_lan_ip`

- [ ] **T3.** `tls_cert::ensure(data_dir)`: genera el certificado si no
      existe (SAN con la IP de `net::detect_lan_ip()` si hay alguna, más
      `127.0.0.1`, `::1` y `localhost`; expiración a décadas vista, no el
      valor por defecto de `rcgen`), lo reutiliza si ya existe y es válido,
      devuelve `Err` sin regenerar si el fichero existe pero está corrupto o
      no se puede leer. `key.pem` con permisos `0600`, directorio `tls/` con
      `0700`.
  - Ficheros: `crates/nexo-core/src/tls_cert.rs` (nuevo), `crates/nexo-core/src/lib.rs`
  - Verificación: `cargo test -p nexo-core -- tls_cert` (cubre: generación y
    reutilización con el mismo fingerprint, expiración lejana, fichero
    corrupto reportado sin regenerar, permisos del fichero de clave)

- [ ] **T4.** `gateway::serve_on_tls(nexo, listener, cert: &LanCertificate)`,
      construida sobre el mecanismo verificado en T1 pero con un
      `tls_cert::LanCertificate` real. Test e2e: arranca en un directorio de
      datos temporal, `tls_cert::ensure()` de verdad, conecta con un cliente
      HTTP que confía en ese certificado concreto (no en la CA del sistema),
      manda una petición con el token de aplicación de siempre y recibe
      respuesta válida; sin token, recibe 401 — igual que hoy en local.
  - Ficheros: `crates/nexo-core/src/gateway/mod.rs`
  - Verificación: `cargo test -p nexo-core -- serve_on_tls`

- [ ] **T5.** `Settings::bind_addr()` devuelve `0.0.0.0:{port}` cuando
      `allow_lan` es `true`. Reescribe el test
      `allow_lan_does_not_widen_the_bind_yet` (la premisa que probaba ya no
      es cierta) como `allow_lan_widens_the_bind`.
  - Ficheros: `crates/nexo-core/src/config.rs`
  - Verificación: `cargo test -p nexo-core -- bind_addr`

- [ ] **T6.** `Nexo::prepare_gateway_bind(&self, settings, data_dir) ->
      GatewayBindPlan { addr, tls: Option<LanCertificate> }`: si
      `!allow_lan`, plan local de siempre. Si `allow_lan` y `tls_cert::ensure`
      tiene éxito, plan con `0.0.0.0` y el certificado. Si `allow_lan` y
      `tls_cert::ensure` falla, plan con `127.0.0.1` (sin TLS) y llama a
      `self.set_bind_error(Some(detalle))` con el motivo exacto — nunca cae a
      `0.0.0.0` sin TLS. Añade también el estado `lan_info` (mismo patrón que
      `bind_error`) y lo expone en `GatewayStatus.lan`.
  - Ficheros: `crates/nexo-core/src/service.rs`
  - Verificación: `cargo test -p nexo-core -- prepare_gateway_bind`
    (cubre: modo local sin cambios, modo red con éxito, modo red con
    certificado corrupto cae a local con `bind_error` visible)

- [ ] **T7.** `main.rs` usa `prepare_gateway_bind()` para decidir la
      dirección antes de la reserva síncrona del puerto que ya existe, y
      elige `serve_on` o `serve_on_tls` según el plan devuelto. Ninguna otra
      rama del arranque cambia.
  - Ficheros: `src-tauri/src/main.rs`
  - Verificación: `cargo check -p nexo` + revisión manual del flujo (este
    binario no tiene batería de pruebas propia; la lógica que decide ya
    quedó cubierta por los tests de T6)

- [ ] **T8.** Comando `lan_risk_notice() -> RiskNotice` con el aviso
      concreto (qué implica activar el modo red: quién queda expuesto, qué
      no protege el certificado autofirmado, que las redes VPN/ajenas también
      quedan expuestas si están activas). `save_settings` gana el parámetro
      `lan_risk_acknowledged: bool` y rechaza el guardado si
      `settings.allow_lan && !lan_risk_acknowledged`.
  - Ficheros: `src-tauri/src/commands.rs`
  - Verificación: `cargo check -p nexo` (los comandos de Tauri no tienen
    tests unitarios propios en este repo; se verifica junto con T9 en la app
    instalada)

- [ ] **T9.** Interfaz: interruptor "Permitir acceso desde mi red local" en
      Configuración, desactivado por defecto; al activarlo muestra el aviso
      de `lan_risk_notice()` con su casilla de confirmación (mismo patrón
      visual que el aviso de suscripción OAuth en Proveedores) antes de poder
      guardar. Con el modo activo y el gateway ya arrancado en red, muestra
      la dirección para conectar desde otro equipo y la huella del
      certificado. El mensaje de "reiniciar para aplicar" ya existente se
      aplica igual a este campo.
  - Ficheros: `src/lib/api.ts`, `src/lib/views/Settings.svelte`
  - Verificación: `npm run check`; verificación manual en la app instalada
    (activar el interruptor, guardar, reiniciar, comprobar que el panel
    muestra dirección y huella)

- [ ] **T10.** Regresión de punta a punta del criterio de aceptación 1: con
      `allow_lan = false` (el valor por defecto, sin tocar nada), un test e2e
      completo arranca Nexo, comprueba `bind_addr` en `127.0.0.1`, hace una
      petición sin token (401) y con token (200), y comprueba que no existe
      ningún fichero en `<data_dir>/tls/`. Este es el test que demuestra que
      todo lo anterior no cambió el camino que ya funcionaba.
  - Ficheros: `crates/nexo-core/tests/gateway_e2e.rs`
  - Verificación: `cargo test -p nexo-core -- allow_lan_false_is_identical_to_today`

## Cierre

- [ ] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check`
- [ ] Aplicación de macOS compilada **e instalada**: `npm run app:install`, con la hora de compilación y de instalación
- [ ] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real (los que dependen de clic manual en la interfaz, señalados como tales si no se pudo verificar por accesibilidad)
- [ ] Documentación actualizada si lo aprendido contradice lo escrito
- [ ] `specs/README.md` actualizado a `hecho`
