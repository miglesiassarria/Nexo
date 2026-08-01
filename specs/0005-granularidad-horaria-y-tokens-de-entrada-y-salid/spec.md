# 0005 · Periodos por horas y tokens de entrada/salida en el panel

- **Estado:** build
- **Creada:** 2026-08-01
- **Pedida por:** el usuario, tras revisar el panel («Panel») y comprobar en la
  base de datos real que «Últimas peticiones» no filtra por tiempo y que la
  columna «Tokens» mezcla entrada y salida: «podríamos poner que filtre por 1, 2,
  5 y 12 horas además de las opciones que ya aparecen? podemos separar la
  información en tokens de entrada y de salida?»

## Problema

El selector de periodo del Panel («24 horas / 7 días / 30 días / 90 días») no
baja del día completo, así que no sirve para mirar lo que ha pasado en la última
hora o en las últimas horas — justo la ventana más útil cuando se está probando
una integración nueva ahora mismo. Además ese selector solo afecta a «Uso
agregado»: «Últimas peticiones» no tiene filtro de tiempo propio, siempre
enseña las últimas N filas sin importar su antigüedad, así que los dos paneles
pueden mostrar ventanas distintas sin que se note.

Por separado, la columna «Tokens» de ambos paneles junta entrada y salida en un
solo número. Para entender cuánto cuesta un prompt largo frente a una respuesta
larga —o para detectar que una aplicación está mandando un contexto enorme por
un mensaje trivial— hace falta verlos por separado; hoy no se puede sin ir a la
base de datos a mano.

## Comportamiento esperado

- El selector de periodo del Panel ofrece, además de las opciones actuales, 1,
  2, 5 y 12 horas.
- Cambiar el periodo filtra **los dos paneles a la vez** («Uso agregado» y
  «Últimas peticiones») a la misma ventana de tiempo. Hoy solo el primero
  reacciona al selector.
- En «Últimas peticiones», cada fila muestra los tokens de entrada y de salida
  de esa petición en dos columnas, en vez de un único número combinado.
- En «Uso agregado», la fila de cada vía/aplicación/proveedor/modelo muestra
  también entrada y salida por separado.
- Cuando el proveedor no informó del consumo de una petición, entrada y salida
  se muestran cada una como «no disponible» — nunca como cero. Es el mismo
  principio que ya aplica hoy al número combinado.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | El selector de periodo incluye 1, 2, 5 y 12 horas junto a las opciones ya existentes | Lectura de `Dashboard.svelte`; `npm run check` |
| 2 | El backend acepta ventanas por debajo de un día sin redondear a día completo (una petición de hace 90 minutos no aparece en un periodo de 1 hora) | `cargo test -p nexo-core -- usage_summary` con un caso de ventana de 1h que excluye una fila de hace 2h |
| 3 | «Últimas peticiones» respeta la misma ventana de tiempo elegida en el selector — una fila anterior a la ventana no aparece en esa lista | `cargo test -p nexo-core -- recent_requests` con una fila insertada fuera de ventana |
| 4 | «Últimas peticiones» muestra entrada y salida en columnas separadas para cada fila | Lectura de `Dashboard.svelte` y de `RequestRow` en `src/lib/api.ts`; `npm run check` |
| 5 | «Uso agregado» muestra entrada y salida en columnas separadas por fila de la tabla | Lectura de `Dashboard.svelte`; `npm run check` (el dato ya existe en `UsageBucket`, no requiere cambio de backend) |
| 6 | Una petición con `usage_source = unavailable` muestra «no disponible» en ambas columnas, no `0` | `cargo test -p nexo-core` sobre el formateo/consulta con una fila de tokens `NULL` |
| 7 | El filtro por ventana de tiempo sigue usando el índice existente de `ts`, sin necesidad de uno nuevo | Lectura de `migrations.rs`: `idx_requests_ts` ya cubre la consulta |

## Fuera de alcance

