# Contrato de proveedor

Define la frontera entre el núcleo de Nexo y cualquier proveedor de modelos. Todo lo específico de un proveedor vive detrás de este contrato; nada de eso debe filtrarse al router, al catálogo, a las políticas ni a las estadísticas.

Las firmas están en Rust y son orientativas: fijan la forma, no la sintaxis definitiva.

## Los dos ejes

Un adaptador no se identifica solo por el proveedor. Se identifica por **la pareja proveedor + tipo de credencial**, porque la misma cuenta del mismo proveedor ofrece catálogos, capacidades, límites y contabilidad distintos según cómo se haya autenticado.

```rust
pub enum CredentialKind {
    /// API pública y documentada del proveedor. Facturación por token.
    ApiKey,
    /// Flujo OAuth del cliente oficial del proveedor. No soportado.
    /// Sin coste marginal, catálogo recortado, sin métricas de uso.
    SubscriptionOauth,
    /// Runtime en la máquina del usuario. Sin credencial ni coste.
    Local,
}

pub struct AdapterId {
    pub provider: String,        // "openai", "google", "anthropic", "ollama", "mock"
    pub kind: CredentialKind,
}
```

Diseñar el contrato contra el caso fácil (Ollama, o OpenAI por API key) y encajar después el OAuth de suscripción obliga a rehacerlo. El eje va desde la primera línea.

## El trait

```rust
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> AdapterId;

    /// Modelos disponibles con esta credencial concreta. No el catálogo
    /// completo del proveedor: lo que esta credencial puede realmente usar.
    async fn catalog(&self, cred: &Credential) -> Result<Vec<ModelDescriptor>, AdapterError>;

    async fn complete(
        &self,
        req: &ChatRequest,
        cred: &Credential,
    ) -> Result<ChatResponse, AdapterError>;

    async fn stream(
        &self,
        req: &ChatRequest,
        cred: &Credential,
        cancel: CancellationToken,
    ) -> Result<BoxStream<'static, Result<ChatEvent, AdapterError>>, AdapterError>;

    /// Comprobación de salud. Debe ser barata y no consumir cuota facturable
    /// ni de suscripción. Si el proveedor no ofrece un endpoint gratuito,
    /// devolver Health::Unknown en lugar de gastar una petición.
    async fn health(&self, cred: &Credential) -> Health;
}
```

`Credential` nunca contiene el secreto. Contiene una referencia al almacén seguro del sistema y los metadatos necesarios (identificador de cuenta, caducidad, proyecto). La resolución del secreto y la renovación de tokens son responsabilidad del gestor de identidad, no del adaptador.

## Representación interna: superconjunto, no mínimo común denominador

`ChatRequest` modela la unión de lo que ofrecen los proveedores, no la intersección. Cada adaptador rechaza explícitamente lo que no puede hacer.

```rust
pub struct ChatRequest {
    pub model: ModelRef,              // proveedor + credencial + id de API resueltos
    pub messages: Vec<Message>,       // partes de texto, imagen, audio, fichero
    pub tools: Vec<ToolDef>,
    pub tool_choice: ToolChoice,
    pub reasoning: Option<ReasoningConfig>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Vec<String>,
    pub response_format: ResponseFormat,
    pub app_id: AppId,                // siempre presente: identidad y contabilidad
    pub request_id: RequestId,
}
```

Regla, y es la que evita el peor fallo posible del producto:

> **Si el destino no soporta una capacidad solicitada, el adaptador devuelve `AdapterError::Unsupported` y el gateway responde `422` con un mensaje que nombra la capacidad y la alternativa. Nunca se elimina en silencio, ni se sustituye por un equivalente aproximado.**

Degradar en silencio produce respuestas peores sin que el usuario sepa por qué, y es exactamente el fallo que la promesa de «catálogo unificado» invita a cometer.

## Descriptor de modelo

```rust
pub struct ModelDescriptor {
    pub api_id: String,          // como lo llama el proveedor
    pub public_name: String,     // como lo expone Nexo: "openai/gpt-5.5"
    pub caps: Capabilities,      // texto, visión, audio, imagen, herramientas, embeddings, razonamiento
    pub limits: Limits,          // contexto, entrada máxima, salida máxima
    pub accounting: Accounting,
    pub pricing: Option<Pricing>,
}

pub enum Accounting {
    /// El proveedor factura por token e informa del uso.
    Metered,
    /// Cubierto por la suscripción del usuario. Coste marginal cero,
    /// cuota consumida desconocida. NO es lo mismo que gratis.
    Subscription,
    /// Ejecución local. Sin coste ni cuota.
    Local,
}
```

`Capabilities` no es descubrible mediante las APIs de los proveedores. Se obtiene combinando, por este orden de precedencia: anulaciones locales del usuario, el manifiesto versionado que se distribuye con Nexo, y lo que el proveedor anuncie en su endpoint de modelos.

