# 0007 · Diseño

- **Spec:** [spec.md](spec.md)
- **ADR:** [0003](../../docs/adr/0003-acceso-desde-la-red-local.md) — ya
  aceptado, este documento no cambia esa decisión, la implementa.

## Resumen del cómo

`Settings.allow_lan` ya existe (`crates/nexo-core/src/config.rs:17`) y hoy es
un no-op documentado a propósito: *"el gateway se niega a arrancar en 0.0.0.0
mientras no exista transporte seguro"*. Este diseño rellena esa pieza
pendiente: un módulo nuevo que genera y persiste un certificado autofirmado,
una segunda vía de arranque del gateway que sirve HTTPS con `axum-server`
sobre ese certificado, y la superficie de Configuración para activarlo con
aviso previo. **Nada de esto se ejecuta cuando `allow_lan == false`**: ese
camino sigue exactamente el `bind()` + `serve_on()` de siempre, sin ninguna
rama nueva en medio.

## Dependencias nuevas, verificadas contra el registro real

`nexo-core` no tiene hoy ninguna pieza para terminar TLS del lado servidor —
solo lo usa como cliente saliente vía `reqwest`. Comprobado contra
`crates.io` el 2026-08-02 (no de memoria, seed de otras veces que una
suposición de librería resultó equivocada):

```toml
axum-server = { version = "0.8.0", features = ["tls-rustls"] }
rcgen = "0.14.8"
```

- **`axum-server` 0.8.0**, feature `tls-rustls`: sirve un `Router` de axum
  sobre TLS con `rustls`. Esa feature activa `rustls/aws-lc-rs` como proveedor
  criptográfico.
- **`reqwest` 0.13.4** (ya en el árbol de dependencias), feature `rustls`:
  comprobado en su manifiesto de crates.io que también activa
  `__rustls-aws-lc-rs`. **Mismo backend criptográfico que `axum-server` en las
  dos direcciones**, cliente saliente y servidor entrante. No hace falta la
  variante `tls-rustls-no-provider` de `axum-server` para forzar `ring`: no
  hay conflicto de proveedor que evitar.
- **`rcgen` 0.14.8**: genera el par de claves y el certificado autofirmado.
  Se usa solo en el módulo nuevo, no se filtra al resto del núcleo.

## Ficheros que se tocan

| Fichero | Cambio |
| --- | --- |
| `crates/nexo-core/Cargo.toml` | añade `axum-server` y `rcgen` |
| `crates/nexo-core/src/net.rs` (nuevo) | `detect_lan_ip()`: la única IP no-loopback que el sistema operativo usaría para salir a la red, sin dependencia nueva |
| `crates/nexo-core/src/tls_cert.rs` (nuevo) | genera, persiste, lee y valida el certificado autofirmado |
| `crates/nexo-core/src/gateway/mod.rs` | nueva `serve_on_tls()`; `serve_on()` no cambia |
| `crates/nexo-core/src/config.rs` | `bind_addr()` empieza a devolver `0.0.0.0` cuando `allow_lan` es `true` |
| `crates/nexo-core/src/service.rs` | estado `lan_info` (mismo patrón que `bind_error`, ya existente); `status()` lo expone |
| `crates/nexo-core/src/lib.rs` | declara los dos módulos nuevos |
| `src-tauri/src/main.rs` | rama nueva de arranque cuando `allow_lan == true` |
| `src-tauri/src/commands.rs` | `save_settings` exige `lan_risk_acknowledged` cuando `allow_lan == true`; nuevo comando `lan_risk_notice()` |
| `src/lib/api.ts`, `src/lib/views/Settings.svelte` | interruptor, aviso, checkbox y bloque con la dirección y la huella del certificado |

## Decisiones, con la alternativa descartada

### 1. `detect_lan_ip()` sin dependencia nueva, con el truco del socket UDP

