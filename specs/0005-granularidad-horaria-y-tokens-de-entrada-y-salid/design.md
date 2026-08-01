# 0005 · Diseño

## Enfoque

El parámetro de ventana pasa de `days: i64` a `minutes: i64` en los dos comandos
Tauri del panel (`usage_summary`, `recent_requests`), que siguen calculando
`since_ms` en el servidor como hoy. `recent_requests()` en la capa de datos
gana un `WHERE ts >= ?` que no tenía. `usage_summary()` mantiene su firma
(`since_ms: i64`) pero, para ventanas de un día o menos, deja de leer el rollup
`usage_hourly` — cuya granularidad es la hora completa y no puede darle una
respuesta exacta a un filtro de «1 hora» — y consulta `requests` directamente,
que sí tiene el timestamp exacto de cada petición. Para ventanas mayores sigue
usando `usage_hourly` sin cambios, porque ese rollup es lo que permite que las
tendencias largas sobrevivan al borrado por retención del detalle. En el
frontend, ambas tablas sustituyen su columna «Tokens» combinada por «Entrada» y
«Salida», reutilizando el `usage_source` de cada fila para marcar «no
disponible»/«estimado» igual que ya hace hoy la columna que sustituyen.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `src-tauri/src/commands.rs` | `usage_summary` y `recent_requests` pasan a recibir `minutes: i64` en vez de `days`/nada; `DAY_MS` se sustituye por `MINUTE_MS = 60_000`. `recent_requests` calcula `since` igual que `usage_summary` y lo pasa a la capa de datos. |
| `crates/nexo-core/src/db/stats.rs` | `recent_requests(&self, since_ms: i64, limit: i64)`: nuevo parámetro y `WHERE r.ts >= ?1`, y el `SELECT` incorpora `r.input_tokens, r.output_tokens`. `RequestRow` gana esos dos campos. `usage_summary`: bifurca por tamaño de ventana entre la consulta actual sobre `usage_hourly` y una nueva sobre `requests` con la misma forma de fila (mismo orden de columnas, mismo cierre de mapeo). |
| `crates/nexo-core/src/service.rs` | Actualizar las 3 llamadas a `db.recent_requests(N)` de los tests (líneas ~1870, 1878, 2192) a `db.recent_requests(0, N)`. |
| `crates/nexo-core/tests/gateway_e2e.rs` | Actualizar las 9 llamadas a `.recent_requests(N)` a `.recent_requests(0, N)`. |
| `src/lib/api.ts` | `usageSummary(minutes, group, operation)`; `recentRequests(limit, minutes)`; `RequestRow` gana `input_tokens: number | null` y `output_tokens: number | null`. |
| `src/lib/views/Dashboard.svelte` | Estado `days` → `minutes` (valores en minutos); el `<select>` de periodo gana las opciones de 1/2/5/12 horas; las dos tablas cambian su columna «Tokens» por «Entrada» y «Salida». |

## Decisiones

### D1. Bifurcar `usage_summary` por tamaño de ventana en vez de tocar `usage_hourly`

- **Decisión:** si la ventana pedida es de 24 horas o menos, `usage_summary`
  consulta `requests` directamente (exacta); si es mayor, sigue consultando
  `usage_hourly` exactamente como hoy.
- **Alternativa descartada:** dar a `usage_hourly` granularidad de minuto —
  significaría una migración nueva, más escritura por petición (dos rollups en
  vez de uno) y una tabla más que mantener, para un beneficio que solo hace
  falta en 4 de los 8 periodos del selector. Se descarta por sobrar para lo que
  se pide.
- **Alternativa descartada:** dejar que todos los periodos, incluidos los de
  horas, sigan usando `usage_hourly` con su redondeo a la hora. Se descarta
  porque incumple el criterio 2 de la especificación: con ese redondeo, un
  filtro de «1 hora» puede seguir enseñando una petición de hace casi 2 horas
  si cae en el mismo cubo horario — el filtro deja de significar lo que dice.
- **Alternativa descartada:** que todos los periodos, incluidos 30/90 días,
  consulten `requests` directamente. Se descarta porque rompe la razón de ser
  de `usage_hourly`: sobrevivir al borrado por retención del detalle (ver
  `docs/producto.md` y el texto de `Settings.svelte`). Un periodo de 90 días
  después de que la retención haya borrado el detalle de hace 40 días
  devolvería un total incompleto sin avisar — justo el «degradar en silencio»
  que `CLAUDE.md` prohíbe.
- **Consecuencia que hay que asumir:** dos consultas SQL distintas para la
  misma función pública, elegidas por una condición (`ahora - since_ms <=
  86_400_000`). Se documenta en el propio código por qué existen las dos, para
  que quien lo lea dentro de un año no las junte «para simplificar» y
  reintroduzca el problema que esto resuelve.

### D2. `minutes: i64` como unidad en el límite Tauri, no `since_ms` calculado en el cliente

- **Decisión:** los comandos `usage_summary` y `recent_requests` siguen
  calculando `since` a partir de `util::now_ms()` en el servidor, a partir de
  un entero en minutos que manda el frontend.
