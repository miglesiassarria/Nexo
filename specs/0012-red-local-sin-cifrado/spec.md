# 0012 · Red local sin cifrado

- **Estado:** build
- **Creada:** 2026-08-17
- **Pedida por:** el usuario, al descubrir que el certificado del modo red iba
  atado a la IP de la máquina que sirve Nexo: «no tiene ningún sentido y es
  totalmente inaceptable para mí […] son o pueden ser ordenadores portátiles y
  ese certificado no tiene que ir vinculado a ninguna IP. Es más, si podemos
  ahorrarnos el tema del certificado y simplemente servir el acceso a los
  modelos sin ningún certificado. Esto tendría mejor encaje para mí». Se le
  ofreció la alternativa de atar el certificado a `<equipo>.local` como opción
  recomendada, con el coste de la otra detallado; eligió quitar el cifrado.
  Decisión registrada en el [ADR 0005](../../docs/adr/0005-red-local-sin-cifrado.md).

## Problema

Activar «permitir acceso desde mi red local» sirve HTTPS con un certificado
autofirmado atado a **una** IP: la de la interfaz por la que sale la ruta por
defecto. En un portátil eso caduca cada vez que cambias de red. Las
consecuencias, todas comprobadas en la instalación real del usuario:

- El certificado de su máquina se generó el 2 de agosto con `192.168.0.19`.
  Hoy está en `192.168.11.230`, y cualquier cliente de la red lo rechazaba.
- Sus otras dos direcciones (`192.168.139.3`, `192.168.215.0`, bridges de
  máquina virtual) nunca estuvieron cubiertas: por ahí el rechazo es
  permanente.