**Decisión.** `std::net::UdpSocket::bind("0.0.0.0:0")?.connect("8.8.8.8:80")?`
seguido de `.local_addr()?.ip()`. Un socket UDP con `connect()` no manda
ningún paquete: el kernel solo resuelve la interfaz de salida según su tabla
de rutas y fija esa IP local. Es la misma técnica que usan herramientas de
diagnóstico de red sin privilegios especiales; no depende de que `8.8.8.8`
responda ni de haber Internet, solo de que exista una ruta por defecto (que
en cualquier red local con un router, existe).

**Descartada: crate `local-ip-address` o `if-addrs`.** Resolverían lo mismo,
listando además todas las interfaces. Se descarta para la v1 porque la spec
pide *"al menos una IP alcanzable, no un listado exhaustivo"* — añadir una
dependencia para un dato que ya se puede obtener con `std` no se justifica
todavía. Si en el uso real una sola IP resulta insuficiente (varias interfaces
activas a la vez, por ejemplo Ethernet y Wi-Fi), se reconsidera.

**Qué puede romperse.** En una máquina sin ninguna ruta por defecto (rarísimo,
pero posible recién arrancada sin red), `detect_lan_ip()` devuelve `None`. El
certificado se genera igualmente, con SAN solo para `127.0.0.1`/`localhost`;
el panel muestra que no se detectó ninguna IP de red y sugiere revisar la
conexión. No es un error que impida activar el modo, porque el modo puede
activarse antes de que el cable esté enchufado.

### 2. El SAN del certificado se fija al generarlo, no se recalcula solo

**Decisión.** El certificado autofirmado incluye como
`Subject Alternative Name`: la IP detectada por `detect_lan_ip()` en el
momento de generarlo, `127.0.0.1`, `::1` y `localhost`. Se genera una sola vez
y se reutiliza en cada arranque mientras el fichero exista y sea válido — tal
como fija el criterio de aceptación 4 de `spec.md`.

**Descartada: regenerar automáticamente si la IP detectada cambia entre
arranques (DHCP).** Es la solución "completa", pero invalida en silencio la
confianza que el usuario ya depositó en el certificado anterior en cada
dispositivo cliente, sin que lo pida ni lo note hasta que falla una conexión.
`spec.md` ya deja anotado el límite exacto de esto en "Fuera de alcance":
*"renovarlo hoy significa borrar el fichero a mano"*. Se documenta aquí como
riesgo (más abajo) en vez de resolverse con más código.

### 3. Fallo del certificado: cae a `127.0.0.1`, nunca a HTTP plano en `0.0.0.0`

**Decisión.** En `main.rs`, si `allow_lan == true`:

1. Se reserva el puerto igual que hoy, de forma síncrona
   (`nexo_core::gateway::bind`), para seguir fallando pronto si está ocupado.
2. Se llama a `tls_cert::ensure(data_dir)`.
   - **Si tiene éxito:** se sirve con `serve_on_tls()` sobre `0.0.0.0`, y se
     guarda en `Nexo` la información de conexión (`set_lan_info`) que el panel
     va a mostrar.
   - **Si falla** (fichero corrupto, sin permisos de escritura en el
     directorio de datos, lo que sea): se sirve igual que si
     `allow_lan == false` — `127.0.0.1`, HTTP plano — y se llama a
     `nexo.set_bind_error(Some(detail))` con el motivo exacto, reutilizando el
     mecanismo que ya existe para "el puerto está ocupado". El panel ya sabe
     pintar ese campo.

**Descartada: no servir nada (ni siquiera localhost) si el certificado
falla.** Dejaría inutilizada toda la aplicación por un fallo de una función
que el usuario activó como añadido, no como su uso principal. Caer al modo
que ya funcionaba es la opción que no degrada en silencio (el error se ve) y
tampoco convierte un fallo de una parte en un fallo total.

**Cómo se detecta cuando se rompe.** Test que fuerza el fallo (fichero
`tls/cert.pem` con contenido corrupto antes de arrancar) y comprueba que:
`bind_addr` efectivo es `127.0.0.1`, `bind_error()` no es `None`, y una
petición contra `127.0.0.1` sigue funcionando con el token de siempre.

