# 0022 · Límites por aplicación y errores de Gemini

- **Estado:** build
- **Creada:** 2026-11-15
- **Pedida por:** usuario: solucionar los errores de límite de Gemini desde DeepSeek Harness y permitir desactivar el límite.

## Problema

Las peticiones a Gemini aparecen como `rate_limited` pese a tener configurado un límite local alto. El usuario no puede desactivar el límite por aplicación y la información visible no explica suficientemente el origen ni el motivo del rechazo.

La inspección inicial indica que `rate_limited` corresponde a una respuesta HTTP 429 del proveedor y que `local_limit` representa el rechazo de Nexo. El adaptador de Gemini descarta el mensaje del proveedor en los 429; la causa concreta de las respuestas observadas todavía no está demostrada.

## Comportamiento esperado

Investigar las peticiones de Gemini y corregir cualquier fallo reproducible de Nexo que provoque o clasifique incorrectamente el rechazo. Distinguir claramente los rechazos locales de los del proveedor y transmitir al cliente información útil y segura del proveedor cuando esté disponible. Permitir desactivar explícitamente el límite local por aplicación y vía, conservar esa elección y permitir volver a activarlo. Mantener los límites existentes al actualizar.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | Se identifica el origen de los errores observados y se corrige cualquier fallo reproducible de Nexo; si el proveedor rechaza la petición, se informa sin afirmar que se ha eliminado su cuota. | Prueba de regresión para cada fallo identificado y comprobación real cuando sea posible; registrar límites de la evidencia. |
| 2 | Un 429 de Gemini se distingue de un límite local y expone al cliente el motivo útil disponible y el plazo de reintento cuando exista, sin revelar credenciales. | Pruebas con respuestas simuladas de Gemini y de la API pública, con y sin detalles y cabecera de reintento. |
| 3 | Se puede desactivar y reactivar el límite para una aplicación y vía concretas; desactivado no bloquea por cuota local. | Pruebas de política y persistencia, incluidas varias aplicaciones y vías; comprobación de interfaz. |
| 4 | La actualización conserva los límites existentes; un límite ausente o inválido no se confunde con una desactivación explícita. | Pruebas de migración y de estados inválidos. |
| 5 | La interfaz distingue el origen de los rechazos y explica que desactivar el límite de Nexo no desactiva el del proveedor. | Comprobación de interfaz y pruebas pertinentes. |
| 6 | La versión instalada en macOS contiene el cambio y supera las verificaciones del repositorio. | `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `npm run check` y `npm run app:install`; comprobar fechas del build y de la aplicación instalada. |

## Fuera de alcance

- Cambiar o eludir las cuotas, restricciones o decisiones de Google u otros proveedores.
- Guardar el contenido de las conversaciones o credenciales en los registros de errores.

## Supuestos asumidos

- La desactivación es explícita y por aplicación y vía, no global; los valores existentes permanecen activos hasta que el usuario los cambie.
- El comportamiento de otras vías y aplicaciones no cambia al desactivar una vía concreta.

## Riesgos

- Sin el contenido original de las respuestas históricas no se puede establecer retrospectivamente la causa exacta del 429.
- Una suscripción compartida sin límite local puede consumir rápidamente su cuota o provocar restricciones del proveedor.
- Los detalles del proveedor pueden contener información sensible y deben tratarse antes de mostrarlos o registrarlos.

## Invariantes y decisiones

- Esta petición cambia expresamente la invariante 4 y la mitigación del ADR 0001 que obligan a limitar todas las vías de suscripción. Se requiere un ADR nuevo y actualizar la invariante al implementar.
- Siguen vigentes la protección de credenciales, la identificación honesta, la separación por proveedor y tipo de credencial y la no conservación por defecto del contenido de conversaciones.

## Evidencia de la implementación

- La ventana antigua se reproduce en `apps::tests::changing_window_removes_the_old_hidden_limit`; se comprueba que al cambiar de ventana solo queda la elegida.
- La política comprueba desactivación y reactivación por vía en `policy::tests::explicitly_disabled_limit_is_scoped_and_can_be_restored`.
- `provider::gemini_subscription::tests::google_429_reports_safe_recognized_reason_and_retry` comprueba el error, motivo reconocido, espera y exclusión de datos arbitrarios.
- `cargo clippy --workspace --all-targets` y `npm run check` pasan. El resto de las pruebas pasó en serie excluyendo `lan_mode_serves_plain_http_over_the_network`, que se quedó esperando más de 60 segundos. La ejecución paralela también mostró un fallo intermitente de `unauthenticated_or_paused_request_is_rejected_early`; pasa aislada y en serie.
- `npm run app:install` instaló y arrancó la versión compilada el 15/11/2026 a las 21:27:12. Se comprobó visualmente el interruptor y la diferenciación en el panel.
- Pendiente: verificar contra Gemini real y completar la prueba de red. Los 429 históricos no conservan el motivo enviado por Google.

## Comprobación real del 15/11

DeepSeek Harness solicitó `gemini-4-argon-medium` y `gemini-3.8-flash-high` con el límite local de Gemini desactivado. Ambos devolvieron 429. La respuesta real del proveedor incluyó `error.status: RESOURCE_EXHAUSTED` y un mensaje genérico de recursos agotados que sugiere comprobar la cuota; no especificó causa exacta ni plazo. Se corrigió el reconocimiento de ese campo y la alineación del control. La prueba de respuesta satisfactoria sigue pendiente; no se afirma que el rechazo remoto esté resuelto.

## Comparación entre clientes

El 15/11, Msty Go obtuvo una respuesta correcta con `gemini-4-argon-medium` usando el mismo token de aplicación de Nexo que DeepSeek Harness; segundos después, Harness obtuvo 429. La petición de Harness en modo PTC incluía aproximadamente 95 KB de instrucciones de sistema y solicitaba 128 000 tokens de salida; la de Msty incluía aproximadamente 23 KB y no enviaba ese parámetro. Cambiar temporalmente la salida de Harness a 8192 no eliminó los 429 y no permite atribuirles una causa. El ajuste de Harness se restauró.

Durante las comprobaciones posteriores, Google respondió HTTP 403 indicando que el servicio se había deshabilitado para la cuenta por presunta infracción de condiciones y que debía presentarse una apelación; la petición de Msty también falló. Se dejaron de enviar peticiones y se retiró el diagnóstico temporal. No hay evidencia suficiente para atribuir el bloqueo a ninguna petición o cliente específico.