- **Alternativa descartada:** que el frontend calcule `Date.now() - ventana` y
  mande un timestamp absoluto a los dos comandos. Se descarta porque son dos
  llamadas asíncronas independientes (`usageSummary` y `recentRequests`); si el
  frontend calculase el `since` una vez y lo repartiera, un pequeño desfase
  entre cuándo se calculó y cuándo se ejecuta cada llamada no importaría mucho,
  pero mantener el patrón ya existente (el servidor calcula `since` a partir de
  su propio reloj) evita depender del reloj del sistema del cliente, que en
  Tauri es el mismo proceso pero no hay necesidad de romper la convención que
  ya sigue `usage_summary` hoy.
- **Consecuencia que hay que asumir:** un solo `<select>` en `Dashboard.svelte`
  cuyo valor (en minutos) se pasa igual a las dos llamadas — si en el futuro se
  desincroniza una de las dos, se nota porque un panel mostraría más o menos
  peticiones que el otro para el mismo periodo.

### D3. Sustituir la columna «Tokens» por «Entrada»/«Salida» en ambas tablas, no añadirlas aparte

- **Decisión:** en «Uso agregado» y en «Últimas peticiones», la columna
  «Tokens» se sustituye por dos columnas nuevas. No se conserva una tercera
  columna con el total en ninguna de las dos tablas.
- **Alternativa descartada:** mantener «Tokens» y añadir «Entrada»/«Salida»
  como columnas extra (tres en total). Se descarta por ruido visual — el total
  es la suma de las otras dos, y las dos tablas ya son anchas (7 y 9 columnas
  respectivamente, con scroll horizontal). El total agregado no se pierde de
  la vista: la tarjeta superior «Tokens — entrada + salida» sigue mostrándolo.
- **Consecuencia que hay que asumir:** quien quiera el total de una fila
  concreta tiene que sumar las dos columnas mentalmente; se acepta porque el
  problema que se pidió resolver es justo verlas por separado.

### D4. Reutilizar el `usage_source` de la fila para marcar «no disponible» en las dos columnas nuevas

- **Decisión:** en «Últimas peticiones», las celdas «Entrada» y «Salida» usan
  la misma insignia (`≈` si `usage_source = estimated`, `?` con el tooltip «El
  proveedor no informa» si `unavailable`) que hoy usa la celda «Tokens», una
  vez por columna.
- **Alternativa descartada:** un estado de disponibilidad independiente por
  columna. Se descarta porque, comprobado contra la base de datos real, las 27
  peticiones registradas nunca tienen una columna presente y la otra ausente —
  `usage_source` ya describe la fila entera, no cada número por separado.
- **Consecuencia que hay que asumir:** si algún día aparece un proveedor que
  informe entrada pero no salida (o viceversa), esta decisión se queda corta y
  hay que revisarla — está anotado como riesgo en `spec.md`.

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| Cambiar la firma de `recent_requests` en `db/stats.rs` deja de compilar los ~12 sitios que hoy la llaman con un solo argumento (`service.rs`, `gateway_e2e.rs`, `commands.rs`) | `cargo check`/`cargo test --workspace` falla inmediatamente al compilar, no en tiempo de ejecución |
| Cambiar `usage_summary` de `days` a `minutes` en el comando Tauri sin actualizar `Dashboard.svelte` deja el selector mandando un número que el backend interpreta como minutos en vez de días (una selección de «7 días» pasaría a filtrar 7 minutos) | `npm run check` no lo detecta por sí solo (es un cambio de semántica, no de tipos); se cubre con una prueba manual explícita en `/build`: elegir «7 días» y comprobar que sigue trayendo la misma ventana que antes de este cambio |
| La bifurcación de `usage_summary` por tamaño de ventana, si el umbral se calcula mal (por ejemplo comparando `since_ms` con `minutes` en unidades distintas), podría hacer que un periodo de 24 horas exactas caiga en la rama equivocada | Prueba unitaria en `stats.rs` con una ventana de exactamente 24h y una de 24h+1min, comprobando qué tabla se consultó (indirectamente, con datos solo en una de las dos fuentes) |
| El `GroupBy::Hour` (no expuesto en la UI hoy) no tiene columna `hour` en `requests`; si algo lo usa contra la rama nueva sin la expresión equivalente, rompe en tiempo de ejecución con un error de SQL sobre columna inexistente | `cargo test -p nexo-core` con un caso que ejercite `GroupBy::Hour` contra una ventana corta |

## ¿Hace falta un ADR?

No. Esto no cambia ninguna decisión de arquitectura de `docs/adr/`: es una
extensión del contrato interno ya existente entre `src-tauri` y
`crates/nexo-core` (más granularidad en un parámetro que ya existía, más
columnas en una fila que ya se devolvía), no introduce un concepto nuevo del
producto ni toca ninguna de las invariantes de `CLAUDE.md` más allá de
respetarlas.

## Qué queda pendiente de descubrir

- Si el redondeo a minutos (en vez de segundos) del parámetro `minutes: i64`
  es suficientemente fino para el caso de uso real, o si alguien pedirá luego
  «últimos 15 minutos» — hoy no está en la lista pedida, pero la unidad
  elegida ya lo soportaría sin otro cambio de contrato.
- Cómo se ve en la práctica la tabla de «Últimas peticiones» con 9 columnas
  (una más que hoy) en una ventana estrecha — puede que en `/build` haga falta
  ajustar qué columnas se ocultan primero en `.scroll-x`.
