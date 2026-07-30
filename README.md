# Nexo

## Descripción

Nexo es un proyecto para crear un punto común de acceso a modelos de inteligencia artificial. Su propósito es que una persona pueda conectar sus aplicaciones, asistentes, herramientas de desarrollo y automatizaciones a un único gateway local, en lugar de configurar cada proveedor y cada credencial por separado en cada aplicación.

La idea toma como referencia conceptual a [Msty Nexus](https://msty.ai/products/nexus/), pero Nexo debe ser un proyecto independiente, más abierto y orientado a resolver algunas limitaciones de ese tipo de herramientas. El objetivo no es crear otro chat, sino una capa de infraestructura personal que controle cómo las aplicaciones acceden a diferentes modelos y que, al mismo tiempo, funcione como centro de información sobre su utilización.

Nexo debe ser un hub completo en dos dimensiones inseparables:

- **Hub funcional:** conecta aplicaciones, modelos, proveedores, credenciales y políticas mediante una interfaz común.
- **Hub de información:** explica qué se está usando, desde dónde, con qué frecuencia, con qué rendimiento y, cuando sea posible, con qué consumo o coste.

El repositorio contiene inicialmente la definición del producto y la dirección técnica. No incluye código todavía: su función es servir como documento de partida para que otra persona pueda entender el problema, tomar decisiones de arquitectura y comenzar la implementación.

## Problema que resuelve

Actualmente, cada aplicación suele pedir sus propias credenciales y configuración:

- Una API key de OpenAI.
- Otra API key de Google o Gemini.
- Configuración independiente para modelos locales.
- Diferentes nombres y formatos para realizar una misma operación.
- Ausencia de una visión común sobre uso, permisos, errores y disponibilidad.

Esto provoca duplicación de configuración, secretos repartidos por varios sitios, dificultad para cambiar de proveedor y poca visibilidad sobre qué aplicación está utilizando cada modelo.

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
- Traducir formatos de petición y respuesta cuando sea necesario.
- Gestionar la autenticación con cada proveedor.
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

## Objetivo principal: aprovechar autenticación OAuth

Una de las razones principales para crear Nexo es evitar que el usuario tenga que introducir una API key de cada proveedor para poder usar una suscripción que ya tiene contratada.

La dirección deseada es:

1. El usuario inicia sesión una vez desde Nexo.
2. Nexo obtiene y conserva de forma segura la autorización concedida.
3. Las aplicaciones cliente se conectan a Nexo mediante un token propio y limitado.
4. Nexo realiza las llamadas al proveedor correspondiente usando la autorización del usuario.

### ChatGPT / OpenAI

La investigación inicial debe distinguir dos conceptos que no son equivalentes:

- **Iniciar sesión con ChatGPT:** autentica la identidad del usuario en una aplicación compatible.
- **Usar los modelos y límites de una suscripción de ChatGPT desde una aplicación externa:** requiere que OpenAI ofrezca un mecanismo oficial de acceso delegado para ese uso.

Nexo debe perseguir esta segunda posibilidad únicamente mediante mecanismos oficiales y documentados por OpenAI. El proyecto no debe basarse en scraping, automatización del navegador, reutilización de cookies, extracción de tokens privados ni endpoints internos de la aplicación web. Esas técnicas serían frágiles, inseguras y podrían incumplir las condiciones del servicio.

Mientras no exista un mecanismo oficial que permita usar la suscripción de ChatGPT desde aplicaciones de terceros, el diseño debe mantener separado el acceso mediante la API oficial de OpenAI y el acceso mediante una suscripción de ChatGPT. El login de identidad por sí solo no debe presentarse como acceso a conversaciones, memoria, archivos, tokens, facturación o modelos de ChatGPT.

### Google / Gemini

Nexo debe contemplar OAuth para la Gemini API cuando el usuario disponga de un proyecto de Google Cloud y conceda los permisos requeridos. La autorización de la API de Gemini y una suscripción de la aplicación Gemini son conceptos distintos, por lo que el diseño no debe asumir que una suscripción de consumidor se convierte automáticamente en cuota o crédito de API.

El adaptador de Google debería soportar, según la política y documentación vigente de Google:

- OAuth de usuario para la Gemini API.
- Renovación segura de tokens.
- Selección del proyecto de Google Cloud cuando sea necesario.
- API key como alternativa explícita, no como requisito para cada aplicación cliente.

## Características previstas

### Gateway unificado

Nexo debe ofrecer una API local compatible, en la medida de lo posible, con los formatos más extendidos del ecosistema. La primera interfaz recomendada es la compatible con OpenAI, porque muchas herramientas ya permiten configurar una URL base y un token personalizados.

La API debe permitir como mínimo:

- Consultar los modelos disponibles.
- Enviar conversaciones de texto.
- Recibir respuestas normalizadas.
- Solicitar respuestas en streaming.
- Propagar errores de forma comprensible.
- Identificar el proveedor mediante el nombre del modelo o una configuración explícita.

### Catálogo de modelos

El usuario debe poder ver en un único catálogo:

- Proveedor.
- Nombre original y nombre normalizado.
- Capacidades: texto, visión, audio, imagen, herramientas o embeddings.
- Contexto máximo y límites conocidos.
- Estado de disponibilidad.
- Método de autenticación configurado.

El catálogo no debe ocultar las diferencias importantes entre modelos. La normalización debe facilitar el uso, no prometer capacidades que el proveedor no ofrece.

### Gestión de credenciales

Las credenciales deben almacenarse en el equipo del usuario usando el almacén seguro del sistema operativo siempre que sea posible. No deben guardarse en texto plano dentro de la configuración del proyecto.

Nexo debe separar:

- Credenciales de proveedores.
- Tokens emitidos a aplicaciones cliente.
- Sesiones y refresh tokens OAuth.
- Configuración no sensible.

Cada aplicación conectada debería recibir un token propio, revocable y limitado por scopes. Así no sería necesario compartir una credencial maestra con todas las herramientas.

### Políticas y permisos

El usuario debe poder decidir:

- Qué aplicaciones pueden utilizar Nexo.
- Qué proveedores y modelos puede utilizar cada aplicación.
- Si una aplicación puede enviar contenido multimodal.
- Si puede utilizar herramientas o funciones.
- Qué límites de uso se aplican.
- Si las peticiones y respuestas se registran o se excluyen del historial.

La configuración inicial debe ser segura: escucha local, acceso LAN desactivado y aprobación explícita antes de permitir conexiones externas.

### Hub de información, estadísticas y diagnóstico

Las estadísticas de uso son una capacidad central del producto. Nexo no debe limitarse a enrutar peticiones: debe ayudar al usuario a comprender cómo utiliza la IA en el conjunto de sus aplicaciones y proveedores.

Nexo debe registrar y mostrar localmente, siempre que el proveedor facilite la información necesaria:

- Número de peticiones.
- Aplicación que originó cada petición.
- Proveedor y modelo utilizado.
- Fecha, hora y duración de cada operación.
- Latencia total y, cuando pueda medirse, tiempo hasta el primer token.
- Resultado, cancelaciones y tipo de error.
- Tokens de entrada, tokens de salida y total consumido.
- Otras unidades de uso para imagen, audio, vídeo, embeddings o herramientas.
- Estimación de coste cuando exista información pública y fiable sobre precios.
- Estado de salud de cada conexión.
- Límites, cuotas o rate limits comunicados por el proveedor.

El panel de información debe permitir:

- Filtrar por periodo, aplicación, proveedor, modelo y tipo de operación.
- Comparar el uso entre modelos y proveedores.
- Consultar tendencias diarias, semanales y mensuales.
- Identificar los modelos más utilizados, los más lentos y los que más errores producen.
- Ver el reparto de consumo por aplicación cliente.
- Distinguir datos reales comunicados por el proveedor de estimaciones calculadas por Nexo.
- Exportar estadísticas en formatos abiertos para análisis externo.
- Configurar la retención y eliminar los datos almacenados.

Las métricas deben normalizarse para poder comparar proveedores sin perder los datos originales. Cuando un proveedor no comunique tokens, coste, cuota u otra métrica, Nexo debe indicarlo como dato no disponible y no inventar una cifra.

El sistema debe recoger por defecto metadatos operativos, pero no el contenido completo de prompts y respuestas. El usuario debe poder configurar el nivel de registro, la retención, la exportación y el borrado. Las estadísticas deben permanecer en el equipo salvo que el usuario habilite expresamente alguna sincronización futura.

### Modelos locales

El proyecto debe poder incorporar proveedores locales como Ollama, MLX y llama.cpp. El gateway debe tratar estos proveedores como adaptadores más, con el mismo catálogo, permisos y métricas que los servicios cloud.

## Arquitectura conceptual

La primera implementación debería separar claramente estas piezas:

1. **Interfaz del gateway:** recibe peticiones de las aplicaciones y devuelve respuestas compatibles.
2. **Router:** decide qué adaptador debe atender cada modelo o perfil.
3. **Adaptadores de proveedores:** encapsulan autenticación, formatos, capacidades y errores específicos de OpenAI, Google y runtimes locales.
4. **Gestor de identidad:** ejecuta los flujos OAuth, maneja callbacks y renueva autorizaciones.
5. **Almacén seguro:** guarda secretos y tokens mediante el keychain o credential vault del sistema.
6. **Catálogo:** mantiene modelos, capacidades y estado de las conexiones.
7. **Políticas:** aplica tokens por aplicación, scopes, límites y aprobaciones.
8. **Observabilidad local:** recoge, normaliza y conserva métricas y logs con controles de privacidad.
9. **Motor de estadísticas:** agrega datos por tiempo, aplicación, proveedor y modelo para alimentar comparativas e informes.
10. **Servicio en segundo plano:** mantiene operativo el gateway aunque la ventana principal esté cerrada.
11. **Aplicación de escritorio:** ofrece el panel principal y la integración con la barra de estado o bandeja del sistema.

La lógica específica de un proveedor no debe filtrarse al resto del sistema. Añadir un proveedor nuevo debería consistir principalmente en implementar su adaptador y describir sus capacidades.

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
- Enrutado y adaptadores de proveedores.
- Flujos OAuth y renovación de tokens.
- Políticas, permisos y tokens por aplicación.
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
├── Adaptadores de proveedores
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

No se utilizará inicialmente un daemon, sidecar o servicio independiente. Esa separación solo se planteará si aparecen necesidades reales de aislamiento, recuperación independiente o ejecución sin una sesión gráfica. Evitar procesos adicionales simplifica el producto y reduce su consumo base.

### Persistencia y estadísticas

SQLite será la única base de datos y se ejecutará embebida en la aplicación. Las peticiones se almacenarán como eventos compactos y las agregaciones estadísticas se calcularán por lotes o de forma incremental, evitando recorrer continuamente todo el histórico.

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

La elección de Tauri debe validarse durante el primer prototipo midiendo el consumo real en reposo, con tráfico y con el panel abierto. Si las mediciones no cumplen los objetivos del proyecto, se reevaluará la capa de escritorio sin reemplazar el núcleo Rust.

## Alcance inicial recomendado

La primera versión funcional debería centrarse en un único usuario y una única máquina:

- Aplicación de escritorio multiplataforma, desarrollada y validada inicialmente en macOS.
- Servicio local capaz de seguir funcionando en segundo plano.
- Icono permanente en la barra de estado de macOS mientras Nexo esté activo.
- Gateway escuchando en localhost.
- API compatible con OpenAI.
- Catálogo de modelos configurados.
- Proveedor mock para validar el gateway sin credenciales.
- Un adaptador oficial de Google Gemini mediante OAuth.
- Adaptador de OpenAI mediante el mecanismo oficial disponible.
- Tokens independientes para aplicaciones cliente.
- Registro local de métricas sin guardar por defecto el contenido de los mensajes.
- Panel inicial de estadísticas por aplicación, proveedor, modelo y periodo.

El soporte multiusuario, sincronización entre equipos, acceso remoto y despliegue empresarial deben quedar fuera del primer alcance.

## Roadmap sugerido

### Fase 0: decisiones y validación

- Confirmar qué accesos OAuth ofrecen oficialmente OpenAI y Google para aplicaciones de terceros.
- Definir los proveedores y modelos prioritarios.
- Validar Tauri 2, Rust y Svelte 5 mediante un prototipo mínimo en macOS y mediciones reproducibles de memoria, CPU y tiempo de arranque.
- Definir el comportamiento del servicio en segundo plano y de la integración con la barra de estado.
- Definir el modelo común de métricas y qué datos puede aportar realmente cada proveedor.
- Definir el modelo de amenazas y la política de privacidad.

### Fase 1: gateway mínimo

- Crear la interfaz local del gateway.
- Definir el contrato común de proveedor.
- Implementar catálogo, enrutado, errores y streaming.
- Añadir pruebas de contrato.
- Añadir proveedor mock y un primer proveedor real.
- Ejecutar el gateway como servicio local controlado desde la aplicación de macOS.
- Añadir el icono de la barra de estado con salud y acciones básicas.

### Fase 2: identidad y seguridad

- Implementar OAuth con callback local.
- Guardar tokens en el almacén seguro del sistema.
- Añadir renovación, revocación y desconexión de cuentas.
- Añadir tokens por aplicación y scopes.
- Bloquear por defecto toda exposición fuera de localhost.

### Fase 3: control y experiencia de usuario

- Crear la interfaz de configuración.
- Añadir aprobaciones de conexiones.
- Añadir perfiles y reglas de enrutado.
- Añadir el panel completo de estadísticas, comparativas, health checks y diagnóstico.
- Añadir filtros, periodos, exportación y gestión de retención.
- Añadir configuración de privacidad para logs.

### Fase 4: proveedores y distribución

- Añadir Ollama, MLX y llama.cpp.
- Añadir capacidades multimodales.
- Consolidar la versión de macOS y crear instaladores para Windows y Linux.
- Adaptar la integración permanente al system tray de Windows y a los indicadores disponibles en Linux.
- Documentar integraciones con clientes populares.

## Criterios de aceptación del producto

Nexo podrá considerarse útil cuando un usuario pueda:

1. Instalarlo y ejecutarlo localmente sin desplegar un servidor externo.
2. Autorizar un proveedor mediante un flujo oficial, sin copiar la credencial en cada aplicación.
3. Conectar una aplicación compatible con OpenAI usando una URL local y un token de Nexo.
4. Elegir un modelo de varios proveedores sin cambiar la integración de la aplicación cliente.
5. Revocar el acceso de una aplicación sin invalidar todas las demás.
6. Saber qué proveedor atendió cada petición y cuánto tardó.
7. Desconectar una cuenta y eliminar sus tokens del equipo.
8. Usar modelos locales cuando no quiera enviar datos a un proveedor cloud.
9. Comprobar claramente qué capacidades y límites tiene cada modelo.
10. Operar con una configuración segura por defecto.
11. Consultar el uso por aplicación, proveedor, modelo y periodo desde un panel local.
12. Comparar consumo, latencia, errores y coste estimado sin confundir estimaciones con datos reales.
13. Cerrar la ventana principal sin detener el gateway.
14. Consultar y controlar Nexo desde un icono siempre disponible en la barra de estado de macOS.
15. Instalar futuras versiones en Windows y Linux manteniendo el mismo comportamiento esencial.

## Fuera de alcance y restricciones

- No se deben implementar accesos no oficiales a cuentas de ChatGPT o Gemini.
- No se deben reutilizar cookies, tokens privados ni sesiones del navegador.
- No se debe enviar la información de una aplicación a otra sin consentimiento.
- No se debe guardar por defecto el contenido completo de las conversaciones.
- No se deben enviar las estadísticas fuera del equipo sin consentimiento explícito.
- No se deben presentar costes, tokens o cuotas estimados como datos confirmados por el proveedor.
- No se debe habilitar acceso por red sin autenticación, autorización y transporte seguro.
- No se debe presentar la compatibilidad de formatos como equivalencia total de capacidades.

## Primera tarea para quien implemente el proyecto

Antes de escribir código, la persona encargada debería producir:

- Una matriz de proveedores, modelos, capacidades y métodos de autenticación.
- Un registro de decisión arquitectónica sobre Tauri 2, Rust y Svelte 5, con objetivos medibles de memoria, CPU y tiempo de arranque.
- Un prototipo técnico que valide la ejecución en segundo plano, la barra de estado de macOS y el ciclo de vida del WebView.
- Un modelo de datos para cuentas, tokens, aplicaciones, scopes y modelos.
- Un modelo de eventos y métricas que permita estadísticas comparables sin perder los datos originales de cada proveedor.
- Un boceto del panel de uso y del menú disponible desde el icono de estado.
- Un diagrama de flujo de los procesos OAuth.
- Un contrato de proveedor independiente del formato concreto de cada API.
- Una política de almacenamiento, logs, borrado y privacidad.
- Un plan de pruebas que incluya expiración de tokens, errores, revocación y caída de proveedores.

El resultado de esa tarea debe permitir implementar el gateway mínimo sin depender de APIs privadas ni de decisiones implícitas.

## Fuentes de contexto

- [Msty Nexus](https://msty.ai/products/nexus/), referencia de producto para gateway local, catálogo, credenciales, tokens por aplicación y observabilidad.
- [Gemini API: autenticación con OAuth](https://ai.google.dev/gemini-api/docs/oauth), referencia oficial para el flujo OAuth de la API de Gemini.
- [OpenAI: Sign in with ChatGPT](https://help.openai.com/en/articles/20001410-sign-in-with-chatgpt), referencia oficial sobre autenticación de identidad y límites de lo que ese login concede a aplicaciones externas.
- [Tauri 2: arquitectura](https://v2.tauri.app/es/concept/architecture/), referencia para el núcleo Rust, WebViews y soporte multiplataforma.
- [Tauri 2: distribución](https://v2.tauri.app/distribute/), referencia para los formatos de instalación y firma en macOS, Windows y Linux.
- [Tauri 2: plugins oficiales](https://v2.tauri.app/plugin/), referencia para system tray, autostart, actualización, SQL y almacenamiento seguro.
