# 0019 · Actualizar automáticamente el catálogo de ChatGPT por suscripción

- **Estado:** build
- **Creada:** 2026-09-27
- **Pedida por:** el usuario: «cuando le dé a refrescar, que Nexo haga todo lo necesario para mostrar los modelos presentes y más modernos sin que tenga que tocar una versión»; después pidió implementación, PR y merge.

## Problema

Nexo consulta el catálogo de ChatGPT por suscripción declarando una versión de
cliente fija (`0.144.0`). El endpoint devuelve modelos según esa versión: la
consulta autenticada de la cuenta conectada devolvió cuatro modelos con
`0.144.0`, mientras que `0.155.0` y `99.0.0` devolvieron siete e incluyeron
`gpt-6-sol` y `gpt-6-luna`. El selector de Configuración expone hoy ese detalle
técnico y el usuario debe saber que existe para obtener modelos nuevos.

## Comportamiento esperado

El usuario podrá actualizar el catálogo desde el botón existente en Modelos y
Nexo descubrirá automáticamente los modelos actuales permitidos a su cuenta,
sin que tenga que editar una versión técnica. Si la consulta no puede obtener un
catálogo válido, el catálogo anterior seguirá disponible y la interfaz
informará del error.

## Criterios de aceptación

Cada uno con su forma de comprobarse. Sin verificación no es un criterio.

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | El adaptador prueba primero la versión de catálogo más reciente verificada (`99.0.0`) y descubre modelos posteriores a la lista local sin un cambio de Nexo. | Prueba del adaptador contra servidor HTTP simulado; consulta autenticada ya observada: `99.0.0` incluye GPT‑6 Sol/Luna. |
| 2 | Si la versión más reciente no es aceptada o entrega un catálogo vacío, Nexo prueba versiones alternativas en orden y termina en la versión guardada existente, sin repetir intentos en errores de autenticación o cuota. | Pruebas HTTP simuladas para 400/lista vacía, 401, 429 y alternativa exitosa. |
| 3 | Solo se guardan modelos visibles y admitidos por API; si no se obtiene ningún catálogo válido, se conserva el anterior. | Regresión de `parse_models` y prueba del servicio/catalog refresh que comprueba que no sustituye catálogo en error. |
| 4 | El botón existente refresca con negociación automática y la interfaz ya no exige que el usuario conozca o edite la versión de cliente. | `npm run check`; inspección de la pantalla Modelos y Configuración. |
| 5 | Nexo sigue identificándose como Nexo, no guarda tokens nuevos ni cambia el flujo de inferencia. | Tests de cabeceras y revisión del diff; la negociación solo afecta al GET del catálogo. |

## Fuera de alcance

Obligatorio y no puede estar vacío. Lo que alguien podría esperar de esto y
deliberadamente no se va a hacer, con el motivo.

- Añadir otro botón de refresco: ya existe uno en Modelos y basta con mejorar su comportamiento.
- Refrescar modelos automáticamente en cada consulta de chat: añade tráfico al endpoint interno y no hace falta para la actualización manual solicitada.
- Cambiar la traducción de inferencia, permisos por aplicación, límites o contabilidad de suscripción.

## Supuestos asumidos

Lo que se ha decidido sin preguntar, o lo que se asumió al no recibir respuesta.
Ponerlos por escrito permite descubrir pronto que uno era falso.

- `99.0.0` devuelve el catálogo más actual al que tiene derecho la cuenta mientras el endpoint conserve el comportamiento verificado el 2026-09-27.
- `0.155.0` es una alternativa verificada para la familia GPT‑6; la configuración previamente guardada queda como último fallback de compatibilidad.
- El endpoint es interno y no versionado. Errores de autenticación o rate limit no se deben reintentar con varias versiones.

## Riesgos

Qué puede hacer que esto no funcione, o que funcione hoy y no mañana.

- OpenAI puede dejar de aceptar el endpoint o cambiar su esquema; Nexo conserva el catálogo anterior y deja visible el error de refresco.
- El primer catálogo válido puede incluir modelos cuya generación aún no se ha probado desde Nexo; el GET solo demuestra disponibilidad anunciada, no una inferencia exitosa.

## Invariantes que esto no puede romper

Las de `CLAUDE.md` que aplican a este caso, nombradas explícitamente.

- Se mantiene el ADR 0001: ruta no soportada, acceso mediante OAuth autorizado por el usuario y Nexo se identifica como Nexo.
- Se respeta `docs/contrato-proveedor.md`: solo se ofrecen modelos que el proveedor publique visibles y compatibles con API; no se degradan capacidades silenciosamente.
- Los errores del refresco no eliminan el catálogo válido ya persistido.
