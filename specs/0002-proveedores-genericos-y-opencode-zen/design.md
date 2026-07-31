# 0002 · Diseño

## Enfoque

Un adaptador nuevo, `OpenAiCompatAdapter`, que **no está atado a un proveedor
concreto**: sirve a cualquier proveedor que el usuario haya añadido, tomando su
dirección de la credencial igual que hace LM Studio. Los proveedores añadidos viven
en una tabla nueva, y el router cae a este adaptador cuando el `provider_id` no
corresponde a ninguno de los integrados.

Las capacidades y los precios se cruzan con `models.dev`, cacheado en disco.

OpenCode Zen tiene su **propia opción en la interfaz** con la URL ya rellena —solo
hay que pegar la clave—, pero por dentro es ese mismo proveedor OpenAI-compatible.
La distinción es de interfaz, no de código.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/db/migrations.rs` | **Migración v2**: tabla `custom_providers` |
| `crates/nexo-core/src/db/mod.rs` | CRUD de proveedores añadidos |
| `crates/nexo-core/src/catalog/models_dev.rs` | **Nuevo.** Cliente y caché de `models.dev`, y traducción a `ModelDescriptor` |
| `crates/nexo-core/src/provider/openai_compat.rs` | **Nuevo.** El adaptador genérico |
| `crates/nexo-core/src/translate/chat_completions.rs` | Clasificación de errores por cuerpo, no solo por status |
| `crates/nexo-core/src/service.rs` | Resolución del adaptador con respaldo genérico; alta y baja de proveedores; catálogo cruzado |
| `src-tauri/src/commands.rs`, `main.rs` | Comandos de alta, baja y listado |
| `src/lib/views/Providers.svelte` | Formulario de proveedor nuevo y atajo de Zen |
| `src/lib/api.ts` | Tipos y llamadas |

El gateway, el motor de estadísticas y las políticas **no se tocan**. Es la tercera
vez que se comprueba el contrato de proveedor con un adaptador nuevo.

## Decisiones

### D1. El adaptador genérico se resuelve por descarte, no por registro

- **Decisión:** `self.adapters` sigue siendo un mapa fijo con los integrados. Cuando
  no hay entrada para `<provider>:<kind>` y ese `provider_id` existe en
  `custom_providers`, se usa una instancia compartida de `OpenAiCompatAdapter`.
- **Alternativa descartada:** registrar dinámicamente un adaptador por proveedor
  añadido, es decir convertir `adapters` en un mapa mutable. Se descarta porque
  obligaría a sincronizar ese mapa con la base de datos en cada alta, baja y arranque,
  con un candado por medio, para no ganar nada: el adaptador es idéntico para todos y
  lo único que cambia por instancia es la dirección, que ya viaja en la credencial.
- **Consecuencia:** el adaptador genérico es **sin estado por proveedor**. Toda su
  configuración llega en `ResolvedCredential`, lo que además lo hace trivial de probar.

### D2. La dirección viaja en la credencial, no en el adaptador

- **Decisión:** `external_id` de la cuenta guarda la URL base, y el adaptador la lee
  en cada llamada. Igual que LM Studio tras la corrección de la especificación 0001.
- **Alternativa descartada:** un campo nuevo en `ResolvedCredential`. No hace falta:
  `external_id` ya significa «de dónde viene esta autorización», y para un endpoint
  configurable eso *es* su dirección.
- **Consecuencia:** cambiar la URL de un proveedor surte efecto sin reiniciar, y el
  `UNIQUE (provider_id, credential_kind, external_id)` de `accounts` sigue teniendo
  sentido.

### D3. Los proveedores añadidos viven en su propia tabla

- **Decisión:** migración v2 con `custom_providers (id, name, base_url, created_at)`,
  donde `id` es el slug derivado del nombre y es la clave primaria.
- **Alternativa descartada:** deducirlos de la tabla `accounts`, ya que allí hay
  `provider_id` libre. Se descarta porque haría imposible distinguir un proveedor
  añadido por el usuario de un `provider_id` desconocido por corrupción o por una
  versión anterior, y porque el nombre legible tiene que sobrevivir a desconectar la
  cuenta sin borrar el proveedor.
- **Consecuencia:** la clave primaria da gratis el criterio 3 —dos proveedores con el
  mismo nombre no pueden existir— con un error de la base de datos que se traduce a un
  mensaje claro.

### D4. `models.dev` se cachea en disco, no en la base de datos

- **Decisión:** se descarga a un fichero junto a la base de datos, con caducidad de 7
  días, y se parsea a memoria solo al sincronizar catálogo.
- **Alternativa descartada:** volcarlo a una tabla. Se descarta porque son 3,3 MB y
  miles de modelos de 176 proveedores, de los que interesan unas decenas; meterlo en
  SQLite obligaría a una migración, a mantenerlo sincronizado y a consultarlo por SQL
  para algo que se lee una vez por sincronización.
- **Consecuencia:** si `models.dev` no está disponible y no hay caché, el catálogo se
  ofrece **solo como texto** en lugar de quedarse vacío. Degradar la riqueza de los
  metadatos es aceptable; prometer capacidades sin dato no lo es.

### D5. El precio de `models.dev` produce coste estimado, nunca reportado

- **Decisión:** un modelo con precio en `models.dev` obtiene `Accounting::Metered` y
  `Pricing`, lo que por el camino ya existente da `CostBasis::Estimated`.
- **Alternativa descartada:** marcarlo `Reported` porque el precio es un dato
  publicado. Se descarta porque **el proveedor no informa del coste de la petición**:
  Nexo lo calcula multiplicando tokens por una tarifa de terceros. Eso es una
  estimación, y la invariante nº3 existe precisamente para no confundirlas.
- **Consecuencia:** ningún cambio en el motor de estadísticas.

### D6. Los errores se clasifican por el cuerpo cuando el status no es fiable

- **Decisión:** `classify_http_error` intenta primero leer un sobre de error JSON
  (`{"error": {"type": ...}}`) y usa ese tipo; si no lo hay, cae al status.
- **Alternativa descartada:** un clasificador propio dentro del adaptador genérico. Se
  descarta porque el problema no es de Zen: cualquier proveedor compatible puede
  devolver un status pobre, y la lógica pertenece al módulo del formato.
- **Consecuencia:** «saldo insuficiente» deja de presentarse como «clave inválida»,
  que es exactamente lo que el usuario vio en Msty. Verificado con los tres cuerpos
  reales capturados en T0, los tres con HTTP 401.

### D7. OpenCode Zen es una opción propia en la interfaz, con la URL ya rellena

- **Decisión:** en *Proveedores* hay una **sección dedicada a OpenCode Zen**, aparte
  del formulario genérico, con su nombre y su URL ya puestos. El usuario le da y solo
  rellena la API key. Por dentro crea exactamente el mismo tipo de proveedor y usa el
  mismo adaptador.
- **Alternativa descartada:** ofrecer solo el formulario genérico y que el usuario
  teclee la URL de Zen. Se descarta porque obliga a saberla y a escribirla bien para
  algo que Nexo ya conoce; la fricción es innecesaria y es justo la que hizo falta
  resolver en Msty a mano.
- **Alternativa también descartada:** un `provider_id` reservado con trato especial en
  el núcleo. No aporta nada y crearía un proveedor que no se puede borrar como los
  demás. La distinción es **de interfaz, no de código**.
- **Consecuencia:** añadir mañana un atajo para OpenRouter, Groq o DeepSeek —los tres
  están en `models.dev` con su `api` ya publicada— es una línea de datos, no un
  adaptador nuevo.

```rust
/// Atajos que la interfaz ofrece como opción propia. Solo son datos: el
/// proveedor que se crea es un OpenAI-compatible como cualquier otro.
pub struct ProviderPreset {
    pub suggested_name: &'static str,
    pub base_url: &'static str,
    pub docs_url: &'static str,
}

