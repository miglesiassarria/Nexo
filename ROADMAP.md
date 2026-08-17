# Roadmap de Nexo

Alcance, fases y criterios de aceptación. La visión del producto está en
[docs/producto.md](docs/producto.md) y las decisiones tomadas en [docs/adr/](docs/adr/).

## Alcance de la primera versión

Un único usuario y una única máquina. El criterio de éxito es que el autor pueda
dejar de meter API keys en sus propias herramientas.

- Aplicación de escritorio multiplataforma, desarrollada y validada en macOS.
- Gateway en localhost que sigue sirviendo con la ventana cerrada.
- Icono permanente en la barra de estado.
- API compatible con OpenAI en formato `chat/completions`, con y sin streaming.
- Adaptador de OpenAI con las dos vías: OAuth de suscripción como objetivo
  principal y API key como respaldo.
- Traducción entre `chat/completions` y el formato Responses, en petición y en stream.
- Proveedor mock para validar el gateway sin credenciales ni coste.
- Catálogo indexado por proveedor y tipo de credencial.
- Tokens independientes y revocables por aplicación, con límites obligatorios en
  la vía de suscripción.
- Registro local de métricas con los cuatro estados de contabilidad, sin guardar
  por defecto el contenido de los mensajes.
- Panel de estadísticas por aplicación, proveedor, credencial, modelo y periodo.

Quedaron fuera del alcance inicial Google Gemini, Anthropic, MLX, llama.cpp,
las capacidades multimodales y la superficie compatible con el formato de
Anthropic. Gemini por API key se incorporó posteriormente en la fase 4; el resto
continúa fuera de la primera versión.

Fuera del proyecto: multiusuario, sincronización entre equipos, acceso remoto
fuera de la red local y despliegue empresarial. Conectar otros ordenadores de
la propia red local del usuario sí está dentro de alcance, ver
[ADR 0003](docs/adr/0003-acceso-desde-la-red-local.md).

## Fases

### Fase 0 — Validar lo que puede matar el proyecto

Dos spikes de código, no documentos.

- [x] **Spike de Tauri.** Esqueleto en macOS con icono de barra de estado, cierre
      de ventana que oculta en lugar de terminar, y Axum escuchando en
      `127.0.0.1` en el mismo proceso. Mediciones anotadas en el
      [ADR 0002](docs/adr/0002-stack-tauri-rust-svelte.md).
