# Tareas

- [x] Preparar y probar las versiones derivadas de la etiqueta, incluidos casos inválidos: `node --test scripts/tests/prepare-release-version.test.mjs`.
- [x] Crear y revisar workflow y documentación: `actionlint` para ambos workflows y `npm run site:build`. La publicación real se verifica en la última tarea.
- [ ] Verificación completa: `npm run check`, `cargo clippy --workspace --all-targets`, pruebas salvo LAN, build web y `actionlint` pasan. El DMG universal se generó y `hdiutil verify` y `lipo -archs` verificaron su integridad y arquitecturas arm64/x86_64. Aplicación compilada e instalada el 15/11/2026 a las 19:04:37. La prueba LAN completa permanece fallida por la interfaz de túnel 100.96.0.1 de esta máquina; no se da por aprobada.
- [ ] Verificar publicación real mediante una etiqueta real después de integrar el workflow.
