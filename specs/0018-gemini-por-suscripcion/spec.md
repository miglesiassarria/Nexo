# 0018 · Gemini por suscripción

- **Estado:** build
- **Creada:** 2026-09-06
- **Pedida por:** el usuario: «quiero usar mi suscripción de Gemini en Nexo para aprovechar los tokens que tengo»

## Problema

Quien ya paga una suscripción elegible de Google puede utilizar Gemini desde la
aplicación oficial, pero hoy no puede ofrecer esa cuota a sus herramientas que
solo saben hablar con la API local de Nexo. La alternativa actual de Gemini por
API key es una vía distinta, con facturación y límites propios, y no aprovecha
la suscripción del usuario.

Nexo necesita representar esta cuenta como una vía de suscripción independiente,
con su propio catálogo, límites, contabilidad y tratamiento de errores. No debe
confundirse con el OAuth oficial de Gemini API, que autoriza un proyecto de
Google Cloud y no implica disponer de la cuota de una suscripción de consumidor.

## Comportamiento esperado

Desde *Proveedores*, el usuario podrá elegir «Gemini por suscripción», aceptar el
aviso de riesgo y completar un login OAuth de Google en el navegador. Nexo
guardará los tokens únicamente en su almacén seguro y mostrará la cuenta como una
vía distinta de «Gemini por API key».

Tras conectarla, Nexo descubrirá los modelos que esa cuenta puede usar mediante
la ruta actual de Antigravity para cuentas personales y los ofrecerá con nombres
de modelo propios de la vía de suscripción. Una aplicación autorizada podrá usarlos mediante la
superficie local OpenAI-compatible de Nexo, con y sin streaming, incluyendo las
capacidades que el catálogo confirme.

La vía tendrá límites obligatorios por aplicación, contabilidad «cubierto por
suscripción» y errores comprensibles cuando caduque la autorización, se agote la
cuota o cambie la ruta no versionada. La ruta de Gemini API key seguirá funcionando
sin cambios.

## Criterios de aceptación

Cada uno con su forma de comprobarse. Sin verificación no es un criterio.

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | La opción aparece como «Gemini por suscripción», separada de «Gemini» por API key, con un resumen y aviso de riesgo específicos. | Test de `connect_options` y prueba de la vista/check de tipos; lectura de la opción devuelta por el núcleo. |
| 2 | El flujo OAuth de Google se inicia desde Nexo con estado y PKCE, recibe el callback local, obtiene access/refresh token y guarda solo referencias al almacén seguro. | Tests unitarios de URL/PKCE, callback y canje; test del servicio con servidor HTTP de prueba que compruebe que ningún token aparece en SQLite. |
| 3 | Tras autenticar, Nexo descubre la identidad, nivel y, cuando Google lo asigna, el proyecto de Code Assist; una cuenta personal no necesita configurar Google Cloud para conectarse. | Test de parsing de `loadCodeAssist`/onboarding con y sin proyecto; test de persistencia y resolución de la cuenta. |
| 4 | El catálogo de la suscripción se sincroniza desde el proveedor, conserva los ids originales, se indexa por proveedor y tipo de credencial y no ofrece modelos que el proveedor no publique como utilizables. | Tests de parsing/catalogación con fixtures y prueba de aislamiento frente al catálogo de Gemini API key. |
| 5 | Una petición `chat/completions` autorizada se traduce al protocolo nativo de Gemini Code Assist, funciona sin streaming y con streaming SSE, y devuelve texto, uso de tokens y finalización correctos. | Tests de contrato con servidor HTTP local simulado; prueba manual o E2E ignorada contra una cuenta real cuando haya credenciales disponibles. |
| 6 | Se traducen herramientas/function calling y razonamiento solo cuando el catálogo y el contrato interno los soportan; una capacidad no soportada produce un `422` explícito y no se elimina silenciosamente. | Tests de request builder, stream parser y rechazo de capacidades; regresión de `cargo test -p nexo-core`. |
| 7 | La renovación concurrente del access token funciona, conserva un refresh token rotado cuando Google lo devuelve y marca la cuenta como expirada con instrucciones claras si falla. | Tests de refresh, rotación y carrera de dos peticiones; test de error de reautenticación. |
| 8 | Las peticiones de esta vía se contabilizan como `Subscription`, exigen límite por aplicación y no se presentan como coste cero sin indicar que la cuota consumida es desconocida. | Tests de políticas/estadísticas y prueba de integración del registro de uso. |
| 9 | La desconexión elimina los secretos de Google, la cuenta y sus modelos/permisos asociados, sin afectar a Gemini API key ni a ChatGPT. | Test de desconexión y regresión del catálogo/cuentas restantes. |
| 10 | La ruta frágil queda aislada en módulos de autenticación y transporte propios, se identifica honestamente como Nexo y no importa cookies, tokens de otros programas ni perfiles del navegador. | Revisión de estructura/código, tests de cabeceras y búsqueda automatizada de accesos prohibidos; inspección del diff. |

