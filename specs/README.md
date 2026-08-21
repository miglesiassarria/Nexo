# Especificaciones

Índice de todo lo que se ha especificado en Nexo. La metodología está en
[`.claude/skills/spec-driven/SKILL.md`](../.claude/skills/spec-driven/SKILL.md) y
resumida en el [README](../README.md#cómo-se-desarrolla-aquí).

| # | Título | Estado | Notas |
| --- | --- | --- | --- |
| [0001](0001-proveedor-local-lm-studio/spec.md) | Usar los modelos de LM Studio desde Nexo | `hecho` | Verificada contra LM Studio 0.4.20. Ollama queda para otra |
| [0002](0002-proveedores-genericos-y-opencode-zen/spec.md) | Proveedores OpenAI-compatible añadidos por el usuario, y OpenCode Zen | `hecho` | Verificada contra Zen real, 60 modelos. Anthropic-compatible aplazado a petición del usuario |
| [0003](0003-vista-de-proveedores-legible/spec.md) | Una vista de Proveedores que se lee de un vistazo | `build` | Arregla además que un proveedor propio con API key salía duplicado y en la sección de OpenAI |
| [0004](0004-modelos-permitidos-por-aplicacion/spec.md) | Elegir qué modelos sirve Nexo a cada aplicación | `build` | La mitad del almacenamiento ya existía sin usarse; el catálogo no aplicaba la misma regla que el gateway |
| [0005](0005-granularidad-horaria-y-tokens-de-entrada-y-salid/spec.md) | Periodos por horas y tokens de entrada/salida en el panel | `hecho` | Verificación completa e instalación registradas en `tasks.md`; para ventanas cortas se consulta el detalle directamente |

| [0006](0006-atajo-de-proveedor-openrouter/spec.md) | Atajo de proveedor: OpenRouter | `hecho` | Verificado contra `models.dev` y la API real; cierre completo registrado en `tasks.md` |
| [0007](0007-acceso-red-local/spec.md) | Acceso desde la red local | `hecho` | Requiere [ADR 0003](../docs/adr/0003-acceso-desde-la-red-local.md); verificado por test y contra el binario real instalado; falta el clic real en el interruptor (bloqueado por el mismo permiso de Accesibilidad de siempre) |
| [0008](0008-atajo-de-proveedor-gemini-api-key/spec.md) | Atajo de proveedor: Gemini (API key) | `hecho` | Verificado con clave real. Dos arreglos reales encontrados al construir: id de modelo con prefijo `models/` no reconocido por `models.dev`, y el sobre de error de Gemini (`google.rpc.Status`, a veces en array) no lo reconocía el clasificador compartido |
| [0009](0009-esfuerzo-de-razonamiento-por-aplicacion-y-modelo/spec.md) | Esfuerzo de razonamiento por aplicación y modelo | `build` | Primer valor por modelo en `app_grants`; arregla que `grant_for` elegía la primera fila coincidente y no la más específica. Falta comprobar el selector con el catálogo real de suscripción: hoy guarda el manifiesto, sin niveles |
| [0010](0010-web-publica-y-documentacion-en-github-pages/spec.md) | Web pública y documentación en GitHub Pages | `hecho` | Publicada con HTTPS; build, QA responsive, CI, instalación macOS y recursos HTTP verificados |
| [0011](0011-token-de-aplicacion-recuperable/spec.md) | Token de aplicación recuperable | `build` | Requiere [ADR 0004](../docs/adr/0004-tokens-de-aplicacion-recuperables.md); `revoke_app`/`delete_app` no pasaban por `Nexo`, hoy no podrían limpiar el almacén seguro |
| [0012](0012-red-local-sin-cifrado/spec.md) | Red local sin cifrado | `build` | Requiere [ADR 0005](../docs/adr/0005-red-local-sin-cifrado.md), que sustituye el punto 2 del [ADR 0003](../docs/adr/0003-acceso-desde-la-red-local.md) y modifica la invariante 9. El certificado iba atado a la IP: en un portátil obligaba a volver a aceptarlo en cada cliente en cada cambio de red. Se retira TLS entero, con `rcgen` y `axum-server` |
| [0013](0013-proveedor-local-ollama/spec.md) | Proveedor local: Ollama | `build` | Verificado contra Ollama 0.32.14 real. Sus capacidades salen de `/api/tags`, que las publica; arregla además que la vista llamaba a `set_lmstudio_url` para cualquier servidor local, así que con dos, configurar uno habría pisado al otro |

| [0013](0013-proveedor-local-ollama/spec.md) | Proveedor local: Ollama | `hecho` | Verificado contra Ollama 0.32.14 real. Sus capacidades salen de `/api/tags`, que las publica; arregla además que la vista llamaba a `set_lmstudio_url` para cualquier servidor local, así que con dos, configurar uno habría pisado al otro |
| [0014](0014-sin-icono-en-el-dock-sin-ventana/spec.md) | Sin icono en el Dock cuando no hay ventana | `hecho` | Verificado con `lsappinfo` contra la app instalada: `Foreground` con panel, `UIElement` sin él, y el gateway sirviendo en los dos estados |
| [0015](0015-clave-maestra-en-llavero/spec.md) | Clave maestra en el Llavero del sistema y almacenamiento cifrado de credenciales | `hecho` | Requiere [ADR 0006](../docs/adr/0006-clave-maestra-en-llavero-y-almacen-cifrado.md); reduce de 4-6 a 1 las peticiones de contraseña del Llavero en cada nuevo despliegue |
| [0016](0016-correlacion-tool-calls-responses-api/spec.md) | Correlación de identificadores en llamadas a herramientas de Responses API | `hecho` | Corrige que `ChunkBuilder` cambiaba el índice de 0 a 1 por la disparidad entre `item.id` (`fc_xxx`) y `item.call_id` (`call_xxx`) |
| [0017](0017-limite-tamano-peticiones/spec.md) | Límite de tamaño de peticiones de chat y archivos | `hecho` | Corrige el 413 en payloads grandes (imágenes base64) con límite configurable (default 32 MiB, 1 MiB–5 GiB / sin límite) e ingestión protegida por disco |

Estados: `spec` · `design` · `tasks` · `build` · `hecho` · `descartado`

## Qué va aquí y qué no

Aquí van los **cambios**: funcionalidad nueva, comportamiento distinto, un
proveedor más. Cada uno en su carpeta `NNNN-nombre-corto/`.

Aquí **no** va:

- La **visión del producto** ni sus características estables → [`docs/producto.md`](../docs/producto.md).
- Las **decisiones de arquitectura** y los riesgos aceptados → [`docs/adr/`](../docs/adr/).
- El **plan a medio plazo** → [`ROADMAP.md`](../ROADMAP.md).
- Los **contratos** entre piezas → [`docs/contrato-proveedor.md`](../docs/contrato-proveedor.md) y [`docs/modelo-datos.md`](../docs/modelo-datos.md).

Una especificación es temporal: describe un cambio y, cuando se completa, lo que
haya aprendido de duradero se lleva a `docs/`. Lo que queda en `specs/` es el
registro de por qué se hizo así.

## Empezar una

```bash
scripts/new-spec.sh "nombre corto de lo que quieres"
```

O, mejor, deja que se ocupe el ciclo: `/spec lo que quieres conseguir`.
