# 0006 · Diseño

## Enfoque

Se añade una segunda constante `ProviderPreset` (`OPENROUTER`) junto a la que ya
existe para Zen, y se mete en el vector que devuelve `presets()`. Como
`connect_options()` ya itera ese vector sin conocer los nombres concretos, el
atajo aparece solo con ese cambio — nada de frontend, nada de adaptador nuevo.
Se corrige además el texto de la opción genérica, que hoy usa «OpenRouter» como
ejemplo de servicio sin atajo propio y con esto deja de serlo.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `crates/nexo-core/src/provider/openai_compat.rs` | Nueva constante `OPENROUTER: ProviderPreset` con `base_url = "https://openrouter.ai/api/v1"` (verificado contra `models.dev/api.json` real, ver D2) y `docs_url = "https://openrouter.ai/models"`; se añade a `presets()`. |
| `crates/nexo-core/src/service.rs` | Línea ~1090: el texto de «Otro servicio OpenAI-compatible» cambia el ejemplo «OpenRouter» por «Groq». Tests nuevos junto a `the_opencode_zen_shortcut_arrives_with_its_name_and_address_filled` (línea ~2780). |

## Decisiones

### D1. Un preset nuevo, no un caso especial dentro del genérico

- **Decisión:** OpenRouter se añade como una segunda entrada en `presets()`,
  con el mismo `struct ProviderPreset` que ya usa Zen.
- **Alternativa descartada:** detectar por nombre o URL en el formulario
  genérico y auto-rellenar. Se descarta porque ya existe el mecanismo correcto
  para esto (el propio `struct`) y duplicar la idea con lógica ad-hoc en el
  formulario sería justo el tipo de rama de código «un proveedor, un caso
  especial» que la spec 0002 evitó a propósito.
- **Consecuencia que hay que asumir:** ninguna nueva — es el mismo patrón que
  ya se mantiene para Zen.

### D2. Fijar la URL comprobándola contra `models.dev` real, no de memoria

- **Decisión:** `base_url = "https://openrouter.ai/api/v1"` y
  `docs_url = "https://openrouter.ai/models"`, tomados literalmente de
  `curl -s https://models.dev/api.json` consultado el 2026-08-02 (campo
  `openrouter.api` y `openrouter.doc`), no de lo que un modelo de lenguaje
  «recuerda» sobre OpenRouter.
- **Alternativa descartada:** escribir la URL de memoria (es pública y
  conocida) y confiar en que coincida. Se descarta porque `provider_id_for_api`
  compara byte a byte tras recortar solo la barra final (`models_dev.rs:74`):
  una `/` de más, `www.` de más, o un subdominio distinto dejaría el catálogo
  sin enriquecer sin ningún error visible — exactamente el «degradar en
  silencio» que `CLAUDE.md` prohíbe, solo que en el cruce con el catálogo en
  vez de en la petición.
- **Consecuencia que hay que asumir:** si `models.dev` cambia esa URL en el
  futuro, hay que repetir la comprobación y actualizar la constante — ya
  anotado como riesgo en `spec.md`.

### D3. Cambiar el ejemplo del texto genérico, no borrarlo

- **Decisión:** el texto de «Otro servicio OpenAI-compatible» pasa de
  mencionar «OpenRouter, un proxy propio…» a «Groq, un proxy propio…».
- **Alternativa descartada:** dejar el texto como está. Se descarta porque
  sería confuso: ese texto describe la opción para servicios *sin* atajo
  propio, y OpenRouter deja de estarlo con esta spec.
- **Alternativa descartada:** quitar el ejemplo y dejar solo «un proxy propio,
  un servidor de tu empresa…». Se descarta porque un ejemplo de servicio
  público conocido ayuda a entender la opción más que dos genéricos; Groq
  sirve igual y ya estaba citado junto a OpenRouter en el diseño de la 0002.
- **Consecuencia que hay que asumir:** ninguna.

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| Añadir el preset sin actualizar el texto del genérico deja una mención obsoleta a OpenRouter en la interfaz | Prueba nueva que falla si el texto de la opción `compat:custom` contiene la palabra «OpenRouter» |
| La URL del preset no coincide byte a byte con la de `models.dev`, y el catálogo de quien lo conecte queda sin precios/capacidades sin ningún aviso | No hay forma de detectarlo en CI sin red real; se deja anotado en `spec.md` como riesgo aceptado, igual que ya lo es para Zen, y se comprueba a mano en `/build` si hay clave real disponible |
| Dos presets con el mismo `suggested_name` (o cuyo slug colisione) generarían IDs repetidos en `connect_options()` | Cubierto por el test ya existente `connect_options_cover_the_four_form_shapes`, que falla si hay `id` duplicados |

## ¿Hace falta un ADR?

No. La decisión de que los atajos son «solo datos, mismo adaptador» ya está
tomada y documentada en la D7 de `specs/0002-proveedores-genericos-y-opencode-zen/design.md`;
esto es una instancia de esa decisión, no una nueva.

## Qué queda pendiente de descubrir

- Si el usuario aporta una clave real de OpenRouter durante `/build`, queda
  por ver si el catálogo se enriquece con precio y capacidades tal como se
  espera (criterio 5) — es lo único de esta spec que solo se sabe probando
  contra la realidad.