### 4. `save_settings` exige `lan_risk_acknowledged` cuando se guarda con `allow_lan = true`

**Decisión.** Mismo patrón que `connect_chatgpt` (`commands.rs:78-93`), que ya
exige `risk_acknowledged: bool` y rechaza con un error de texto si no viene a
`true`. `save_settings` gana un parámetro análogo:

```rust
pub fn save_settings(
    state: State<'_, AppState>,
    settings: Settings,
    lan_risk_acknowledged: bool,
) -> CmdResult<Value> {
    if settings.allow_lan && !lan_risk_acknowledged {
        return Err("hay que aceptar el aviso antes de activar el acceso en red".into());
    }
    ...
}
```

Se comprueba sobre el valor **entrante** de `allow_lan`, no sobre si cambió
respecto al guardado anterior. Guardar con el interruptor activado exige
haber marcado la casilla en esa misma edición del formulario, siempre — igual
de estricto que el guardado repetido no lo es para otros campos, pero es la
regla más simple que no puede quedar desalineada: no hay que llevar cuenta de
"cambió de `false` a `true`" en ningún sitio.

**Descartada: solo exigirlo en la transición `false → true`.** Ahorraría al
usuario reconfirmar si solo cambia el puerto con el modo ya activo, pero
obliga a comparar contra el valor anterior en el propio comando, con el
riesgo de que otra vía de guardado (o una prueba) se olvide de esa
comparación y active el modo en silencio. Se prefiere la regla sin memoria.

**El aviso mismo.** Nuevo comando `lan_risk_notice() -> RiskNotice` (mismo
tipo que ya devuelve `risk_notice()`, `commands.rs:70-74`), con el texto que
"vive en el núcleo para que no pueda quedar desalineado con lo que el código
hace" — cita literal del comentario que ya existe sobre el otro aviso, y
aplica igual aquí.

### 5. `axum-server` se construye a partir de un listener ya reservado, no de una dirección

**Decisión.** `serve_on_tls()` recibe el mismo `tokio::net::TcpListener` que
ya devuelve `bind()`, lo convierte a `std::net::TcpListener` (`into_std()`,
método existente en `tokio::net::TcpListener`) y lo pasa a
`axum_server::from_tcp_rustls`. Esto conserva la propiedad que el gateway ya
tiene desde el principio: el puerto se reserva de forma síncrona antes de
lanzar la tarea de fondo, así que un puerto ocupado se detecta y se informa
igual que hoy, con o sin TLS de por medio.

**Riesgo de esta decisión, no de la anterior.** Si en `/build` resulta que
`axum-server` 0.8.0 no ofrece exactamente esa función con esa firma, la
alternativa de respaldo es construir el `RustlsConfig` igual y usar la
variante de `axum-server` que sí acepte un `std::net::TcpListener` (existe
más de una, todas documentadas en su API pública); no cambia el resto del
diseño, solo el nombre de la llamada. Se anota aquí para que `/tasks` incluya
verificarlo como primer paso, antes de construir el resto encima.

### 6. El certificado no expira en un plazo corto, de forma explícita

**Decisión.** `tls_cert::ensure()` fija `not_after` a una fecha lejana (varias
décadas) en vez de aceptar el valor por defecto de `rcgen`, y un test
comprueba que la fecha de expiración generada está, como mínimo, a 10 años
vista. Un certificado que caduca solo es una forma de romper el modo red en
silencio semanas o meses después de activarlo, exactamente el tipo de fallo
que este proyecto ya se ha encontrado por no probar contra la realidad.

### 7. Alcance de familia de IP: solo IPv4

