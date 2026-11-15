import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, copyFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { test } from 'node:test';

const files = ['package.json', 'package-lock.json', 'Cargo.toml', 'src-tauri/Cargo.toml', 'src-tauri/tauri.conf.json', 'scripts/prepare-release-version.mjs'];
test('la etiqueta determina todas las versiones y se rechazan etiquetas inválidas', () => {
  const root = mkdtempSync(join(tmpdir(), 'nexo-release-test-'));
  try {
    for (const file of files) {
      mkdirSync(dirname(join(root, file)), { recursive: true });
      copyFileSync(resolve(file), join(root, file));
    }
    for (const tag of ['v1.2.3', '2.3.4']) {
      execFileSync('node', ['scripts/prepare-release-version.mjs', tag], { cwd: root });
      const version = tag.replace(/^v/, '');
      for (const file of ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json']) {
        const data = JSON.parse(readFileSync(join(root, file), 'utf8'));
        assert.equal(data.version, version);
        if (file === 'package-lock.json') assert.equal(data.packages[''].version, version);
      }
      assert.match(readFileSync(join(root, 'Cargo.toml'), 'utf8'), new RegExp(`version = "${version.replaceAll('.', '\\.') }"`));
      assert.ok(readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf8').includes(`nexo-core = { version = "${version}"`));
    }
    const before = readFileSync(join(root, 'Cargo.toml'), 'utf8');
    for (const tag of ['otro', 'v1.2', 'v01.2.3', 'v1.2.3-beta']) {
      assert.notEqual(spawnSync('node', ['scripts/prepare-release-version.mjs', tag], { cwd: root }).status, 0);
      assert.equal(readFileSync(join(root, 'Cargo.toml'), 'utf8'), before);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
