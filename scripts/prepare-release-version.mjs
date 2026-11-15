// Solo modifica el checkout temporal de la compilación de GitHub Actions.
import fs from 'node:fs';

const tag = process.argv[2] ?? '';
if (!/^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(tag)) {
  console.error(`Etiqueta inválida: ${tag}. Usa vX.Y.Z o X.Y.Z.`);
  process.exit(1);
}
const version = tag.replace(/^v/, '');
for (const path of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json']) {
  const data = JSON.parse(fs.readFileSync(path, 'utf8'));
  data.version = version;
  if (path === 'package-lock.json') data.packages[''].version = version;
  fs.writeFileSync(path, `${JSON.stringify(data, null, 2)}\n`);
}
for (const [path, pattern] of [
  ['Cargo.toml', /(\[workspace\.package\]\s*\nversion\s*=\s*")[^"]+(")/],
  ['src-tauri/Cargo.toml', /(nexo-core\s*=\s*\{\s*version\s*=\s*")[^"]+(")/],
]) {
  const contents = fs.readFileSync(path, 'utf8');
  if (!pattern.test(contents)) throw new Error(`No se encontró la versión en ${path}`);
  fs.writeFileSync(path, contents.replace(pattern, (_, prefix, suffix) => `${prefix}${version}${suffix}`));
}
console.log(`Versión preparada: ${version}`);
