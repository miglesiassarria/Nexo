# 0011 · Diseño

## Enfoque

`Nexo::create_app` pasa a escribir el token en el almacén seguro justo después
de que `Db::create_app` lo genere — el valor en claro ya vuelve en
`IssuedApp.token`, así que no hace falta cambiar cómo se genera ni cómo se
guarda el hash. `revoke_app` y `delete_app` dejan de ser una llamada directa a
`db()` desde `commands.rs` y pasan a ser métodos de `Nexo` que, además de lo
que ya hacían, borran esa misma entrada — exactamente el patrón que ya usa
`disconnect_account` para las cuentas de proveedor. Un comando nuevo,
`app_token_secret`, lee el almacén seguro y devuelve el token o `None`. La
interfaz decide qué copiar y qué decir **después** de preguntar, no antes: no
hay ninguna señal previa en la lista que distinga una aplicación recuperable
de una que no lo es.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/service.rs` | `create_app` escribe el secreto; `revoke_app` y `delete_app` nuevos, con la limpieza; `app_token_secret` nuevo |
| `src-tauri/src/commands.rs` | `revoke_app`/`delete_app` pasan a llamar a `state.nexo.*` en vez de `state.nexo.db().*`; comando nuevo `app_token_secret` |
| `src-tauri/src/main.rs` | Registra `app_token_secret` en `invoke_handler` |
| `src/lib/api.ts` | `appTokenSecret(appId)` nuevo |
| `src/lib/views/Apps.svelte` | El botón de la clave llama al comando nuevo; copia la clave completa o el prefijo según la respuesta, con mensaje distinto; sin botón (texto plano) en aplicaciones revocadas |

## Decisiones

### D1. La limpieza del almacén seguro vive en `Nexo`, no en `Db`

- **Decisión:** `Db` (en `apps.rs`) no toca el almacén seguro — no tiene
  acceso a él, y no debe tenerlo: es una capa de persistencia SQL. La
  limpieza va en `Nexo::revoke_app` y `Nexo::delete_app`, que sí tienen
  `self.secrets`, igual que `disconnect_account`.
- **Alternativa descartada:** pasar `Arc<dyn SecretStore>` a `Db` para que
  `apps.rs` pueda borrar directamente. Se descarta porque mezclaría dos
  responsabilidades que hoy están limpiamente separadas (SQL vs. almacén del
  sistema) y porque ya existe el patrón correcto un nivel por encima —
  seguirlo es menos código, no más.
- **Consecuencia que hay que asumir:** `commands.rs` tiene que dejar de
  llamar a `state.nexo.db().revoke_app(...)` y `state.nexo.db().delete_app(...)`
  directamente. Es el criterio 7: hoy ninguno de los dos pasa por `Nexo`.

### D2. Un fallo al escribir o borrar el secreto no aborta la operación

- **Decisión:** si `self.secrets.set(...)` falla al crear la aplicación, o
  `self.secrets.delete(...)` falla al revocar/borrar, se registra un
  `tracing::warn!` y se continúa. La aplicación se crea/revoca/borra igual;
  el hash en SQLite es lo que de verdad autentica.
- **Alternativa descartada:** propagar el error y abortar la operación
  completa. Se descarta porque un llavero bloqueado o sin permisos
  bloquearía crear una aplicación por un fallo en una funcionalidad
  *adicional* (poder recuperar la clave más tarde), no en la funcionalidad
  central (que la aplicación funcione). Es el mismo criterio que ya aplica
  `disconnect_account`, que hace exactamente esto.
- **Consecuencia que hay que asumir:** una aplicación puede quedar sin
  token recuperable por un fallo transitorio del almacén seguro, sin que la
  interfaz lo sepa distinguir de «se creó antes de este cambio». No hace
  falta que lo sepa: el resultado hacia el usuario es el mismo («esta clave
  no es recuperable»), y el motivo real queda en el log para quien lo
  investigue.

### D3. La interfaz pregunta al hacer clic, no adivina antes

- **Decisión:** la lista de Aplicaciones no sabe de antemano si una
  aplicación tiene secreto recuperable. Al hacer clic, llama a
  `app_token_secret`, y según la respuesta copia la clave completa o el
  prefijo, con un mensaje distinto en cada caso.
- **Alternativa descartada:** que `list_apps` devuelva también si cada
  aplicación tiene secreto (`token_recoverable: bool`), para que el botón ya
  se vea distinto antes de pulsarlo — es lo que sugería el supuesto de
  `spec.md` sobre «que el icono cambie según haya o no secreto». Se descarta
  al diseñar: exigiría una lectura del almacén seguro del sistema **por
  aplicación, en cada carga de la lista**, con el coste y el riesgo de un
  proveedor de credenciales que puede pedir permiso la primera vez que algo
  lo toca. Preguntar solo al hacer clic —una vez, cuando el usuario ya
  quiere el dato— consigue lo mismo con una décima parte de las lecturas.
- **Consecuencia que hay que asumir:** esto **corrige** el supuesto de
  `spec.md` («el icono cambia según haya secreto») a «el mensaje tras el
  clic lo dice». El criterio 6 sigue cumplido igual: la interfaz
  explica sin ambigüedad que no es recuperable, solo que lo hace justo
  después de pulsar y no antes. No cambia el alcance ni el problema
  resuelto, así que no se vuelve a `/spec`.

### D4. Una aplicación revocada no ofrece copiar nada: ni clave ni prefijo con apariencia de botón

- **Decisión:** si `app.revoked_at` tiene valor, la clave se muestra como
  texto plano (`<code>{token_prefix}…</code>`, igual que hoy antes de esta
  especificación), sin envolverla en un botón. No se llama a
  `app_token_secret` para una aplicación revocada: ya se sabe que no debe
  ofrecerse como copiable, preguntar sería trabajo de más para un resultado
  que no se va a usar.
- **Alternativa descartada:** dejar el botón activo y que, al copiar, el
  mensaje explique que está revocada. Se descarta porque el comportamiento
  esperado ya lo dice explícito en `spec.md`: «no tiene sentido seguir
  ofreciéndolo como copiable». Ofrecer el botón y luego desmentirlo es peor
  interfaz que no ofrecerlo.
- **Consecuencia que hay que asumir:** ninguna real — es exactamente el
  comportamiento que ya describía la especificación.

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| Revocar o borrar una aplicación deja el secreto huérfano en el llavero (si `secrets.delete` falla y nadie se entera) | El `tracing::warn!` de D2 queda en el log; no hay impacto funcional porque nada vuelve a pedir ese secreto una vez la aplicación no existe o está revocada — es basura inerte, no un fallo de seguridad activo |
| `app_token_secret` se llama para un `app_id` que no existe o no es del usuario que llama | `SecretRef::app_token(app_id)` es una clave que solo el propio Nexo genera y solo la interfaz local puede invocar (comando Tauri, no expuesto por el gateway HTTP); un id inventado simplemente no tiene entrada y devuelve `None`, igual que una aplicación sin secreto |
| El botón de clave llama al comando pero el usuario no ve ninguna diferencia entre «clave completa copiada» y «solo prefijo» | Criterio 6: la prueba manual comprueba el texto exacto que aparece en cada caso, no solo que algo se copia |

## ¿Hace falta un ADR?

No, esta especificación es la implementación del [ADR 0004](../../docs/adr/0004-tokens-de-aplicacion-recuperables.md),
ya aceptado. Ninguna decisión de este diseño toca una decisión de
arquitectura que no estuviera ya tomada ahí.

## Qué queda pendiente de descubrir

- **Si `keyring` en macOS pide permiso de Keychain la primera vez que Nexo
  escribe una entrada nueva (`app/<id>/token`), y si eso interrumpe visualmente
  la creación de una aplicación.** Ya lo hace hoy para las cuentas de
  proveedor sin que se haya reportado como problema, así que no se espera
  sorpresa, pero solo se confirma probando con la aplicación instalada.
- **Si borrar y volver a crear una aplicación con el mismo nombre deja dos
  entradas de Keychain distintas o reutiliza la vieja.** No debería: el
  `SecretRef` se indexa por `app_id`, que es un id nuevo por aplicación
  (`util::new_id("app")`), nunca por nombre. Se anota para confirmarlo con
  una prueba si al implementar surge alguna duda.
