# Diseño

Un workflow de `push` de etiquetas valida `vX.Y.Z` o `X.Y.Z`, prepara versiones temporalmente en Cargo, npm y Tauri, compila el DMG universal y solo entonces crea la release con `gh release create --verify-tag` y el DMG más su suma SHA-256. Se descarta mantener números separados por requerir cambios manuales previos y permitir que la etiqueta y el ejecutable diverjan. No se reescriben etiquetas ni releases existentes. La firma Developer ID y notarización requieren credenciales que no forman parte de este cambio. No cambia ningún invariante; no hace falta ADR.
