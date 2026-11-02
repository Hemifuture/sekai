// SHA256 of the original pinned Civ Atlas files and the installed oracle dependency entries.
// Civ Atlas commit: 176cc9168a69be30d1e09bfd85db84a5f3fb99da.
import { createHash } from 'node:crypto';

export const SOURCE_PINS = Object.freeze({
  'util.ts': 'a5b9c35b769579b2d0f18bd865b5f54e57ad501a01234b3a7c04f4eb00828c51',
  'noise.ts': '6f1a0d080d33f50e282a40fce2f216869c1f5c9a5f747720a1d16ab4a87ac3f8',
  'civ/engine/rand.ts': '09cb84edb16e3b21930eeb0ee669eb11748380227132f5af27608b1a20afecf8',
  'geometry.ts': 'deec31f1a9003fe5ce6116f19ff718540770daa1d3e756f7967d97bbb6ec6ce1',
  'mesh.ts': 'd1dca22d575fe0628876324947e3583df2f5072bfbe8b75eb241e2f2b49f8b99',
});

export const DEPENDENCY_PINS = Object.freeze({
  'simplex-noise/dist/esm/simplex-noise.js':
    '64e571e6d28da06fdd230110b8ce242285e76137e7258938f3b68a0fd6ff8146',
  'delaunator/index.js':
    '1a941d291859ebe1f6755dfbc2024686103c5ab2f31659177c01088ab88c2ca3',
  'd3-delaunay/src/delaunay.js':
    '21b26b06b768e5f248adff6301638b87cf9ac32edf423eccb7784e60b9851292',
  'robust-predicates/esm/orient2d.js':
    'ba1af2b1ea71924fa2e0a3e50b8972d3a83ab9a9f406dd73e851c9763a8eb741',
});

// Return provenance only after the content matches a known pin.
export function verifyPin(pins, name, content) {
  const actual = createHash('sha256').update(content).digest('hex');
  if (!Object.hasOwn(pins, name) || actual !== pins[name]) {
    throw new Error(`Unexpected pinned source: ${name}`);
  }
  return `${name} SHA256 ${actual}`;
}