**Decisión.** `detect_lan_ip()` y el SAN del certificado cubren IPv4. IPv6 no
se descarta por ningún motivo técnico salvo que no lo pide el caso de uso
(varios ordenadores en una red doméstica, que hoy en la práctica se
identifican entre sí por IPv4) y añadiría SAN, detección y textos de UI por
duplicado. Si hace falta, es una ampliación de este mismo módulo, no un
rediseño.

## Contrato de `tls_cert.rs`

```rust
pub struct LanCertificate {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub fingerprint_sha256: String,
    pub address: Option<IpAddr>,
}

pub enum TlsCertError {
    Io(std::io::Error),
    Generation(String),
    Corrupt(String),
}

/// Si `<data_dir>/tls/cert.pem` y `key.pem` existen y son válidos, los lee y
/// devuelve. Si no existen, los genera. Si existen pero no se pueden leer o
/// no son un certificado válido, devuelve `Err` — nunca regenera por encima
/// de un fichero que no se pudo entender, para no pisar algo que el usuario
/// podría necesitar diagnosticar.
pub fn ensure(data_dir: &Path) -> Result<LanCertificate, TlsCertError>;
```

`key.pem` se escribe con permisos `0600`; el directorio `tls/` con `0700`
(Unix). `cert.pem` no es secreto, permisos por defecto.

## Contrato de `net.rs`

```rust
/// La IP no-loopback que el sistema usaría para salir a la red, o `None` si
/// no hay ninguna ruta disponible. No manda tráfico real.
pub fn detect_lan_ip() -> Option<std::net::IpAddr>;
```

## Cambios de contrato existentes

- `Settings::bind_addr()` (`config.rs:45`): devuelve `0.0.0.0:{port}` si
  `allow_lan`, `127.0.0.1:{port}` si no. Los dos tests que ya existen sobre
  esta función (`defaults_are_safe`,
  `allow_lan_does_not_widen_the_bind_yet`) se reescriben: el segundo pasa a
  llamarse `allow_lan_widens_the_bind` y comprueba `0.0.0.0`, porque la
  premisa que probaba ("sin TLS no se expone") deja de ser cierta — ahora sí
  hay TLS.
- `GatewayStatus` (`service.rs:1808`): campo nuevo `lan: Option<LanAccessInfo>`
  con `{ address: Option<String>, port: u16, cert_fingerprint_sha256: String,
  cert_path: String }`, `None` cuando el modo red no está activo o no arrancó
  (fallo de certificado, cubierto por `bind_error` en su lugar).

## Qué puede romperse y cómo se nota

| Riesgo | Detección |
| --- | --- |
| Cambio de IP por DHCP invalida el SAN del certificado ya emitido | El usuario ve el aviso de "nombre no coincide" en el cliente al conectar; documentado en `spec.md` como límite conocido de la v1, mitigación manual (borrar `tls/cert.pem`) |
| `axum-server` no compone con un listener ya reservado como se espera | Se verifica como primera tarea de `/tasks`, antes de construir el resto |
| Certificado corrupto o sin permisos de lectura | Test dedicado (decisión 3); cae a `127.0.0.1` con `bind_error` visible, nunca a HTTP plano en `0.0.0.0` |
| Ninguna interfaz de red al generar el certificado | SAN solo con loopback; el panel indica que no se detectó IP de red, no bloquea la activación |
| Doble backend criptográfico de `rustls` (`ring` vs `aws-lc-rs`) causando pánico al arrancar | Descartado por inspección real de features de `reqwest` y `axum-server` (ver arriba); si `cargo test --workspace` fallara al arrancar el runtime con un error de proveedor por defecto, sería la primera señal y anularía esta fila de la tabla |

## Consecuencias para `spec.md`

Ninguna: el diseño no revela que la especificación sea irrealizable ni que
haya que recortar alcance. Se añade a "Riesgos" de `spec.md` la mención
explícita del límite de DHCP/SAN, que en la spec original quedaba implícito
dentro de "Fuera de alcance → rotación automática" pero no nombrado como
riesgo del modo activo; se corrige ahí para que quede en el sitio correcto.
