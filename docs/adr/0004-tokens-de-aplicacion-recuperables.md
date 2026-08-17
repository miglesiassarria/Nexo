# ADR 0004: Tokens de aplicación recuperables mediante el almacén seguro

- **Fecha:** 2026-08-17
- **Estado:** aceptada
- **Decide:** Manuel Iglesias

## Contexto

El token de una aplicación cliente solo se muestra una vez, en el momento de
emitirlo: Nexo guarda su hash en SQLite y descarta el valor en claro. Si el
usuario lo pierde —cierra el aviso sin copiarlo, borra el portapapeles, cambia
de ordenador— no hay forma de recuperarlo. La única salida hoy es revocar la
aplicación y crear otra, lo que invalida de inmediato la clave que la
herramienta cliente (Msty, Cursor, un script) tenía configurada.

El usuario ha pedido poder copiar la clave completa desde la lista de
aplicaciones, en cualquier momento, no solo al crearla.

## Decisión

El token de cada aplicación se guarda también en el almacén seguro del
sistema operativo (Keychain en macOS, Credential Manager en Windows, Secret
Service en Linux) — el mismo almacén donde ya viven las API keys y los
tokens OAuth de cada cuenta de proveedor. SQLite sigue guardando solo el
hash, que sigue siendo lo que autentica cada petición.

El código para esto ya existía sin usar: `SecretRef::app_token(app_id)`
([secrets.rs](../../crates/nexo-core/src/secrets.rs)), con un comentario que
describe exactamente este diseño. Nunca se conectó a nada.

### Alternativas descartadas

- **Guardar el token en claro en SQLite.** Es lo que se pidió primero.
  Se descarta porque rompe la propiedad que hace inofensivo un volcado de la
  base de datos: hoy una copia de `nexo.sqlite` —una copia de seguridad, una
  carpeta sincronizada, el fichero compartido por error— no sirve para
  autenticarse como ninguna aplicación. En claro, sí serviría, para todas a
  la vez.
- **Cifrar el token con una clave que Nexo también guarda.** Sería la misma
  alternativa anterior con un paso intermedio: si Nexo necesita poder
  descifrarlo para mostrártelo, la clave de descifrado vive al lado del
  dato cifrado, y un atacante con la base de datos tiene las dos piezas
  igual. No añade nada real.

## Riesgos aceptados

Guardar el token en el almacén seguro **no es guardarlo hasheado**: quien
tenga acceso de lectura al llavero del usuario obtiene la clave en claro de
cada aplicación, igual que hoy obtiene las API keys de los proveedores. Es
exactamente el mismo nivel de protección que Nexo ya da a esas claves —no
uno mayor ni uno menor— y el usuario lo ha aceptado explícitamente sabiendo
que:

- Un volcado de **solo** `nexo.sqlite` sigue sin dar nada usable: sigue
  siendo el riesgo que este ADR reduce respecto a la alternativa descartada.
- Malware corriendo con la sesión del usuario ya iniciada, o una herramienta
  con permiso para leer el llavero, sí puede leer estos tokens — como ya
  puede leer las claves de los proveedores. Este ADR no cambia esa
  superficie; la iguala a la que ya existía para otras credenciales.
- Los tokens emitidos **antes** de esta decisión no tienen copia en el
  almacén seguro (nunca se guardó nada recuperable) y no se pueden
  recuperar retroactivamente. Solo los tokens nuevos, emitidos o
  regenerados después de este cambio, son recuperables.

## Consecuencias arquitectónicas

1. **Emitir un token pasa a escribir en dos sitios, no en uno**: el hash en
   SQLite (autenticación) y el valor en claro en el almacén seguro
   (recuperación). Revocar o borrar una aplicación debe limpiar los dos, o
   se queda un secreto huérfano en el llavero — el mismo cuidado que ya
   existe al desconectar una cuenta de proveedor.
2. **La invariante 1 de `CLAUDE.md` se corrige**, no se abandona: donde decía
   *«los tokens de aplicación se guardan hasheados»* como excepción al
   resto de secretos, ahora sigue la misma regla general que todos los
   demás — nada de secretos en SQLite, la referencia sí, el secreto va al
   almacén seguro del sistema. Deja de ser una excepción.

## Revisión

Esta decisión debe revisarse si algún día Nexo necesita operar en un entorno
sin almacén seguro del sistema disponible (por ejemplo, un servidor headless
sin sesión de usuario ni Keychain/Secret Service accesible) — ahí no habría
dónde guardar el token recuperable y habría que decidir si se admite
degradar a «solo hash» para ese caso, o si Nexo simplemente no sirve ahí.
