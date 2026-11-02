// SHA256 of the original pinned Civ Atlas files and the installed oracle dependency entries.
// Civ Atlas commit: 176cc9168a69be30d1e09bfd85db84a5f3fb99da.
import { createHash } from 'node:crypto';

export const SOURCE_PINS = Object.freeze({
  'raster.ts': 'af5ddaa9f5227cc998051596b4fa9bdc91ade0bfb8246b1be0884f6982e88af8',
  'rasterWindow.ts': 'f803eceab2930a7bd38cda36bc0058a7a82bc824aa01df0c0bff4293f608cd07',
  'gully.ts': 'f5d1d2db9567be1289ed48ef0af8819f2910e3ed1ee7af990697de5138d1f1bb',
  'riverGeometry.ts': '154b2c359b5b7c05455bb970656a7578f0ccce3a5f45044b5945356f662f61d4',
  'valleys.ts': '8be3251014105c4eed012a7cb7bcc5eb192176600e00a2f41780e62f92a321c7',
  'creeks.ts': 'ed0c77f15c7ca3691a7061f56813970bb377514b05dd14f0fc1c78aa010e0b8d',
  'estuary.ts': 'db524a64107a57d44676420721a6ff7073af341aeb324ca2c3734eb315dc580d',
  'seaice.ts': '7952e3745bb27832cf5a489644b16e450c8bc616fac02a39a64e0ecdeb27df57',
  'util.ts': 'a5b9c35b769579b2d0f18bd865b5f54e57ad501a01234b3a7c04f4eb00828c51',
  'noise.ts': '6f1a0d080d33f50e282a40fce2f216869c1f5c9a5f747720a1d16ab4a87ac3f8',
  'civ/engine/rand.ts': '09cb84edb16e3b21930eeb0ee669eb11748380227132f5af27608b1a20afecf8',
  'geometry.ts': 'deec31f1a9003fe5ce6116f19ff718540770daa1d3e756f7967d97bbb6ec6ce1',
  'mesh.ts': 'd1dca22d575fe0628876324947e3583df2f5072bfbe8b75eb241e2f2b49f8b99',
  'tectonics.ts': 'f4592a01f8abd0fbcf3927823a0d63134085aaf70bb896b5997982f8e68d9209',
  'inlandSea.ts': '14bcc5e44a79c83bfd043a7902bdd721ef5f0c2b18fad56b72b35d8ad9455b73',
  'erosion.ts': '32f787f0ec22fe5a254ff526d1ee6f0522ed8aae02e21e16e166fb0fff71b9b9',
  'climate.ts': '488e911073544e82a7d5d9db2ac78378cc96b12d1bae3996184cb07d15bb7a25',
  'currents.ts': '4f5dc38a20fe65325e5dba70adbc028c362da9a8d877ff18f9a5438efff282e6',
  'world.ts': '62a6b2c61b7ef14d57f611cf3353d454cfe423f2ac41b8f69c88a9bc33b18f05',
  'params.ts': 'da0d32750d227975091bf6836567c0081d85c68664d7f4ba5166610ac8514ec3',
  '../ui/genesisSliders.ts': '956cec6b8bc0ec5f219b2f3c58e58627a62b1431fc1899e70a31ad173b228efd',
  '../ui/ParamField.tsx': '3746799e3f24c4be4e2fbc56fe78d703aeb371f7c2cf2cf3a081f0c6f1028bad',
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
