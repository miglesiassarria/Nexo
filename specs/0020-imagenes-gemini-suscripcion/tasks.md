# Tareas

- [x] Reproducir el rechazo antes del arreglo: `cargo test -p nexo-core image_data_is_translated_and_known_gemini_models_declare_vision` falló por `Unsupported`.
- [x] Traducir y validar imágenes, capacidades y contrato simulado con y sin streaming: `cargo test -p nexo-core provider::gemini_subscription` (7 pruebas correctas).
- [ ] Ejecutar verificación completa e instalar la aplicación: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check && npm run app:install`.
- [x] Comprobar imagen real con cuenta y modelo de suscripción y verificar catálogo actualizado: `gemini-3.8-flash-high` respondió HTTP 200 y describió la imagen; el catálogo instalado marca visión.
- La batería completa falló dos veces en `lan_mode_serves_plain_http_over_the_network`: la conexión a 100.96.0.1 agotó el tiempo. Clippy y `npm run check` pasaron; instalada el 15/11/2026 a las 18:42:53.
