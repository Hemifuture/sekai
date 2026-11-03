// SHA256 of the original pinned Civ Atlas files and the installed oracle dependency entries.
// Civ Atlas commit: 176cc9168a69be30d1e09bfd85db84a5f3fb99da.
import { createHash } from 'node:crypto';
import { readFileSync, realpathSync } from 'node:fs';
import { findPackageJSON } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

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

// Upstream identity metadata is verified separately from historical numeric fixture headers.
export const SOURCE_METADATA_PINS = Object.freeze({
  'edits.ts': '77cd1b9dc7567d260c8f181e000d752a0d5ffcbc458f33edacef330065385c67',
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

// These re-export-only entries connect package exports to the existing numerical pins.
// Keep historical fixture headers unchanged; these are additional pre-execution route checks.
const DEPENDENCY_FORWARDER_PINS = Object.freeze({
  'd3-delaunay/src/index.js':
    'c68b94d0d0a4d82fd8a71f30f9f09c65724fc212981687a18d0a98338074cb59',
  'robust-predicates/index.js':
    '1e26fb2e616a0fa84dadb3a15633d9743671884d82f1abb124e20292fd0ab86d',
});

// Return provenance only after the content matches a known pin.
export function verifyPin(pins, name, content) {
  const actual = createHash('sha256').update(content).digest('hex');
  if (!Object.hasOwn(pins, name) || actual !== pins[name]) {
    throw new Error(`Unexpected pinned source: ${name}`);
  }
  return `${name} SHA256 ${actual}`;
}

// Shared preflight for runners that import original modules from the upstream checkout.
export function verifyOriginalModuleProvenance(root, prefix) {
  const sources = Object.keys(SOURCE_PINS).map(file =>
    verifyPin(SOURCE_PINS, file, readFileSync(join(root, 'src/gen', file))));
  for (const file of Object.keys(SOURCE_METADATA_PINS)) {
    verifyPin(SOURCE_METADATA_PINS, file, readFileSync(join(root, 'src/gen', file)));
  }
  return [
    ...verifyDependencyProvenance(prefix, join(root, 'src/gen/noise.ts'),
      join(root, 'src/gen/geometry.ts')),
    ...sources,
  ];
}

// The two stripped-module runners use their generated .mjs locations as importers.
// Primitives consume only simplex; geometry and original-world runners consume all four pins.
export function verifyDependencyProvenance(prefix, noiseImporter, geometryImporter) {
  // Canonical realpath parents match Node's default ESM loader. A different symlink policy
  // would require a different lookup contract, so reject it rather than checking another tree.
  if (process.execArgv.some(arg => arg.startsWith('--preserve-symlinks')) ||
      /--preserve-symlinks/.test(process.env.NODE_OPTIONS ?? '')) {
    throw new Error('Oracle dependency verification requires default Node symlink resolution');
  }
  const verified = new Map();
  const entry = (specifier, importer, relative) => {
    const importerFile = realpathSync(importer);
    const parent = pathToFileURL(importerFile);
    // Node ignores the second resolve argument without this flag. Check its behavior,
    // including when enabled through NODE_OPTIONS, before trusting any parent lookup.
    const probe = './.sekai-resolution-probe';
    if (import.meta.resolve(probe, parent.href) !== new URL(probe, parent).href) {
      throw new Error('Run the oracle with node --experimental-import-meta-resolve');
    }
    if (!importerFile.endsWith('.mjs')) {
      const packageFile = findPackageJSON(parent);
      if (!packageFile || JSON.parse(readFileSync(packageFile, 'utf8')).type !== 'module') {
        throw new Error(`Oracle importer must use ESM dependency conditions: ${importer}`);
      }
    }
    const actual = realpathSync(fileURLToPath(import.meta.resolve(specifier, parent.href)));
    const expected = realpathSync(join(prefix, 'node_modules', relative));
    if (actual !== expected) {
      throw new Error(`Unexpected oracle dependency location: ${specifier} from ${importer}: ${actual}; expected ${expected}`);
    }
    const pins = Object.hasOwn(DEPENDENCY_PINS, relative) ? DEPENDENCY_PINS : DEPENDENCY_FORWARDER_PINS;
    const annotation = verifyPin(pins, relative, readFileSync(actual));
    verified.set(relative, annotation);
    return actual;
  };
  const simplex = 'simplex-noise/dist/esm/simplex-noise.js';
  entry('simplex-noise', noiseImporter, simplex);
  if (!geometryImporter) return [verified.get(simplex)];
  const d3 = entry('d3-delaunay', geometryImporter, 'd3-delaunay/src/index.js');
  const delaunay = entry('./delaunay.js', d3, 'd3-delaunay/src/delaunay.js');
  const delaunator = entry('delaunator', delaunay, 'delaunator/index.js');
  const robust = entry('robust-predicates', delaunator, 'robust-predicates/index.js');
  entry('./esm/orient2d.js', robust, 'robust-predicates/esm/orient2d.js');
  return Object.keys(DEPENDENCY_PINS).map(file => {
    if (!verified.has(file)) throw new Error(`Unresolved pinned dependency: ${file}`);
    return verified.get(file);
  });
}
