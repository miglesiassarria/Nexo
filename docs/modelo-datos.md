# Modelo de datos

Esquema SQLite de la primera versión. Es un borrador de trabajo: fija las decisiones que son caras de cambiar más adelante, no los detalles de tipos ni de índices.

## Principios

1. **Ningún secreto en texto plano en SQLite.** Ni API keys, ni access tokens, ni refresh tokens, ni secretos OAuth. La clave maestra simétrica de 256 bits vive en el almacén seguro del sistema (Keychain en macOS) y los secretos individuales se cifran en reposo con AES-256-GCM (ADR 0006, spec 0015).
2. **El tipo de credencial es una columna, no un detalle.** Aparece en cuentas, catálogo, permisos, límites, eventos y rollups. Sin ella no se puede comparar el uso por suscripción con el uso por API key, que es una de las preguntas centrales del panel.
3. **Los eventos son inmutables y conservan el original.** Cada petición guarda las métricas normalizadas y, además, el objeto de uso tal como lo devolvió el proveedor.
4. **Las agregaciones son incrementales.** Rollups horarios actualizados al cerrar cada petición. El panel nunca recorre el histórico completo.
5. **Los tokens de aplicación se guardan hasheados.** El secreto en claro solo existe en el momento de la emisión.

## Cuentas y credenciales

```sql
CREATE TABLE accounts (
  id              TEXT PRIMARY KEY,
  provider_id     TEXT NOT NULL,           -- 'openai', 'google', 'anthropic', 'ollama', 'mock'
  credential_kind TEXT NOT NULL,           -- 'api_key' | 'subscription_oauth' | 'local'
  label           TEXT NOT NULL,           -- lo que ve el usuario: "ChatGPT Plus (personal)"
  keychain_ref    TEXT,                    -- clave lógica en el almacén seguro. NUNCA el secreto
  external_id     TEXT,                    -- p.ej. identificador de cuenta del proveedor
  project_id      TEXT,                    -- proyecto de Google Cloud cuando aplique
  scopes          TEXT,                    -- concedidos, separados por espacio
  expires_at      INTEGER,                 -- caducidad del access token, epoch ms
  status          TEXT NOT NULL,           -- 'active' | 'expired' | 'revoked' | 'broken'
  risk_ack_at     INTEGER,                 -- cuándo aceptó el usuario el riesgo de la vía no soportada
  created_at      INTEGER NOT NULL,
  last_used_at    INTEGER,
  UNIQUE (provider_id, credential_kind, external_id)
);
```

`risk_ack_at` es obligatorio y no nulo para toda cuenta con `credential_kind = 'subscription_oauth'`: es el registro de que el usuario recibió la advertencia del ADR 0001 y la aceptó. El gateway rechaza usar una cuenta de suscripción sin ese reconocimiento.

`status = 'broken'` es el estado al que pasa una cuenta de suscripción cuando el adaptador devuelve `SubscriptionPathBroken`. Permite que la interfaz explique lo ocurrido en lugar de mostrar errores repetidos.

## Aplicaciones cliente

```sql
CREATE TABLE apps (
  id           TEXT PRIMARY KEY,
  name         TEXT NOT NULL,
  token_hash   TEXT NOT NULL UNIQUE,       -- hash del bearer token emitido
  token_prefix TEXT NOT NULL,              -- primeros caracteres, para identificarlo en la interfaz
  created_at   INTEGER NOT NULL,
  last_seen_at INTEGER,
  revoked_at   INTEGER,
  notes        TEXT
);
```

Un token identifica una aplicación. Esa es la única forma de saber quién llama, porque la mayoría de herramientas cliente solo permiten configurar una URL base y una clave.

## Permisos

```sql
CREATE TABLE app_grants (
  app_id           TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
  provider_id      TEXT NOT NULL,
  credential_kind  TEXT NOT NULL,
  model_pattern    TEXT NOT NULL DEFAULT '*',
  allow_tools      INTEGER NOT NULL DEFAULT 0,
  allow_multimodal INTEGER NOT NULL DEFAULT 0,
  log_content      INTEGER NOT NULL DEFAULT 0,   -- registrar prompts y respuestas: opt-in
  PRIMARY KEY (app_id, provider_id, credential_kind, model_pattern)
);
```

