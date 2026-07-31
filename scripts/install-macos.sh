#!/usr/bin/env bash
# Compila e INSTALA Nexo en /Applications, dejándolo en marcha.
#
#   npm run app:install
#
# Existe porque «compilado» e «instalado» son cosas distintas y confundirlas ya ha
# hecho perder el tiempo varias veces: se probaba una versión antigua creyendo que
# era la nueva. Toda implementación debe terminar aquí, no en el bundle.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

app="target/release/bundle/macos/Nexo.app"
target="/Applications/Nexo.app"

echo "▸ Compilando…"
npm run tauri build

if [ ! -d "$app" ]; then
  echo "✗ no se generó $app" >&2
  exit 1
fi

built=$(stat -f '%Sm' "$app/Contents/MacOS/nexo")

if pgrep -f "Nexo.app/Contents/MacOS/nexo" >/dev/null 2>&1; then
  echo "▸ Cerrando el Nexo en marcha…"
  osascript -e 'quit app "Nexo"' 2>/dev/null || true
  for _ in $(seq 1 15); do
    pgrep -f "Nexo.app/Contents/MacOS/nexo" >/dev/null 2>&1 || break
    sleep 1
  done
  # Los datos y las credenciales viven fuera de la app, así que no se pierde nada.
  pkill -f "/Applications/Nexo.app/Contents/MacOS/nexo" 2>/dev/null || true
  sleep 1
fi

# Llaves obligatorias: pegar un carácter multibyte a $var hace que bash lo
# lea como parte del nombre y falle con `unbound variable`.
echo "▸ Instalando en ${target}…"
rm -rf "$target"
ditto "$app" "$target"

echo "▸ Arrancando…"
open "$target"

echo
echo "✓ Nexo instalado y en marcha."
echo "  compilado:  $built"
echo "  instalado:  $(stat -f '%Sm' "$target/Contents/MacOS/nexo")"
