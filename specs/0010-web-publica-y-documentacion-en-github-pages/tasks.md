# 0010 · Tareas

- [x] **T1. Reconciliar las fuentes que alimentan la web.**
  - Ficheros: `README.md`, `ROADMAP.md`, `docs/adr/0003-acceso-desde-la-red-local.md`, specs 0001/0002/0005/0006, `specs/README.md`
  - Verificación: búsquedas dirigidas de estados, Gemini y “pendiente de implementación”; revisión del diff
  - Evidencia: estados 0001/0002/0005/0006 y ADR 0003 corregidos; README y roadmap distinguen Gemini API key de Gemini OAuth

- [x] **T2. Construir el sitio estático con identidad y contenido completos.**
  - Ficheros: `website/index.html`, `website/site.css`, `website/site.js`, `website/public/*`, `website/site.config.ts`
  - Verificación: `npm run site:build`; comprobación de secciones y ausencia de imports Tauri
  - Evidencia: build Vite correcto; 13 secciones, assets relativos y sin imports del runtime Tauri

- [x] **T3. Integrar el build en npm y CI.**
  - Ficheros: `package.json`, `package-lock.json`, `.github/workflows/ci.yml`
  - Verificación: `npm ci && npm run site:build && npm run check`; validación sintáctica de los workflows
  - Evidencia: `npm run check` sin diagnósticos, build correcto y ambos workflows parseados como YAML

- [x] **T4. Configurar el despliegue de GitHub Pages.**
  - Ficheros: `.github/workflows/pages.yml`
  - Verificación: revisión de permisos, triggers y artifact; push de la rama y PR

- [x] **T5. Validar la página en navegador.**
  - Ficheros: build `site-dist/` no versionado
  - Verificación: servidor local, recorrido de escritorio y móvil, enlaces internos, botones de copia, consola y captura visual
  - Evidencia: viewport 1280 y 390 px sin overflow; menú móvil y pestañas probados; imágenes completas y consola sin avisos ni errores

- [ ] **T6. Publicar y comprobar la URL real.**
  - Ficheros: configuración GitHub Pages y ejecución Actions
  - Verificación: CI verde, merge squash, workflow de Pages verde, API de Pages y HTTP 200 de página y recursos

## Cierre

- [x] Verificación del repositorio: `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check && npm run site:build`
- [x] Aplicación de macOS compilada **e instalada**: `npm run app:install`; compilada e instalada el 2026-08-05 a las 12:46:51 CEST
- [ ] Criterios de aceptación de `spec.md` repasados uno por uno, con su resultado real
- [ ] Documentación actualizada si lo aprendido contradice lo escrito
- [ ] `specs/README.md` actualizado
