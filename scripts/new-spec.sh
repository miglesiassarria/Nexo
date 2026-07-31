#!/usr/bin/env bash
# Crea el andamiaje de una especificación nueva a partir de las plantillas.
#
#   scripts/new-spec.sh "proveedores locales lm studio y ollama"
#
# Numera de forma determinista a partir de las que ya existen, para que dos
# especificaciones no compartan número.
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "uso: $0 \"nombre corto de la especificación\"" >&2
  exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
specs="$root/specs"

# El slug se genera con Python para no depender de iconv ni del dialecto de sed:
# el `sed` de BSD no soporta `\+` en expresiones basicas y dejaba los espacios.
slug=$(printf '%s' "$1" | python3 -c '
import re, sys, unicodedata
raw = sys.stdin.read()
plain = unicodedata.normalize("NFKD", raw).encode("ascii", "ignore").decode()
slug = re.sub(r"[^a-z0-9]+", "-", plain.lower()).strip("-")[:48].strip("-")
print(slug or "sin-nombre")
')

last=0
for d in "$specs"/[0-9][0-9][0-9][0-9]-*; do
  [ -d "$d" ] || continue
  n=$(basename "$d" | cut -d- -f1)
  n=$((10#$n))
  [ "$n" -gt "$last" ] && last=$n
done
num=$(printf '%04d' $((last + 1)))

dir="$specs/$num-$slug"
if [ -d "$dir" ]; then
  echo "ya existe $dir" >&2
  exit 1
fi

mkdir -p "$dir"
today=$(date +%Y-%m-%d)
for f in spec.md design.md tasks.md; do
  sed -e "s/^# NNNN/# $num/" -e "s/AAAA-MM-DD/$today/" "$specs/TEMPLATE/$f" > "$dir/$f"
done

echo "$dir"
