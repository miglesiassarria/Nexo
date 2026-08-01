# Nexo · cómo se trabaja en este repositorio

## Metodología

Este repositorio usa **desarrollo dirigido por especificación**. Ante cualquier
petición de funcionalidad nueva, cambio de comportamiento o proveedor nuevo,
invoca la skill `spec-driven` y sigue su ciclo: `/spec` → `/design` → `/tasks` →
`/build`. No empieces a escribir código de producción sin una especificación
aceptada por el usuario.

Excepciones: erratas, textos de interfaz, formateo y comentarios. Un arreglo de
fallo no necesita especificación, pero **sí una prueba que lo reproduzca antes**
del arreglo.

Cuando falte información que cambie materialmente el trabajo, **pregunta**: máximo
3 preguntas por ronda, numeradas, cortas, en lenguaje del usuario y con la opción
recomendada marcada. Si hay un valor por defecto razonable, elígelo, decláralo y
sigue. Si el usuario no contesta, ejecuta declarando los supuestos.

## Verificación

Nada se considera terminado sin que esto pase, ejecutado de verdad:

```bash
cargo test --workspace && cargo clippy --workspace --all-targets && npm run check
```

**Y sin dejar la aplicación de macOS compilada e instalada:**

```bash
npm run app:install
```

Toda implementación termina así, informando de la hora del build **y** de la de lo
instalado en `/Applications`. Tres cosas distintas que se confunden con facilidad:

1. Las pruebas pasan.
2. La aplicación compila y empaqueta.
3. El usuario tiene esa versión instalada.

Las tres han fallado por separado en este proyecto, y varias veces se probó una
versión antigua creyendo que era la nueva. **Solo la tercera es «terminado».** Si
falla o se salta cualquiera de ellas, se dice; no se da por hecho.

`npm run tauri build` a secas solo cubre el punto 2, y sirve cuando no se quiere
tocar lo instalado. El guion de instalación cierra Nexo si está en marcha: no se
pierde nada, porque los datos y las credenciales viven fuera de la aplicación.

Recordatorio de fontanería: construir solo el DMG (`--bundles dmg`) **borra** el
`Nexo.app` existente, y el empaquetado falla si queda un volumen `Nexo` montado o un
`rw.*.dmg` de un intento anterior. Pide los dos en la misma orden.

Informa siempre del resultado real. Si una prueba falla, dilo con su salida. Si un
criterio de aceptación no se cumple, dilo en lugar de maquillarlo.

Para probar contra la realidad sin tocar los datos del usuario:
`NEXO_DATA_DIR=/ruta/temporal` y un puerto distinto del 8787.

## Flujo de trabajo con git y PR

Autorización permanente, dada el 2026-08-01: nunca se trabaja directamente sobre
`main`. Cada arreglo o especificación vive en su propia rama, se abre un PR y se
fusiona solo si el CI pasa. Esto ocurre siempre, sin que el usuario tenga que
pedirlo cada vez.

1. Rama nueva antes de tocar código: `fix/<slug>` para un arreglo de fallo,
   `spec/NNNN-<slug>` para una especificación (mismo número que su carpeta en
   `specs/`), `chore/<slug>` para el resto (CI, documentación, mantenimiento).
2. Al terminar la implementación y pasar la verificación local de este documento,
   se abre el PR contra `main` sin esperar instrucción.
3. El PR no se fusiona hasta que el workflow `.github/workflows/ci.yml` esté en
   verde. Si falla, se lee el error real y se corrige en la misma rama — nunca
   `--no-verify`, nunca forzar el merge, nunca reducir la verificación para que
   pase.
4. En verde, se fusiona (squash) y la rama origen se borra sola: el repositorio
   tiene `delete_branch_on_merge` activado para eso.

Esta autorización cubre crear ramas, abrir PRs y fusionarlos por este camino. No
cubre `push --force`, saltarse el CI, ni cambiar más configuración del
repositorio de la descrita aquí: eso sigue pidiéndose en el chat.

## Invariantes del producto

No se negocian dentro de una especificación. Cambiarlas exige un ADR nuevo que lo
haga explícito.

1. **Ningún secreto en SQLite.** Ni API keys, ni access tokens, ni refresh tokens.
   Van al almacén seguro del sistema; en la base de datos solo la referencia. Los
   tokens de aplicación se guardan hasheados.

2. **Nunca degradar en silencio.** Si el destino no soporta una capacidad
   solicitada, error `422` explícito que nombra la capacidad. Jamás eliminarla de
   la petición y seguir. Ver [ADR 0001](docs/adr/0001-oauth-de-suscripcion.md) y
   [contrato de proveedor](docs/contrato-proveedor.md).

3. **Cuatro estados de contabilidad, no dos.** `reported`, `estimated`,
   `subscription` y `unavailable`. Un coste cubierto por suscripción no se muestra
   como cero euros a secas: el coste marginal es cero y la cuota consumida es
   desconocida, y confundirlos es exactamente lo que el producto promete no hacer.
   Cuando el proveedor no informa, se marca no disponible; no se inventa una cifra.

4. **Los límites por aplicación son obligatorios en las vías de suscripción.** Es
   la mitigación del riesgo de multiplexación del ADR 0001, no una preferencia.
   Sin límite, Nexo se niega a atender la petición.

5. **El eje de credencial es de primer nivel.** El catálogo, los permisos, los
   límites y las estadísticas se indexan por proveedor **y** tipo de credencial. El
   mismo modelo por dos vías son dos filas distintas.

6. **Se conserva el dato original del proveedor.** Normalizar sirve para comparar;
   perder el original impide auditar.

7. **Lo frágil vive aislado.** Los valores de un flujo no soportado (client_id,
   endpoints, cabeceras, versiones) van en un único módulo por proveedor, con la
   fecha de su última verificación. Romperse debe afectar a un fichero.

8. **Nexo se identifica como Nexo.** No se suplanta el `User-Agent` ni la identidad
   de otro cliente cuando el flujo permita identificarse honestamente.

9. **Solo localhost.** El gateway escucha en `127.0.0.1`. No se expone en red sin
   autenticación, autorización y transporte seguro.

10. **El contenido de las conversaciones no se guarda por defecto.**

## Arquitectura, en una frase

Todo lo que el gateway necesita para funcionar vive en Rust (`crates/nexo-core`).
La capa de escritorio (`src-tauri`) y la interfaz (`src/`) solo orquestan y
presentan. Por eso Nexo sigue sirviendo con la ventana cerrada, y por eso la
interfaz es reemplazable.

Añadir un proveedor consiste en implementar su adaptador y describir sus
capacidades. Si para añadir un proveedor hay que tocar el router, el catálogo o las
estadísticas, el contrato está mal y hay que arreglar el contrato.

## Idioma

Documentación, comentarios, mensajes de error y textos de interfaz en **español**.
Identificadores de código y nombres de prueba en **inglés**. Los mensajes de error
los lee un humano que está atascado: tienen que decir qué pasó y qué hacer.

## Un aviso ganado a base de golpes

Tres suposiciones de diseño de este proyecto se demostraron falsas al probarlas
contra la realidad, no al razonar sobre ellas: el proveedor sí informa de tokens,
sí publica su catálogo, y su SSE llega sin cabecera `content-type`. Además, la
guarda que debía detectar una vía rota rechazaba una vía que funcionaba.

Prueba contra lo real en cuanto puedas, y desconfía de las conclusiones a las que
llegaste solo leyendo.