Sin fila no hay permiso. El acceso se concede, no se deniega.

**Una fila es un modelo concedido, no una vía concedida.** Marcar tres modelos de un
proveedor son tres filas, y por eso `model_pattern` está en la clave primaria. Un
conjunto vacío de modelos no es un estado: son cero filas, y eso es exactamente lo
mismo que no haber concedido nada. Por consiguiente, marcar el primer modelo concede
la vía y desmarcar el último la retira.

`model_pattern` admite tres formas, y solo la primera la escribe hoy la interfaz:

| Valor | Significado |
| --- | --- |
| `openai/gpt-5.5` | Ese modelo exacto. Es lo que escribe la interfaz al marcar |
| `*` | Todos los modelos de esa vía, **incluidos los que el proveedor añada después**. Es lo que había antes de que se pudieran elegir modelos, y se conserva para no cambiar el comportamiento de las aplicaciones existentes |
| `openai/*` | Prefijo. El almacenamiento lo respeta, pero la interfaz no ofrece escribirlos |

`*` y «marcar los sesenta modelos uno a uno» **no son equivalentes**: ante un modelo
nuevo del proveedor, el primero lo sirve y el segundo no.

Quién decide si una aplicación puede usar un modelo es una única función,
`policy::grant_for`. Estuvo escrita dos veces —una en el control de la petición y otra
en el catálogo que responde `GET /v1/models`— y la del catálogo se quedó sin comparar
el patrón: con un permiso estrecho anunciaba modelos que el gateway después rechazaba.
Cualquier camino que decida sobre modelos permitidos pasa por esa función.

Las filas de una misma vía llevan `allow_tools`, `allow_multimodal` y `log_content`
repetidos e iguales: esas capacidades son de la vía, no del modelo. Es una
desnormalización conocida; llevarlas a su propia tabla exige una migración y no
resolvía nada de lo pedido.

Un modelo marcado que desaparece del catálogo **conserva su fila**. Son intención
declarada del usuario, y un proveedor que falle un minuto o devuelva un catálogo
incompleto no debe borrar permisos para siempre. La interfaz las muestra señaladas, y
la consulta de catálogo vacía las distingue con el motivo `no_models_match`.

## Límites

```sql
CREATE TABLE app_limits (
  app_id           TEXT NOT NULL REFERENCES apps(id) ON DELETE CASCADE,
  provider_id      TEXT NOT NULL,
  credential_kind  TEXT NOT NULL,
  window_seconds   INTEGER NOT NULL,
  max_requests     INTEGER,
  max_input_tokens INTEGER,
  max_output_tokens INTEGER,
  PRIMARY KEY (app_id, provider_id, credential_kind, window_seconds)
);
```

**Invariante que la aplicación debe garantizar:** toda pareja `(app_id, provider_id)` con un grant sobre `credential_kind = 'subscription_oauth'` tiene al menos una fila de límites con `max_requests` no nulo. Es el requisito de mitigación del riesgo de multiplexación del ADR 0001, y debe estar cubierto por una prueba, no solo por la interfaz.

El consumo dentro de la ventana se mantiene en memoria y se reconstruye desde `requests` al arrancar.

## Catálogo

```sql
CREATE TABLE models (
  provider_id     TEXT NOT NULL,
  credential_kind TEXT NOT NULL,
  api_id          TEXT NOT NULL,           -- como lo llama el proveedor
  public_name     TEXT NOT NULL,           -- "openai/gpt-5.5"
  caps            TEXT NOT NULL,           -- JSON de capacidades
  context_max     INTEGER,
  input_max       INTEGER,
  output_max      INTEGER,
  accounting      TEXT NOT NULL,           -- 'metered' | 'subscription' | 'local'
  price_input     INTEGER,                 -- micros por millón de tokens; NULL si no aplica
  price_output    INTEGER,
  price_cached_input INTEGER,
  price_source    TEXT,                    -- 'manifest' | 'user' | NULL
  manifest_version TEXT,
  available       INTEGER NOT NULL DEFAULT 1,
  updated_at      INTEGER NOT NULL,
  PRIMARY KEY (provider_id, credential_kind, api_id)
);
```

