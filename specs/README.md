# Especificaciones

Índice de todo lo que se ha especificado en Nexo. La metodología está en
[`.claude/skills/spec-driven/SKILL.md`](../.claude/skills/spec-driven/SKILL.md) y
resumida en el [README](../README.md#cómo-se-desarrolla-aquí).

| # | Título | Estado | Notas |
| --- | --- | --- | --- |
| [0001](0001-proveedor-local-lm-studio/spec.md) | Usar los modelos de LM Studio desde Nexo | `hecho` | Verificada contra LM Studio 0.4.20. Ollama queda para otra |
| [0002](0002-proveedores-genericos-y-opencode-zen/spec.md) | Proveedores OpenAI-compatible añadidos por el usuario, y OpenCode Zen | `hecho` | Verificada contra Zen real, 60 modelos. Anthropic-compatible aplazado a petición del usuario |
| [0003](0003-vista-de-proveedores-legible/spec.md) | Una vista de Proveedores que se lee de un vistazo | `build` | Arregla además que un proveedor propio con API key salía duplicado y en la sección de OpenAI |
| [0004](0004-modelos-permitidos-por-aplicacion/spec.md) | Elegir qué modelos sirve Nexo a cada aplicación | `build` | La mitad del almacenamiento ya existía sin usarse; el catálogo no aplicaba la misma regla que el gateway |

Estados: `spec` · `design` · `tasks` · `build` · `hecho` · `descartado`

## Qué va aquí y qué no

Aquí van los **cambios**: funcionalidad nueva, comportamiento distinto, un
proveedor más. Cada uno en su carpeta `NNNN-nombre-corto/`.

Aquí **no** va:

- La **visión del producto** ni sus características estables → [`docs/producto.md`](../docs/producto.md).
- Las **decisiones de arquitectura** y los riesgos aceptados → [`docs/adr/`](../docs/adr/).
- El **plan a medio plazo** → [`ROADMAP.md`](../ROADMAP.md).
- Los **contratos** entre piezas → [`docs/contrato-proveedor.md`](../docs/contrato-proveedor.md) y [`docs/modelo-datos.md`](../docs/modelo-datos.md).

Una especificación es temporal: describe un cambio y, cuando se completa, lo que
haya aprendido de duradero se lleva a `docs/`. Lo que queda en `specs/` es el
registro de por qué se hizo así.

## Empezar una

```bash
scripts/new-spec.sh "nombre corto de lo que quieres"
```

O, mejor, deja que se ocupe el ciclo: `/spec lo que quieres conseguir`.
