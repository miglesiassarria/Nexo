# 0011 · Token de aplicación recuperable

- **Estado:** build
- **Creada:** 2026-08-17
- **Pedida por:** el usuario, tras pedir poder copiar la clave desde la lista de
  Aplicaciones (donde hoy solo se ve el prefijo cortado) y, al confirmarse que
  la clave completa no existe en ningún sitio recuperable, aceptar
  explícitamente el cambio de seguridad del [ADR 0004](../../docs/adr/0004-tokens-de-aplicacion-recuperables.md)
  para conseguirlo

## Problema

El token de una aplicación solo se ve una vez, al crearla. Si se pierde (se
cierra el aviso sin copiarlo, se borra el portapapeles, cambia el ordenador),
no hay ninguna forma de volver a obtenerlo: Nexo guarda solo su hash. La única
salida es revocar la aplicación y crear otra, lo que invalida de inmediato la
clave que la herramienta cliente tenía configurada — no sirve como
recuperación, es empezar de cero.

El ADR 0004 ya decidió el mecanismo: el token también se guarda en el almacén
seguro del sistema, igual que las API keys de los proveedores. Esta
especificación es sobre el flujo, no sobre si hacerlo:

- ¿Cuándo se escribe el token en el almacén seguro?
- ¿Qué pasa al revocar o borrar una aplicación?
- ¿Cómo lo lee y lo copia la interfaz?
- ¿Qué ve el usuario en una aplicación creada antes de este cambio, cuyo token
  nunca se guardó en claro en ningún sitio?

Hay además un hallazgo de lectura de código que condiciona el diseño:
`revoke_app` y `delete_app` en
[commands.rs](../../src-tauri/src/commands.rs) llaman hoy a
`state.nexo.db()` **directamente**, sin pasar por `Nexo`. Solo `create_app` va
por el servicio. Como limpiar el almacén seguro solo se puede hacer desde
`Nexo` (es quien tiene `self.secrets`), esos dos comandos tienen que empezar a
pasar por ahí — hoy no podrían limpiar nada aunque quisieran.

## Comportamiento esperado

- Al crear una aplicación, su token se guarda en dos sitios: el hash en SQLite
  (autenticación, sin cambios) y el valor en claro en el almacén seguro del
  sistema (recuperación).
- En la lista de Aplicaciones, hacer clic en la clave de una aplicación activa
  copia **la clave completa** al portapapeles, con la misma confirmación breve
  que ya existe («✓ copiado»).
- Revocar una aplicación borra también su copia del almacén seguro: un token
  revocado no autentica, así que no tiene sentido seguir ofreciéndolo como
  copiable — sería una clave que parece válida y no lo es.
- Borrar una aplicación borra su copia del almacén seguro, igual que ya pasa
  con las cuentas de proveedor al desconectarlas.
- Una aplicación creada **antes** de este cambio no tiene copia en el almacén
  seguro. Al hacer clic en su clave, se copia el prefijo (como hasta ahora) y
  la interfaz lo explica: ese token no es recuperable, hay que revocar y crear
  uno nuevo si se necesita.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | Crear una aplicación deja su token en claro en el almacén seguro, además del hash en SQLite | `cargo test -p nexo-core -- create_app_stores_the_token_in_the_secret_store` |
| 2 | Revocar una aplicación borra su token del almacén seguro | `cargo test -p nexo-core -- revoking_an_app_deletes_its_secret` |
| 3 | Borrar una aplicación borra su token del almacén seguro | `cargo test -p nexo-core -- deleting_an_app_deletes_its_secret` |
| 4 | Un comando nuevo devuelve el token en claro si existe en el almacén seguro, y `None` si no (aplicación anterior a este cambio, o ya revocada) | `cargo test -p nexo-core -- app_token_secret_returns_none_when_there_is_nothing_stored` y su contraparte con secreto presente |
| 5 | En la interfaz, una aplicación activa con secreto disponible copia la clave completa al hacer clic | `npm run check` en verde + comprobación manual en la app instalada: clic copia la clave completa, no el prefijo |
| 6 | Una aplicación sin secreto disponible (anterior al cambio) sigue copiando solo el prefijo, y la interfaz lo explica sin ambigüedad | Comprobación manual en la app instalada, con una aplicación creada antes de este cambio |
| 7 | `revoke_app` y `delete_app` pasan por `Nexo`, no por `db()` directamente | Lectura de `commands.rs`; `cargo build --workspace` en verde |

