// Execute pinned upstream TypeScript unchanged, except module import paths.
import {readFileSync,writeFileSync,mkdirSync} from 'node:fs';
import {resolve,join} from 'node:path';
import {pathToFileURL} from 'node:url';
import {stripTypeScriptTypes} from 'node:module';
import {execFileSync} from 'node:child_process';
import {SOURCE_PINS,DEPENDENCY_PINS,verifyPin} from './source-pins.mjs';
const [root,prefix,out]=process.argv.slice(2).map(p=>resolve(p));
const temp=join(prefix,'geometry-modules'); mkdirSync(temp,{recursive:true});
const hashes=[];
for(const file of Object.keys(DEPENDENCY_PINS)) {
 hashes.push(verifyPin(DEPENDENCY_PINS,file,readFileSync(join(prefix,'node_modules',file))));
}
for(const [name,rel] of [['util','util.ts'],['noise','noise.ts'],['rand','civ/engine/rand.ts'],['geometry','geometry.ts'],['mesh','mesh.ts']]) {
 const src=readFileSync(join(root,'src/gen',rel),'utf8');
 hashes.push(verifyPin(SOURCE_PINS,rel,src));
 const js=stripTypeScriptTypes(src,{mode:"transform"}).replaceAll("'./util'","'./util.mjs'").replaceAll("'../../util'","'./util.mjs'").replaceAll("'./noise'","'./noise.mjs'").replaceAll("'./geometry'","'./geometry.mjs'").replaceAll("'./civ/engine/rand'","'./rand.mjs'");
 writeFileSync(join(temp,name+'.mjs'),js);
}
const {mulberry32,subSeed}=await import(pathToFileURL(join(temp,'util.mjs')));
const {buildMesh,blurField}=await import(pathToFileURL(join(temp,'mesh.mjs')));
const {geometryOf,sphereSpacing}=await import(pathToFileURL(join(temp,'geometry.mjs')));
const m=buildMesh(2048,1024,sphereSpacing(2048,256),mulberry32(subSeed(42,'mesh'))),g=geometryOf(m);
const buf=new DataView(new ArrayBuffer(8));
const bits=x=>{buf.setFloat64(0,x);return buf.getBigUint64(0).toString()};
const fbits=x=>{buf.setFloat32(0,x);return buf.getUint32(0).toString()};
const lines=['// Generated directly from pinned upstream 176cc9168a69be30d1e09bfd85db84a5f3fb99da.',...hashes.map(s=>'// '+s)];
for(const key of ['xyz','x','y','triangles','adjStart','adj']) {
 const float=['xyz','x','y'].includes(key);
 lines.push(`const ${key.replace(/[A-Z]/g,c=>'_'+c).toUpperCase()}: &[${float?'u32':'usize'}] = &[${Array.from(m[key],float?fbits:x=>x).join(',')}];`);
}
const field=Float32Array.from({length:m.n},(_,i)=>i%7===0?i/13:0);
for(const passes of [0,1,8]) lines.push(`const BLUR_${passes}: &[u32] = &[${Array.from(blurField(m,field,passes),fbits)}];`);
const sparse=new Float32Array(m.n);sparse[0]=1;
lines.push(`const BLUR_SPARSE: &[u32] = &[${Array.from(blurField(m,sparse,8),fbits)}];`);
const group=Int32Array.from({length:m.n},(_,i)=>i%3),size=Int32Array.from([79,79,79]);
const centers=g.groupCentroids(group,3,size),motion=g.plateMotion(Float32Array.from([1,-2,.3]),Float32Array.from([.5,.4,-1]),centers.x,centers.y,[.1,-.3,.5]);
lines.push(`const CENTERS: &[u32] = &[${[...centers.x,...centers.y].map(fbits)}];`,`const OMEGA: &[u32] = &[${Array.from(motion.omega,fbits)}];`);
const water=Uint8Array.from({length:m.n},(_,i)=>i%3===0?0:1),cuts=g.windCuts(water),zonal=g.zonalLand(water,32);
lines.push(`const CUTS: &[u64] = &[${Array.from(cuts,bits)}];`,`const ZONAL_W: &[u32] = &[${Array.from(zonal.west,fbits)}];`,`const ZONAL_E: &[u32] = &[${Array.from(zonal.east,fbits)}];`);
let values=[];const out2=[0,0];
for(const i of [0,1,50,236]) {
 motion.at(i%3,i,out2); values.push(...out2);
 g.moveCell(out2,i,20,-15,3,7,-2,4);values.push(...out2);
 g.mid(i,(i+1)%m.n,out2);values.push(...out2);
 values.push(g.dist(i,(i+1)%m.n),g.dist2(i,(i+1)%m.n),g.edgeDot(i,(i+1)%m.n,.3,-.7),g.latitude(i),g.downwind(i,.3,-.7,cuts),g.noise(12).at(i,.01,1.3,7,9),g.fbm(12,4,.6).atScale(i,80,3,5),g.ridged(12,4,.5).stretched(i,.01,.4));
}
values.push(g.pointDist(0,200,2047,800),g.pointDot(10,200,1800,900,.4,-.2));
lines.push(`const GEOMETRY: &[u64] = &[${values.map(bits)}];`);
lines.push('const CELL_QUERIES: &[(usize, f64, &[usize])] = &[');
for(const [c,r]of [[0,0],[0,30],[0,300],[236,50000]])lines.push(`(${c},${r}.0,&[${Array.from(g.cellsNear(c,r))}]),`);
lines.push('];','const POINT_QUERIES: &[([f64;2], f64, &[usize])] = &[');
for(const [x,y,r]of [[0,0,30],[2048,1024,500],[.1,500,100],[2047.9,500,100],[1000,500,800]])lines.push(`([${Number(x).toFixed(1)},${Number(y).toFixed(1)}],${r}.0,&[${Array.from(g.cellsNearPoint(x,y,r))}]),`);
lines.push('];','const NEAREST: &[([f64;2], usize, usize)] = &[');
for(const [x,y]of [[0,0],[2048,1024],[.1,500],[2047.9,500]])for(const h of [0,100])lines.push(`([${Number(x).toFixed(1)},${Number(y).toFixed(1)}],${h},${g.nearest(x,y,h)}),`);
lines.push('];');
const lens=g.edgeLengths();lines.push(`const EDGE_LENGTHS: &[(usize,u64)] = &[${[0,3,100,lens.length-1].map(i=>`(${i},${bits(lens[i])})`)}];`);
const {round24}=await import(pathToFileURL(join(temp,'rand.mjs')));
lines.push('const TRIG: &[(f64,[u64;4])] = &[');
for(const x of [.1,.3,.7,-.2,-.9])lines.push(`(${x},[${[Math.sin(x),Math.cos(x),Math.asin(x),Math.atan2(x,-.3)].map(v=>bits(round24(v)))}]),`);
lines.push('];');
// One tied-distance grid exercises the JS quicksort partition above its insertion cutoff.
const {default:Delaunator}=await import(pathToFileURL(join(prefix,'node_modules/delaunator/index.js')));
const planar=[];for(let y=-2;y<=2;y++)for(let x=-2;x<=2;x++)planar.push(x,y);
const planarMesh=new Delaunator(planar);
lines.push(`const PLANAR: &[f64] = &[${planar.map(v=>`${v}.0`)}];`);
for(const key of ['triangles','halfedges','hull'])lines.push(`const PLANAR_${key.toUpperCase()}: &[usize] = &[${Array.from(planarMesh[key],v=>v<0?'usize::MAX':v)}];`);
writeFileSync(out,lines.join('\n').replaceAll('const ','pub(crate) const '));
execFileSync('rustfmt',['--edition','2021',out]);
