# ADR 0007: Límite configurable de peticiones e ingestión protegida por disco

- **Estado:** Aceptado
- **Fecha:** 2026-08-21
- **Contexto:** [Spec 0017](../../specs/0017-limite-tamano-peticiones/spec.md)

## Contexto y problema

Axum aplica por defecto un límite de 2 MiB en la recepción de cuerpos HTTP. Cuando clientes compatibles con OpenAI (como Msty Go) envían imágenes o capturas en base64 dentro de `/v1/chat/completions`, el cuerpo de la petición supera habitualmente los 3–4 MB, recibiendo un `HTTP 413` en texto plano antes de llegar a la lógica de Nexo.

Aumentar este límite a varios gigabytes de forma ingenua mantendría el cuerpo completo en memoria RAM, lo que multiplicado por la deserialización JSON y la traducción de adaptadores provocaría riesgos graves de agotamiento de memoria (*Out of Memory*, OOM) y ataques de denegación de servicio (DoS) por parte de clientes no autenticados.

## Decisión

1. **Límite configurable por el usuario**:
   - Por defecto: **32 MiB**.
   - Rango: **1 MiB a 5 GiB**, más la opción avanzada **«Sin límite impuesto por Nexo»** (persistido como `NULL` en `settings.max_request_body_bytes`).
   - Aplicación dinámica e inmediata a nuevas peticiones sin requerir reinicio del gateway.

2. **Pre-autenticación antes de recibir el cuerpo**:
   - El router de Nexo inspecciona la cabecera `Authorization: Bearer ...` y el estado del servicio antes de consumir el cuerpo HTTP.
   - Peticiones sin token o no autorizadas son rechazadas inmediatamente con `HTTP 401` sin consumir ancho de banda ni almacenar datos en disco/memoria.

3. **Ingestión híbrida protegida (Memoria / Disco)**:
   - Umbral en memoria de **4 MiB**: peticiones de hasta 4 MiB se almacenan directamente en un buffer de memoria RAM.
   - Peticiones que superen los 4 MiB se canalizan en streaming hacia un archivo temporal seguro en disco (`0600` en Unix).
   - Los archivos temporales se gestionan mediante RAII: se eliminan automáticamente al completar la petición, ante errores de red, cancelaciones del cliente o al reiniciar la aplicación (rutina de saneamiento al arrancar).

4. **Error 413 OpenAI-compatible**:
   - Al superar el límite configurado por Nexo se devuelve un `HTTP 413` estructurado en JSON con el formato estándar de error de OpenAI (`code: request_too_large`).

## Consecuencias

- **Positivas**:
  - Clientes multimodales con imágenes en alta resolución funcionan sin fallos de tamaño.
  - El usuario puede adaptar el límite a sus necesidades y hardware.
  - La memoria de Nexo permanece acotada independientemente de si se reciben archivos de 100 MiB o 2 GiB.
  - Protección activa frente a DoS por parte de clientes no autenticados.
- **Compromisos aceptados**:
  - Peticiones muy grandes que pasen a disco implican operaciones de E/S temporales.
  - «Sin límite impuesto por Nexo» sigue condicionado por los límites físicos del hardware (espacio libre en disco) y por los límites propios del proveedor upstream seleccionado.
