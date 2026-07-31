---
name: spec-driven
description: Metodología de desarrollo de Nexo. Úsala siempre que el usuario pida una funcionalidad nueva, un cambio de comportamiento o un proveedor nuevo, y cuando pregunte cómo se trabaja en este repositorio. Cubre el ciclo spec → design → tasks → build → verificación, con preguntas cortas al usuario cuando falte información.
---

# Desarrollo dirigido por especificación en Nexo

Nada sustancial se implementa sin una especificación escrita y acordada. La
especificación no es burocracia: es el sitio donde se descubre que el problema no
era el que parecía, antes de gastar código en el equivocado.

Esto no es teoría en este repositorio. En su primer día, tres suposiciones del
diseño se demostraron falsas al contrastarlas con la realidad (el proveedor sí
informa de tokens, sí publica su catálogo, y su SSE llega sin `content-type`). El
ciclo existe para que esas correcciones se hagan en un documento de una página y
no en tres fases construidas encima.

## El ciclo

```
/spec  →  /design  →  /tasks  →  /build
  ↑                                 │
  └─────── la realidad corrige ─────┘
```

Cada fase vive en `specs/NNNN-nombre-corto/` y produce un fichero.

**La puerta que importa es `/spec`.** Ahí se decide qué se construye y para qué, y
ahí es donde la opinión del usuario es decisiva: no se pasa a diseñar sin su visto
bueno. `/design` y `/tasks` se le muestran, pero no le piden aprobación: son
consecuencia técnica de algo que ya aceptó, y pedir permiso tres veces seguidas es
fricción, no rigor.

Solo se vuelve a preguntar si el diseño **cambia lo acordado**: si revela que la
especificación era irrealizable, que resuelve el problema equivocado o que hay que
recortar el alcance. Entonces sí, porque eso ya no es cómo, es qué.

| Fase | Fichero | Pregunta que responde |
| --- | --- | --- |
| `/spec` | `spec.md` | Qué problema, para quién, y cómo se sabrá que está resuelto |
| `/design` | `design.md` | Cómo, qué se toca, qué puede salir mal |
| `/tasks` | `tasks.md` | En qué orden, y cómo se verifica cada paso |
| `/build` | código | La implementación, con las tareas tachándose |

## Reglas que hacen que esto sirva de algo

### 1. Los criterios de aceptación son ejecutables

Un criterio que no se puede comprobar con una orden o una prueba no es un
criterio, es un deseo. Prohibido «debe ser rápido»; obligatorio «`GET /v1/models`
responde en menos de 200 ms con 50 modelos en catálogo, medido con
`cargo test -p nexo-core catalog_latency`».

Cada criterio de `spec.md` lleva su forma de verificarse. `/build` no da una tarea
por terminada hasta que su verificación pasa de verdad, ejecutada, no supuesta.

### 2. Preguntas cortas, pocas, y con recomendación

Cuando falte información que cambie materialmente el trabajo, se pregunta. Reglas:

- **Máximo 3 preguntas por ronda.** Numeradas.
- **Cortas y en lenguaje del usuario**, no del código. Mal: «¿qué `CredentialKind`
  usamos?». Bien: «¿Nexo solo debe consumir los modelos, o también arrancarlos?».
- **Con la opción recomendada marcada**, para que responder sea barato.
- **Solo si la respuesta cambia lo que se construye.** Si hay un valor por defecto
  razonable, se elige, se declara y se sigue. Preguntar lo obvio es hacer trabajar
  al usuario en tu lugar.
- **Si no contesta, se ejecuta** declarando explícitamente el supuesto asumido.

### 3. Lo que se descubre vuelve al documento

Si al implementar resulta que la especificación estaba equivocada, se corrige la
especificación **y** se anota qué se aprendió. Un documento que contradice al
código es peor que no tener documento.

Cuando lo aprendido invalida una decisión de arquitectura, se actualiza el ADR
correspondiente en `docs/adr/`, no solo la especificación.

### 4. El alcance se recorta, no se estira

`spec.md` tiene una sección **Fuera de alcance** que es obligatoria y no puede
estar vacía. Si no sabes qué dejar fuera, no has entendido el problema.

### 5. Las invariantes del producto no se negocian en una especificación

Hay decisiones que están por encima de cualquier funcionalidad nueva. Están en
`CLAUDE.md` y en los ADR. Una especificación que las rompa se rechaza, o se
convierte primero en un ADR nuevo que las cambie de forma explícita.

## Cómo se comporta cada fase

### `/spec <lo que quiere el usuario>`

1. Lee `specs/README.md`, los ADR y el código relevante antes de preguntar nada:
   media pregunta se responde leyendo.
2. Identifica lo que falta y **pregunta** (regla 2). Si no falta nada, no pregunta.
3. Crea `specs/NNNN-nombre/spec.md` desde la plantilla.
4. Enseña al usuario el resumen y los criterios de aceptación, y espera su visto
   bueno o sus correcciones.

Contenido de `spec.md`: problema, quién lo tiene, comportamiento esperado,
criterios de aceptación verificables, fuera de alcance, riesgos, supuestos
declarados.

### `/design [NNNN]`

Traduce el qué en el cómo. Debe nombrar:

- Los ficheros y contratos que se tocan.
- Las decisiones tomadas **con su alternativa descartada y por qué**.
- Lo que puede romperse, y cómo se detectará cuando se rompa.
- Si hace falta un ADR nuevo, se dice aquí.

Si el diseño revela que la especificación era irrealizable, se vuelve a `/spec`.

### `/tasks [NNNN]`

Descompone el diseño en tareas que caben en una sesión, cada una con:

- Una casilla `- [ ]`.
- El fichero o ficheros que toca.
- **Su verificación**: la orden que demuestra que está hecha.

Las tareas se ordenan para que el sistema esté funcionando en todo momento. No se
deja el repositorio roto entre tareas.

### `/build [NNNN]`

Ejecuta las tareas en orden. Por cada una:

1. Implementa.
2. Ejecuta su verificación. Si falla, arregla; no marca la casilla.
3. Marca la casilla en `tasks.md`.

Al terminar, ejecuta la verificación completa del repositorio y comprueba **uno
por uno** los criterios de aceptación de `spec.md`, informando del resultado real.
Si alguno no se cumple, se dice, no se maquilla.

## Verificación del repositorio

```bash
cargo test --workspace && cargo clippy --workspace --all-targets && npm run check
```

Una especificación no está terminada mientras esto no pase.

## Cuándo saltarse el ciclo

Para lo trivial y reversible: una errata, un texto de interfaz, un `rustfmt`, un
comentario. Si dudas de si algo es trivial, no lo es.

Un arreglo de un fallo tampoco necesita especificación completa, pero **sí
necesita una prueba que lo reproduzca** antes del arreglo. Es el equivalente
mínimo: la prueba es la especificación de ese fallo.