## Fuera de alcance

- **Un botón de «Regenerar token».** El ADR 0004 hace recuperable cualquier
  token *nuevo*; para uno anterior que se perdió, el camino sigue siendo
  revocar y crear una aplicación nueva, que ya existe en el producto. Añadir
  un atajo de un clic que haga las dos cosas a la vez es una comodidad
  razonable, pero no es lo que se pidió — se deja para otra especificación si
  hace falta.
- **Mostrar la clave sin copiarla** (por ejemplo, revelarla en pantalla con un
  «ojo» como en un campo de contraseña). Se pidió específicamente el resultado
  en el portapapeles, no verla.
- **Migrar tokens antiguos.** No hay ninguna forma de hacerlo: Nexo nunca
  guardó el valor en claro de un token emitido antes de este cambio, así que
  no hay nada que migrar, solo que aceptar como no recuperable.

## Supuestos asumidos

- Un token revocado no vuelve a ser copiable ni aunque su fila siga
  visible en la lista (marcada «Revocada»): mostrarlo sugeriría que sigue
  sirviendo, y no sirve. Declarado por mí, no preguntado.
- La copia al portapapeles sigue usando `navigator.clipboard.writeText`, el
  mismo mecanismo que ya usan el token recién emitido y el botón de prefijo
  del PR anterior.
- El icono/etiqueta del botón cambia según haya o no secreto recuperable
  (p. ej. el texto del `title` y si copia clave completa o prefijo), para que
  el usuario no descubra la diferencia solo al pegar la clave y que falle.

## Riesgos

- **Un almacén seguro que falle al escribir** (llavero bloqueado, sin
  permisos) no debe impedir crear la aplicación: el hash en SQLite es lo que
  de verdad autentica. Si falla la escritura del secreto recuperable, se
  registra el aviso y se sigue — la aplicación queda utilizable, solo que su
  token no sería recuperable, como las de antes de este cambio.
- **Aceptado y documentado en el ADR 0004, no de esta especificación**: quien
  tenga acceso de lectura al almacén seguro del usuario obtiene el token en
  claro de cada aplicación, igual que ya obtiene las API keys de los
  proveedores.

## Lo que se descubrió al construir

- Verificado con un ejemplo desechable (`cargo run --example`, borrado tras
  usarlo) que el llavero real de macOS soporta el ciclo completo
  (escribir/leer/borrar) bajo el mismo servicio `com.nexo.gateway` que usa
  Nexo. No había ninguna suposición equivocada que corregir: el diseño se
  sostiene tal como se planteó en `design.md`.
- **Los criterios 5 y 6 no se pudieron verificar de forma autónoma.**
  `osascript`/System Events no tiene permiso de Accesibilidad concedido para
  pulsar en la ventana real de Nexo — el mismo bloqueo ya documentado en la
  spec 0007. La lógica que el clic ejecuta está probada de extremo a extremo
  contra el llavero real (criterios 1-4) y tipada (`npm run check`), pero el
  clic físico —abrir Aplicaciones, crear una, pulsar su clave, pegarla en
  algún sitio— lo tiene que dar el usuario para cerrar el criterio.

## Invariantes que esto no puede romper

- **1. Ningún secreto en SQLite.** Corregida por el ADR 0004: el hash sigue
  siendo lo único que vive en SQLite; el valor en claro va al almacén seguro,
  igual que el resto de secretos.
