# 0007 · Acceso desde la red local

- **Estado:** hecho
- **Creada:** 2026-08-02
- **Pedida por:** Manuel Iglesias — *"quiero implementar la opcion para
  permitir servir nexo al resto de equipos de la red"*, acotado después a
  *"otros ordenadores de mi red"* (no acceso remoto por Internet) y con la
  condición explícita de que el modo actual, "solo este ordenador", no cambie
  en nada: *"cuando no se exponga hacia fuera no quiero usar autenticacion ni
  certificados ni nada por el estilo [...] debe de comportarse igual que
  ahora"*.

Decisión de arquitectura previa y ya aceptada:
[ADR 0003](../../docs/adr/0003-acceso-desde-la-red-local.md).

## Problema

Nexo hoy solo atiende peticiones desde la propia máquina donde corre
(`127.0.0.1`). El usuario tiene varios ordenadores en su red doméstica y
querría que todos hablaran con la misma instancia de Nexo — las mismas
cuentas conectadas, el mismo catálogo, las mismas estadísticas — en lugar de
tener que instalar y mantener Nexo, y volver a conectar cada proveedor, en
cada máquina por separado.

El código ya deja constancia de que esto se pensó desde el principio y se
aplazó a propósito: `Settings.allow_lan` existe desde el primer commit del
proyecto, documentado como *"desactivado por defecto y sin implementación de
transporte seguro: el gateway se niega a arrancar en 0.0.0.0 mientras no
exista"* (`crates/nexo-core/src/config.rs:14-17`). Esta especificación es la
que por fin implementa esa pieza pendiente.

## Comportamiento esperado

En Configuración aparece un interruptor, **desactivado por defecto**,
"Permitir acceso desde mi red local". Mientras esté desactivado, Nexo se
comporta exactamente como hoy: escucha en `127.0.0.1`, sirve HTTP plano, y
cada petición exige el mismo `Authorization: Bearer <token de aplicación>`
que ya exige ahora. Nada nuevo en ese camino — ni certificados, ni pasos
adicionales, ni cambios de comportamiento observables.

Al activarlo, el usuario ve antes una advertencia explícita de lo que implica
(qué queda expuesto, qué no protege) y tiene que confirmarla. Tras guardar y
reiniciar Nexo (el mismo patrón que ya existe para el cambio de puerto), el
gateway:

- Escucha en todas las interfaces de la máquina (`0.0.0.0`), mismo puerto de
  siempre.
- Sirve **HTTPS**, nunca HTTP, para ese bind. La primera vez que se activa,
  Nexo genera un certificado autofirmado propio y lo guarda en su directorio
  de datos; en arranques siguientes reutiliza el mismo certificado.
- Sigue exigiendo el mismo token de aplicación que hoy. No hay ningún modo de
  acceso por red sin token.
- El panel muestra la dirección a la que conectar desde otro equipo de la
  red y cómo identificar el certificado (para que el usuario pueda aceptarlo
  a conciencia en el otro dispositivo, en vez de aceptar cualquier aviso de
  "certificado no confiable" sin mirar).

Si en algún arranque el certificado no puede generarse o leerse, Nexo **no**
arranca en `0.0.0.0` sirviendo HTTP plano como respaldo: se niega a escuchar
en red y lo dice como error visible, igual que hoy dice "el puerto ya está
en uso".

## Criterios de aceptación

