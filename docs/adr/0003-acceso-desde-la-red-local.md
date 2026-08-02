# ADR 0003: Acceso desde la red local

- **Fecha:** 2026-08-02
- **Estado:** aceptada, pendiente de implementación
- **Decide:** Manuel Iglesias

## Contexto

Nexo escucha hoy exclusivamente en `127.0.0.1`. El usuario quiere conectar
**otros ordenadores de su propia red local** (no acceso desde fuera de esa
red, no multiusuario, no despliegue empresarial) a la misma instancia de
Nexo, para no tener que ejecutar o configurar una por máquina.

`ROADMAP.md` lista hoy «acceso remoto» dentro de lo explícitamente fuera del
proyecto, junto a multiusuario, sincronización entre equipos y despliegue
empresarial. Esta decisión **acota** esa exclusión: separa «otra máquina de mi
propia red local» de «acceso remoto» en el sentido amplio (Internet,
multiusuario, empresarial), que sigue fuera. El invariante 9 de `CLAUDE.md` no
prohíbe esto de forma absoluta — dice *«no se expone en red sin
autenticación, autorización y transporte seguro»* — así que la decisión
pendiente no es si romper el invariante, sino qué implementación concreta lo
satisface, porque hoy esas tres piezas no existen para tráfico no-loopback.

De las tres, la autenticación y la autorización ya existen y no hay que
diseñarlas de nuevo: cada petición al gateway exige ya una cabecera
`Authorization: Bearer <token>` verificada contra `nexo.db().authenticate()`
(`crates/nexo-core/src/gateway/routes.rs`), con tokens por aplicación,
hasheados en SQLite y revocables uno a uno. Eso no cambia. Lo que falta es
**transporte seguro** y una forma explícita, con opt-in informado, de decidir
que el gateway deja de escuchar solo en loopback.

## Decisión

Nexo podrá escuchar en la red local, **desactivado por defecto**, tras
activación explícita en Configuración con una advertencia de riesgo concreta
(mismo patrón que la confirmación de OAuth de suscripción del
[ADR 0001](0001-oauth-de-suscripcion.md)), y solo cuando se cumplan a la vez:

1. **Autenticación por token, sin cambios.** El mismo Bearer token por
   aplicación que ya es obligatorio hoy. No se añade ningún modo de acceso sin
   token para la red local.
2. **Transporte cifrado obligatorio.** El gateway sirve HTTPS cuando escucha
   fuera de loopback. Nexo genera un certificado autofirmado propio la primera
   vez que se activa esta opción, y lo persiste como fichero en el directorio
   de datos de la aplicación (no en SQLite, no en el almacén seguro del
   sistema: no es una credencial de proveedor, es la identidad TLS del propio
   servidor, y su pérdida no compromete ninguna cuenta). Cada dispositivo
   cliente debe aceptar ese certificado una vez, de forma manual. Nexo no
   instala una CA raíz en ningún almacén de confianza del sistema, ni del
   suyo ni del de otras máquinas: eso sería una modificación de configuración
   de seguridad del sistema operativo, y excede lo que esta decisión autoriza.
3. **Bind explícito y visible.** La dirección deja de ser una constante
   (`127.0.0.1`, hoy fija en `src-tauri/src/main.rs`); pasa a ser una opción de
   Configuración con dos valores: «solo este ordenador» (por defecto,
   `127.0.0.1`) y «mi red local» (`0.0.0.0`, mismo puerto configurable que ya
   existe). El panel muestra la IP y el puerto reales a los que conectar desde
   otra máquina cuando el segundo modo está activo.

Nada de esto habilita acceso desde fuera de la red local: no hay reenvío de
puertos, no hay UPnP, no hay integración con Tailscale/ngrok ni servicio de
DNS dinámico. Quien quiera eso sigue teniendo que montarlo por su cuenta, como
hoy, sin ayuda de Nexo.

## Alcance: qué es y qué no es esta decisión

- **Es:** varios ordenadores del propio usuario, en la misma red doméstica de
  confianza, hablando con una única instancia de Nexo.
- **No es** acceso remoto por Internet, no es multiusuario (los tokens siguen
  siendo del mismo usuario, para sus propias aplicaciones), no es
  sincronización entre equipos, no es despliegue empresarial. Esas cuatro
  cosas siguen fuera del proyecto, sin cambios.

`ROADMAP.md` se actualiza para reflejar exactamente esta línea: de «acceso
remoto» a «acceso remoto fuera de la red local».

## Alternativas descartadas

- **Token por red, sin TLS.** Cumple autenticación, no transporte seguro: el
  token y el contenido de cada conversación viajarían en claro por el aire.
  Cualquier otro dispositivo asociado a la misma red Wi-Fi puede capturarlo
  con herramientas triviales. Incumple el invariante 9 en la letra y en el
  motivo por el que existe.
- **CA raíz propia instalada automáticamente en cada máquina.** Evitaría la
  advertencia de certificado no confiable en el navegador o cliente, pero
  exige que Nexo modifique el almacén de confianza del sistema operativo en
  cada dispositivo — la propia máquina y, peor, las demás. Es exactamente el
  tipo de cambio de configuración de seguridad que este proyecto reserva para
  que lo haga el usuario a mano, no una aplicación por su cuenta.