- [x] **Spike de OAuth de suscripción.** Ejercido contra una cuenta real el
      2026-07-31: el flujo funciona con `originator=nexo`, los tres modelos del
      catálogo responden, y el proveedor informa de tokens aunque no de cuota.
      Resultados y correcciones en el
      [ADR 0001](docs/adr/0001-oauth-de-suscripcion.md#validación-contra-una-cuenta-real).

### Fase 1 — Gateway mínimo de extremo a extremo ✅

- [x] Contrato de proveedor sobre los dos ejes de proveedor y credencial.
- [x] `GET /v1/models` y `POST /v1/chat/completions` con y sin streaming.
- [x] Proveedor mock con streaming y recuento de tokens, sin credenciales.
- [x] Traductor de formatos y pruebas de contrato del stream.
- [x] Tokens de aplicación y registro de eventos en SQLite.
- [x] Validación por HTTP real en `crates/nexo-core/tests/gateway_e2e.rs`.

### Fase 2 — OpenAI en producción

- [x] Adaptador de OpenAI por API key.
- [x] Adaptador de OpenAI por OAuth de suscripción, con los valores frágiles
      aislados en un único módulo.
- [x] Tokens en el almacén seguro del sistema, con renovación y desconexión.
- [x] Aviso explícito y confirmación del usuario antes del primer login de
      suscripción.
- [x] Límites por aplicación aplicados, visibles y obligatorios en esa vía.
- [x] Respaldo automático a API key cuando la ruta de suscripción falla, con
      error comprensible cuando no hay respaldo configurado.
- [x] Catálogo diferenciado por credencial.
- [x] Validación con una cuenta real.
- [ ] Revocación desde Nexo del consentimiento OAuth en el proveedor.

### Fase 3 — Control y experiencia de usuario

- [x] Interfaz de configuración, proveedores, aplicaciones y modelos.
- [x] Panel de estadísticas con comparativas y últimas peticiones.
- [x] Gestión de retención y borrado de estadísticas, aplicada sola en cada
      arranque además de a demanda desde el botón.
- [ ] Aprobación interactiva de conexiones nuevas.
- [ ] Perfiles y reglas de enrutado.
- [x] Consultas de catálogo registradas con el motivo cuando salen vacías.
- [ ] Health checks por proveedor y vista de diagnóstico.
- [ ] Exportación de estadísticas en formatos abiertos.
- [ ] Registro opcional de contenido por aplicación, con su propia retención.
- [ ] Gráficas de tendencia en el panel.
- [x] Acceso desde la red local, desactivado por defecto, con el token de
      aplicación ya existente como autenticación. Sin cifrado desde el
      [ADR 0005](docs/adr/0005-red-local-sin-cifrado.md): el certificado
      autofirmado que había iba atado a la IP de la máquina, y en un portátil
      eso obligaba a volver a aceptarlo en cada cliente en cada cambio de red
      ([ADR 0003](docs/adr/0003-acceso-desde-la-red-local.md),
      [spec 0007](specs/0007-acceso-red-local/spec.md)).

### Fase 4 — Más proveedores

- [x] LM Studio, con detección automática y catálogo descubierto
      ([spec 0001](specs/0001-proveedor-local-lm-studio/spec.md)).
- [ ] Ollama, como proveedor propio y no como variante de LM Studio.
- [ ] `/v1/embeddings`: los modelos ya se listan, pero la superficie no existe.
- [x] Google Gemini con API key mediante su capa OpenAI-compatible, verificado
      contra la API real ([spec 0008](specs/0008-atajo-de-proveedor-gemini-api-key/spec.md)).
- [ ] Google Gemini con OAuth de API. No se confunde con una suscripción de la
      aplicación Gemini: autoriza un proyecto de Google Cloud.
- [ ] Anthropic por API key y, tras su propia investigación, por OAuth de
      suscripción. El flujo de Anthropic no se deriva del de OpenAI.
- [ ] Superficie compatible con el formato nativo de Anthropic, necesaria para
      herramientas que no hablan `chat/completions`.
- [ ] Capacidades multimodales de extremo a extremo.
- [x] Catálogo descubierto del proveedor en la vía de suscripción, con el
      manifiesto local como respaldo.
- [x] Proveedores OpenAI-compatible añadidos por el usuario, con varios simultáneos
      y catálogo cruzado con `models.dev`
      ([spec 0002](specs/0002-proveedores-genericos-y-opencode-zen/spec.md)).
- [x] OpenCode Zen como atajo del tipo anterior, verificado con 60 modelos reales.
- [ ] Proveedores Anthropic-compatible añadidos por el usuario. Aplazado a
      petición del usuario: era la única parte no verificable de la spec 0002
      (ningún modelo gratuito de Zen habla ese formato). Retomar cuando haya saldo
      en Zen o una clave directa de Anthropic para probar contra la realidad.
- [ ] Lo mismo para la vía de API key de OpenAI, cuyo endpoint no publica capacidades.

### Fase 5 — Distribución

Solo cuando Nexo funcione para su autor. La decisión de publicar es
independiente de la de construir: distribuir una herramienta que usa flujos no
soportados tiene implicaciones que se valorarán entonces.

- [ ] Firma y notarización de macOS.
- [ ] Instaladores de Windows y Linux, probados de verdad.
- [ ] Integración permanente con el system tray de Windows y los indicadores de Linux.
- [ ] Actualizaciones automáticas.
- [ ] Documentación de integración con clientes populares.

## Criterios de aceptación del producto

Nexo podrá considerarse útil cuando un usuario pueda:

| | Criterio | Estado |
| --- | --- | --- |
| 1 | Instalarlo y ejecutarlo localmente sin desplegar un servidor externo | ✅ |
| 2 | Autorizar su suscripción de ChatGPT con un único login y usarla después desde cualquier aplicación sin API key | ✅ validado el 2026-07-31 |
| 3 | Conectar una aplicación compatible con OpenAI usando una URL local y un token de Nexo | ✅ |
| 4 | Elegir un modelo de varios proveedores sin cambiar la integración del cliente | ✅ OpenAI y LM Studio conviven |
| 5 | Ver qué modelos están disponibles por suscripción y qué modelos exigen API key, sin sorpresas en ejecución | ✅ con el catálogo real del proveedor |
| 6 | Revocar el acceso de una aplicación sin invalidar las demás | ✅ |
| 7 | Saber qué proveedor, credencial y modelo atendió cada petición y cuánto tardó | ✅ |
| 8 | Fijar un límite de uso por aplicación y ver el consumo acumulado | ⏳ límite aplicado; falta mostrar el consumo restante en el panel |
| 17 | Entender desde el panel por qué un cliente no ve modelos | ✅ |
| 9 | Desconectar una cuenta y eliminar sus tokens del equipo | ✅ |
| 10 | Usar modelos locales cuando no quiera enviar datos a un proveedor cloud | ✅ con LM Studio |
| 11 | Consultar el uso por aplicación, proveedor, credencial, modelo y periodo | ✅ |
| 12 | Comparar consumo, latencia y errores sin confundir un dato reportado, una estimación, un consumo cubierto por suscripción y un dato no disponible | ✅ |
| 13 | Recibir un error comprensible, y el respaldo por API key si lo configuró, cuando la vía de suscripción deje de funcionar | ✅ |
| 14 | Cerrar la ventana principal sin detener el gateway | ✅ |
| 15 | Consultar y controlar Nexo desde un icono siempre disponible en la barra de estado | ✅ |
| 16 | Operar con una configuración segura por defecto: solo localhost salvo activación explícita del acceso en red local, sin límites en blanco, sin registro de contenido | ✅ |
| 18 | Conectar otros ordenadores de la propia red local, con autenticación y transporte cifrado obligatorios | ✅ [spec 0007](specs/0007-acceso-red-local/spec.md), verificado contra el binario real instalado |

Los objetivos numéricos de memoria, CPU y arranque están en el
[ADR 0002](docs/adr/0002-stack-tauri-rust-svelte.md).

## Deuda conocida

- El puerto solo cambia al reiniciar la aplicación.
- Los percentiles de latencia se calculan sobre el detalle, no sobre histogramas:
  en periodos largos solo hay media y máximo.
- La cancelación por parte del cliente cierra el stream pero no se propaga como
  cancelación explícita al proveedor.
- El manifiesto de modelos se distribuye con la aplicación y no tiene ruta de
  actualización propia.