| # | Criterio | Cómo se verifica | Resultado |
| --- | --- | --- | --- |
| 1 | Con `allow_lan = false` (el valor por defecto), el comportamiento es idéntico al actual: bind en `127.0.0.1`, HTTP plano, token de aplicación obligatorio, sin ningún fichero de certificado generado. | `cargo test -p nexo-core --test gateway_e2e -- allow_lan_false_is_identical_to_today` | ✅ Verificado por test y contra la app real instalada (tras reinstalar, `curl http://127.0.0.1:8787/healthz` responde igual que siempre) |
| 2 | Con `allow_lan = true` guardado y Nexo reiniciado, el gateway escucha en `0.0.0.0:<puerto>` y responde por HTTPS con el certificado autofirmado generado por Nexo; una conexión por HTTP plano a ese mismo puerto no obtiene una respuesta válida del gateway. | `cargo test -p nexo-core --test gateway_e2e -- lan_mode_serves_https_with_a_valid_request a_plain_http_request_to_the_tls_port_gets_no_valid_response` | ✅ Verificado por test **y** contra el binario real instalado: `NEXO_DATA_DIR` temporal, `allow_lan=1` escrito directamente en el SQLite, arranque real → log `gateway escuchando addr=0.0.0.0:9799 tls=true`; `curl --cacert` con el certificado generado responde `200`, `curl` sin confiar en él lo rechaza como autofirmado, y HTTP plano contra ese puerto falla (`HTTP/0.9` inválido) |
| 3 | El token de aplicación sigue siendo obligatorio en modo red: una petición HTTPS sin `Authorization: Bearer` válido se rechaza con 401, igual que hoy en local. | `cargo test -p nexo-core --test gateway_e2e -- lan_mode_still_requires_the_app_token` | ✅ |
| 4 | El certificado se genera solo la primera vez que hace falta y se reutiliza en arranques siguientes (mismo fingerprint) mientras no se borre el fichero. | `cargo test -p nexo-core -- tls_cert::tests::reuses_the_same_certificate_and_fingerprint_across_calls` | ✅ Verificado por test. El intento de repetirlo contra el binario real reinstalando y relanzando no aportó nada nuevo: el primer proceso seguía vivo por una limitación de control de trabajos entre llamadas de esta sesión, así que esa repetición manual no reinició de verdad nada — se anota para no reclamar una comprobación que no ocurrió; la evidencia real de este criterio es el test unitario |
| 5 | Si el certificado no puede generarse o leerse (p. ej. fichero corrupto o sin permisos), Nexo no sirve en `0.0.0.0` y reporta un error explícito de arranque; en ningún caso cae a HTTP plano en esa interfaz. | `cargo test -p nexo-core -- service::tests::prepare_gateway_bind_falls_back_to_loopback_when_the_certificate_is_broken tls_cert::tests::a_corrupt_certificate_file_is_reported_and_not_silently_regenerated` | ✅ |
| 6 | La Configuración muestra el interruptor desactivado por defecto, con una advertencia explícita que hay que confirmar antes de poder activarlo. | `npm run check` + revisión de `Settings.svelte` | ⚠️ Implementado y sin errores de tipos; **no verificado por clic real** — requiere el permiso de Accesibilidad de macOS para automatizar la interfaz, que no se ha concedido en esta ni en sesiones anteriores (mismo límite ya señalado en las specs 0005 y 0006) |
| 7 | Con el modo activo, el panel muestra la dirección (IP:puerto) para conectar desde otro equipo y una forma de identificar el certificado (huella SHA-256 y ruta del fichero) para aceptarlo a conciencia en el otro dispositivo. | `npm run check` + revisión de `Settings.svelte`; los datos que consume (`GatewayStatus.lan`) están cubiertos por los tests de `prepare_gateway_bind` | ⚠️ Igual que el criterio 6: los datos están verificados, el renderizado no se pudo clicar |
| 8 | Cambiar el interruptor exige reiniciar Nexo para aplicarse, y el panel lo dice, igual que ya ocurre con el puerto. | Revisión de `commands::save_settings` / mensaje de la UI | ✅ Por código: el mensaje se actualizó y `bind_addr`/el plan de arranque solo se leen una vez, al arrancar |
| 9 | Verificación completa del repositorio en verde y aplicación instalada. | `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check` + `npm run app:install` | ✅ 273 tests + 28 e2e (11 ignorados) en verde, clippy sin avisos, check sin errores; compilado e instalado `Aug 2 17:41:34 2026` |

## Fuera de alcance

- **Acceso fuera de la red local** (Internet, reenvío de puertos, UPnP,
  túneles tipo Tailscale/ngrok, DNS dinámico). Sigue fuera del proyecto, sin
  cambios respecto al ADR 0003.
- **Elegir una interfaz de red concreta.** Se escucha en todas (`0.0.0.0`) o
  solo en loopback; no hay una tercera opción de "solo mi Wi-Fi de casa". Si
  hace falta, es una especificación aparte.
- **Enumerar todas las IPs de todas las interfaces en el panel.** Se muestra
  al menos una dirección alcanzable con la que conectar; un listado
  exhaustivo de interfaces queda para más adelante si resulta necesario en el
  uso real.
- **Instalar una CA de confianza en otros dispositivos.** El usuario acepta
  el certificado autofirmado a mano en cada cliente nuevo; Nexo no automatiza
  ni facilita esa instalación en el sistema de otra máquina.
- **Rotación o caducidad automática del certificado.** El certificado
  autofirmado se genera una vez y no expira en un plazo corto; renovarlo hoy
  significa borrar el fichero a mano. Rotación automática, si hace falta, es
  otra especificación.
- **Ningún cambio en el modo local.** No se introduce ninguna pieza nueva
  (token distinto, certificado, cabecera, aviso) en el camino
  `allow_lan = false`. Es una condición explícita del usuario, no solo un
  valor por defecto conveniente.
- **Tokens o alcance distinto para peticiones de red.** Se reutiliza el
  mismo mecanismo de tokens de aplicación que ya existe; no hay un tipo de
  token "solo LAN" ni permisos diferenciados por origen de la petición.

## Supuestos asumidos

- El certificado autofirmado se identifica ante el usuario con su huella
  SHA-256 y la ruta del fichero en el panel; no se implementa nada más
  elaborado (como servir una página de ayuda con capturas por sistema
  operativo) en esta especificación.
