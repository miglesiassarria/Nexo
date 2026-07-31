---
description: Escribe la especificación de una funcionalidad nueva, preguntando lo que falte
argument-hint: [lo que quieres conseguir]
---

Invoca la skill `spec-driven` y ejecuta su fase `/spec` para esto:

$ARGUMENTS

Antes de preguntar nada, lee `specs/README.md`, `CLAUDE.md`, los ADR de `docs/adr/`
y el código relevante: media pregunta se responde leyendo.

Después, si falta información que cambie materialmente lo que hay que construir,
haz **como máximo 3 preguntas** numeradas, cortas, en lenguaje del usuario y con la
opción que recomiendas marcada. Si no falta nada, no preguntes: escribe la
especificación y enséñala.

Cuando tengas las respuestas (o decidas seguir con supuestos declarados), crea
`specs/NNNN-nombre-corto/spec.md` con `scripts/new-spec.sh`, rellénalo, y termina
mostrando el problema, los criterios de aceptación y lo que queda fuera de alcance
para que el usuario los confirme o los corrija.
