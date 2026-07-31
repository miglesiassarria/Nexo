# 0001 · Diseño

## Enfoque

Un adaptador nuevo, `lmstudio`, con vía `CredentialKind::Local`. Descubre el
catálogo por el endpoint nativo `/api/v0/models`, que publica tipo, cuantización,
contexto y estado de carga, y sirve el chat por la superficie compatible con OpenAI
`/v1/chat/completions`. Como esa superficie es la misma que ya habla el adaptador de
API key, lo primero es **extraer esa traducción a un módulo compartido** en lugar de
duplicarla.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/translate/chat_completions.rs` | **Nuevo.** Construcción de la petición y traducción de chunks, extraídos de `openai_apikey.rs` |
| `crates/nexo-core/src/provider/openai_apikey.rs` | Pasa a usar el módulo compartido; pierde el código duplicado |
| `crates/nexo-core/src/provider/lmstudio.rs` | **Nuevo.** El adaptador |
| `crates/nexo-core/src/provider/mod.rs` | Registra el módulo |
| `crates/nexo-core/src/config.rs` | Ajuste `lmstudio_base_url` |
| `crates/nexo-core/src/db/mod.rs` | Persistencia de ese ajuste |
| `crates/nexo-core/src/service.rs` | Registro del adaptador, detección al arrancar, conexión y desconexión |
| `src-tauri/src/commands.rs` | `connect_lmstudio`, `detect_local_providers` |
| `src/lib/views/Providers.svelte` | Sección de LM Studio con su estado |
| `src/lib/views/Models.svelte` | Columnas de cuantización y estado de carga |
| `src/lib/views/Dashboard.svelte` | Etiqueta de coste para la vía local |
| `src/lib/api.ts` | Tipos y llamadas nuevas |

Nada de esto toca el router, el motor de estadísticas ni el gateway: es la prueba
de que el contrato de proveedor aguanta. Si hubiera que tocarlos, el contrato
estaría mal.

## Decisiones

### D1. Descubrir por el endpoint nativo, servir por el compatible

- **Decisión:** catálogo desde `GET /api/v0/models`; chat contra
  `POST /v1/chat/completions`.
- **Alternativa descartada:** usar solo `/v1/models` para todo, porque sería un único
  camino y menos código. Se descarta porque devuelve únicamente `id`: sin `type` no
  se puede saber que un modelo hace visión, ni que otro solo hace embeddings, y sin
  eso el catálogo mentiría sobre las capacidades. Eso es lo que la invariante nº2
  prohíbe.
- **Consecuencia:** dependemos de un endpoint propio de LM Studio, verificado con la
  0.4.20. Si desaparece, se cae a `/v1/models` con metadatos pobres (ver D5).

### D2. El tipo de modelo decide las capacidades, y un modelo de embeddings no hace texto

- **Decisión:** `type: "vlm"` → `text` + `vision`; `type: "llm"` → `text`;
  `type: "embeddings"` → `embeddings: true` y **`text: false`**. Las herramientas se
  activan si `capabilities` incluye `tool_use`.
- **Alternativa descartada:** excluir del catálogo los modelos de embeddings. Se
  descarta porque el usuario los tiene y quiere verlos; ocultarlos sería mentir por
  omisión, y además el día que exista `/v1/embeddings` ya estarán ahí.
- **Consecuencia:** poner `text: false` hace que `check_capabilities` rechace el chat
  con `Unsupported { capability: "text" }` **sin escribir ni una línea nueva de
  comprobación**. El criterio 3 de la especificación se cumple reutilizando la
  invariante que ya existe. Ese encaje es la señal de que el modelo de capacidades
  estaba bien diseñado.

### D3. Lo local se representa como cuenta, aunque no tenga credencial

- **Decisión:** al detectar LM Studio se crea una fila en `accounts` con
  `credential_kind = "local"`, sin `keychain_ref`, con la dirección en `external_id`.
- **Alternativa descartada:** tratarlo como excepción igual que el proveedor mock,
  que está cableado y no necesita cuenta. Se descarta porque el usuario tiene que
  poder verlo, editar su dirección y desconectarlo en *Proveedores*, y todo eso ya
  funciona sobre `accounts`. Meter una segunda forma de existir para los proveedores
  locales duplicaría la interfaz y las comprobaciones de `models_for_app`.
- **Consecuencia:** `resolve_credential` devuelve un secreto vacío para `Local`, que
  es lo que ya hacía. No hay cambios en el gestor de identidad.

### D4. El coste local es cero **conocido**, y no se muestra como una cifra

- **Decisión:** se mantiene `Accounting::Local → CostBasis::Reported` con
  `cost_micros = None`. La interfaz muestra «Local» cuando la vía es `local`, en
  lugar de «0.0000 $ (dato)».
- **Alternativa descartada:** añadir un quinto estado de contabilidad, `Local`. Se
  descarta porque la invariante nº3 fija cuatro y cambiarla exigiría un ADR nuevo
  para ganar muy poco: el coste local **es** cero y **es** conocido, así que
  `Reported` es literalmente cierto. El problema era solo de presentación.
- **Consecuencia:** el criterio 8 se cumple sin tocar el motor de estadísticas,
  porque `cost_micros = None` suma cero al agregado reportado.

### D5. La detección tiene que confirmar que es LM Studio

- **Decisión:** se considera detectado solo si `/api/v0/models` responde `200` con un
  objeto que tenga `data` como array. Si responde algo pero no con esa forma, no se
  conecta y se dice por qué.
- **Alternativa descartada:** dar por bueno cualquier `200` en el puerto 1234. Se
  descarta porque ese puerto lo usa más de un programa y acabaríamos ofreciendo un
  catálogo de otro producto como si fuera de LM Studio.
- **Consecuencia:** si LM Studio cambiara ese endpoint, la detección fallaría y el
  usuario podría añadir la dirección a mano; el chat seguiría funcionando porque va
  por la superficie compatible.

### D6. Sin tiempo máximo en la primera petición

- **Decisión:** no se impone un timeout total a las peticiones locales. El cliente
  HTTP solo tiene `connect_timeout`.
- **Alternativa descartada:** un timeout generoso, por ejemplo 120 s. Se descarta
  porque cargar un modelo de 35B puede tardar más y cortarlo produciría un fallo
  falso, que es justo el error que ya cometí una vez hoy con la guarda del
  `content-type`.
- **Consecuencia:** hay que **medir** cuánto tarda la primera petición a un modelo
  `not-loaded` y decirlo en la interfaz, para que el usuario sepa que la espera es
  normal y no un cuelgue.

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| `/api/v0/models` cambia de forma | El parser devuelve lista vacía → `Malformed` con el detalle, y respaldo a `/v1/models` |
| LM Studio se apaga con Nexo en marcha | `health()` pasa a `Down`; las peticiones dan `Transport` con un mensaje que nombra la dirección y sugiere comprobar que LM Studio está abierto |
| El puerto lo ocupa otro programa | D5 lo rechaza por la forma de la respuesta, no por el código HTTP |
| La primera petición tarda minutos | Se mide y se documenta; el evento queda registrado con su latencia real |
| Un modelo desaparece del catálogo entre el descubrimiento y la petición | `resolve_model` no lo encuentra → `422` nombrando el catálogo, no un `502` del proveedor |

## ¿Hace falta un ADR?

**No.** No se toma ninguna decisión de arquitectura nueva: se usa el eje de
credencial que el ADR 0001 ya estableció, y la vía local no añade riesgos de los que
ese documento trata. La discusión sobre el quinto estado de contabilidad (D4) se
resuelve **sin** cambiar la invariante, así que tampoco toca el ADR.

Lo que sí hay que actualizar al terminar es `docs/producto.md`, donde los modelos
locales figuran como algo por hacer, y `docs/contrato-proveedor.md`, para dejar
constancia de que un tercer adaptador entró sin tocar el núcleo.

## Qué queda pendiente de descubrir

- **Cuánto tarda de verdad la primera petición** a un modelo `not-loaded` de 35B en
  esta máquina. Solo se sabe probando.
- **Si LM Studio informa de `usage`** en su respuesta compatible con OpenAI. Si lo
  hace, los tokens serán `reported`; si no, `unavailable`. No se asume ninguna de las
  dos: se mira.
- **Si el streaming de LM Studio manda el chunk de `usage`** al final, como pide
  `stream_options.include_usage`, o lo ignora.
- **Si `capabilities: ["tool_use"]`** aparece en todos los modelos o solo en algunos,
  y qué otros valores admite ese campo.