El nombre público lleva **siempre** el proveedor delante. Los alias y perfiles se configuran encima, no se resuelven por adivinación sobre nombres ambiguos.

## Eventos de stream

Vocabulario común al que traduce cada adaptador, independiente de si el origen habla `chat/completions`, Responses, el formato de Anthropic o el de Ollama.

```rust
pub enum ChatEvent {
    Started { provider_request_id: Option<String> },
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallDelta { id: String, args_json: String },
    ToolCallEnd { id: String },
    Usage(UsageReport),
    Finished { reason: FinishReason },
}
```

`Started` marca el punto de medición del tiempo hasta el primer token. `Usage` puede no llegar nunca: en las rutas de suscripción no llega.

## Uso reportado

```rust
pub struct UsageReport {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cached_input_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
    pub source: UsageSource,
    /// El objeto de uso tal como lo devolvió el proveedor, sin transformar.
    pub raw: Option<serde_json::Value>,
}

pub enum UsageSource {
    /// Lo comunicó el proveedor. Es un dato.
    Reported,
    /// Nexo lo calculó. Es una estimación y debe presentarse como tal.
    Estimated,
    /// Ni reportado ni estimable con fiabilidad.
    Unavailable,
}
```

`raw` se conserva siempre. Normalizar sirve para comparar; perder el original impide auditar.

## Taxonomía de errores

Cerrada y traducible a HTTP sin ambigüedad. Cada adaptador mapea los errores de su proveedor a esta lista; el gateway no interpreta cuerpos de error ajenos.

```rust
pub enum AdapterError {
    /// Capacidad solicitada no soportada por esta pareja proveedor+credencial. -> 422
    Unsupported { capability: String, hint: Option<String> },
    /// Credencial ausente, caducada o revocada. Requiere acción del usuario. -> 401
    Auth { reason: AuthFailure, reauth_required: bool },
    /// El proveedor limitó la tasa. -> 429
    RateLimited { retry_after: Option<Duration>, scope: LimitScope },
    /// Límite de Nexo, no del proveedor. Se distingue en las estadísticas. -> 429
    LocalLimit { app_id: AppId, window: Duration },
    /// El proveedor falló. -> 502
    Upstream { status: u16, provider_code: Option<String>, message: String },
    /// Red, TLS, DNS, timeout de conexión. -> 503
    Transport { detail: String },
    /// La respuesta del proveedor no encaja con lo esperado. Señal fuerte de
    /// que un flujo no soportado ha cambiado de forma. -> 502
    Malformed { detail: String },
    /// La ruta de suscripción ya no funciona. El mensaje al usuario debe
    /// explicarlo y ofrecer el respaldo por API key si está configurado. -> 502
    SubscriptionPathBroken { provider: String, detail: String },
    /// Cancelado por el cliente. Se registra, no es un error del proveedor.
    Cancelled,
}
```

`Malformed` y `SubscriptionPathBroken` existen precisamente por el ADR 0001: son la señal de que un flujo no versionado ha cambiado, y deben ser visibles en el diagnóstico en lugar de disolverse en un `502` genérico.

## Reglas para los adaptadores de suscripción

Aplican a todo adaptador con `CredentialKind::SubscriptionOauth`:

1. Los valores frágiles (client_id, issuer, endpoint, cabeceras, parámetros del flujo) viven en **un único módulo** por proveedor, con un comentario que indique la fecha de la última verificación.
2. `catalog()` devuelve solo los modelos realmente accesibles por esta vía, con `Accounting::Subscription` y sin precios.
3. `health()` no consume cuota. Si no hay forma gratuita de comprobarlo, devuelve `Unknown`.
4. Un error de forma inesperada se traduce a `Malformed` o `SubscriptionPathBroken`, nunca a `Upstream` genérico.
5. El adaptador se identifica honestamente ante el proveedor cuando el flujo lo permita.
6. El router debe poder caer al adaptador `ApiKey` del mismo proveedor cuando exista y el usuario lo haya configurado.

## Pruebas de contrato

Toda implementación del trait pasa la misma batería, ejecutada primero contra el proveedor mock y luego contra cada adaptador real:

- Respuesta no streaming: campos completos, `Usage` con el `source` correcto.
- Streaming: orden de eventos, `Started` antes de cualquier delta, `Finished` una sola vez.
- Cancelación a mitad de stream: cierra la conexión de salida y emite `Cancelled`.
- Capacidad no soportada: devuelve `Unsupported`, no una respuesta degradada.
- Credencial caducada: devuelve `Auth` con `reauth_required` correcto.
- Token renovado a mitad de vuelo: una sola renovación concurrente, sin peticiones duplicadas.
- `429` del proveedor: propaga `retry_after` cuando lo haya.
- Cuerpo de respuesta truncado o inesperado: devuelve `Malformed`, no entra en pánico.
- Ida y vuelta de traducción: `chat/completions` → formato nativo → stream nativo → chunks de `chat/completions`, verificando que el texto concatenado y las llamadas a herramientas son idénticos al origen.