pub const OPENCODE_ZEN: ProviderPreset = ProviderPreset {
    suggested_name: "OpenCode Zen",
    base_url: "https://opencode.ai/zen/v1",
    docs_url: "https://opencode.ai/docs/zen/",
};
```

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| `models.dev` cambia de forma o desaparece | El parser devuelve vacío → catálogo solo-texto, con un aviso en el log; nunca catálogo vacío |
| Un `provider_id` en `accounts` sin fila en `custom_providers` (base de datos manipulada, versión anterior) | El router no encuentra adaptador y devuelve un error que nombra el proveedor, no un pánico |
| El usuario mete una URL que no habla `chat/completions` | El error de traducción (`Malformed`) lo hace evidente, con el cuerpo recibido |
| Dos proveedores cuyos nombres dan el mismo slug («Mi Proveedor» y «mi-proveedor») | La clave primaria lo rechaza; el mensaje debe decir con qué nombre choca |
| Un modelo existe en varios proveedores de `models.dev` con capacidades distintas | Se prefiere la coincidencia exacta de proveedor; el respaldo por nombre queda documentado y el coste sigue siendo estimado |

## ¿Hace falta un ADR?

**No.** No se cambia ninguna decisión de arquitectura: se usa el eje de credencial
del ADR 0001 y el contrato de proveedor tal como está. La migración v2 es aditiva.

Sí hay que actualizar `docs/modelo-datos.md` con la tabla nueva y
`docs/contrato-proveedor.md` con la nota de que un adaptador puede servir a varios
proveedores.

## Lo que queda pendiente de descubrir

- **Si `GET {base}/models` es fiable en proveedores arbitrarios.** Con Zen sí. Con
  otros habrá que verlo; por eso existe la ruta de añadir modelos a mano.
- **Si `models.dev` acierta con los identificadores de un proveedor cualquiera.** Con
  Zen es 60 de 60, pero un proxy que renombre sus modelos no coincidirá. Se mide al
  probar y se acepta el respaldo solo-texto.