## Fuera de alcance

Obligatorio y no puede estar vacío. Lo que alguien podría esperar de esto y
deliberadamente no se va a hacer, con el motivo.

- La OAuth oficial de Gemini API con proyecto de Google Cloud y facturación por API.
- Importar credenciales, cookies, perfiles o ficheros de Gemini CLI/Antigravity u otras aplicaciones instaladas.
- Automatizar el navegador o raspar la aplicación web de Gemini.
- Reproducir el prompt de sistema o la identidad de un cliente oficial. Se usa
  únicamente la cabecera técnica mínima que el backend de Antigravity exige.
- Mostrar la cuota restante si el proveedor no la expone.
- Implementar la superficie nativa de Anthropic, embeddings, generación de imagen,
  vídeo, audio o cualquier modelo que el catálogo no declare utilizable para chat.
- Revocar desde Nexo el consentimiento OAuth en la cuenta Google; se mantiene como
  deuda igual que en la vía actual de ChatGPT.

## Supuestos asumidos

Lo que se ha decidido sin preguntar, o lo que se asumió al no recibir respuesta.
Ponerlos por escrito permite descubrir pronto que uno era falso.

- «Mi suscripción» significa una cuenta Google personal elegible para Antigravity,
  como Google AI Pro o Ultra; una licencia de Workspace o un plan
  distinto puede no ofrecer esta cuota y se debe mostrar como incompatibilidad
  clara, no como fallo genérico.
- El proveedor seguirá aceptando únicamente el acceso que conceda explícitamente
  el usuario mediante OAuth iniciado desde Nexo.
- La ruta de Antigravity continuará siendo no oficial/no versionada para una
  aplicación de terceros y puede dejar de funcionar sin aviso.
- Las cuentas personales Pro/Ultra no deben necesitar un proyecto de Google Cloud;
  si Google responde con BYOP o exige una licencia empresarial, Nexo lo mostrará
  como incompatibilidad y no activará facturación automáticamente.
- El nombre técnico estable será `gemini_subscription` y el nombre visible será
  «Gemini por suscripción», para que sus modelos nunca colisionen con los de
  `gemini` por API key.
- El respaldo automático a Gemini API key solo se habilitará si el usuario tiene
  esa vía configurada y el modelo solicitado existe también allí; nunca ocultará
  un error de capacidad o de autorización de la suscripción.

## Riesgos

Qué puede hacer que esto no funcione, o que funcione hoy y no mañana.

- Google cambia el client id, scopes, endpoints `v1internal`, cabeceras o formato
  de respuesta; se detectará con tests de fixtures, errores tipados y E2E manual.
- La cuenta no tiene nivel o proyecto compatible; la conexión debe terminar con
  una explicación accionable y no dejar una cuenta parcialmente activa.
- El endpoint de cuota y el endpoint de inferencia usan proyectos o hosts
  distintos; el adaptador debe usar el proyecto que devuelva el onboarding y no
  asumir que el catálogo implica cuota disponible.
- El protocolo nativo no cubre alguna combinación de herramientas, razonamiento o
  multimodalidad; se detectará mediante validación previa y respuestas `422`.
- Las cuotas de la suscripción se consumen antes de que Nexo pueda observarlas;
  los límites por aplicación reducen el riesgo, pero no lo eliminan.
- La implementación de referencia puede usar rutas o credenciales que no sean
  necesarias o apropiadas para Nexo; se copiará el comportamiento comprobado, no
  sus ficheros de credenciales ni su suplantación de cliente.

## Invariantes que esto no puede romper

Las de `CLAUDE.md` que aplican a este caso, nombradas explícitamente.

- Se conserva el almacén seguro y nunca se guardan secretos en SQLite ni en texto
  plano.
- Se mantiene la separación proveedor + tipo de credencial en cuentas, catálogo,
  permisos, límites y estadísticas.
- Se mantiene el límite obligatorio por aplicación para toda vía de suscripción.
- No se degrada en silencio ninguna capacidad solicitada.
- Se conservan los ids y datos originales publicados por Google.
- Los valores frágiles quedan aislados y fechados en módulos específicos de Google.
- Nexo no importa credenciales de Antigravity ni perfiles locales; usa únicamente
  el perfil HTTP técnico mínimo que exige su backend público y se presenta como
  Nexo ante sus aplicaciones.
- El contenido de las conversaciones no se guarda por defecto.
