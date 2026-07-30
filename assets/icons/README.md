# Iconos de Nexo

Este directorio contiene el sistema completo de iconos de Nexo basado en la dirección visual **Core Node**.

## Identidad

El símbolo combina:

- Una `N` geométrica que identifica a Nexo.
- Un nodo central que representa el gateway.
- Cuatro conexiones que representan aplicaciones, proveedores y modelos.

Paleta principal:

| Color | Hex | Uso |
| --- | --- | --- |
| Midnight navy | `#071A3D` | Fondo y símbolo corporativo |
| Cobalt | `#1D4FFF` | Conexiones horizontales y nodo central |
| Cyan | `#13CBD4` | Conexiones verticales |
| White | `#FFFFFF` | Letra sobre fondo oscuro |
| Black | `#000000` | Iconos template y monocromos |

## Estructura

### `source/`

Fuentes maestras y material de referencia:

- `nexo-app-icon.svg`: fuente vectorial del icono de aplicación.
- `nexo-symbol-color.svg`: símbolo corporativo sin fondo.
- `nexo-tray-template.svg`: fuente monocroma para barra de estado.
- `nexo-app-icon-1024.png`: máster rasterizado del icono.
- `nexo-symbol-color-1024.png`: máster rasterizado del símbolo.
- `nexo-tray-template-1024.png`: máster rasterizado del icono monocromo.
- `core-node-concept-board.png`: propuesta visual original seleccionada.

Los archivos derivados deben regenerarse siempre desde los SVG. No se deben editar individualmente.

### `tauri/`

Conjunto convencional preparado para Tauri:

- `icon.icns`
- `icon.ico`
- `icon.png`
- `32x32.png`
- `128x128.png`
- `128x128@2x.png`
- Logos `Square*` y `StoreLogo.png` para Windows.

Cuando se inicialice la aplicación, estos archivos pueden copiarse a `src-tauri/icons/` o configurarse directamente desde `tauri.conf.json`.

El icono de la barra de estado no debe utilizar el icono de aplicación. Debe cargarse desde `tray/` para garantizar legibilidad y adaptación al tema del sistema.

### `macos/`

- `Nexo.icns`: contenedor completo para la aplicación.
- `AppIcon.iconset/`: tamaños fuente de 16 a 1024 píxeles.

Para la barra de menús deben utilizarse:

- `tray/macos/nexoTemplate.png`
- `tray/macos/nexoTemplate@2x.png`

El sufijo `Template` permite que macOS adapte automáticamente el color del símbolo al modo claro u oscuro.

### `windows/`

- `Nexo.ico`: icono multirresolución de 16 a 256 píxeles.
- `png/`: exportaciones PNG independientes.
- `Square*Logo.png` y `StoreLogo.png`: recursos para paquetes MSIX y Microsoft Store.

La bandeja del sistema dispone de versiones claras, oscuras y contenedores ICO en `tray/windows/`.

### `linux/`

La estructura `hicolor/<tamaño>/apps/nexo.png` puede instalarse directamente siguiendo el estándar de temas de iconos de escritorios Linux. Incluye tamaños de 16 a 1024 píxeles.

Los indicadores de estado están disponibles en `tray/linux/` en versiones claras y oscuras.

### `web/`

Recursos auxiliares para documentación o una futura interfaz web:

- Favicons PNG e ICO.
- `apple-touch-icon.png`.
- Iconos PWA de 192 y 512 píxeles.
- Variante maskable de 512 píxeles.

## Uso por contexto

| Contexto | Archivo recomendado |
| --- | --- |
| Tauri | `tauri/icon.*` y PNG asociados |
| Aplicación macOS | `macos/Nexo.icns` |
| Barra de menús macOS | `tray/macos/nexoTemplate.png` |
| Aplicación Windows | `windows/Nexo.ico` |
| Bandeja Windows, tema claro | `tray/windows/nexo-tray-dark.ico` |
| Bandeja Windows, tema oscuro | `tray/windows/nexo-tray-light.ico` |
| Aplicación Linux | `linux/hicolor/<tamaño>/apps/nexo.png` |
| Indicador Linux | `tray/linux/nexo-tray-<tema>-<tamaño>.png` |
| Fuente editable | `source/*.svg` |

## Reglas de uso

- No deformar ni girar el símbolo.
- No cambiar la relación entre la `N`, el nodo y las conexiones.
- No utilizar el icono de aplicación en la barra de estado.
- Mantener las versiones monocromas sin degradados, sombras ni colores.
- Utilizar siempre el tamaño más cercano al tamaño de presentación para evitar reescalados innecesarios.
- En Windows y Linux, elegir el icono de bandeja claro u oscuro en función del tema.
