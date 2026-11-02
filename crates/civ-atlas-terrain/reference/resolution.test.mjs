// Only provenance is checked: neither original world algorithms nor Rust fixtures execute.
// Supply the existing offline checkout/prefix through ATLAS_ORACLE_ROOT / ATLAS_ORACLE_PREFIX.
import assert from 'node:assert/strict';
import test from 'node:test';
import { cpSync, existsSync, mkdirSync, mkdtempSync, rmSync, symlinkSync } from 'node:fs';
import { basename, dirname, join, resolve, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { SOURCE_PINS, DEPENDENCY_PINS, verifyOriginalModuleProvenance, verifyDependencyProvenance } from './source-pins.mjs';

const root = resolve(process.env.ATLAS_ORACLE_ROOT);
const prefix = resolve(process.env.ATLAS_ORACLE_PREFIX);
const reference = dirname(fileURLToPath(import.meta.url));
const check = `import {verifyOriginalModuleProvenance} from ${JSON.stringify(pathToFileURL(join(reference, 'source-pins.mjs')).href)}; verifyOriginalModuleProvenance(process.argv[1], process.argv[2]);`;

test('a resolver that ignores the importer argument is rejected', () => {
  const result = spawnSync(process.execPath,
    ['--input-type=module', '--eval', check, root, prefix],
    { encoding: 'utf8', timeout: 10000, env: { ...process.env, NODE_OPTIONS: '' } });
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /experimental-import-meta-resolve/);
});

test('the actual offline checkout junction resolves to the verified entries', () => {
  assert.equal(verifyOriginalModuleProvenance(root, prefix).length,
    Object.keys(SOURCE_PINS).length + Object.keys(DEPENDENCY_PINS).length);
});

test('alternate direct and transitive dependency locations fail before world execution', () => {
  const temp = mkdtempSync(join(tmpdir(), 'sekai-oracle-resolution-'));
  assert.equal(dirname(temp), resolve(tmpdir()));
  assert.ok(basename(temp).startsWith('sekai-oracle-resolution-'));
  try {
    const upstream = join(temp, 'upstream');
    const oracle = join(temp, 'oracle');
    const modules = join(oracle, 'node_modules');
    mkdirSync(upstream, { recursive: true });
    cpSync(join(root, 'package.json'), join(upstream, 'package.json'));
    for (const file of Object.keys(SOURCE_PINS)) {
      const target = join(upstream, 'src/gen', file);
      mkdirSync(dirname(target), { recursive: true });
      cpSync(join(root, 'src/gen', file), target);
    }
    for (const name of ['simplex-noise', 'd3-delaunay', 'delaunator', 'robust-predicates']) {
      cpSync(join(prefix, 'node_modules', name), join(modules, name), { recursive: true });
    }
    symlinkSync(modules, join(upstream, 'node_modules'), process.platform === 'win32' ? 'junction' : 'dir');

    // Identical bytes in another directory still cannot stand in for the verified files.
    const direct = join(upstream, 'src/gen/node_modules/simplex-noise');
    cpSync(join(modules, 'simplex-noise'), direct, { recursive: true });
    assert.throws(() => verifyOriginalModuleProvenance(upstream, oracle), /dependency location/);
    for (const script of ['terrain.mjs', 'fine-world.mjs', 'fine.mjs']) {
      const output = join(temp, `${script}.rs`);
      const result = spawnSync(process.execPath,
        ['--experimental-import-meta-resolve', join(reference, script), upstream, oracle, output],
        { encoding: 'utf8', timeout: 10000 });
      assert.equal(result.status, 1, result.stderr);
      assert.match(result.stderr, /dependency location/);
      assert.equal(existsSync(output), false);
      assert.equal(existsSync(join(oracle, 'world-instrumented.mts')), false);
    }
    assert.ok(resolve(direct).startsWith(temp + sep));
    rmSync(direct, { recursive: true });

    for (const [script, directory] of [['geometry.mjs', 'geometry-modules'], ['primitives.mjs', 'primitive-modules']]) {
      const generated = join(oracle, directory);
      const shadow = join(generated, 'node_modules/simplex-noise');
      cpSync(join(modules, 'simplex-noise'), shadow, { recursive: true });
      const output = join(temp, `${script}.rs`);
      const result = spawnSync(process.execPath,
        ['--experimental-import-meta-resolve', join(reference, script), upstream, oracle, output],
        { encoding: 'utf8', timeout: 10000 });
      assert.equal(result.status, 1, result.stderr);
      assert.match(result.stderr, /dependency location/);
      assert.equal(existsSync(output), false);
      assert.ok(resolve(shadow).startsWith(temp + sep));
      rmSync(shadow, { recursive: true });
      assert.equal(verifyDependencyProvenance(oracle, join(generated, 'noise.mjs'),
        script === 'geometry.mjs' ? join(generated, 'geometry.mjs') : undefined).length,
      script === 'geometry.mjs' ? Object.keys(DEPENDENCY_PINS).length : 1);
    }

    // A nested dependency is resolved from the real Delaunator parent, not from root/prefix.
    const nested = join(modules, 'delaunator/node_modules/robust-predicates');
    cpSync(join(modules, 'robust-predicates'), nested, { recursive: true });
    // Use a fresh resolver after changing fixture directories: Node caches resolved URLs.
    const result = spawnSync(process.execPath,
      ['--experimental-import-meta-resolve', '--input-type=module', '--eval', check, upstream, oracle],
      { encoding: 'utf8', timeout: 10000 });
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /dependency location/);
  } finally {
    // This directory was exclusively created by this test; existing oracle inputs are untouched.
    rmSync(temp, { recursive: true, force: true });
  }
});
