---
description: Implementa las tareas pendientes de una especificación
argument-hint: [número de spec, opcional]
---

Invoca la skill `spec-driven` y ejecuta su fase `/build`.

Especificación objetivo: $ARGUMENTS
Si no se indica ninguna, usa la más reciente que tenga tareas sin marcar.

Por cada tarea: implementa, **ejecuta su verificación de verdad**, y solo entonces
marca la casilla. Si la verificación falla, arréglalo; no marques la casilla ni
pases a la siguiente.

Al acabar, ejecuta la verificación del repositorio completa y repasa **uno por uno**
los criterios de aceptación de `spec.md`, informando del resultado real de cada uno.
Si alguno no se cumple, dilo con claridad en lugar de maquillarlo.

Si al implementar descubres que la especificación estaba equivocada, corrígela y
anota qué se aprendió. Si lo aprendido invalida una decisión de arquitectura,
actualiza también el ADR correspondiente.