- **El bug de filas duplicadas** encontrado al investigar esta spec (2-3 filas
  por una misma petición real a LM Studio y OpenCode Zen, con el mismo `ts` y
  `provider_request_id`). Es un fallo de fiabilidad de datos, no de este panel;
  se tratará aparte con su propia prueba de reproducción, según exige
  `CLAUDE.md` para arreglos de fallo.
- **Rango de fechas personalizado o calendario.** Solo se añaden los presets de
  horas pedidos (1, 2, 5, 12); no se construye un selector de fecha/hora libre.
- **Paginación de «Últimas peticiones».** El tope de filas que ya existe
  (`LIMIT`, hoy 40) se mantiene igual; la ventana de tiempo se añade como
  condición adicional, no lo sustituye. Si en una ventana amplia hay más de 40
  peticiones, se sigue viendo solo un vistazo, no el listado completo.
- **Desglosar `cached_input_tokens` o `reasoning_tokens` como columnas propias.**
  Siguen contando dentro de entrada/salida tal como cada proveedor ya los
  incluye en `input_tokens`/`output_tokens`; no se añade una tercera y cuarta
  columna para ese detalle.
- **Cambiar cómo se calcula o estima el coste.** Esta spec toca cómo se muestran
  los tokens y el periodo, no la contabilidad de coste, que sigue las mismas
  cuatro bases de siempre.

## Supuestos asumidos

- El selector de periodo pasa a ser uno solo, compartido por los dos paneles —
  es lo que se pidió explícitamente («ese mismo selector») y evita que ambos
  paneles cuenten historias distintas de la misma ventana.
- En «Últimas peticiones» la columna combinada «Tokens» se sustituye por
  «Entrada» y «Salida»: mostrar tres columnas (entrada, salida y total) sería
  redundante, ya que el total es su suma y el objetivo pedido es precisamente
  distinguirlos. En «Uso agregado» se decide en `/design` si el total se
  conserva junto a las dos columnas nuevas, porque ahí sirve para comparar el
  tamaño relativo entre vías/aplicaciones de un vistazo.
- Las nuevas opciones de hora conviven con «24 horas» sin sustituirla; el orden
  en el selector va de menor a mayor ventana (1h, 2h, 5h, 12h, 24h, 7d, 30d,
  90d).
- El parámetro de ventana pasa de «días» a una unidad más fina (minutos u
  horas) en el contrato entre `src-tauri` y `crates/nexo-core`; la decisión
  exacta del tipo (minutos enteros, horas fraccionarias, timestamp `since_ms`)
  se toma en `/design`.

## Riesgos

- Cambiar la unidad del parámetro de ventana (`days` → algo más fino) es un
  cambio de contrato entre `src-tauri/src/commands.rs` y
  `crates/nexo-core/src/service.rs` / `db/stats.rs`. Si un lado se actualiza y
  el otro no, el workspace no compila — lo detecta `cargo check`/`npm run
  check` antes de llegar a build, no es un riesgo silencioso.
- Si en el futuro se añade un proveedor que informe un total de tokens sin
  desglose entrada/salida, esta funcionalidad tendría que degradar esa fila a
  «no disponible» en ambas columnas en vez de inventar un reparto. Hoy no
  ocurre con ningún proveedor real verificado (comprobado contra la base de
  datos de producción: `input_tokens` y `output_tokens` están siempre los dos
  presentes o los dos ausentes), pero un adaptador nuevo que rompa esa pareja
  rompería la promesa del criterio 6 si no se revisa al añadirlo.

## Invariantes que esto no puede romper

- **3. Cuatro estados de contabilidad, no dos.** Separar entrada y salida no
  puede convertir un dato «no disponible» en un `0` disponible; cada columna
  nueva respeta el mismo estado que hoy tiene el total combinado.
- **6. Se conserva el dato original del proveedor.** Las columnas nuevas se
  pintan a partir de `input_tokens`/`output_tokens` ya normalizados; no se
  toca ni se sustituye `provider_usage_raw`.
