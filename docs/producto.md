# Nexo: definición de producto

Qué problema resuelve, qué es y qué no es. Las instrucciones de uso y compilación
están en el [README](../README.md); el plan de trabajo en [ROADMAP.md](../ROADMAP.md).

## El problema

Cada aplicación pide sus propias credenciales y su propia configuración:

- Una API key de OpenAI.
- Otra API key de Google o Gemini.
- Configuración independiente para modelos locales.
- Distintos nombres y formatos para la misma operación.
- Ninguna visión común sobre uso, permisos, errores y disponibilidad.

Eso provoca duplicación de configuración, secretos repartidos por varios sitios,
dificultad para cambiar de proveedor y poca visibilidad sobre qué aplicación está
usando cada modelo.

Y provoca un gasto innecesario: quien ya paga una suscripción mensual de ChatGPT
vuelve a pagar por token cada vez que una aplicación de terceros consume la API
con una key.

## Qué es Nexo

Una capa de infraestructura personal que controla cómo las aplicaciones acceden a
los modelos, y que al mismo tiempo funciona como centro de información sobre su
utilización. No es otro chat.

Es un hub en dos dimensiones inseparables:

- **Hub funcional:** conecta aplicaciones, modelos, proveedores, credenciales y
  políticas mediante una interfaz común.
- **Hub de información:** explica qué se está usando, desde dónde, con qué
  frecuencia, con qué rendimiento y con qué consumo o coste.

