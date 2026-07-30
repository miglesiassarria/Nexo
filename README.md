# Nexo

## Descripción

Nexo es un proyecto para crear un punto común de acceso a modelos de inteligencia artificial. Su propósito es que una persona pueda conectar sus aplicaciones, asistentes, herramientas de desarrollo y automatizaciones a un único gateway local, en lugar de configurar cada proveedor y cada credencial por separado en cada aplicación.

La idea toma como referencia conceptual a [Msty Nexus](https://msty.ai/products/nexus/), pero Nexo debe ser un proyecto independiente, más abierto y orientado a resolver algunas limitaciones de ese tipo de herramientas. El objetivo no es crear otro chat, sino una capa de infraestructura personal que controle cómo las aplicaciones acceden a diferentes modelos y que, al mismo tiempo, funcione como centro de información sobre su utilización.

Nexo debe ser un hub completo en dos dimensiones inseparables:

- **Hub funcional:** conecta aplicaciones, modelos, proveedores, credenciales y políticas mediante una interfaz común.
- **Hub de información:** explica qué se está usando, desde dónde, con qué frecuencia, con qué rendimiento y, cuando sea posible, con qué consumo o coste.

El repositorio contiene la definición del producto y la dirección técnica. Todavía no incluye la implementación: su función es servir como documento de partida para entender el problema, tomar decisiones de arquitectura y comenzar a construir. Las decisiones ya tomadas y sus consecuencias están en [`docs/`](docs/).

## Problema que resuelve

Actualmente, cada aplicación suele pedir sus propias credenciales y configuración:

- Una API key de OpenAI.
- Otra API key de Google o Gemini.
- Configuración independiente para modelos locales.
- Diferentes nombres y formatos para realizar una misma operación.
- Ausencia de una visión común sobre uso, permisos, errores y disponibilidad.

Esto provoca duplicación de configuración, secretos repartidos por varios sitios, dificultad para cambiar de proveedor y poca visibilidad sobre qué aplicación está utilizando cada modelo.

Y sobre todo provoca un gasto innecesario: el usuario que ya paga una suscripción mensual de ChatGPT vuelve a pagar por token cada vez que una aplicación de terceros consume la API con una key.

Nexo debe centralizar esa complejidad en un único lugar y ofrecer a las aplicaciones cliente una interfaz estable y sencilla.

## Visión del producto

Nexo será un gateway local que se ejecutará en el equipo del usuario y actuará como intermediario entre las aplicaciones y los proveedores de IA.

```text
Aplicaciones y herramientas del usuario
                  |
                  v
          Nexo: gateway común
          /       |        \
         v        v         v
      OpenAI    Google    Modelos locales
                Gemini    Ollama / MLX / llama.cpp
```

Las aplicaciones no deberían necesitar conocer los detalles internos de cada proveedor. Nexo se encargará de:

- Mantener un catálogo unificado de modelos.
- Traducir formatos de petición y respuesta.
- Gestionar la autenticación con cada proveedor, tanto con API key como con OAuth de suscripción.
- Aplicar permisos, límites y políticas.
- Registrar, conservar y visualizar estadísticas locales de uso y rendimiento.
- Permitir cambiar de proveedor sin modificar todas las aplicaciones conectadas.

## Aplicación multiplataforma y presencia permanente

Nexo debe diseñarse desde el principio como una aplicación de escritorio multiplataforma compatible con macOS, Windows y Linux. El desarrollo y las pruebas comenzarán en macOS, que será la primera plataforma soportada, pero las decisiones de arquitectura no deben cerrar el camino a las demás.

Mientras Nexo esté ejecutándose, su icono debe permanecer disponible en el área de estado del sistema:

- En macOS, en la barra de menús o barra de estado.
- En Windows, en la bandeja del sistema.
- En Linux, mediante el mecanismo de bandeja o indicador compatible con el entorno de escritorio.

La aplicación debe poder seguir funcionando en segundo plano aunque la ventana principal esté cerrada. Desde el icono se debe poder consultar de un vistazo el estado del gateway y acceder a acciones rápidas:

- Saber si Nexo está activo y aceptando conexiones.
- Ver si existe tráfico o actividad reciente.
- Consultar el estado general de los proveedores.
- Abrir el panel principal y sus estadísticas.
- Pausar o reanudar el gateway.
- Acceder a configuración, diagnóstico y cierre de la aplicación.

La interfaz principal debe ofrecer la configuración completa, el catálogo de modelos, las aplicaciones autorizadas, las políticas y el panel de uso. El icono permanente debe actuar como punto de acceso rápido y como indicador de salud, no como sustituto del panel principal.

## Objetivo principal: usar suscripciones ya contratadas sin repartir API keys

La razón de ser de Nexo es que el usuario pueda aprovechar desde cualquier aplicación la suscripción que ya tiene contratada, sin introducir una API key en cada herramienta y sin pagar por token un consumo que su plan ya cubre.

La dirección deseada es:

1. El usuario inicia sesión una vez desde Nexo.
2. Nexo obtiene y conserva de forma segura la autorización concedida.
3. Las aplicaciones cliente se conectan a Nexo mediante un token propio y limitado.
4. Nexo realiza las llamadas al proveedor correspondiente usando la autorización del usuario.

### Dos tipos de credencial, no uno

Nexo debe tratar el tipo de credencial como una dimensión de primer nivel, al mismo nivel que el proveedor. La misma cuenta del mismo proveedor ofrece catálogos, capacidades, límites y contabilidad **distintos** según cómo se haya autenticado.

| | **API key** | **OAuth de suscripción** |
| --- | --- | --- |
| Estabilidad | API pública y documentada | Flujo no soportado, puede romperse sin aviso |
| Coste | Por token, facturado aparte | Cubierto por el plan, sin coste marginal |
| Catálogo | Completo | Subconjunto de modelos, con capacidades recortadas |
| Métricas de uso | El proveedor informa de tokens | El proveedor no informa de tokens ni de cuota |
| Límites | Rate limits documentados | Solo se descubren al recibir un `429` |
| Riesgo para la cuenta | Ninguno | Posible incumplimiento de las condiciones del servicio |

Ninguna de las dos vías sustituye a la otra. El adaptador de un proveedor debe implementar las que soporte y declarar explícitamente qué ofrece cada una.

### Cómo funciona el OAuth de suscripción, y qué implica

Los proveedores no ofrecen un mecanismo oficial para que una aplicación de terceros consuma la cuota de una suscripción de tarifa plana: ese acceso está reservado a sus propios clientes. La técnica que hace viable el objetivo de Nexo, y que ya emplean proyectos como [opencode](https://github.com/anomalyco/opencode), consiste en:

1. Ejecutar el flujo **OAuth 2.0 con PKCE** del proveedor usando el **client_id público de su cliente oficial**.
2. Recibir el callback en un puerto local y canjear el código por tokens de acceso y refresco.
3. Llamar al **endpoint que consume ese cliente oficial**, que no siempre es la API pública del proveedor.

Esto debe quedar registrado sin eufemismos: **no es un mecanismo oficial ni soportado**. Nexo lo adopta de forma consciente porque sin él el producto no tiene razón de ser. La decisión, sus riesgos y sus mitigaciones están documentados en [ADR 0001](docs/adr/0001-oauth-de-suscripcion.md).

Consecuencias que el diseño debe asumir desde el principio:

- **Puede dejar de funcionar en cualquier momento.** El client_id, los parámetros del flujo o el endpoint pueden cambiar sin aviso. Cada proveedor con esta vía necesita una ruta de respaldo con API key y un mensaje de error que explique al usuario qué ha pasado.
- **Puede tener consecuencias sobre la cuenta.** Usar la suscripción desde una aplicación no autorizada puede incumplir las condiciones del servicio. Nexo debe advertirlo de forma clara y explícita antes de que el usuario complete el primer login de este tipo, no en una nota al pie.
- **Nexo multiplica el riesgo respecto a un cliente único.** Un asistente de escritorio es una persona usando su plan. Nexo, por diseño, multiplexa muchas aplicaciones sobre la misma suscripción, y ese patrón es precisamente el que un proveedor interpreta como abuso. Por eso los límites por aplicación no son una función de conveniencia sino un requisito de seguridad del producto: ver [Políticas y permisos](#políticas-y-permisos).
- **Nexo debe identificarse como Nexo.** Cuando el flujo admita un parámetro que identifique a la aplicación que lo origina, Nexo debe declararse honestamente. No debe suplantar el `User-Agent` ni la identidad de otro cliente para parecer una aplicación distinta de la que es.

### Lo que sigue estando prohibido

Reutilizar el flujo OAuth de un cliente oficial no abre la puerta a cualquier técnica. Nexo no debe:

- Hacer scraping ni automatizar el navegador para simular una sesión de usuario.
- Reutilizar cookies, sesiones o almacenamiento local del navegador del usuario.
- Leer, importar o extraer tokens de los ficheros de configuración de otras aplicaciones instaladas.
- Obtener credenciales por cualquier vía que no sea un flujo de autorización iniciado desde Nexo y completado conscientemente por el usuario.
- Ocultar al proveedor o al usuario qué aplicación está realizando las peticiones.

La diferencia es sustantiva: el usuario autoriza a Nexo de forma explícita y puede revocarlo desde su cuenta. Todo lo anterior sería apropiación de credenciales que el usuario no ha concedido a Nexo.

### OpenAI y ChatGPT: primer proveedor

OpenAI es el proveedor prioritario y su adaptador debe soportar las dos vías.

**Vía API key.** Contra `api.openai.com`, catálogo completo, tokens y coste reportados por el proveedor. Es la ruta estable y la que sirve de respaldo cuando la otra falla.

**Vía OAuth de suscripción.** Los parámetros observados en clientes que ya lo implementan, a fecha de julio de 2026, son:

- Issuer `https://auth.openai.com`, con `/oauth/authorize` y `/oauth/token`.
- Client_id público del cliente oficial de línea de comandos de OpenAI.
- PKCE con `code_challenge_method=S256` y scope `openid profile email offline_access`.
- Callback en `http://localhost:1455/auth/callback`, con variante de device flow para entornos sin navegador.
- Parámetros adicionales del flujo: `id_token_add_organizations`, `codex_cli_simplified_flow` y un `originator` que identifica a la aplicación.
- El identificador de cuenta se extrae de las claims del `id_token` y se envía en la cabecera `ChatGPT-Account-Id`.
- Las peticiones no van a la API pública, sino al backend de la aplicación de ChatGPT, que habla el formato **Responses**.
- El catálogo queda restringido a un subconjunto de modelos y excluye los modos de razonamiento más costosos.
- No hay información de tokens, cuota ni coste. El coste marginal es cero y la cuota consumida es invisible.

Ninguno de estos valores está versionado ni documentado por OpenAI. Deben vivir en un único módulo del adaptador, aislados del resto del sistema, y validarse mediante un spike antes de construir nada encima.

Consecuencia arquitectónica que no se puede aplazar: como esta ruta habla Responses y la API pública de Nexo habla `chat/completions`, **la traducción de formatos es el caso base y no la excepción**. Hay que traducir la petición en un sentido y el stream de eventos en el otro, en la ruta más importante del producto.

### Google y Gemini

Nexo debe contemplar OAuth para la Gemini API cuando el usuario disponga de un proyecto de Google Cloud y conceda los permisos requeridos. La autorización de la API de Gemini y una suscripción de la aplicación Gemini son conceptos distintos, por lo que el diseño no debe asumir que una suscripción de consumidor se convierte automáticamente en cuota o crédito de API.

El adaptador de Google debería soportar:

- OAuth de usuario para la Gemini API.
- Renovación segura de tokens.
- Selección del proyecto de Google Cloud cuando sea necesario.
- API key como alternativa explícita, no como requisito para cada aplicación cliente.

Si además se quiere aprovechar la cuota de un plan de consumidor, aplica todo lo descrito para el OAuth de suscripción y hace falta una investigación propia del flujo de su cliente oficial.

### Anthropic y Claude

Anthropic queda contemplado como proveedor futuro, con las dos vías: API key contra la API pública y OAuth de suscripción para planes Pro y Max. El flujo de suscripción de Anthropic requiere una investigación específica desde cero: no es equivalente al de OpenAI y no puede derivarse de él.

Se incluye explícitamente porque varias herramientas de desarrollo relevantes hablan el formato nativo de Anthropic y no el de OpenAI, lo que afecta a la superficie que Nexo debe exponer.

## Características previstas

### Gateway unificado

Nexo debe ofrecer una API local compatible con los formatos más extendidos del ecosistema. La primera interfaz es la compatible con OpenAI en su forma `chat/completions`, porque muchas herramientas ya permiten configurar una URL base y un token personalizados. Una segunda superficie compatible con el formato nativo de Anthropic queda prevista para cuando entre ese proveedor.

La API debe permitir como mínimo:

- Consultar los modelos disponibles.
- Enviar conversaciones de texto.
- Recibir respuestas normalizadas.
- Solicitar respuestas en streaming.
- Propagar errores de forma comprensible.
- Identificar el proveedor y el tipo de credencial mediante el nombre del modelo o una configuración explícita.

La representación interna debe ser un **superconjunto** de lo que ofrecen los proveedores, no el mínimo común denominador. Cuando una aplicación solicite una capacidad que la combinación de proveedor y credencial no soporta, Nexo debe devolver un error explícito y comprensible, nunca degradar la petición en silencio.

### Catálogo de modelos

El catálogo se indexa por proveedor **y tipo de credencial**, porque el mismo modelo no ofrece lo mismo por las dos vías. El usuario debe poder ver:

- Proveedor.
- Tipo de credencial con el que está disponible.
- Nombre original y nombre normalizado, con el proveedor siempre presente en el nombre público.
- Capacidades: texto, visión, audio, imagen, herramientas o embeddings.
- Contexto máximo y límites conocidos.
- Modo de contabilidad: medido por token, cubierto por suscripción o local.
- Estado de disponibilidad.

Las capacidades de un modelo no son descubribles mediante la API de los proveedores, así que el catálogo necesita un manifiesto versionado que se distribuya con la aplicación, se combine con los modelos que sí anuncia el proveedor y admita anulaciones locales del usuario.

El catálogo no debe ocultar las diferencias importantes entre modelos ni entre vías de acceso. La normalización debe facilitar el uso, no prometer capacidades que el proveedor no ofrece.

### Gestión de credenciales

Las credenciales deben almacenarse en el equipo del usuario usando el almacén seguro del sistema operativo. No deben guardarse en texto plano dentro de la configuración del proyecto ni en un fichero JSON con permisos restringidos: eso es lo que hacen otras herramientas y es un requisito donde Nexo debe ser mejor.

Nexo debe separar:

- Credenciales de proveedores.
- Tokens emitidos a aplicaciones cliente.
- Sesiones, access tokens y refresh tokens OAuth, tanto de API como de suscripción.
- Configuración no sensible.

Cada aplicación conectada debe recibir un token propio, revocable y limitado por scopes. Así no es necesario compartir una credencial maestra con todas las herramientas. Los tokens emitidos se guardan **hasheados** en la base de datos; el secreto en claro solo existe en el momento de la emisión y, si debe poder mostrarse de nuevo, en el almacén seguro del sistema.

### Políticas y permisos

El usuario debe poder decidir:

- Qué aplicaciones pueden utilizar Nexo.
- Qué proveedores, tipos de credencial y modelos puede utilizar cada aplicación.
- Si una aplicación puede enviar contenido multimodal.
- Si puede utilizar herramientas o funciones.
- Qué límites de uso se aplican.
- Si las peticiones y respuestas se registran o se excluyen del historial.

**Los límites por aplicación son obligatorios en las rutas respaldadas por una suscripción.** No es una preferencia configurable que se pueda dejar en blanco: sin ellos Nexo convierte una cuenta personal en un pool de API para cualquier proceso con un token válido, que es el escenario con más probabilidad de acabar en bloqueo o cierre de cuenta. El sistema debe traer valores por defecto conservadores, mostrar el consumo acumulado por ventana y rechazar las peticiones que excedan el límite con un error claro.

La configuración inicial debe ser segura: escucha local, acceso LAN desactivado y aprobación explícita antes de permitir conexiones externas.

### Hub de información, estadísticas y diagnóstico

Las estadísticas de uso son una capacidad central del producto. Nexo no debe limitarse a enrutar peticiones: debe ayudar al usuario a comprender cómo utiliza la IA en el conjunto de sus aplicaciones y proveedores.

Nexo debe registrar y mostrar localmente, siempre que el proveedor facilite la información necesaria:

- Número de peticiones.
- Aplicación que originó cada petición.
- Proveedor, tipo de credencial y modelo utilizado.
- Fecha, hora y duración de cada operación.
- Latencia total y, cuando pueda medirse, tiempo hasta el primer token.
- Resultado, cancelaciones y tipo de error.
- Tokens de entrada, tokens de salida y total consumido.
- Otras unidades de uso para imagen, audio, vídeo, embeddings o herramientas.
- Estimación de coste cuando exista información pública y fiable sobre precios.
- Estado de salud de cada conexión.
- Límites, cuotas o rate limits comunicados por el proveedor.

#### Tres estados de contabilidad, no dos

Distinguir entre dato real y estimación no basta. Cada métrica de coste y consumo debe llevar uno de estos estados:

- **Reportado:** el proveedor comunicó la cifra. Es un dato.
- **Estimado:** Nexo la calculó a partir de precios públicos y un recuento propio de tokens. Debe presentarse siempre como estimación.
- **Cubierto por suscripción:** la petición no tiene coste marginal porque el plan del usuario la cubre, y el proveedor no expone cuánta cuota ha consumido. Mostrar cero euros aquí es cierto y a la vez engañoso, así que la interfaz debe decir explícitamente que el coste es cero pero el consumo de cuota es desconocido.
- **No disponible:** el proveedor no informa y Nexo no puede estimar con fiabilidad. Se muestra como no disponible y no se inventa una cifra.

En las rutas de suscripción, los límites y cuotas del proveedor no son consultables. Nexo solo puede registrar los `429` recibidos y su propio consumo acumulado por aplicación, y debe presentarlo como tal.

El panel de información debe permitir:

- Filtrar por periodo, aplicación, proveedor, tipo de credencial, modelo y tipo de operación.
- Comparar el uso entre modelos, proveedores y vías de acceso.
- Consultar tendencias diarias, semanales y mensuales.
- Identificar los modelos más utilizados, los más lentos y los que más errores producen.
- Ver el reparto de consumo por aplicación cliente.
- Distinguir sin ambigüedad los cuatro estados de contabilidad anteriores.
- Exportar estadísticas en formatos abiertos para análisis externo.
- Configurar la retención y eliminar los datos almacenados.

Las métricas deben normalizarse para poder comparar proveedores sin perder los datos originales, que se conservan tal como los devolvió el proveedor.

El sistema debe recoger por defecto metadatos operativos, pero no el contenido completo de prompts y respuestas. El usuario debe poder configurar el nivel de registro, la retención, la exportación y el borrado. Las estadísticas deben permanecer en el equipo salvo que el usuario habilite expresamente alguna sincronización futura.

### Modelos locales

El proyecto debe poder incorporar proveedores locales como Ollama, MLX y llama.cpp. El gateway debe tratar estos proveedores como adaptadores más, con el mismo catálogo, permisos y métricas que los servicios cloud, y con modo de contabilidad local.

## Arquitectura conceptual

La primera implementación debería separar claramente estas piezas:

1. **Interfaz del gateway:** recibe peticiones de las aplicaciones y devuelve respuestas compatibles.
2. **Traductor de formatos:** convierte entre la representación interna y el formato concreto de cada API, en petición y en stream de respuesta.
3. **Router:** decide qué adaptador y qué tipo de credencial debe atender cada modelo o perfil.
4. **Adaptadores de proveedores:** encapsulan autenticación, formatos, capacidades y errores específicos de cada combinación de proveedor y credencial.
5. **Gestor de identidad:** ejecuta los flujos OAuth, maneja callbacks y renueva autorizaciones.
6. **Almacén seguro:** guarda secretos y tokens mediante el keychain o credential vault del sistema.
7. **Catálogo:** mantiene modelos, capacidades y estado de las conexiones, indexado por proveedor y credencial.
8. **Políticas y límites:** aplica tokens por aplicación, scopes, cuotas y aprobaciones.
9. **Observabilidad local:** recoge, normaliza y conserva métricas y logs con controles de privacidad.
10. **Motor de estadísticas:** agrega datos por tiempo, aplicación, proveedor, credencial y modelo.
11. **Servicio en segundo plano:** mantiene operativo el gateway aunque la ventana principal esté cerrada.
12. **Aplicación de escritorio:** ofrece el panel principal y la integración con la barra de estado o bandeja del sistema.

La lógica específica de un proveedor no debe filtrarse al resto del sistema. Añadir un proveedor nuevo debería consistir principalmente en implementar su adaptador y describir sus capacidades. Los valores frágiles de los flujos no oficiales deben estar aislados en un único módulo por proveedor, para que romperse afecte a un fichero y no a la arquitectura.

El contrato exacto está en [`docs/contrato-proveedor.md`](docs/contrato-proveedor.md) y el modelo de datos en [`docs/modelo-datos.md`](docs/modelo-datos.md).

## Stack tecnológico acordado

Nexo se implementará con **Tauri 2, Rust y Svelte 5**. Esta combinación permite construir una aplicación de escritorio multiplataforma reutilizando el WebView disponible en cada sistema operativo, mantener el gateway y los procesos críticos en un núcleo nativo y evitar distribuir un runtime completo de Chromium y Node.js con la aplicación.

La prioridad del stack es minimizar el consumo permanente de memoria y CPU, ya que Nexo debe poder permanecer activo durante toda la sesión del usuario aunque su ventana principal esté cerrada.

| Responsabilidad | Tecnología |
| --- | --- |
| Aplicación de escritorio | Tauri 2 |
| Núcleo del producto | Rust |
| Interfaz de usuario | Svelte 5, TypeScript y Vite |
| Gateway HTTP local | Axum y Tokio |
| Conexiones con proveedores | Reqwest y Rustls |
| Base de datos local | SQLite y Rusqlite |
| OAuth | OAuth 2.0 con PKCE y callback local |
| Credenciales | Almacén seguro nativo del sistema operativo |
| Streaming hacia aplicaciones | Server-Sent Events |
| Logs y diagnóstico | `tracing` |
| Gráficas | Biblioteca ligera basada en Canvas, evitando frameworks de visualización pesados |
| Distribución | DMG o app para macOS, MSI o NSIS para Windows y AppImage, deb o rpm para Linux |

### Reparto de responsabilidades

Todo el núcleo de Nexo debe implementarse en Rust:

- Gateway compatible con OpenAI.
- Traducción de formatos, enrutado y adaptadores de proveedores.
- Flujos OAuth de API y de suscripción, y renovación de tokens.
- Políticas, permisos, límites y tokens por aplicación.
- Captura, normalización y consulta de estadísticas.
- Persistencia en SQLite.
- Acceso al almacén seguro de credenciales.
- Servicio en segundo plano e integración con el sistema operativo.

La interfaz Svelte debe limitarse a presentar información y enviar acciones al núcleo. No debe contener lógica necesaria para que el gateway funcione. Esto permitirá mantener Nexo operativo sin una ventana abierta y hará posible reemplazar o ampliar la interfaz sin reescribir el motor.

### Modelo de ejecución ligero

La primera versión debe funcionar en un único proceso:

```text
Proceso Nexo
├── Icono de la barra de estado
├── Gateway HTTP local
├── Traductor y adaptadores de proveedores
├── Gestor OAuth
├── Motor de estadísticas
├── Base de datos SQLite
└── WebView del panel, visible solo cuando se abre
```

Al cerrar la ventana principal, Nexo no debe finalizar:

- El gateway continuará aceptando conexiones.
- El icono permanecerá en la barra de estado o bandeja del sistema.
- Rust continuará gestionando proveedores y estadísticas.
- La ventana y su WebView podrán ocultarse o destruirse para reducir el consumo.
- El panel se volverá a mostrar o crear cuando el usuario lo solicite.

No se utilizará inicialmente un daemon, sidecar o servicio independiente. Esa separación solo se planteará si aparecen necesidades reales de aislamiento, recuperación independiente o ejecución sin una sesión gráfica.

### Persistencia y estadísticas

SQLite será la única base de datos y se ejecutará embebida en la aplicación. Las peticiones se almacenarán como eventos compactos y las agregaciones estadísticas se calcularán de forma incremental sobre rollups horarios, evitando recorrer continuamente todo el histórico.

La base de datos contendrá métricas y configuración no sensible. Los prompts y respuestas no se guardarán por defecto. Los access tokens, refresh tokens, API keys y secretos OAuth no deben almacenarse en SQLite.

### Credenciales por plataforma

Nexo utilizará el almacén seguro disponible en cada sistema:

- Keychain en macOS.
- Credential Manager en Windows.
- Secret Service o alternativa equivalente en Linux.

La base de datos podrá guardar referencias y metadatos de las credenciales, pero nunca el secreto en texto plano.

### Motivos para descartar otras opciones

- **Electron:** facilita el desarrollo con JavaScript, pero incorpora Chromium y Node.js y utiliza varios procesos. Su coste base no encaja con una aplicación que debe permanecer activa todo el día consumiendo lo mínimo posible.
- **Flutter:** ofrece buen soporte multiplataforma, pero distribuye su propio motor gráfico y runtime. Sus ventajas son mayores en aplicaciones centradas en interfaces visuales complejas que en un gateway que trabaja principalmente en segundo plano.
- **Tres aplicaciones nativas independientes:** permitirían una integración óptima con cada sistema, pero obligarían a mantener implementaciones separadas para macOS, Windows y Linux.
- **Reutilizar un gateway existente como LiteLLM:** resolvería el enrutado y la contabilidad, pero obligaría a distribuir un runtime de Python como proceso adicional, lo que contradice el objetivo de consumo mínimo en reposo. Si las mediciones del prototipo demuestran que ese objetivo no aprieta tanto como se supone, la decisión merece revisarse.

La elección de Tauri debe validarse durante el primer prototipo midiendo el consumo real en reposo, con tráfico y con el panel abierto, contra los objetivos numéricos del [ADR 0002](docs/adr/0002-stack-tauri-rust-svelte.md). Si las mediciones no cumplen, se reevaluará la capa de escritorio sin reemplazar el núcleo Rust.

## Alcance inicial recomendado

La primera versión funcional debe centrarse en un único usuario y una única máquina, y su criterio de éxito es que el autor pueda dejar de meter API keys en sus propias herramientas:

- Aplicación de escritorio multiplataforma, desarrollada y validada inicialmente en macOS.
- Servicio local capaz de seguir funcionando en segundo plano.
- Icono permanente en la barra de estado de macOS mientras Nexo esté activo.
- Gateway escuchando en localhost.
- API compatible con OpenAI en formato `chat/completions`, con y sin streaming.
- Adaptador de OpenAI con las dos vías: OAuth de suscripción como objetivo principal y API key como respaldo.
- Traducción entre `chat/completions` y el formato Responses, en petición y en stream.
- Proveedor mock y adaptador de Ollama para validar el gateway sin credenciales ni coste.
- Catálogo indexado por proveedor y tipo de credencial.
- Tokens independientes y revocables para aplicaciones cliente, con límites obligatorios en las rutas de suscripción.
- Registro local de métricas con los cuatro estados de contabilidad, sin guardar por defecto el contenido de los mensajes.
- Panel inicial de estadísticas por aplicación, proveedor, credencial, modelo y periodo.

Google Gemini, Anthropic, MLX, llama.cpp, capacidades multimodales y la superficie compatible con el formato de Anthropic quedan fuera de la primera versión. El soporte multiusuario, la sincronización entre equipos, el acceso remoto y el despliegue empresarial quedan fuera del proyecto.

## Roadmap

### Fase 0: validar lo que puede matar el proyecto

Dos spikes de código, no documentos. Nada más se construye hasta que los dos concluyan.

- **Spike de OAuth de suscripción.** Implementar en Rust el flujo PKCE contra el issuer de OpenAI, recibir el callback en local, canjear los tokens y hacer una petición real al endpoint de suscripción identificándose como Nexo. Capturar la forma exacta de la petición, del stream de eventos y de los errores. Documentar qué modelos aparecen y qué información de uso llega, si llega alguna. Es el spike que decide si Nexo existe.
- **Spike de Tauri.** Esqueleto en macOS con icono de barra de estado, cierre de ventana que oculta en lugar de terminar, y Axum escuchando en `127.0.0.1` en el mismo proceso. Medir memoria residente, CPU y tiempo de arranque en reposo, con tráfico en streaming y con el panel abierto, contra los objetivos del ADR 0002.

Salida de la fase: los dos spikes funcionando y los ADR actualizados con resultados reales.

### Fase 1: gateway mínimo de extremo a extremo

- Definir el contrato de proveedor sobre los dos ejes de proveedor y credencial.
- Implementar `GET /v1/models` y `POST /v1/chat/completions` con y sin streaming.
- Añadir proveedor mock y adaptador de Ollama, que dan streaming y recuento de tokens reales sin credenciales.
- Implementar el traductor de formatos y las pruebas de contrato del stream.
- Emitir el primer token de aplicación y registrar el primer evento en SQLite.
- Validar con una herramienta cliente real configurada contra la URL local.

### Fase 2: OpenAI en producción

- Adaptador de OpenAI por API key.
- Adaptador de OpenAI por OAuth de suscripción, con el módulo de valores frágiles aislado.
- Tokens en el almacén seguro del sistema, con renovación, revocación y desconexión de cuentas.
- Aviso explícito y confirmación del usuario antes del primer login de suscripción.
- Límites por aplicación aplicados y visibles.
- Respaldo automático a API key cuando la ruta de suscripción falle, con error comprensible cuando no haya respaldo configurado.
- Catálogo diferenciado por credencial.

### Fase 3: control y experiencia de usuario

- Interfaz de configuración y aprobación de conexiones.
- Perfiles y reglas de enrutado.
- Panel completo de estadísticas, comparativas, health checks y diagnóstico.
- Filtros, periodos, exportación y gestión de retención.
- Configuración de privacidad para logs.

### Fase 4: más proveedores

- Google Gemini con OAuth de API y con API key.
- Ollama consolidado, más MLX y llama.cpp.
- Anthropic por API key y, tras su propia investigación, por OAuth de suscripción.
- Superficie compatible con el formato nativo de Anthropic.
- Capacidades multimodales.

### Fase 5: distribución

Solo cuando Nexo funcione para su autor. La decisión de publicar o no es independiente de la de construir: distribuir una herramienta que usa flujos no soportados tiene implicaciones que hay que valorar entonces, no ahora.

- Consolidar macOS y crear instaladores para Windows y Linux.
- Adaptar la integración permanente al system tray de Windows y a los indicadores de Linux.
- Documentar integraciones con clientes populares.

## Criterios de aceptación del producto

Nexo podrá considerarse útil cuando un usuario pueda:

1. Instalarlo y ejecutarlo localmente sin desplegar un servidor externo.
2. Autorizar su suscripción de ChatGPT desde Nexo con un único login, y usarla después desde cualquier aplicación sin introducir una API key.
3. Conectar una aplicación compatible con OpenAI usando una URL local y un token de Nexo.
4. Elegir un modelo de varios proveedores sin cambiar la integración de la aplicación cliente.
5. Ver claramente qué modelos están disponibles por suscripción y qué modelos requieren API key, sin sorpresas en tiempo de ejecución.
6. Revocar el acceso de una aplicación sin invalidar todas las demás.
7. Saber qué proveedor, credencial y modelo atendió cada petición y cuánto tardó.
8. Fijar un límite de uso por aplicación y ver el consumo acumulado contra ese límite.
9. Desconectar una cuenta y eliminar sus tokens del equipo.
10. Usar modelos locales cuando no quiera enviar datos a un proveedor cloud.
11. Consultar el uso por aplicación, proveedor, credencial, modelo y periodo desde un panel local.
12. Comparar consumo, latencia y errores sin confundir un dato reportado, una estimación, un consumo cubierto por suscripción y un dato no disponible.
13. Recibir un error comprensible, y el respaldo por API key si lo ha configurado, cuando la ruta de suscripción deje de funcionar.
14. Cerrar la ventana principal sin detener el gateway.
15. Consultar y controlar Nexo desde un icono siempre disponible en la barra de estado.
16. Operar con una configuración segura por defecto: solo localhost, sin límites en blanco y sin registro de contenido.

Los objetivos numéricos de memoria, CPU y arranque están en el [ADR 0002](docs/adr/0002-stack-tauri-rust-svelte.md).

## Fuera de alcance y restricciones

- No se debe obtener ninguna credencial por una vía distinta a un flujo de autorización iniciado desde Nexo y completado conscientemente por el usuario.
- No se deben reutilizar cookies, sesiones ni almacenamiento del navegador, ni leer los ficheros de credenciales de otras aplicaciones instaladas.
- No se debe hacer scraping ni automatizar el navegador para simular una sesión.
- No se debe suplantar la identidad de otro cliente ante el proveedor cuando exista una forma de identificarse honestamente.
- No se debe habilitar una ruta de suscripción sin límites por aplicación y sin advertencia previa al usuario.
- No se debe presentar un flujo no soportado como si fuera oficial, ni ante el usuario ni en la documentación.
- No se debe enviar la información de una aplicación a otra sin consentimiento.
- No se debe guardar por defecto el contenido completo de las conversaciones.
- No se deben enviar las estadísticas fuera del equipo sin consentimiento explícito.
- No se deben presentar costes, tokens o cuotas estimados como datos confirmados por el proveedor, ni un coste cero por suscripción como ausencia de consumo.
- No se debe habilitar acceso por red sin autenticación, autorización y transporte seguro.
- No se debe presentar la compatibilidad de formatos como equivalencia total de capacidades.
- No se debe degradar en silencio una petición cuya capacidad no soporta el destino.

## Documentación de decisiones

- [ADR 0001: OAuth de suscripción](docs/adr/0001-oauth-de-suscripcion.md) — por qué se adopta un mecanismo no soportado, qué riesgos se aceptan y cómo se mitigan.
- [ADR 0002: Tauri 2, Rust y Svelte 5](docs/adr/0002-stack-tauri-rust-svelte.md) — objetivos numéricos y criterios de reevaluación.
- [Contrato de proveedor](docs/contrato-proveedor.md) — los dos ejes, la representación interna y la taxonomía de errores.
- [Modelo de datos](docs/modelo-datos.md) — esquema SQLite de cuentas, aplicaciones, catálogo, eventos y rollups.

## Fuentes de contexto

- [Msty Nexus](https://msty.ai/products/nexus/), referencia de producto para gateway local, catálogo, credenciales, tokens por aplicación y observabilidad.
- [opencode](https://github.com/anomalyco/opencode), implementación de referencia del OAuth de suscripción. En particular [`packages/opencode/src/plugin/openai/codex.ts`](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/plugin/openai/codex.ts) para OpenAI y [`packages/opencode/src/plugin/github-copilot/copilot.ts`](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/plugin/github-copilot/copilot.ts) para el patrón de device flow.
- [Gemini API: autenticación con OAuth](https://ai.google.dev/gemini-api/docs/oauth), referencia oficial para el flujo OAuth de la API de Gemini.
- [OpenAI: Sign in with ChatGPT](https://help.openai.com/en/articles/20001410-sign-in-with-chatgpt), referencia oficial sobre autenticación de identidad y sobre qué concede ese login a aplicaciones externas.
- [Tauri 2: arquitectura](https://v2.tauri.app/es/concept/architecture/), referencia para el núcleo Rust, WebViews y soporte multiplataforma.
- [Tauri 2: distribución](https://v2.tauri.app/distribute/), referencia para los formatos de instalación y firma en macOS, Windows y Linux.
- [Tauri 2: plugins oficiales](https://v2.tauri.app/plugin/), referencia para system tray, autostart, actualización, SQL y almacenamiento seguro.
