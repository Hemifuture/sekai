import assert from 'node:assert/strict';
import test from 'node:test';
import { SOURCE_PINS, DEPENDENCY_PINS, verifyPin } from './source-pins.mjs';

test('shared pin verification rejects altered source and dependency content', () => {
  const altered = Buffer.from('altered oracle input');
  assert.throws(() => verifyPin(SOURCE_PINS, 'util.ts', altered), /util\.ts/);
  assert.throws(() => verifyPin(DEPENDENCY_PINS,
    'simplex-noise/dist/esm/simplex-noise.js', altered), /simplex-noise/);
  assert.throws(() => verifyPin(SOURCE_PINS, 'unregistered.ts', altered), /unregistered\.ts/);
});