La clave primaria compuesta es la consecuencia práctica del eje de credencial: `gpt-5.5` por API key y `gpt-5.5` por suscripción son dos filas con capacidades, límites y contabilidad distintos. Aplanar esto obliga a rehacer el catálogo y el panel.

## Eventos

```sql
CREATE TABLE requests (
  id                 TEXT PRIMARY KEY,
  ts                 INTEGER NOT NULL,     -- inicio, epoch ms
  app_id             TEXT NOT NULL,
  provider_id        TEXT NOT NULL,
  credential_kind    TEXT NOT NULL,
  account_id         TEXT,
  public_model       TEXT NOT NULL,
  api_model          TEXT NOT NULL,
  operation          TEXT NOT NULL,        -- 'chat' | 'embedding' | 'image' | 'audio'
  streamed           INTEGER NOT NULL,
  status             TEXT NOT NULL,        -- 'ok' | 'error' | 'cancelled'
  error_kind         TEXT,                 -- variante de AdapterError
  http_status        INTEGER,
  latency_ms         INTEGER,
  ttft_ms            INTEGER,
  input_tokens       INTEGER,
  output_tokens      INTEGER,
  cached_input_tokens INTEGER,
  reasoning_tokens   INTEGER,
  total_tokens       INTEGER,
  usage_source       TEXT NOT NULL,        -- 'reported' | 'estimated' | 'unavailable'
  cost_micros        INTEGER,
  cost_basis         TEXT NOT NULL,        -- 'reported' | 'estimated' | 'subscription' | 'unavailable'
  fallback_from      TEXT,                 -- credential_kind del que se cayó, si hubo respaldo
  provider_usage_raw TEXT,                 -- objeto de uso original, sin transformar
  provider_request_id TEXT
);

CREATE INDEX idx_requests_ts ON requests(ts);
CREATE INDEX idx_requests_app_ts ON requests(app_id, ts);
CREATE INDEX idx_requests_model_ts ON requests(provider_id, credential_kind, public_model, ts);
```

Sobre las dos columnas que resuelven el problema de honestidad del panel:

- `usage_source` responde «¿de dónde salen estos tokens?».
- `cost_basis` responde «¿qué significa esta cifra de coste?». El valor `'subscription'` significa coste marginal cero **y cuota consumida desconocida**. La interfaz no puede mostrar ese caso igual que un coste reportado de cero, porque no es lo mismo.

`error_kind = 'LocalLimit'` distingue un rechazo de Nexo de un `429` del proveedor. Confundirlos haría inútil el diagnóstico de límites.

El contenido de prompts y respuestas **no** está en esta tabla. Si el usuario activa `log_content` para una aplicación, va a una tabla aparte con su propia política de retención y borrado, para que eliminarlo no implique perder las métricas.

## Rollups

```sql
CREATE TABLE usage_hourly (
  hour            INTEGER NOT NULL,        -- epoch ms truncado a hora
  app_id          TEXT NOT NULL,
  provider_id     TEXT NOT NULL,
  credential_kind TEXT NOT NULL,
  public_model    TEXT NOT NULL,
  operation       TEXT NOT NULL,
  requests        INTEGER NOT NULL DEFAULT 0,
  errors          INTEGER NOT NULL DEFAULT 0,
  cancels         INTEGER NOT NULL DEFAULT 0,
  rate_limited    INTEGER NOT NULL DEFAULT 0,
  input_tokens    INTEGER NOT NULL DEFAULT 0,
  output_tokens   INTEGER NOT NULL DEFAULT 0,
  total_tokens    INTEGER NOT NULL DEFAULT 0,
  cost_micros     INTEGER NOT NULL DEFAULT 0,
  cost_reported_micros  INTEGER NOT NULL DEFAULT 0,
  cost_estimated_micros INTEGER NOT NULL DEFAULT 0,
  subscription_requests INTEGER NOT NULL DEFAULT 0,
  latency_sum_ms  INTEGER NOT NULL DEFAULT 0,
  latency_max_ms  INTEGER NOT NULL DEFAULT 0,
  ttft_sum_ms     INTEGER NOT NULL DEFAULT 0,
  ttft_count      INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (hour, app_id, provider_id, credential_kind, public_model, operation)
);
```

