# 0020 · Imágenes en Gemini por suscripción

Estado: build. El usuario aprobó implementar el plan de investigación: permitir imágenes en la ruta de suscripción y comprobar capacidades sin afectar a otros proveedores.

## Comportamiento y criterios

- Las imágenes adjuntas como data URL base64 se envían con texto a Code Assist en streaming y sin streaming. Verificación: `cargo test -p nexo-core provider::gemini_subscription` y prueba real con cuenta elegible.
- El catálogo respeta las modalidades explícitas de Google; si faltan, reconoce las familias multimodales conocidas, sin atribuir visión a modelos desconocidos. Verificación: test de descriptor y refresco de catálogo real.
- Imágenes inválidas y URL remotas se rechazan de forma explícita. Verificación: test de entradas inválidas.
- Los demás proveedores y el texto siguen funcionando. Verificación: `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `npm run check`.

## Fuera de alcance

Audio, documentos, generación de imágenes, descarga de URL remotas y cambios en Gemini por API key. La ruta de suscripción es interna y puede diferir de la API pública.

## Riesgos y supuestos

La documentación pública de Google confirma imagen inline para Gemini 2.5 y 3.8, pero no garantiza que Code Assist acepte todas las combinaciones. Se requiere prueba real; no se infiere visión de nombres de modelos futuros.
