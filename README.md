# Nexo

## Descripción

Nexo es un proyecto para crear un punto común de acceso a modelos de inteligencia artificial. Su propósito es que una persona pueda conectar sus aplicaciones, asistentes, herramientas de desarrollo y automatizaciones a un único gateway local, en lugar de configurar cada proveedor y cada credencial por separado en cada aplicación.

La idea toma como referencia conceptual a [Msty Nexus](https://msty.ai/products/nexus/), pero Nexo debe ser un proyecto independiente, más abierto y orientado a resolver algunas limitaciones de ese tipo de herramientas. El objetivo no es crear otro chat, sino una capa de infraestructura personal que controle cómo las aplicaciones acceden a diferentes modelos.

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
- Registrar métricas locales de uso y rendimiento.
- Permitir cambiar de proveedor sin modificar todas las aplicaciones conectadas.

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

### Métricas y diagnóstico

Nexo debería mostrar localmente:

- Número de peticiones.
- Aplicación que originó cada petición.
- Proveedor y modelo utilizado.
- Latencia.
- Resultado y tipo de error.
- Tokens o unidades de uso cuando el proveedor los comunique.
- Estado de salud de cada conexión.

El registro debe poder configurarse para no almacenar el contenido sensible de las conversaciones.

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
8. **Observabilidad local:** recoge métricas y logs con controles de privacidad.
9. **Interfaz de usuario:** permite configurar proveedores, autorizar cuentas, aprobar aplicaciones y consultar el uso.

La lógica específica de un proveedor no debe filtrarse al resto del sistema. Añadir un proveedor nuevo debería consistir principalmente en implementar su adaptador y describir sus capacidades.

## Alcance inicial recomendado

La primera versión funcional debería centrarse en un único usuario y una única máquina:

- Aplicación o servicio local.
- Gateway escuchando en localhost.
- API compatible con OpenAI.
- Catálogo de modelos configurados.
- Proveedor mock para validar el gateway sin credenciales.
- Un adaptador oficial de Google Gemini mediante OAuth.
- Adaptador de OpenAI mediante el mecanismo oficial disponible.
- Tokens independientes para aplicaciones cliente.
- Registro de métricas sin guardar por defecto el contenido de los mensajes.

El soporte multiusuario, sincronización entre equipos, acceso remoto y despliegue empresarial deben quedar fuera del primer alcance.

## Roadmap sugerido

### Fase 0: decisiones y validación

- Confirmar qué accesos OAuth ofrecen oficialmente OpenAI y Google para aplicaciones de terceros.
- Definir los proveedores y modelos prioritarios.
- Elegir plataforma inicial: macOS, Windows, Linux o aplicación multiplataforma.
- Definir el modelo de amenazas y la política de privacidad.

### Fase 1: gateway mínimo

- Crear la interfaz local del gateway.
- Definir el contrato común de proveedor.
- Implementar catálogo, enrutado, errores y streaming.
- Añadir pruebas de contrato.
- Añadir proveedor mock y un primer proveedor real.

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
- Añadir métricas, health checks y diagnóstico.
- Añadir configuración de privacidad para logs.

### Fase 4: proveedores y distribución

- Añadir Ollama, MLX y llama.cpp.
- Añadir capacidades multimodales.
- Crear instaladores para los sistemas operativos prioritarios.
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

## Fuera de alcance y restricciones

- No se deben implementar accesos no oficiales a cuentas de ChatGPT o Gemini.
- No se deben reutilizar cookies, tokens privados ni sesiones del navegador.
- No se debe enviar la información de una aplicación a otra sin consentimiento.
- No se debe guardar por defecto el contenido completo de las conversaciones.
- No se debe habilitar acceso por red sin autenticación, autorización y transporte seguro.
- No se debe presentar la compatibilidad de formatos como equivalencia total de capacidades.

## Primera tarea para quien implemente el proyecto

Antes de escribir código, la persona encargada debería producir:

- Una matriz de proveedores, modelos, capacidades y métodos de autenticación.
- Una decisión documentada sobre la plataforma de escritorio o servicio local.
- Un modelo de datos para cuentas, tokens, aplicaciones, scopes y modelos.
- Un diagrama de flujo de los procesos OAuth.
- Un contrato de proveedor independiente del formato concreto de cada API.
- Una política de almacenamiento, logs, borrado y privacidad.
- Un plan de pruebas que incluya expiración de tokens, errores, revocación y caída de proveedores.

El resultado de esa tarea debe permitir implementar el gateway mínimo sin depender de APIs privadas ni de decisiones implícitas.

## Fuentes de contexto

- [Msty Nexus](https://msty.ai/products/nexus/), referencia de producto para gateway local, catálogo, credenciales, tokens por aplicación y observabilidad.
- [Gemini API: autenticación con OAuth](https://ai.google.dev/gemini-api/docs/oauth), referencia oficial para el flujo OAuth de la API de Gemini.
- [OpenAI: Sign in with ChatGPT](https://help.openai.com/en/articles/20001410-sign-in-with-chatgpt), referencia oficial sobre autenticación de identidad y límites de lo que ese login concede a aplicaciones externas.
