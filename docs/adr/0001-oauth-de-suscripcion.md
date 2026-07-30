# ADR 0001: Usar OAuth de suscripción mediante flujos no soportados

- **Fecha:** 2026-07-30
- **Estado:** aceptada
- **Decide:** Manuel Iglesias

## Contexto

El objetivo declarado de Nexo es que el usuario aproveche desde cualquier aplicación la suscripción de IA que ya paga, sin repartir API keys ni volver a pagar por token.

Ese objetivo choca con una realidad: **ningún proveedor relevante ofrece un mecanismo oficial para que una aplicación de terceros consuma la cuota de una suscripción de tarifa plana.** No es un hueco temporal de documentación, es una decisión comercial. Delegar cuota plana a un proxy genérico se arbitra de inmediato, así que ese acceso queda reservado a los clientes propios del proveedor.

Existe una técnica que sí funciona y que emplean proyectos abiertos ampliamente usados, entre ellos [opencode](https://github.com/anomalyco/opencode):

1. Ejecutar el flujo OAuth 2.0 con PKCE del proveedor usando el **client_id público de su cliente oficial**.
2. Recibir el callback en un puerto local y canjear el código por access y refresh token.
3. Llamar al **endpoint que consume ese cliente oficial**, que suele ser el backend de su aplicación y no la API pública documentada.

Las alternativas evaluadas son:

- **Solo API keys.** Estable y soportado, pero elimina la razón de ser del proyecto: el usuario sigue pagando dos veces y Nexo se reduce a un centralizador de secretos con panel de métricas. Útil, pero no es este producto.
- **Esperar un mecanismo oficial.** Puede no llegar nunca, y si llega es probable que sea «factura la API a la cuenta del usuario», no «usa su plan plano».
- **Reutilizar el flujo del cliente oficial.** Funciona hoy, cumple el objetivo, y no es soportado.

## Decisión

Nexo implementará OAuth de suscripción reutilizando los flujos de los clientes oficiales de los proveedores, empezando por OpenAI/ChatGPT.

La decisión se toma con conocimiento explícito de que **el mecanismo no es oficial, no está documentado, no está versionado y puede dejar de funcionar o tener consecuencias sobre la cuenta del usuario.**

### Límites que la decisión no levanta

Reutilizar un flujo OAuth oficial no autoriza cualquier técnica. Siguen prohibidos:

- Scraping y automatización del navegador para simular una sesión.
- Reutilizar cookies, sesiones o almacenamiento local del navegador del usuario.
- Leer, importar o extraer tokens de los ficheros de configuración de otras aplicaciones instaladas.
- Cualquier obtención de credenciales que no sea un flujo iniciado desde Nexo y completado conscientemente por el usuario.
- Suplantar el `User-Agent` o la identidad de otro cliente cuando el flujo admita identificarse honestamente. Nexo se declara como Nexo.

La diferencia es sustantiva y es la que hace la decisión defendible: el usuario autoriza a Nexo de forma explícita, ve qué está autorizando y puede revocarlo desde su propia cuenta del proveedor. Todo lo de la lista anterior sería apropiarse de credenciales que el usuario no ha concedido a Nexo.

## Riesgos aceptados

### 1. Rotura unilateral

El client_id, los parámetros del flujo, las cabeceras o el endpoint pueden cambiar sin aviso ni deprecación.

**Mitigación.** Todos los valores frágiles de un proveedor viven en un único módulo, aislado del resto del sistema, de forma que una rotura afecte a un fichero. Cada proveedor con ruta de suscripción debe soportar también API key, y Nexo debe caer a esa ruta automáticamente cuando esté configurada. Cuando no lo esté, el error mostrado al usuario debe explicar que la vía de suscripción ha dejado de funcionar y qué puede hacer, no un `502` genérico.

### 2. Consecuencias sobre la cuenta del usuario

Usar la suscripción desde una aplicación no autorizada puede incumplir las condiciones del servicio, con consecuencias que van desde limitación de tasa hasta el cierre de la cuenta.

**Mitigación.** Advertencia explícita y confirmación del usuario **antes** de completar el primer login de suscripción de cada proveedor, indicando el riesgo en términos concretos. No una nota al pie ni un enlace a la documentación.

### 3. Multiplexación: el riesgo propio de Nexo

Este es el riesgo que Nexo añade y que un cliente único no tiene. Un asistente de escritorio es una persona usando su plan. Nexo, **por diseño**, multiplexa N aplicaciones sobre una sola suscripción, y ese patrón de tráfico es exactamente el que un proveedor interpreta como abuso de un plan personal.

**Mitigación, y es un requisito funcional, no una preferencia:**

- Los límites por aplicación son **obligatorios** en toda ruta respaldada por suscripción. Nexo no permite dejarlos en blanco y trae valores por defecto conservadores.
- El consumo acumulado por ventana es visible en el panel y en el menú del icono de estado.
- Las peticiones que excedan el límite se rechazan con un error claro, no se encolan indefinidamente.
- La interfaz debe hacer evidente al usuario que esa ruta comparte una cuota personal entre todas sus aplicaciones.

### 4. Distribución pública

Publicar y distribuir instaladores firmados de una herramienta cuya función central es esta tiene implicaciones distintas a usarla uno mismo.

**Mitigación.** La distribución se aplaza a la fase 5 y se decide entonces, por separado. Construir no obliga a publicar.

### 5. Observabilidad degradada

La ruta de suscripción no informa de tokens, cuota ni coste, y su catálogo es un subconjunto con capacidades recortadas. Esto choca con que la observabilidad es la otra mitad del producto.

**Mitigación.** El modelo de métricas incorpora un estado de contabilidad **«cubierto por suscripción»**, distinto de «reportado», «estimado» y «no disponible». Mostrar cero euros sin más sería cierto y engañoso a la vez: la interfaz debe decir que el coste marginal es cero y que la cuota consumida es desconocida. El catálogo se indexa por proveedor **y** tipo de credencial, para que el usuario nunca descubra en tiempo de ejecución que un modelo no está disponible por la vía que está usando.

## Consecuencias arquitectónicas

Estas no son opcionales; se derivan de la decisión y hay que asumirlas desde el primer día.

1. **El tipo de credencial es un eje de primer nivel**, junto al proveedor. Cada combinación tiene su propio catálogo, capacidades, límites y contabilidad. Ver [`../contrato-proveedor.md`](../contrato-proveedor.md).
2. **La traducción de formatos es el caso base, no la excepción.** El endpoint de suscripción de OpenAI habla el formato Responses y la API pública de Nexo habla `chat/completions`. Hay que traducir la petición en un sentido y el stream de eventos en el otro, en la ruta más importante del producto.
3. **El spike de validación va antes que cualquier otra cosa.** Si el flujo no funciona desde una aplicación que no sea el cliente oficial, todo lo demás sobra.
4. **Nada de tokens en ficheros de texto plano.** La implementación de referencia guarda las credenciales en un JSON con permisos `0600`. Nexo usa el almacén seguro del sistema operativo; es un punto donde debe ser estrictamente mejor.

## Revisión

Esta decisión debe revisarse si:

- Algún proveedor publica un mecanismo oficial de acceso delegado a suscripciones.
- La ruta de suscripción de OpenAI se rompe más de dos veces en un trimestre, lo que indicaría que el coste de mantenimiento supera el valor.
- Aparece evidencia de que el patrón de multiplexación provoca bloqueos de cuenta pese a los límites por aplicación.
