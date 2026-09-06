# 0018 · Diseño

Responde al **cómo**. Lee el código antes de escribirlo: de memoria salen diseños
que no encajan.

## Enfoque

Se añadirá un adaptador integrado `GeminiSubscriptionAdapter` para la pareja
`gemini_subscription + subscription_oauth`. El flujo usará el OAuth de Google
del cliente Antigravity para cuentas personales y el backend Cloud Code Assist
(`cloudcode-pa.googleapis.com`/`daily-cloudcode-pa.googleapis.com`, `v1internal`),
no la API pública de Gemini. Tras el login se ejecutará `loadCodeAssist` y, si
hace falta, `onboardUser`; el proyecto devuelto se guardará como metadato de la cuenta.
Una cuenta personal no tendrá que introducir un proyecto: solo se usará uno si
Google lo devuelve o si el usuario conecta explícitamente una cuenta empresarial.
Como ese backend ha servido variantes de metadata y nombres de campo, el
bootstrap prueba de forma acotada los perfiles Antigravity documentados y la
forma protobuf-compatible observada en OmniRoute. El catálogo se descubrirá con la superficie
de modelos de Code Assist disponible para el token. El gateway seguirá exponiendo
`chat/completions`: el adaptador convertirá la petición interna al sobre nativo
`{model, project, user_prompt_id, request}` y traducirá tanto las respuestas
unarias como los eventos SSE al vocabulario común de Nexo.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/auth/mod.rs` | Registrar el módulo OAuth de Google. |
| `crates/nexo-core/src/auth/gemini_subscription.rs` | Valores frágiles, PKCE, URLs OAuth, canje/refresh y parsing de identidad. |
| `crates/nexo-core/src/auth/callback.rs` | Permitir callback loopback en un puerto dinámico, conservando el helper actual de ChatGPT. |
| `crates/nexo-core/src/provider/mod.rs` | Añadir metadatos no sensibles a la credencial resuelta y registrar el adaptador. |
| `crates/nexo-core/src/provider/gemini_subscription.rs` | Nuevo adaptador: bootstrap, catálogo, request nativo, SSE, errores y uso. |
| `crates/nexo-core/src/provider/gemini_subscription.rs` | Traducir el superconjunto interno a GenerateContent/Code Assist y sus respuestas dentro del adaptador de la ruta. |
| `crates/nexo-core/src/db/migrations.rs` | Migración aditiva para metadatos JSON no secretos de cuenta. |
| `crates/nexo-core/src/db/mod.rs` | Leer/escribir metadatos de cuenta sin guardar tokens. |
| `crates/nexo-core/src/service.rs` | Registrar proveedor, conectar Google, renovar por proveedor, resolver metadatos y fallback equivalente. |
| `src-tauri/src/commands.rs`, `src-tauri/src/main.rs` | Comando de conexión específico y aviso de riesgo de Gemini. |
| `src/lib/api.ts`, `src/lib/views/Providers.svelte` | Invocar el proveedor seleccionado sin hardcodear ChatGPT y mostrar su aviso. |
| `crates/nexo-core/tests/gateway_e2e.rs` | Pruebas simuladas y E2E ignoradas contra una cuenta real. |
| `docs/modelo-datos.md`, `docs/contrato-proveedor.md`, `ROADMAP.md` | Documentar metadatos y capacidad validada. |

## Decisiones

Una entrada por decisión. Sin la alternativa descartada, no es una decisión: es
una preferencia sin justificar.

### D1. Proveedor técnico separado de Gemini API key

- **Decisión:** `provider_id = "gemini_subscription"`, nombre visible «Gemini por suscripción» y modelos públicos con prefijo `gemini_subscription/`.
- **Alternativa descartada:** reutilizar `provider_id = "gemini"` distinguiendo solo por `CredentialKind`, porque el backend, proyecto y catálogo no son los mismos que en la API key.
- **Consecuencia que hay que asumir:** el usuario verá dos entradas de Gemini; es intencionado y evita permisos ambiguos.

### D2. OAuth del cliente Antigravity y valores públicos sin literales

- **Decisión:** usar el client id/secret público del cliente Antigravity, Authorization Code con PKCE, scopes de Cloud Platform y los scopes adicionales que exige el cliente actual, con callback loopback dinámico en `127.0.0.1`. Los valores por defecto se conservan como bytes enmascarados y se reconstruyen solo en ejecución; una pareja completa de variables `NEXO_ANTIGRAVITY_OAUTH_CLIENT_ID` y `NEXO_ANTIGRAVITY_OAUTH_CLIENT_SECRET` permite sustituirlos.
- **Alternativa descartada:** importar los ficheros de credenciales de Gemini CLI o leer cookies del navegador, porque rompe las invariantes de Nexo.
- **Consecuencia que hay que asumir:** el enmascarado no aporta confidencialidad; evita que los escáneres confundan un identificador de cliente público con un token de usuario. Client id, callback y endpoints pueden cambiar; todos quedan aislados en `auth/gemini_subscription.rs`.

### D3. Metadatos de cuenta en columna JSON aditiva

- **Decisión:** añadir `provider_metadata` a `accounts`; contendrá solo datos no secretos como `project_id`, email/subject y tier. Los tokens seguirán en `SecretStore`.
- **Alternativa descartada:** sobrecargar `external_id` o `scopes`, porque ya tienen semántica estable y no representan a la vez identidad y proyecto.
- **Consecuencia que hay que asumir:** hace falta migración v5 y compatibilidad con cuentas antiguas sin metadatos.

### D4. Bootstrap opcional antes del catálogo y de la inferencia

- **Decisión:** resolver `loadCodeAssist`; si no devuelve proyecto, ejecutar onboarding acotado y consultar la operación si es de larga duración. Si la cuenta sigue sin proyecto, se conserva el OAuth como conexión degradada para permitir reintentos, pero no se activa ni ofrece modelos hasta que el backend entregue un contexto válido. Nexo no crea ni activa facturación.
- **Alternativa descartada:** mandar un proyecto fijo de Nexo o el project number del client id, porque un proyecto fantasma puede producir 403 aunque el tier sea válido.
- **Consecuencia que hay que asumir:** una cuenta personal puede quedar temporalmente degradada si Google aún no ha activado su contexto; una cuenta empresarial/BYOP puede requerir un proyecto administrado por el usuario.

### D5. Protocolo nativo de Code Assist

- **Decisión:** construir el sobre `{model, project, user_prompt_id, request}` con `contents`, `systemInstruction`, `tools`, `toolConfig` y `generationConfig`; usar `generateContent` y `streamGenerateContent?alt=sse`.
- **Alternativa descartada:** enviar `chat/completions` a `generativelanguage.googleapis.com`, porque esa ruta usa API key y no consume la suscripción.
- **Consecuencia que hay que asumir:** habrá un traductor nuevo y el streaming será sensible a cambios de formato.

### D6. Perfil de transporte compatible

- **Decisión:** usar el perfil HTTP público y mínimo que el backend Antigravity exige para OAuth/bootstrap/runtime, sin importar credenciales, cookies, prompts ni perfiles locales. Nexo seguirá siendo el intermediario visible ante sus aplicaciones; la cabecera técnica de compatibilidad no se extiende a la identidad ni al prompt del cliente.
- **Alternativa descartada:** conservar el perfil de Gemini CLI anterior, porque Google ya no sirve las suscripciones personales por esa vía.
- **Consecuencia que hay que asumir:** cambios del cliente Antigravity pueden romper la conexión y deben detectarse con errores explícitos.

### D7. Fallback solo por modelo equivalente

- **Decisión:** buscar una cuenta Gemini API key y conservar el mismo `api_id` solo si el modelo también existe en esa vía; errores de capacidad o autorización no disparan fallback silencioso.
- **Alternativa descartada:** reenviar cualquier modelo a Gemini API key, porque puede cambiar coste, capacidades o cuota sin que la aplicación lo sepa.
- **Consecuencia que hay que asumir:** algunas familias descubiertas solo por suscripción no tendrán respaldo.

## Qué puede romperse

Y, sobre todo, **cómo nos enteraremos**. Un fallo silencioso es peor que uno ruidoso.

| Riesgo | Cómo se detecta |
| --- | --- |
| OAuth o refresh devuelve una forma distinta | Tests de fixtures, errores tipados y cuenta marcada como `expired`. |
| `loadCodeAssist` no devuelve proyecto o exige BYOP | Estado `degraded` y mensaje que indica configurar un proyecto; no se guarda una cuenta activa incompleta. |
| El catálogo cambia o contiene modelos no conversacionales | Parser que filtra entradas incompletas y diagnóstico cuando queda vacío. |
| Code Assist cambia el sobre SSE o la estructura de uso | Test de contrato local y E2E ignorada; el adaptador devuelve `Malformed`. |
| La metadata o el perfil de transporte produce 403 | Prueba real y clasificación de `PermissionDenied`; se usan únicamente valores públicos y necesarios del cliente Antigravity. |
| Dos peticiones renuevan a la vez | Lock existente por cuenta y prueba concurrente con refresh simulado. |
| Gemini API key y suscripción se mezclan | Tests de ids, catálogo, permisos, desconexión y resolución del adaptador. |

## ¿Hace falta un ADR?

No hace falta un ADR nuevo: el ADR 0001 ya acepta OAuth de suscripción no soportado
cuando el usuario lo inicia explícitamente, exige advertencia, límites y fallback,
y prohíbe importar credenciales o perfiles locales. Esta spec aplica esas reglas
a Google y añade una migración aditiva para un dato de proveedor que el esquema
actual no podía representar.

## Qué queda pendiente de descubrir

- Qué cuenta exactamente con una suscripción Google AI Pro/Ultra puede completar
  `loadCodeAssist` y `onboardUser` mediante Antigravity sin proyecto propio.
- Qué variante de catálogo es accesible para el OAuth actual y qué campos publica
  para filtrar modelos de chat.
- Qué metadata mínima acepta Google desde una aplicación que se identifica como
  Nexo y si el endpoint permite inferencia con esa identidad.
- Qué forma exacta de `usageMetadata`, function calls y thinking devuelve el
  backend en respuestas reales.
