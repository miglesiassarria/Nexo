# 0019 · Diseño

Responde al **cómo**. Lee el código antes de escribirlo: de memoria salen diseños
que no encajan.

## Enfoque

El adaptador de suscripción consultará el mismo endpoint de modelos con una
lista ordenada de versiones de compatibilidad: primero `99.0.0`, después la
última versión mínima probada para GPT‑6 (`0.155.0`) y finalmente el valor
guardado antes de este cambio, deduplicando candidatos. Un HTTP 400 o una
respuesta sin modelos utilizables permite probar la siguiente versión. Los
fallos de transporte, autenticación, cuota u otros estados se devuelven sin
reintentar porque no dependen de la versión. El primer catálogo no vacío gana y
el flujo de actualización existente lo persiste; si
todos fallan, el servicio ya conserva las filas existentes. La versión técnica
deja de aparecer como ajuste normal en Configuración.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/auth/chatgpt.rs` | Constantes y orden de versiones internas de catálogo. |
| `crates/nexo-core/src/provider/chatgpt_subscription.rs` | Reintentos acotados del GET `/models` y tests del orden, negociación y fallos. |
| `src/lib/views/Settings.svelte` | Retirar el control manual de versión del formulario normal. |
| `docs/adr/0001-oauth-de-suscripcion.md`, `docs/producto.md`, `docs/modelo-datos.md` | Registrar el comportamiento automático, el fallback legado y el carácter no soportado del endpoint. |
| `specs/README.md`, carpeta 0019 | Especificación, decisiones, tareas y evidencia de cierre. |

## Decisiones

Una entrada por decisión. Sin la alternativa descartada, no es una decisión: es
una preferencia sin justificar.

### D1. Negociar el catálogo sin intervención del usuario

- **Decisión:** probar primero `99.0.0`, el valor máximo que Hermes utiliza y que Nexo acaba de verificar en una consulta real; después probar `0.155.0`, y al final el valor histórico guardado, omitiendo duplicados.
- **Alternativa descartada:** subir manualmente `DEFAULT_CLIENT_VERSION` a una cifra y mantener un único intento, porque un cambio futuro volvería a requerir actualizar Nexo y un rechazo impediría el fallback.
- **Consecuencia que hay que asumir:** el valor `99.0.0` es una señal de compatibilidad para un endpoint interno, no una versión de Nexo ni un número oficial de cliente; depende de que el servidor continúe aceptándolo.

### D2. Reintentar solo fallos recuperables por versión

- **Decisión:** reintentar solo ante HTTP 400 o catálogo vacío; devolver de inmediato errores de transporte, autenticación, rate limit y otros estados.
- **Alternativa descartada:** reintentar cualquier fallo, porque la red, las credenciales y la cuota no se corrigen cambiando la versión y sumaríamos solicitudes innecesarias.
- **Consecuencia que hay que asumir:** si el endpoint cambia y comunica que una versión no válida con otro código distinto de 400, se conservará el catálogo anterior hasta ajustar la clasificación.

### D3. Mantener el valor histórico solo para compatibilidad

- **Decisión:** quitar el control de versión del formulario visible, pero conservar la propiedad ya persistida como última alternativa; no hace falta migración ni se descartan preferencias guardadas.
- **Alternativa descartada:** borrar ya el ajuste de la base de datos, porque no aporta nada al objetivo y exigiría retirar el campo de configuración serializada.
- **Consecuencia que hay que asumir:** el dato legado permanece en la configuración local aunque el usuario ya no pueda necesitar editarlo en el flujo normal.

## Qué puede romperse

Y, sobre todo, **cómo nos enteraremos**. Un fallo silencioso es peor que uno ruidoso.

| Riesgo | Cómo se detecta |
| --- | --- |
| El proveedor deja de aceptar versiones altas | Prueba de respuesta 400 y fallback a `0.155.0`; error claro si todas fallan. |
| Auth o cuota fallan | Tests confirman que 401/403/429 terminan sin reintentos por versión. |
| El catálogo nuevo incluye modelos no compatibles | El parser existente filtra por `visibility=list` y `supported_in_api=true`; una prueba cubre ambas condiciones. |
| El refresco no obtiene datos | `refresh_catalog_from_providers` solo escribe después de `Ok(models)`, por lo que se revisa y prueba que una respuesta fallida no sustituye filas. |

## ¿Hace falta un ADR?

No hace falta ADR nuevo. La ruta no soportada y la identidad honesta de Nexo ya
están aceptadas por ADR 0001; esta especificación añade una estrategia acotada
para consultar su endpoint de catálogo.

## Qué queda pendiente de descubrir

La consulta real autenticada ya confirmó el orden de respuesta de `0.144.0`,
`0.155.0` y `99.0.0` para la cuenta del usuario. La implementación valida el
fallback con un servidor simulado; no hace una petición de generación, porque
esta especificación modifica solo el descubrimiento del catálogo.
