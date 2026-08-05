# 0010 · Diseño

## Enfoque

Crear un sitio estático de una sola página bajo `website/`, construido por una
configuración Vite independiente y desplegado con el mecanismo oficial de GitHub
Pages Actions. La composición será un “manual de campo” técnico: jerarquía
editorial, diagramas construidos con HTML/CSS, bloques de terminal, navegación
persistente y una demostración visual del panel, sin copiar ni ejecutar el frontend
Tauri. El contenido se deriva de documentación estable y enlaza al repositorio para
el detalle vivo.

## Qué se toca

| Fichero | Qué cambia |
| --- | --- |
| `website/index.html` | Contenido completo, semántica, metadatos y estructura del sitio |
| `website/site.css` | Sistema visual responsive, accesibilidad, animaciones y componentes |
| `website/site.js` | Menú móvil, sección activa, copiado de ejemplos y mejoras progresivas |
| `website/public/*` | Logo, favicon y `404.html` estáticos |
| `website/site.config.ts` | Build aislado con base relativa compatible con `/Nexo/` |
| `package.json` | Órdenes `site:dev`, `site:build` y `site:preview` |
| `.github/workflows/pages.yml` | Build y despliegue oficial de Pages desde `main` |
| `.github/workflows/ci.yml` | Añadir el build del sitio a la verificación del PR |
| `README.md`, `ROADMAP.md`, `docs/adr/0003-*`, `specs/*` | Reconciliar estado y enlazar la web pública |

## Decisiones

### D1. Sitio estático separado del frontend Tauri

- **Decisión:** `website/` no importa `src/` ni `@tauri-apps/api`.
- **Alternativa descartada:** desplegar el frontend actual porque todas sus vistas
  obtienen datos mediante `invoke()` y quedarían rotas en un navegador normal.
- **Consecuencia que hay que asumir:** identidad y algunos patrones se comparten,
  pero la web tiene componentes propios y no es una demo funcional del gateway.

### D2. Vite y HTML/CSS/JS sin framework documental adicional

- **Decisión:** reutilizar el Vite ya instalado y JavaScript progresivo.
- **Alternativa descartada:** Astro, VitePress o SvelteKit porque añadirían
  dependencias y convenciones innecesarias para una primera página única.
- **Consecuencia que hay que asumir:** el contenido se mantiene manualmente y una
  futura documentación multipágina podría justificar migrar a un generador.

### D3. Base relativa

- **Decisión:** compilar con `base: "./"` y rutas de assets relativas.
- **Alternativa descartada:** fijar `/Nexo/`, que hace más frágil la vista previa y
  acopla el artefacto al nombre actual del repositorio.
- **Consecuencia que hay que asumir:** los anchors internos siguen siendo absolutos
  al documento, pero todos los recursos deben pasar por Vite o `public/`.

### D4. GitHub Pages mediante Actions

- **Decisión:** `configure-pages`, `upload-pages-artifact` y `deploy-pages`, con
  `pages: write` e `id-token: write` solo en el job de despliegue.
- **Alternativa descartada:** rama `gh-pages`, porque introduce contenido generado
  y una segunda historia que mantener.
- **Consecuencia que hay que asumir:** hay que habilitar el sitio con
  `build_type=workflow` y esperar la ejecución posterior al merge.

### D5. Reconciliación documental acotada

- **Decisión:** corregir solo contradicciones demostradas por commits, tests y
  specs: Gemini API key, implementación LAN y estados finalizados 0001/0002/0005/0006.
- **Alternativa descartada:** cerrar todas las specs con implementación fusionada,
  porque 0003, 0004, 0008 y 0009 conservan verificaciones manuales o cierres no
  demostrados y no deben maquillarse.
- **Consecuencia que hay que asumir:** el índice seguirá mostrando trabajo en
  `build` cuando falte evidencia real, aunque parte del código esté fusionada.

## Qué puede romperse

| Riesgo | Cómo se detecta |
| --- | --- |
| Assets rotos bajo el subdirectorio de Pages | Build local y peticiones HTTP a CSS, JS, favicon y logo publicados |
| El sitio importa accidentalmente Tauri | `rg '@tauri-apps|invoke\(' website site-dist` |
| El workflow no tiene permisos o configuración válida | Ejecución real `pages-build-deployment` y API de Pages |
| Navegación o copia falla | Recorrido en navegador, consola limpia y prueba de botones |
| El contenido contradice fuentes estables | Búsquedas dirigidas y revisión cruzada con README, roadmap, ADR y specs |
| Regresión de la aplicación | CI completo y `npm run app:install` |
| Animaciones perjudican accesibilidad | `prefers-reduced-motion`, navegación por teclado y viewport móvil |

## ¿Hace falta un ADR?

No. La web de documentación y su despliegue no cambian arquitectura ni invariantes
del producto. Si en el futuro la web ejecutara servicios, recogiera telemetría o se
convirtiera en canal de distribución firmado, serían decisiones distintas.

## Qué queda pendiente de descubrir

- La URL exacta y el tiempo de propagación solo se confirman tras habilitar Pages.
- El aspecto final se valida en el navegador real; el HTML y el build no bastan.
- GitHub puede crear el entorno `github-pages` automáticamente al habilitar el
  sitio; se comprobará después del primer despliegue.