La referencia conceptual es [Msty Nexus](https://msty.ai/products/nexus/), pero
Nexo es un proyecto independiente y más abierto.

## Los dos tipos de credencial

Nexo trata el tipo de credencial como una dimensión de primer nivel, al mismo
nivel que el proveedor. La misma cuenta del mismo proveedor ofrece catálogos,
capacidades, límites y contabilidad **distintos** según cómo se haya autenticado.

| | **API key** | **OAuth de suscripción** |
| --- | --- | --- |
| Estabilidad | API pública y documentada | Flujo no soportado, puede romperse sin aviso |
| Coste | Por token, facturado aparte | Cubierto por el plan, sin coste marginal |
| Catálogo | Completo | Subconjunto de modelos, con capacidades recortadas |
| Métricas de uso | Tokens reportados | Tokens reportados; cuota consumida no expuesta |
| Límites | Rate limits documentados | Solo se descubren al recibir un `429` |
| Riesgo para la cuenta | Ninguno | Posible incumplimiento de las condiciones del servicio |

Ninguna de las dos vías sustituye a la otra. Cada adaptador implementa las que
soporte y declara explícitamente qué ofrece por cada una.

Las consecuencias de adoptar la vía de suscripción, y las reglas que Nexo se
impone al hacerlo, están en el [ADR 0001](adr/0001-oauth-de-suscripcion.md). En
resumen: se reutiliza el flujo OAuth del cliente oficial del proveedor, se avisa
al usuario antes del primer login, se exige límite por aplicación, y siguen
prohibidos el scraping, las cookies del navegador, la lectura de credenciales de
otras aplicaciones instaladas y la suplantación de otro cliente.

## Proveedores contemplados

**OpenAI y ChatGPT** es el proveedor prioritario, con las dos vías. La de
suscripción habla el formato **Responses** contra el backend de la aplicación de
ChatGPT, no la API pública, así que la traducción de formatos es el caso base del
producto y no una excepción. Está validada contra una cuenta real: ver la
sección de validación del [ADR 0001](adr/0001-oauth-de-suscripcion.md).

**Google y Gemini** entra con OAuth para la Gemini API cuando el usuario tenga un
proyecto de Google Cloud. La autorización de la API y una suscripción de la
aplicación Gemini son cosas distintas: el diseño no asume que una suscripción de
consumidor se convierta en cuota de API.

**Anthropic y Claude** queda como proveedor futuro con las dos vías. Su flujo de
suscripción necesita una investigación propia desde cero y no se deriva del de
OpenAI. Se incluye explícitamente porque varias herramientas de desarrollo
relevantes hablan el formato nativo de Anthropic y no el de OpenAI, lo que afecta
a la superficie que Nexo debe exponer.

**Modelos locales** (Ollama, MLX, llama.cpp) son adaptadores como cualquier otro,
con el mismo catálogo, permisos y métricas, y contabilidad local.

## Características

### Gateway unificado

Una API local compatible con OpenAI en su forma `chat/completions`, porque muchas
herramientas ya permiten configurar una URL base y un token. Una segunda
superficie compatible con el formato nativo de Anthropic queda prevista para
cuando entre ese proveedor.

La representación interna es un **superconjunto** de lo que ofrecen los
proveedores, no el mínimo común denominador. Cuando una aplicación pide una
capacidad que la combinación de proveedor y credencial no soporta, Nexo devuelve
un error explícito. Nunca degrada la petición en silencio: eso produciría
respuestas peores sin que el usuario sepa por qué, y es el fallo que la promesa de
«catálogo unificado» invita a cometer.

### Catálogo de modelos

Indexado por proveedor **y** tipo de credencial, porque el mismo modelo no ofrece
lo mismo por las dos vías. Muestra proveedor, vía, nombre original y normalizado,
capacidades, contexto y límites, modo de contabilidad y disponibilidad.

Las capacidades de un modelo no son descubribles mediante las APIs de los
proveedores: solo se puede consultar qué modelos existen, no qué hacen. Por eso
hay un manifiesto versionado que se distribuye con la aplicación, se cruza con lo
que el proveedor anuncie y admite anulaciones locales.

El nombre público lleva siempre el proveedor delante.

### Gestión de credenciales

En el almacén seguro del sistema operativo: Keychain en macOS, Credential Manager
en Windows, Secret Service en Linux. Nunca en la configuración del proyecto ni en
un fichero JSON con permisos restringidos.

Nexo separa credenciales de proveedores, tokens emitidos a aplicaciones, sesiones
OAuth y configuración no sensible. Cada aplicación recibe un token propio,
revocable y limitado, para no compartir una credencial maestra con todas las
herramientas. Los tokens emitidos se guardan **hasheados**.

### Políticas y permisos

El usuario decide qué aplicaciones pueden usar Nexo, con qué proveedores, vías y
modelos, si pueden enviar contenido multimodal, si pueden usar herramientas, qué
límites se aplican y si sus peticiones se registran.

**Los límites por aplicación son obligatorios en las rutas de suscripción.** No es
una preferencia que se pueda dejar en blanco: sin ellos Nexo convierte una cuenta
personal en un pool de API para cualquier proceso con un token válido, que es el
escenario con más probabilidad de acabar en bloqueo de cuenta.

La configuración inicial es segura: escucha local, acceso LAN desactivado y
aprobación explícita antes de permitir conexiones externas.

### Hub de información

Las estadísticas son una capacidad central, no un añadido. Nexo registra número
de peticiones, aplicación de origen, proveedor, vía y modelo, fecha y duración,
latencia total y tiempo hasta el primer token, resultado y tipo de error, tokens
de entrada y salida, estimación de coste cuando hay precios públicos fiables, y
los límites que el proveedor comunique.

#### Cuatro estados de contabilidad, no dos

Distinguir entre dato y estimación no basta. Cada métrica de coste lleva uno de
estos estados:

- **Reportado:** el proveedor comunicó la cifra. Es un dato.
- **Estimado:** Nexo la calculó a partir de precios públicos. Se presenta siempre
  como estimación.
- **Cubierto por suscripción:** la petición no tiene coste marginal porque el plan
  la cubre. Los tokens sí se conocen, pero el proveedor no expone cuánta cuota se
  ha consumido. Mostrar cero euros aquí es cierto y engañoso a la vez, así que la
  interfaz dice que el coste es cero **y** que el consumo de cuota es desconocido.
- **No disponible:** el proveedor no informa y Nexo no puede estimar con
  fiabilidad. No se inventa una cifra.

Los datos originales del proveedor se conservan tal como llegaron: normalizar
sirve para comparar, perder el original impide auditar.

El panel permite filtrar por periodo, aplicación, proveedor, vía, modelo y tipo de
operación; comparar entre ellos; ver tendencias; identificar los modelos más
usados, más lentos y con más errores; exportar en formatos abiertos; y configurar
retención y borrado.

Por defecto se recogen metadatos operativos, no el contenido de prompts y
respuestas. Las estadísticas no salen del equipo.

## Presencia permanente

Nexo se diseña como aplicación de escritorio multiplataforma (macOS, Windows,
Linux), empezando por macOS. Mientras está en marcha, su icono permanece en el
área de estado del sistema, y sigue funcionando en segundo plano aunque la ventana
principal esté cerrada.

Desde el icono se consulta el estado del gateway y se accede a acciones rápidas:
saber si está activo, ver actividad reciente, consultar el estado de los
proveedores, abrir el panel, pausar o reanudar, y salir.

El icono es punto de acceso rápido e indicador de salud, no sustituto del panel.

## Arquitectura conceptual

1. **Interfaz del gateway:** recibe peticiones y devuelve respuestas compatibles.
2. **Traductor de formatos:** convierte entre la representación interna y el
   formato de cada API, en petición y en stream.
3. **Router:** decide qué adaptador y qué vía atienden cada modelo.
4. **Adaptadores:** encapsulan autenticación, formatos, capacidades y errores de
   cada combinación de proveedor y credencial.
5. **Gestor de identidad:** ejecuta los flujos OAuth, maneja callbacks y renueva.
6. **Almacén seguro:** guarda secretos en el keychain del sistema.
7. **Catálogo:** modelos, capacidades y estado, por proveedor y credencial.
8. **Políticas y límites:** tokens por aplicación, scopes, cuotas y aprobaciones.
9. **Observabilidad local:** recoge y normaliza métricas con controles de privacidad.
10. **Motor de estadísticas:** agrega por tiempo, aplicación, proveedor, credencial y modelo.
11. **Servicio en segundo plano:** mantiene el gateway operativo sin ventana.
12. **Aplicación de escritorio:** panel e integración con la barra de estado.

La lógica de un proveedor no debe filtrarse al resto del sistema: añadir uno
nuevo consiste en implementar su adaptador y describir sus capacidades. Los
valores frágiles de los flujos no oficiales viven aislados en un módulo por
proveedor, para que romperse afecte a un fichero y no a la arquitectura.

El contrato exacto está en [contrato-proveedor.md](contrato-proveedor.md) y el
modelo de datos en [modelo-datos.md](modelo-datos.md).

## Restricciones

- No se obtiene ninguna credencial por una vía distinta a un flujo de
  autorización iniciado desde Nexo y completado conscientemente por el usuario.
- No se reutilizan cookies, sesiones ni almacenamiento del navegador, ni se leen
  los ficheros de credenciales de otras aplicaciones instaladas.
- No se hace scraping ni se automatiza el navegador para simular una sesión.
- No se suplanta la identidad de otro cliente ante el proveedor cuando exista una
  forma de identificarse honestamente.
- No se habilita una ruta de suscripción sin límites por aplicación y sin
  advertencia previa al usuario.
- No se presenta un flujo no soportado como si fuera oficial.
- No se envía la información de una aplicación a otra sin consentimiento.
- No se guarda por defecto el contenido de las conversaciones.
- No se envían las estadísticas fuera del equipo sin consentimiento explícito.
- No se presentan costes, tokens o cuotas estimados como datos confirmados, ni un
  coste cero por suscripción como ausencia de consumo.
- No se habilita acceso por red sin autenticación, autorización y transporte seguro.
- No se presenta la compatibilidad de formatos como equivalencia de capacidades.
- No se degrada en silencio una petición cuya capacidad no soporta el destino.

## Fuentes de contexto

- [Msty Nexus](https://msty.ai/products/nexus/), referencia de producto.
- [opencode](https://github.com/anomalyco/opencode), implementación de referencia
  del OAuth de suscripción; en particular
  [`packages/opencode/src/plugin/openai/codex.ts`](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/plugin/openai/codex.ts).
- [Gemini API: autenticación con OAuth](https://ai.google.dev/gemini-api/docs/oauth).
- [OpenAI: Sign in with ChatGPT](https://help.openai.com/en/articles/20001410-sign-in-with-chatgpt),
  sobre qué concede realmente ese login a aplicaciones externas.
- [Tauri 2: arquitectura](https://v2.tauri.app/es/concept/architecture/) y
  [distribución](https://v2.tauri.app/distribute/).
