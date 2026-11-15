# 0021 · Release automática para macOS

Estado: build. El usuario solicita que cada etiqueta numerada genere una release de GitHub con esa numeración y un instalador de macOS descargable.

## Criterios

- Al subir una etiqueta `vX.Y.Z` o `X.Y.Z`, GitHub compila el código de esa etiqueta y publica una release con el mismo nombre y un DMG universal para Apple Silicon e Intel. Comprobar el workflow y, cuando se publique una etiqueta real, su ejecución, release y DMG.
- La versión de la aplicación y del paquete se deriva de la etiqueta durante la compilación sin modificar el repositorio. Comprobar con el test de preparación de versión y el bundle.
- Una compilación fallida no publica una release sin instalador. Comprobar el orden de pasos del workflow.
- El README permite encontrar la descarga y explica la advertencia de macOS para binarios sin notarizar. Inspeccionar el texto.

## Fuera de alcance

Firma y notarización de Apple sin credenciales, instaladores de Windows y Linux, actualización automática y creación automática de etiquetas. Una etiqueta debe seguir el formato numérico indicado; otras etiquetas fallan de forma explícita.

## Riesgos

La firma ad hoc no ofrece notarización; Gatekeeper puede impedir la apertura hasta que el usuario autorice la aplicación. El primer workflow real y la compatibilidad Intel requieren comprobación en GitHub y hardware correspondiente.