El coste se acumula separado por base (`reported` frente a `estimated`) para que el panel nunca sume una estimación con un dato y presente el total como si fuera un dato. Las peticiones cubiertas por suscripción se cuentan aparte en `subscription_requests` y no aportan coste.

Las medias y los percentiles se derivan de las sumas y los contadores. Un percentil exacto requeriría histogramas; para la primera versión bastan media y máximo, y los percentiles se calculan sobre `requests` cuando el periodo consultado es corto.

## Configuración y retención

```sql
CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);
```

Solo configuración no sensible. Claves iniciales previstas: puerto de escucha, exposición en LAN (por defecto desactivada), días de retención de `requests`, días de retención de contenido, nivel de log y versión del manifiesto de modelos.

La retención se aplica sobre `requests` y sobre la tabla de contenido, nunca sobre `usage_hourly`: el histórico agregado sobrevive al borrado del detalle, que es lo que permite conservar tendencias largas con poco espacio.

## Proveedores añadidos por el usuario

Migración v2 (spec 0002). Nombre, dirección y —en el Keychain, nunca aquí— la clave
de cualquier servicio que hable el formato de OpenAI: OpenCode Zen, OpenRouter, un
proxy propio.

```sql
CREATE TABLE custom_providers (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  base_url   TEXT NOT NULL,
  -- Formato de cable. Hoy solo 'openai_compat'; el de Anthropic queda aplazado.
  compat     TEXT NOT NULL DEFAULT 'openai_compat',
  created_at INTEGER NOT NULL
);
```

`id` es el slug derivado del nombre que le da el usuario y es la clave primaria: dos
proveedores que produzcan el mismo slug no pueden coexistir, sin comprobación
aparte. No se deducen de `accounts` —donde `provider_id` es un `TEXT` libre— porque
hay que distinguir un proveedor añadido a propósito de un `provider_id` desconocido
por corrupción, y porque el nombre legible debe sobrevivir a desconectar la cuenta
sin borrar el proveedor.

La dirección real que usa el adaptador en cada petición es la de `accounts.external_id`,
no la de esta tabla: es lo que permite cambiar la URL sin reiniciar Nexo (el mismo
mecanismo que usa LM Studio). Esta tabla es la fuente para la interfaz y para volver
a crear la cuenta si hiciera falta.

Las capacidades y el precio de estos proveedores no viven en SQLite: se cruzan en
memoria contra una caché en disco de `models.dev` (ver
`crates/nexo-core/src/catalog/models_dev.rs`), refrescada semanalmente. Son 3,3 MB de
miles de modelos que no interesa duplicar en la base de datos para leerlos una vez
por sincronización.

## Almacén de secretos cifrados

Migración v4 (spec 0015, ADR 0006). Guarda las API keys, tokens de refresh,
tokens de acceso y tokens recuperables de aplicación cifrados en reposo mediante
AES-256-GCM.

```sql
CREATE TABLE encrypted_secrets (
  key         TEXT PRIMARY KEY,
  nonce       BLOB NOT NULL,
  ciphertext  BLOB NOT NULL,
  updated_at  INTEGER NOT NULL
);
```

La clave maestra necesaria para descifrar esta tabla reside exclusivamente en el
Llavero del sistema operativo (`com.nexo.gateway / master_key`). Un volcado de
`nexo.sqlite` no contiene texto plano ni permite recuperar credenciales.

## Migraciones

Versionadas y aplicadas al arrancar, con la versión en `PRAGMA user_version`. Solo hacia adelante: no hay `down`. Antes de aplicar una migración, copia de seguridad del fichero de base de datos con el número de versión de origen en el nombre.
