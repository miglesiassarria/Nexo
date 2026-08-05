# 0001 · Usar los modelos de LM Studio desde Nexo

- **Estado:** hecho
- **Creada:** 2026-07-31
- **Pedida por:** Manuel Iglesias — «poder gestionar a través de Nexo la conexión
  con los modelos de LM Studio y de Ollama».

## Problema

Nexo sirve hoy dos vías de OpenAI, las dos en la nube. El usuario tiene modelos
corriendo en su propia máquina en LM Studio, y para usarlos desde una herramienta
tiene que configurar `http://127.0.0.1:1234/v1` en cada una, por separado,
saltándose Nexo. Eso rompe las dos promesas del producto a la vez:

- **Un único punto de acceso:** vuelve a haber una URL distinta por proveedor en
  cada aplicación.
- **Un único centro de información:** el uso local no aparece en el panel, así que
  no se puede comparar lo que se gasta en la nube con lo que se resuelve en casa.

Hay además un motivo de privacidad que hoy no está cubierto: cuando el usuario no
quiere que un contenido salga de su equipo, no tiene forma de dirigirlo a un modelo
local **desde la misma integración** que usa para todo lo demás.

## Comportamiento esperado

1. El usuario abre *Proveedores* y ve **LM Studio** con su estado: si el servidor
   responde, en qué dirección, y cuántos modelos hay disponibles y cuántos cargados.
2. Si LM Studio está en marcha en el puerto por defecto, Nexo lo detecta sin que el
   usuario configure nada. Si usa otro puerto, puede cambiar la dirección.
3. En *Modelos* aparecen los modelos locales junto a los de la nube, marcados como
   vía **Local**, con su contexto, su cuantización y si están cargados.
4. El usuario concede acceso a una aplicación igual que con cualquier otra vía, y
   apunta su herramienta a Nexo con el nombre `lmstudio/<modelo>`.
5. Las peticiones locales aparecen en el panel con su latencia y sus tokens, y su
   coste se presenta como local, distinguible de lo cubierto por suscripción y de
   lo facturado por token.
6. Si una aplicación pide chat a un modelo que solo hace embeddings, recibe un
   error explícito que nombra el motivo, no una respuesta vacía ni un fallo del
   proveedor.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | El catálogo se descubre del servidor, no de una lista escrita a mano, con contexto y cuantización por modelo | `cargo test -p nexo-core lmstudio`, sobre una respuesta real capturada de `/api/v0/models` |
| 2 | Un modelo `vlm` se declara con visión; uno `llm`, sin ella | mismo test: comprueba `caps.vision` según el campo `type` |
| 3 | Un modelo de embeddings se lista pero rechaza chat con `422` nombrando la capacidad | test de contrato: falla con `Unsupported { capability: "text" }` |
| 4 | Con LM Studio en marcha, `GET /v1/models` incluye los modelos locales con `credential_kind: "local"` | `curl` contra un Nexo real y comprobación del bloque `nexo` |
| 5 | Una petición de chat local responde y queda registrada con latencia, tokens y tiempo al primer token | `curl` real y consulta de `recent_requests`; el evento tiene `credential_kind = "local"` |
| 6 | El streaming funciona: los chunks reensamblados dan el mismo texto que la respuesta no-streaming | prueba de extremo a extremo, como la que ya existe para el mock |
| 7 | Si LM Studio no está en marcha, el estado lo dice y las peticiones fallan con un error que explica qué hacer, no con un `502` genérico | apagar LM Studio y comprobar el cuerpo del error y el panel |
| 8 | El coste de una petición local no se suma ni al estimado ni al de suscripción | `usage_summary` agrupado por vía: la fila local no aporta coste estimado |
| 9 | La vía local **no** exige límite por aplicación | conceder acceso sin límite y comprobar que la petición pasa |
| 10 | El repositorio sigue verde | `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check` |

## Fuera de alcance

- **Ollama.** Se pidió, pero no está instalado en la máquina y no se puede verificar
  contra la realidad. Va en una especificación aparte, cuando esté en marcha. Hoy ya
  han fallado tres suposiciones de diseño por no poder probarlas; no se añade una
  cuarta a ciegas.
- **`/v1/embeddings`.** Los modelos de embeddings se listan y se rechazan para chat,
  pero Nexo no expone todavía esa superficie. Sería una ruta nueva con su propia
  contabilidad y estadísticas.
- **Cargar y descargar modelos desde Nexo.** LM Studio ya los carga al recibir la
  primera petición. Administrar otro programa es un producto distinto del de hablar
  con él.
- **MLX y llama.cpp directos.** LM Studio ya ejecuta MLX y GGUF por debajo; ir por
  detrás de él no aporta nada hoy.
- **Enrutado automático a local por privacidad.** Que el usuario diga «esto no sale
  de mi equipo» y Nexo elija destino es una regla de enrutado, y las reglas de
  enrutado son otra especificación.
- **Respaldo automático de la nube a local.** El respaldo actual va de suscripción a
  API key porque son el mismo modelo; caer a un modelo local distinto cambiaría la
  respuesta sin avisar, y eso se parece demasiado a degradar en silencio.

## Supuestos asumidos

- Dirección por defecto `http://127.0.0.1:1234`, detectada al arrancar y editable
  desde *Proveedores*. No se pregunta el puerto: hay un valor bueno por defecto.
- Se usa el endpoint **nativo** `/api/v0/models` para descubrir, porque publica
  `type`, `quantization`, `state` y `max_context_length`, y el `/v1/models`
  compatible no. Para el chat se usa la superficie **compatible con OpenAI**, que es
  estable y que ya sabemos traducir.
- El proveedor se identifica como `lmstudio`, distinto de un futuro `ollama`: son dos
  servidores con endpoints y metadatos distintos, y el usuario puede tener los dos.
- Los nombres públicos serán `lmstudio/<slug>`, respetando la invariante de que el
  proveedor va delante.
- LM Studio no necesita credencial: la vía es `CredentialKind::Local`.

## Riesgos

| Riesgo | Consecuencia |
| --- | --- |
| La primera petición a un modelo `not-loaded` tarda mucho, porque LM Studio lo carga en ese momento | Un cliente con timeout corto lo verá como fallo. Hay que medirlo y documentarlo, no esconderlo |
| `/api/v0/models` es propio de LM Studio y puede cambiar entre versiones | Verificado con la 0.4.20. Si cambia, hay que caer a `/v1/models` con metadatos pobres en lugar de quedarse sin catálogo |
| El puerto 1234 puede estar ocupado por otro programa que también hable OpenAI | La detección debe confirmar que es LM Studio, no solo que algo responde |
| Un modelo local puede declarar más contexto del que la máquina aguanta | Nexo informa de lo que el servidor declara; no promete que quepa en memoria |

## Invariantes que esto no puede romper

- **Nunca degradar en silencio** (nº2): a un modelo de embeddings al que se le pide
  chat se le responde `422`, no se le manda la petición a ver qué pasa.
- **Cuatro estados de contabilidad** (nº3): lo local es coste cero **conocido**, y no
  debe confundirse con lo cubierto por suscripción, donde el coste es cero pero la
  cuota consumida es desconocida.
- **El eje de credencial es de primer nivel** (nº5): `lmstudio` entra como proveedor
  con vía `local`, no como un caso especial fuera del modelo.
- **Añadir un proveedor toca su adaptador, no el núcleo**: si para esto hay que
  modificar el router, el catálogo o las estadísticas, el contrato está mal y se
  arregla el contrato.
- **Solo localhost** (nº9): sin cambios; el destino también es local.