- **Exigir una VPN o herramienta tipo Tailscale como único camino.** Más
  seguro por defecto y resuelve el acceso también fuera de la LAN, pero añade
  una dependencia externa y un segundo programa que instalar y mantener,
  justo lo que el criterio de aceptación 1 del producto evita («instalarlo y
  ejecutarlo localmente sin desplegar un servidor externo»). Queda como
  alternativa que el propio usuario puede montar hoy sin que Nexo haga nada
  distinto; no sustituye la petición concreta de este ADR.
- **mDNS/Bonjour para que el host se anuncie solo.** Cómodo, pero no es
  necesario para el caso de uso (el usuario ya sabe qué máquinas tiene en su
  red) y añade superficie nueva. Se aplaza; no bloquea esta decisión.

## Riesgos aceptados

### 1. Certificado no confiable por defecto

Cada cliente nuevo verá la advertencia estándar de certificado autofirmado la
primera vez.

**Mitigación.** El panel de Configuración debe mostrar, junto al toggle, el
paso exacto para aceptar el certificado desde otro dispositivo. No se
oculta ni se intenta evitar con trucos (como servir en texto plano si el
cliente no trae el certificado): sin TLS válido o aceptado, la petición se
rechaza igual que sin token.

### 2. Ampliación real de la superficie de ataque

Escuchar en `0.0.0.0` expone el gateway a cualquier dispositivo de la misma
red, no solo a los que el usuario tiene en mente — incluye invitados en el
Wi-Fi doméstico o, en redes menos confiables, a cualquier otro equipo con
acceso a esa red.

**Mitigación.** Desactivado por defecto (invariante 16 del producto:
configuración segura por defecto). Activarlo exige el mismo patrón de
advertencia explícita y confirmación ya usado para el login de suscripción:
decir en términos concretos qué implica, no un aviso genérico. El toggle
vive junto al puerto en Configuración, visible y reversible en cualquier
momento.

### 3. Redes no controladas (VPN, redes corporativas, hotspots)

`0.0.0.0` escucha en **todas** las interfaces, no solo en la que el usuario
tiene en mente. Si el usuario está conectado a una VPN o a una red no
doméstica cuando activa la opción, queda expuesto también ahí, sin que Nexo
pueda distinguir la intención.

**Mitigación.** El panel debe listar las interfaces de red disponibles y sus
IPs cuando se active el modo «mi red local», para que el usuario vea
exactamente por dónde queda expuesto, no solo que «está activado». Documentar
en el propio texto de la opción que conviene desactivarla antes de conectarse
a una red que no sea la propia. No se implementa selección de interfaz
concreta en esta primera versión: es una mejora de la fase de UX, no un
bloqueante de esta decisión.

## Consecuencias arquitectónicas

1. **El bind deja de ser una constante en `main.rs` y pasa a `Settings`.**
   `src-tauri/src/main.rs` hoy calcula `settings.bind_addr()` a partir de un
   puerto guardado; gana un segundo campo (modo de escucha: local o red) que
   `bind_addr()` combina con el puerto para producir `127.0.0.1:P` o
   `0.0.0.0:P`.
2. **El gateway gana una capa TLS opcional.** `nexo_core::gateway::bind` y
   `serve_on` (`crates/nexo-core/src/gateway/mod.rs`) deben poder servir sobre
   un listener TLS (por ejemplo con `tokio-rustls`, reutilizando `rustls` que
   ya es dependencia del proyecto) cuando el modo de escucha no es loopback.
   En modo «solo este ordenador» el comportamiento actual no cambia: sigue en
   HTTP plano sobre loopback, sin coste ni riesgo nuevo.
3. **Generación y persistencia del certificado.** Un módulo nuevo, aislado
   (mismo principio que el invariante 7: lo frágil vive en un único fichero),
   genera el par de claves y el certificado autofirmado la primera vez que
   hace falta, y lo reutiliza en arranques siguientes mientras no cambie. Vive
   como fichero en el directorio de datos, no en SQLite ni en el almacén de
   credenciales de proveedores.
4. **La autenticación por Bearer token no cambia de forma ninguna.** Esta es
   la pieza que ya cumplía el invariante 9; no se toca.
5. **`ROADMAP.md` y `CLAUDE.md` se actualizan** para que la exclusión de
   «acceso remoto» pase a decir explícitamente «fuera de la red local», y el
   criterio de aceptación 16 refleje que el valor por defecto sigue siendo
   solo localhost, con la red local como opción explícita y no como cambio
   del valor por defecto.

## Revisión

Esta decisión debe revisarse si:

- El usuario pide en algún momento acceso desde fuera de la red local
  (Internet): eso es un ADR distinto, con un modelo de amenaza distinto
  (certificados válidos por dominio, NAT/reenvío de puertos o un proveedor de
  túnel, y probablemente autenticación más allá de un token estático).
- La fricción de aceptar el certificado autofirmado en cada dispositivo
  resulta impracticable en el uso real: entonces se reconsidera una CA local
  que el propio usuario instale de forma manual y consciente en sus propios
  dispositivos (no automatizada por Nexo), documentando el paso en lugar de
  ejecutarlo por él.
