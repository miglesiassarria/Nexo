# Diseño

El adaptador de suscripción traduce data URL base64 a `inlineData` con MIME y datos, valida el formato y rechaza URL remotas sin descargarlas. El catálogo respeta `inputModalities` cuando existe; cuando falta, reconoce una lista acotada de familias Gemini multimodales y sus variantes. Se descartó declarar visión en todos los modelos porque incluye modelos ajenos y variantes desconocidas. Los cambios quedan aislados en el adaptador; no se necesita ADR. La compatibilidad del backend interno requiere una prueba real independiente del contrato simulado.