- Con el arreglo del [PR #18](https://github.com/miglesiassarria/Nexo/pull/18)
  el certificado se rehace cuando deja de cubrir la dirección actual, lo que
  evita el rechazo pero obliga a volver a aceptarlo en cada cliente en cada
  cambio de red. «Aceptar una vez por dispositivo» nunca fue una vez.

El usuario no quiere que ningún cliente tenga que aceptar nada para consumir
modelos en su propia red, con sus propios equipos. El ADR 0005 acepta el
riesgo y decide servir en HTTP plano.

## Comportamiento esperado

Con «solo este ordenador» (por defecto) **nada cambia**: `127.0.0.1:<puerto>`
en HTTP plano, como siempre.

Con «permitir acceso desde mi red local» activo:

1. El gateway escucha en `0.0.0.0:<puerto>` en **HTTP plano**, un solo
   listener. Los clientes de la propia máquina y los de la red usan el mismo
   `http://…:<puerto>/v1`.
2. No se genera, no se lee y no se anuncia ningún certificado. El módulo
   `tls_cert` y `gateway::serve_on_tls` dejan de existir, y con ellos las
   dependencias `rcgen` y `axum-server`.
3. Sigue haciendo falta el token de aplicación en cada petición, y los límites
   por aplicación siguen siendo obligatorios en las vías de suscripción. Nada
   de eso se toca.
4. El aviso que hay que aceptar para activarlo dice que el tráfico **no va
   cifrado**, con esas palabras, y qué implica: que el token y las
   conversaciones son legibles para quien esté en la misma red.
5. El panel enumera **todas** las direcciones IPv4 no-loopback de la máquina
   con su URL de conexión, no solo la de la ruta por defecto, para que se vea
   por dónde queda expuesto. Era la mitigación del riesgo 3 del ADR 0003,
   nunca implementada; al quitar el cifrado es la principal que queda.
6. Los ficheros de certificado ya generados no se borran solos. El panel dice
   que sobran y dónde están.

## Criterios de aceptación

Cada uno con la orden que lo comprueba.

1. **Modo local intacto.** Con `allow_lan: false`, el plan de arranque es
   `127.0.0.1:<puerto>` y nada más.
   `cargo test -p nexo-core prepare_gateway_bind_with_allow_lan_false_changes_nothing`
2. **Modo red = HTTP plano en `0.0.0.0`, un solo listener.** El plan no tiene
   certificado ni dirección de loopback aparte.
   `cargo test -p nexo-core lan_mode_plans_a_single_plain_listener`
3. **Se sirve de verdad por la red, sin TLS y sin certificado.** Una petición
   `http://<IP de red>:<puerto>/v1/chat/completions` con token válido responde
   `200`; sin token, `401`.
   `cargo test -p nexo-core --test gateway_e2e lan_mode_serves_plain_http_over_the_network`
4. **No queda rastro de TLS en el código.** No existe `crates/nexo-core/src/tls_cert.rs`,
   ni `serve_on_tls`, ni las dependencias `rcgen` y `axum-server`.
   `test ! -f crates/nexo-core/src/tls_cert.rs && ! grep -rn "serve_on_tls\|tls_cert\|axum_server\|rcgen" crates/ src-tauri/src src/`
5. **El panel enumera todas las direcciones de escucha.** `lan_info` devuelve
   las tres direcciones IPv4 no-loopback de esta máquina, no una.
   `cargo test -p nexo-core lan_info_lists_every_listening_address`
6. **El aviso dice que no va cifrado.** El texto de `lan_risk_notice` contiene
   «no va cifrado» y no promete ningún certificado.
   `cargo test -p nexo lan_risk_notice_says_the_traffic_is_not_encrypted`
7. **La verificación del repositorio pasa y la app queda instalada.**
   `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check && npm run app:install`
8. **Contra la realidad, con la app instalada.** Desde esta máquina, una
   conversación real por `http://<IP de red>:8787/v1` con la clave de una
   aplicación existente responde `200`, y `https://` ya no responde nada.
   Comprobación manual con `curl`, informando de la salida real.

## Fuera de alcance

- **Certificado atado a `<equipo>.local`.** Es la alternativa descartada en el
  ADR 0005. Queda registrada ahí como la vía de vuelta si la decisión se
  revierte, no como trabajo pendiente.
- **Acceso desde fuera de la red local.** Sin cifrado, menos discutible que
  antes.
- **Selección de interfaz concreta.** Se enumeran todas las direcciones para
  que el usuario vea la exposición; elegir en cuál escuchar es otra cosa y no
  se pide.
- **Borrar los certificados viejos automáticamente.** Son ficheros del
  usuario; se dice que sobran y dónde están.
- **Autenticación distinta del token.** El token estático por aplicación sigue
  siendo el mecanismo, sin cambios.
- **Aviso al cliente de que el transporte cambió.** Quien tenga configurado
  `https://…` tendrá que cambiarlo a `http://…` a mano; Nexo no puede
  avisarle, porque ya no habrá nadie escuchando por TLS.

## Riesgos

- **El token viaja en claro.** Riesgo principal, aceptado y detallado en el
  ADR 0005. Mitigación: el aviso lo dice, el token es revocable y el modo es
  reversible.
- **Quitar código que funciona.** `tls_cert` y `serve_on_tls` están probados y
  funcionan. Si la decisión se revierte hay que reescribirlos; el ADR 0005
  guarda el por qué y el cómo, y git guarda el código.
- **Los clientes ya configurados con `https://` dejan de conectar.** Solo
  afecta al equipo de la red del usuario, que hoy no conecta de todas formas.

## Supuestos declarados

- Las direcciones que interesa enumerar son las IPv4 no-loopback. IPv6 se
  detecta pero no se anuncia: ningún cliente de los que usa el usuario pide
  una URL IPv6, y añadirlas alargaría la lista sin utilidad.
- `if-addrs` es la forma de enumerar interfaces. Comprobado contra la máquina
  real antes de escribir esto: devuelve `en0 192.168.11.230`,
  `bridge100 192.168.139.3` y `bridge101 192.168.215.0`, y filtra las
  link-local sin activar su característica opcional.
- El puerto sigue siendo el mismo valor configurable de siempre.
