# 0012 · Diseño

Casi todo el diseño es **borrar**. Lo que queda es dónde se corta y qué ocupa
el hueco que deja el certificado en la interfaz.

## Ficheros que se tocan

| Fichero | Qué pasa |
| --- | --- |
| `crates/nexo-core/src/tls_cert.rs` | se elimina completo |
| `crates/nexo-core/src/lib.rs` | fuera `pub mod tls_cert` |
| `crates/nexo-core/src/gateway/mod.rs` | fuera `serve_on_tls` y el módulo de prueba `tls_from_reserved_listener` |
| `crates/nexo-core/Cargo.toml` | fuera `rcgen` y `axum-server`; dentro `if-addrs` |
| `crates/nexo-core/src/net.rs` | `detect_lan_ip` se mantiene; se añade `listening_addresses()` |
| `crates/nexo-core/src/service.rs` | `GatewayBindPlan` vuelve a una dirección; `LanAccessInfo` cambia de forma; `prepare_gateway_bind` se simplifica |
| `src-tauri/src/main.rs` | un solo `bind` + `serve_on`, como antes del PR #18 |
| `src-tauri/src/commands.rs` | texto de `lan_risk_notice` |
| `src/lib/api.ts` | forma de `LanAccessInfo` |
| `src/lib/views/Settings.svelte` | lista de direcciones y aviso; fuera huella y ruta del certificado |
| `crates/nexo-core/tests/gateway_e2e.rs` | fuera los tres tests de TLS; dentro los de HTTP plano por la red |

## Decisiones

### 1. Se elimina el código de TLS, no se deja apagado

**Alternativa descartada:** conservar `tls_cert` y `serve_on_tls` detrás de una
opción, para poder volver sin reescribir. Se descarta porque un camino que
nadie ejecuta se podre sin avisar: `rcgen` y `axum-server` seguirían pidiendo
actualizaciones, sus pruebas seguirían costando tiempo en cada CI, y el primer
día que se intentara usar habría dejado de funcionar por algo ajeno. El ADR
0005 guarda el motivo y la vía de vuelta; git guarda el código. Volver es
reescribir doscientas líneas con el diseño ya decidido, no investigar de cero.

**Cómo se detectará si esto fue un error:** el criterio 4 de la spec es un
`grep` que falla si queda algún resto. Si la decisión se revierte, el ADR 0005
dice exactamente por dónde empezar.

### 2. Un solo listener, no dos

Sin TLS desaparece la razón del par de listeners del PR #18: `0.0.0.0` en HTTP
plano ya atiende a loopback y a la red con el mismo protocolo. `GatewayBindPlan`
vuelve a `{ addr }`.

**Alternativa descartada:** mantener el listener de loopback aparte «por si
acaso». Sobra: sería el mismo protocolo en el mismo puerto, dos sockets para
un solo comportamiento.

**Qué puede romperse:** que `settings.bind_addr()` deje de devolver `0.0.0.0`
con `allow_lan` activo. Ya está cubierto por sus propias pruebas y por el
criterio 2.

### 3. `LanAccessInfo` pasa de huella a lista de direcciones

De:

```rust
pub struct LanAccessInfo {
    pub address: Option<String>,
    pub port: u16,
    pub cert_fingerprint_sha256: String,
    pub cert_path: String,
    pub cert_rotated: bool,
}
```

a:

```rust
pub struct LanAccessInfo {
    /// Todas las IPv4 no-loopback de la máquina, con el nombre de su
    /// interfaz, porque sin cifrado la exposición es la información
    /// importante. La primera es la de la ruta por defecto, que es la que el
    /// usuario querrá casi siempre.
    pub addresses: Vec<ListeningAddress>,
    pub port: u16,
}

pub struct ListeningAddress {
    pub interface: String,
    pub address: String,
    /// `true` para la de la ruta por defecto.
    pub preferred: bool,
}
```

**Alternativa descartada:** dejar `address: Option<String>` y añadir la lista
al lado. Duplica la misma información en dos campos y obliga a la interfaz a
decidir cuál cree. Con `preferred` en la lista, hay una sola fuente.

**Qué puede romperse:** la interfaz deja de compilar si algún sitio sigue
leyendo `cert_fingerprint_sha256`. Lo detecta `npm run check`.

### 4. `net` gana `listening_addresses()` y conserva `detect_lan_ip()`

`detect_lan_ip()` sigue siendo la forma de saber cuál es la dirección de la
ruta por defecto —no la sustituye `if-addrs`, que enumera pero no sabe qué
interfaz usa el sistema para salir—. `listening_addresses()` enumera con
`if-addrs`, filtra loopback e IPv6, y marca como `preferred` la que coincida
con `detect_lan_ip()`.

**Alternativa descartada:** deducir la preferida por el nombre de la interfaz
(`en0` primero). Es una heurística que falla en cuanto hay Ethernet, Wi-Fi y
VPN a la vez; la tabla de rutas ya sabe la respuesta.

**Qué puede romperse:** una máquina sin ninguna dirección no-loopback devuelve
lista vacía. El panel tiene que decir «sin direcciones de red detectadas» en
lugar de mostrar una lista vacía sin explicación.

### 5. El aviso previo es la mitigación, así que es explícito

`lan_risk_notice` cambia su segundo punto —el que prometía un certificado— por
uno que dice que el tráfico no va cifrado, qué se puede leer (el token y las
conversaciones) y quién puede leerlo (cualquier equipo de la misma red). El
resto de puntos se mantienen. El aviso sigue exigiendo aceptación explícita:
eso no se toca, y ahora es lo único que queda entre el usuario y el riesgo.

## Lo que puede romperse, en conjunto

| Riesgo | Cómo se detecta |
| --- | --- |
| Queda un resto de TLS sin quitar | criterio 4: `grep` que falla |
| El modo red deja de servir por la red | criterio 3: e2e real por la IP de red |
| El modo local cambia sin querer | criterio 1: prueba existente, sin tocar |
| La interfaz sigue leyendo campos que ya no existen | `npm run check` |
| El aviso queda prometiendo cifrado | criterio 6: prueba sobre el texto |
| Los clientes con `https://` configurado dejan de conectar | conocido y aceptado; solo afecta al equipo de la red, que hoy no conecta |

## ADR

Ya escrito: [ADR 0005](../../docs/adr/0005-red-local-sin-cifrado.md). Sustituye
el punto 2 del ADR 0003 y modifica la invariante 9 de `CLAUDE.md`, las dos
cosas ya hechas antes de esta especificación porque son la autorización para
escribirla.
