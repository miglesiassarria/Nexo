# ADR 0002: Tauri 2, Rust y Svelte 5

- **Fecha:** 2026-07-30
- **Estado:** aceptada, pendiente de validación con mediciones
- **Decide:** Manuel Iglesias

## Contexto

Nexo debe permanecer activo durante toda la sesión del usuario, sirviendo tráfico con la ventana principal cerrada. El coste base en reposo es por tanto una restricción de producto, no un detalle de implementación: una herramienta de fondo que consume como un navegador se desinstala.

Al mismo tiempo, el producto necesita una interfaz de escritorio real (panel de estadísticas, catálogo, políticas) y presencia en la barra de estado de tres sistemas operativos.

## Decisión

Núcleo en **Rust**, aplicación de escritorio con **Tauri 2**, interfaz con **Svelte 5**, TypeScript y Vite. Un solo proceso. Gateway HTTP con Axum y Tokio, persistencia con SQLite y Rusqlite, HTTP de salida con Reqwest y Rustls.

Todo lo que el gateway necesita para funcionar vive en Rust. La interfaz solo presenta información y envía acciones. Esto es lo que permite cerrar la ventana sin detener el servicio, y lo que permitiría cambiar la capa de escritorio sin reescribir el motor.

## Objetivos numéricos

La decisión se acepta **condicionada** a que el prototipo de la fase 0 cumpla estos objetivos en macOS. Sin cifras el criterio «consume poco» no es verificable y el ADR no vale nada.

| Métrica | Objetivo | Límite de rechazo |
| --- | --- | --- |
| Memoria residente, en reposo y sin ventana | < 60 MB | 100 MB |
| Memoria residente, con el panel abierto | < 150 MB | 250 MB |
| CPU en reposo, media a 5 minutos | < 0,5 % | 1 % |
| CPU sirviendo un stream | < 5 % | 15 % |
| Arranque en frío hasta aceptar conexiones | < 1,0 s | 2,0 s |
| Reapertura del panel tras haberlo cerrado | < 400 ms | 1,0 s |
| Sobrecoste de latencia añadido por el gateway | < 15 ms p95 | 50 ms p95 |
| Tamaño del instalador de macOS | < 25 MB | 60 MB |

Método de medición: proceso corriendo 5 minutos sin tráfico para el reposo; 100 peticiones en streaming contra el proveedor mock para el tráfico; tres repeticiones de cada medición, se toma la peor. Las mediciones se anotan en este ADR al cerrar la fase 0.

## Alternativas descartadas

- **Electron.** Desarrollo más rápido en JavaScript, pero incorpora Chromium y Node.js y utiliza varios procesos. Su coste base en reposo es incompatible con los objetivos anteriores.
- **Flutter.** Buen soporte multiplataforma, pero distribuye su propio motor gráfico y runtime. Sus ventajas están en interfaces visuales complejas, no en un servicio que trabaja en segundo plano.
- **Tres aplicaciones nativas.** Integración óptima con cada sistema a cambio de mantener tres implementaciones.
- **Reutilizar LiteLLM u otro gateway existente.** Resolvería enrutado, adaptadores y contabilidad de tokens con código ya probado, y sería con diferencia la vía más rápida al producto. Se descarta porque obligaría a distribuir un runtime de Python como proceso adicional, lo que contradice frontalmente los objetivos de esta tabla. **Esta es la alternativa con el argumento más fuerte en contra de la decisión tomada**, y su descarte depende por completo de que la restricción de consumo sea real. Si al medir resulta que el coste base no aprieta tanto como se supone, esta opción merece reconsiderarse antes de escribir los adaptadores.

## Criterios de reevaluación

- Si el prototipo no alcanza los objetivos: se reevalúa **la capa de escritorio**, no el núcleo Rust. El gateway, los adaptadores y las estadísticas siguen siendo válidos con otra carcasa.
- Si el WebView de alguna plataforma da problemas de compatibilidad graves con Svelte 5, se considera simplificar la interfaz antes que cambiar de framework.
- Si la validación de la fase 0 revela que el consumo base es irrelevante para el uso real, se reabre la comparación con gateways existentes.

## Mediciones

Primera medición, 2026-07-31, macOS 26.5 en Apple Silicon, binario `--release`.

| Métrica | Objetivo | Medido | |
| --- | --- | --- | --- |
| Memoria residente, con el panel abierto | < 150 MB | **114,6 MB** | ✅ |
| CPU en reposo | < 0,5 % | **0,4 %** | ✅ |
| Arranque hasta aceptar conexiones | < 1,0 s | **~0,25 s** | ✅ |
| Tamaño del instalador de macOS | < 25 MB | **4,2 MB** | ✅ |
| Binario | — | 9,1 MB | |

Pendientes de medir, porque requieren automatizar el ciclo de la ventana o
generar carga sostenida:

- Memoria residente en reposo **sin ventana**, que es el objetivo que de verdad
  importa para una aplicación que vive todo el día en la barra de estado.
- CPU sirviendo un stream, y sobrecoste de latencia p95 del gateway.
- Tiempo de reapertura del panel tras cerrarlo.

Conclusión provisional: la decisión se confirma. El coste base con panel abierto
está a la mitad del límite de rechazo y el instalador es seis veces menor que el
objetivo. Queda cerrar la medición sin ventana antes de dar la fase 0 por
completa.
