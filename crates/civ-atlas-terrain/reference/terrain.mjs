// Run with the pinned oracle's tsx loader. Upstream algorithms are executed unchanged;
// instrumentation only snapshots existing arrays at intermediate phase boundaries.
import {readFileSync,writeFileSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {execFileSync} from 'node:child_process';
import {SOURCE_PINS,DEPENDENCY_PINS,verifyPin} from './source-pins.mjs';
const [root,prefix,out]=process.argv.slice(2).map(p=>resolve(p));
const provenance=[];
for(const file of Object.keys(DEPENDENCY_PINS))provenance.push(verifyPin(DEPENDENCY_PINS,file,readFileSync(join(prefix,'node_modules',file))));
for(const file of Object.keys(SOURCE_PINS))provenance.push(verifyPin(SOURCE_PINS,file,readFileSync(join(root,'src/gen',file))));
let source=readFileSync(join(root,'src/gen/world.ts'),'utf8');
const captures={};
globalThis.__capture=(name,a)=>{captures[name]=Array.from(a)};
let climateCall=0;
globalThis.__captureRain=c=>{for(const key of ['temperature','precipitation','windX','windY'])globalThis.__capture(`INITIAL_CLIMATE_${climateCall}_${key.replace(/[A-Z]/g,c=>'_'+c).toUpperCase()}`,c[key]);climateCall++;};
for(const [anchor,extra]of [
 ['let rain = rainWeight(proxy);',"globalThis.__capture('RAIN_0',rain);globalThis.__capture('H_0',h);"],
 ['const c = computeClimate(mesh, elevM, water0, climateP);','globalThis.__captureRain(c);'],
 ['rain = rainWeight(toMeters(h, peak));',"globalThis.__capture('H_18',h);globalThis.__capture('RAIN_18',rain);"],
 ['const flow = drainage(mesh, land, h, 1e-5);',"globalThis.__capture('H_36',h);"],
 ['const elevation = toMeters(h, peak, base);',"globalThis.__capture('METERS',elevation);"],
]) {
 if(source.split(anchor).length!==2)throw Error('instrumentation anchor changed: '+anchor);
 source=source.replace(anchor,anchor+extra);
}
source=source.replace(/from '(\.\/[^']+)'/g,(_,p)=>`from '${pathToFileURL(join(root,'src/gen',p+'.ts')).href}'`);
const instrumented=join(prefix,'world-instrumented.mts');writeFileSync(instrumented,source);
const {generateWorld,DEFAULT_PARAMS}=await import(pathToFileURL(instrumented));
const world=generateWorld({...DEFAULT_PARAMS,seed:42,cells:256});
for(const key of ['plate','plateVx','plateVy','plateOmega','plateContinental','convergence','distConv','strengthConv','distDiv','strengthDiv','land','oceanDepth','uplift','plateau'])captures['TECT_'+key.replace(/[A-Z]/g,c=>'_'+c).toUpperCase()]=Array.from(world.tect[key]);
for(const key of ['elevation','water','waterLevel','temperature','precipitation','flux'])captures[key.replace(/[A-Z]/g,c=>'_'+c).toUpperCase()]=Array.from(world[key]);
for(const key of ['sst','u','v'])captures['CURRENT_'+key.toUpperCase()]=Array.from(world.currents[key]);
const buf=new DataView(new ArrayBuffer(4)),fbits=x=>{buf.setFloat32(0,x);return buf.getUint32(0)};
const ints=new Set(['TECT_PLATE','TECT_PLATE_CONTINENTAL','TECT_LAND','WATER']);
const lines=['// Pinned upstream 176cc9168a69be30d1e09bfd85db84a5f3fb99da; seed42/request256/actual237.',...provenance.map(s=>'// '+s)];
for(const [key,a]of Object.entries(captures))lines.push(`pub(crate) const ${key}: &[${ints.has(key)?'usize':'u32'}] = &[${a.map(ints.has(key)?x=>x:fbits)}];`);
lines.push(`pub(crate) const RIVERS: &[(&[usize],&[u32])] = &[${world.rivers.map(r=>`(&[${Array.from(r.cells)}],&[${Array.from(r.pts,fbits)}])`)}];`);
const capturedFields=Object.keys(captures).length;
// One combined author-control scene exercises the optional branches without a seed sweep.
const authored=generateWorld({...DEFAULT_PARAMS,seed:42,cells:256,plates:24,landFraction:.45,
 mountains:1.4,temperature:5,rainfall:1.4,continents:2,gather:.8,rugged:1.6,hardness:.6,north:.75});
for(const key of ['elevation','water','waterLevel'])lines.push(`pub(crate) const AUTHORED_${key.replace(/[A-Z]/g,c=>'_'+c).toUpperCase()}: &[${key==='water'?'usize':'u32'}] = &[${Array.from(authored[key],key==='water'?x=>x:fbits)}];`);
writeFileSync(out,lines.join('\n'));execFileSync('rustfmt',['--edition','2021',out]);
console.log(`Captured ${world.mesh.n} cells, ${world.rivers.length} rivers, ${capturedFields} default phase fields and 3 authored scene fields`);
