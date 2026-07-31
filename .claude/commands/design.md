---
description: Convierte una especificación aceptada en un diseño técnico
argument-hint: [número de spec, opcional]
---

Invoca la skill `spec-driven` y ejecuta su fase `/design`.

Especificación objetivo: $ARGUMENTS
Si no se indica ninguna, usa la más reciente que esté en estado `spec` en
`specs/README.md`.

Lee `spec.md` completa y el código que se va a tocar de verdad, no de memoria.
Escribe `design.md` nombrando los ficheros afectados, cada decisión con su
alternativa descartada y el motivo, y qué puede romperse junto a cómo se detectará.

Si el diseño demuestra que la especificación es irrealizable o que resuelve el
problema equivocado, **dilo y vuelve a `/spec`** en lugar de forzar un diseño que
no se sostiene. Si hace falta un ADR nuevo, propónlo.