- La advertencia antes de activar el interruptor es un modal de confirmación
  con texto explícito, siguiendo el mismo patrón visual ya usado para el
  aviso de login de suscripción OAuth; no hace falta que el usuario escriba
  nada para confirmar, basta un botón explícito de "Activar".
- El fichero del certificado y su clave viven en el directorio de datos de
  Nexo (el mismo donde ya vive `models-dev-cache.json` y la base de datos),
  no en un subdirectorio nuevo con su propia gestión de retención.
- La detección de "una IP alcanzable para mostrar en el panel" usa la interfaz
  de red no-loopback que el sistema operativo reporte como principal; si hay
  varias, se muestra una y se documenta que puede no ser la que el usuario
  espera (mitiga parte de esto el criterio de aceptación 7, no lo resuelve
  del todo — ver "Fuera de alcance").

## Riesgos

- **Advertencia de certificado no confiable en cada cliente nuevo.** Es
  esperado y aceptado en el ADR 0003; se mitiga mostrando cómo verificar la
  huella, no evitando el aviso.
- **Ampliación real de la superficie de ataque** al escuchar en `0.0.0.0`:
  cualquier dispositivo de la misma red (incluidos invitados de un Wi-Fi
  doméstico) puede intentar conectar. El token sigue siendo la barrera; el
  aviso previo a activar el interruptor es la otra.
- **Redes no controladas.** Si el usuario activa el modo estando conectado a
  una VPN o una red ajena, queda expuesto también ahí, no solo en su red de
  confianza. Documentado en el ADR; el panel debe advertirlo en el texto del
  interruptor.
- **Dependencia nueva para servir TLS.** `nexo-core` no tiene hoy ninguna
  pieza para terminar TLS del lado servidor (solo se usa `rustls` como
  cliente saliente, vía `reqwest`). Elegir esa pieza es una decisión de
  `/design`, no de esta especificación, pero es un riesgo de calendario: si
  la librería elegida no compone bien con el `axum::serve` actual, el diseño
  puede necesitar más cambios de los previstos en `gateway/mod.rs`.
- **Un cambio de IP por DHCP invalida el certificado ya emitido.** El
  certificado se genera con la IP detectada en ese momento como parte de su
  identidad (`Subject Alternative Name`); si la máquina recibe una IP distinta
  más adelante, los clientes que ya conectaron verán un aviso de "nombre no
  coincide" además del de certificado no confiable. Mitigación de esta
  versión: borrar `tls/cert.pem` a mano para forzar una regeneración (mismo
  mecanismo ya aceptado en "Fuera de alcance" para la rotación en general).
- **El certificado autofirmado no protege de un atacante activo en la red**
  que intercepte la primera conexión antes de que el usuario compruebe la
  huella (ataque de tipo *trust-on-first-use*). Es una limitación conocida y
  aceptada de este mecanismo frente a un certificado firmado por una CA
  pública, que aquí no es viable sin un dominio propio.

## Lo que se descubrió al construir

- **`rcgen` 0.14.8 ya fija `not_after` al año 4096 por defecto.** El diseño
  preveía tener que fijarlo a mano; al leer el código fuente real de la
  librería (no de memoria) resultó innecesario. Se dejó un test que
  comprueba el valor igualmente, para detectar un cambio de esa librería en
  el futuro sin esperar a que fallara un `handshake` TLS meses después.
- **Conectar desde un segundo dispositivo físico de la red no se pudo
  probar.** Todas las verificaciones contra el binario real instalado se
  hicieron desde la misma máquina (con `curl` contra `127.0.0.1` y el puerto
  de red), porque no había un segundo equipo disponible en esta sesión. El
  SAN del certificado sí incluye la IP de red detectada (cubierto por test),
  pero que un ordenador distinto la acepte y conecte de verdad queda como
  algo pendiente de confirmar en el uso real.

## Invariantes que esto no puede romper

- **9. Solo localhost por defecto.** El valor por defecto sigue siendo
  `127.0.0.1`; el modo red exige autenticación (token, ya existente),
  autorización (igual) y transporte seguro (TLS, lo que añade esta spec) a la
  vez, nunca por separado.
- **2. Nunca degradar en silencio.** Si el certificado falla, el gateway no
  cae a HTTP plano en `0.0.0.0`: se niega a escuchar y lo dice.
- **1. Ningún secreto en SQLite.** El certificado y su clave privada no son
  una credencial de proveedor, pero por coherencia tampoco se guardan en
  SQLite: viven como fichero en el directorio de datos.
- **16 (criterio de producto, no invariante de `CLAUDE.md`, pero igual de
  vinculante aquí).** Configuración segura por defecto: activar el modo red
  es un acto explícito del usuario, nunca el resultado de otra acción.
