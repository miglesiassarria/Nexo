# Nexo

Nexo es un gateway local, propio y extensible para centralizar el acceso a modelos de IA desde una única interfaz compatible con OpenAI. La intención es tener un punto común entre aplicaciones, proveedores cloud y modelos locales, con control de credenciales, modelos disponibles, permisos y uso.

El proyecto toma como referencia conceptual los gateways locales de IA, como Msty Nexus, pero no reutiliza su código ni depende de él.

## Estado actual

Este repositorio contiene el primer MVP técnico:

- Gateway HTTP local con `/health`, `/v1/models` y `/v1/chat/completions`.
- Formato de modelo explícito: `proveedor/modelo`.
- Adaptadores separados para OpenAI, Google Gemini y un proveedor `mock` para pruebas.
- Token opcional para proteger el gateway incluso cuando se ejecuta en localhost.
- Base preparada para añadir streaming, métricas, políticas, tokens por aplicación y runtimes locales.

## Arranque rápido

Requiere Node.js 20 o superior.

```bash
cp .env.example .env
npm test
npm start
```

Comprobar el gateway:

```bash
curl http://127.0.0.1:3000/health
curl http://127.0.0.1:3000/v1/models
curl http://127.0.0.1:3000/v1/chat/completions \\
  -H 'content-type: application/json' \\
  -d '{"model":"mock/echo","messages":[{"role":"user","content":"hola"}]}'
```

## Autenticación y suscripciones

La prioridad de Nexo es que el usuario no tenga que copiar una API key en cada aplicación. Para lograrlo de forma sostenible, las credenciales viven en el gateway y cada proveedor tiene su propio adaptador.

### Google Gemini

El adaptador acepta un access token OAuth de Google Cloud mediante `GOOGLE_OAUTH_ACCESS_TOKEN`. La documentación oficial de Gemini contempla OAuth para autenticarse contra la Gemini API; requiere un proyecto de Google Cloud y los permisos correspondientes. También se puede usar `GOOGLE_GEMINI_API_KEY` como alternativa.

Esto es distinto de convertir automáticamente una suscripción de la aplicación Gemini en crédito de API: el OAuth autorizado aquí es el de la API/proyecto de Google Cloud.

### ChatGPT/OpenAI

Nexo no incluye una integración contra endpoints privados de ChatGPT, automatización del navegador, extracción de cookies ni reutilización de tokens de la aplicación web. Esas técnicas serían frágiles y podrían incumplir las condiciones del servicio.

El login oficial “Sign in with ChatGPT” sirve para identidad en aplicaciones compatibles; por sí solo no concede acceso a conversaciones, memoria, archivos, tokens o facturación de ChatGPT. Por eso el adaptador actual usa la API oficial de OpenAI con `OPENAI_API_KEY`. Si OpenAI publica una delegación oficial de uso de modelos con una suscripción de ChatGPT para aplicaciones de terceros, se podrá implementar como un adaptador adicional sin cambiar el gateway.

## Dirección técnica

```text
Aplicaciones cliente
        |
        | OpenAI-compatible API
        v
   Nexo Gateway
   /    |      \
OpenAI Gemini  Local runtimes
```

Próximos hitos recomendados:

1. Flujo OAuth de Google con callback local, refresh token cifrado y keychain del sistema.
2. Streaming SSE y normalización multimodal.
3. Almacén seguro de credenciales y tokens por aplicación con scopes.
4. Catálogo dinámico, health checks y métricas locales.
5. Adaptadores para Ollama, llama.cpp y MLX.
6. UI de escritorio y configuración por perfiles.
7. Tests de contrato por proveedor y auditoría de privacidad.

## Seguridad

Nexo escucha en `127.0.0.1` por defecto. No guardes secretos en el repositorio. Antes de habilitar acceso LAN habrá que añadir TLS, autenticación fuerte, rotación de tokens, cifrado en reposo y una política explícita de exposición de datos.

Consulta [SECURITY.md](SECURITY.md) para reportar problemas.

## Licencia

MIT. Consulta [LICENSE](LICENSE).
