# 0010 · Web pública y documentación en GitHub Pages

- **Estado:** tasks
- **Creada:** 2026-08-05
- **Pedida por:** el usuario: «incorpórala y déjalo ya todo subido a GitHub para que se pueda ver ahí»
- **Aprobación:** explícita en el mismo mensaje, después de revisar y aceptar el alcance propuesto en la auditoría previa

## Problema

Nexo se entiende hoy leyendo el README, el roadmap, varios ADR, contratos, specs y
el código. Un tercero que recibe el enlace del repositorio no tiene una entrada
visual, guiada y públicamente navegable que explique qué resuelve, cómo se instala,
cómo se integra una aplicación, qué proveedores existen, qué riesgos tiene la vía
de suscripción y qué datos registra.

Además, GitHub Pages no está habilitado y algunas afirmaciones estables de README,
roadmap y ADR han quedado por detrás del código ya fusionado. Publicarlas sin
reconciliarlas convertiría esa deriva en documentación pública incorrecta.

## Comportamiento esperado

- El repositorio publica una web en GitHub Pages con una URL estable que se puede
  entregar a terceros.
- La web presenta Nexo como gateway local, no como un servicio cloud, y explica
  visualmente el recorrido aplicación → Nexo → proveedor → estadísticas locales.
- Un visitante puede entender capacidades actuales, proveedores y vías, primeros
  pasos, integración OpenAI-compatible, permisos, límites, estadísticas,
  privacidad, acceso LAN, riesgos del OAuth de suscripción y metodología SDD.
- La web funciona como sitio estático independiente. No simula que el gateway,
  OAuth, Keychain o SQLite se ejecutan en GitHub Pages.
- El diseño reutiliza la identidad de Nexo, es responsive, accesible y mantiene
  una dirección visual propia de manual técnico/editorial.
- Cada push aceptado en `main` vuelve a desplegar el sitio mediante GitHub Actions.

## Criterios de aceptación

| # | Criterio | Cómo se verifica |
| --- | --- | --- |
| 1 | `npm run site:build` genera un sitio estático completo sin depender del runtime de Tauri | Ejecutar `npm run site:build`; comprobar `site-dist/index.html` y que `rg '@tauri-apps|invoke\(' website site-dist` no encuentra dependencias de ejecución |
| 2 | La página cubre propuesta de valor, arquitectura, proveedores, inicio rápido, integración, permisos, estadísticas, seguridad/privacidad, LAN, SDD, roadmap y preguntas frecuentes | Comprobación estructural de secciones en `website/index.html` y recorrido visual local |
| 3 | La navegación, los ejemplos copiables y la adaptación móvil funcionan sin errores de JavaScript | Abrir el build local, probar navegación/copia/menú en escritorio y viewport móvil; revisar consola |
| 4 | El sitio explica de forma visible que la vía de suscripción no es oficial y que GitHub Pages no ejecuta Nexo | Comprobación visual y búsqueda de los avisos en el HTML generado |
| 5 | README, roadmap, ADR 0003 e índice de specs dejan de contradecir las funcionalidades ya fusionadas que la web publica | Revisión del diff y búsquedas de Gemini, Pages, estados 0001/0002/0005/0006 y estado del ADR 0003 |
| 6 | Existe un workflow de Pages separado del CI, con permisos mínimos, build reproducible y despliegue solo desde `main` o manual | Validar YAML y revisar `.github/workflows/pages.yml`; ejecución real en GitHub Actions tras fusionar |
| 7 | GitHub Pages queda habilitado y la URL pública responde con HTTP 200, carga estilos, scripts e imágenes | Consultar la API de Pages y hacer peticiones HTTP reales a la URL publicada y a sus recursos |
| 8 | La verificación completa del repositorio sigue en verde y la aplicación macOS queda compilada e instalada | `cargo test --workspace && cargo clippy --workspace --all-targets && npm run check && npm run site:build`; después `npm run app:install` |

## Fuera de alcance

- **Ejecutar Nexo dentro del navegador.** GitHub Pages no puede alojar el gateway
  Rust, OAuth, Keychain, SQLite ni proveedores; la web documenta y demuestra, no
  sustituye la aplicación.
- **Publicar instaladores o crear una release.** La web enlaza el repositorio y
  explica la compilación actual; firma, notarización y distribución siguen en la
  fase 5 del roadmap.
- **Dominio personalizado, analítica o formularios con backend.** Se usará el
  dominio estándar de GitHub Pages y no se enviará telemetría.
- **Traducir el sitio a otros idiomas.** La primera versión sigue la regla del
  repositorio y se publica en español.
- **Completar funcionalidades pendientes del producto.** La web diferencia con
  claridad lo disponible de lo planificado.
- **Rediseñar el panel Tauri.** La web es una superficie independiente.

## Supuestos asumidos

- La URL pública será `https://miglesiassarria.github.io/Nexo/`, al ser un
  repositorio público de proyecto y no un dominio de usuario.
- Se reutiliza Vite ya presente, sin añadir un framework documental ni nuevas
  dependencias de producción.
- El sitio será una página editorial de navegación interna, suficiente para la
  primera versión y más fácil de mantener que un generador documental separado.
- El mensaje del usuario aprueba el alcance descrito en la respuesta anterior:
  web pública completa, demo visual estática y reconciliación documental previa.

## Riesgos

- Un base path incorrecto puede dejar la página sin CSS o imágenes bajo `/Nexo/`.
- GitHub Pages requiere configuración externa además del workflow; un build verde
  no demuestra por sí solo que la URL pública esté activa.
- La documentación hardcoded puede volver a quedarse atrás; la web debe enlazar
  las fuentes estables y evitar cifras o listas volátiles innecesarias.
- La vía de suscripción podría parecer oficial si el aviso pierde prominencia.
- Una web visualmente compleja puede degradar accesibilidad o rendimiento en móvil.

## Invariantes que esto no puede romper

- **2. Nunca degradar en silencio:** la web distingue capacidades reales y futuras.
- **3. Cuatro estados de contabilidad:** se explican sin convertir suscripción en
  coste cero sin contexto.
- **4. Límites obligatorios en suscripción:** forman parte del recorrido público.
- **8. Nexo se identifica como Nexo:** no se presenta como producto oficial de un
  proveedor.
- **9. Solo localhost por defecto:** el acceso LAN aparece como opt-in con TLS.
- **10. No guardar conversaciones por defecto:** privacidad visible y precisa.
