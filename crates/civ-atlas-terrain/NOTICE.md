# Civ Atlas 地形移植出处与许可

本包直接移植 `guaner-334/civ-atlas`，固定提交
`176cc9168a69be30d1e09bfd85db84a5f3fb99da`（上游版本 1.5.0）：
https://github.com/guaner-334/civ-atlas/tree/176cc9168a69be30d1e09bfd85db84a5f3fb99da

上游采用 AGPL-3.0-only，许可证原文完整保留于本目录 `LICENSE`。
上游这些 TypeScript 文件没有逐文件版权人声明；本移植不替原作者添加或转让版权。
修改日期：2026-11-01。修改内容：将实际调用的算法翻译为原生 Rust，保留运算顺序、
JavaScript 32 位溢出、UTF-16 字符串散列和 Float32Array 写回边界；不包含网页代码。
本包的许可不改写其他文件或第三方组件原有的许可证。

## 文件映射

- `src/raster.rs`、`src/shoreline.rs`、`src/fine.rs`：原 `raster.ts` 与
  `rasterWindow.ts` 的球面覆盖、Float32 重心插值、两遍球面盒式模糊、周期 B 样条、
  三向细节纹理、细岸扭曲及 zoom 高程求值。仅保留基础分辨率底图；高倍查询使用
  固定的原尺度，不保存全球高倍 Raster，不从相机读取 scale。
- `src/river_geometry.rs`、`src/valleys.rs`、`src/creeks.rs`：原同名 TypeScript
  文件（大小写按上游），保留主干重串、汇合点、Catmull–Rom、分层曲流、河谷及溪流。
  溪流复用生成结束时同一参数的原 receiver，省去完全相同的第二次 Priority-Flood。
- `src/gully.rs`：原 `gully.ts` 的全部实际 zoom 高程消费者；每个采样批次独占
  Layer 的最近格缓存，世界只持有不可变输入。原余弦/正弦 Float32 LUT 不变。
- `src/initial_sea_ice.rs`：原 `seaice.ts::computeSeaIce/seaIceNodes`，它通过
  `dropSpecks` 影响河口参考的全图水类；只排除后续浮冰绘色。
- `src/estuary.rs`：原 `estuary.ts` 的海口选择、桥接、喇叭口、三角洲/水下浅滩、
  分汊水道的所有高程及水类写入，按原 mouths 顺序执行。仅删除无形状消费者的
  plume/rivTint/rivCore、湿地/温雨绘色和对应洋流查询。
- `src/noise.rs::value_noise`：原 `valueNoise/hash2`，保留 Number 乘法后 ToInt32
  与 Math.imul 的边界，为河口真实地形消费者服务。

这些函数中的数值、振幅、作者 floor 与分支阈值全部来自固定上游，是已批准的
作者初态规则，不宣称地球物理常量、侵蚀演化或物质守恒。湖床 A3 残差是明确的
Sekai 初态适配；公开湖先验仍未闭合，不能当最终水面或水深发布。
无原湖顶点的三角形直接保留源高程。A3b允许湖节点star内的初态海平面交线随床恢复
改变，不使用海岸切割器、taper或clamp；所有最终水体必须从恢复床重新闭合。
原输出的 noisy lake 掩膜只用于离线保真对照。

`reference/fine.mjs` 和 `reference/fine-world.mjs` 使用离线 tsx 直接执行经
`source-pins.mjs` 校验的原 TypeScript。前者是最低成本几何/噪声夹具；后者沿用
同一个默认 seed42 世界的首河口、首湖、接缝、首桥接，以及固定输入流量的三角洲
分支夹具。没有另写 JavaScript 地形算法或将 Rust 输出反钉成 oracle。
运行时不依赖 Node，不加载预制世界；大尺寸诊断二进制只存离线 scratch。

- `src/random.rs`：`src/gen/util.ts` 的 `mulberry32`、`subSeed`、`keyed`、
  `fmix`；`src/gen/civ/engine/rand.ts` 的 `round24`。
- `src/noise.rs`：`src/gen/noise.ts` 的 `noise3`、`fbm3`、`ridged3`、
  `tileableFbm`。分形偏移、幅度、频率和周期纹理的数值处理均来自上游作者先验，
  不是新引入的物理常量。`tileableFbm` 的真实消费者为上游 `src/gen/raster.ts`。
- `src/noise.rs` 的 3D simplex 和排列构造：上游锁定的 `simplex-noise@4.0.3`，
  MIT 许可；保留下面的完整许可。没有用其他噪声库代替。

`simplex-noise` 的原实现说明：Jonas Wagner 的 JavaScript 实现，基于 Stefan
Gustavson 的 simplex 示例代码及 Peter Eastman 的优化，排序方法为 Gustavson (2012)。
项目：https://github.com/jwagner/simplex-noise.js/tree/4.0.3

## 离线参照

`reference/primitives.mjs` 使用 Node 的 `stripTypeScriptTypes` 读取并执行固定上游
源码，仅改写模块导入后缀，不重新实现算法。运行时不需要 Node 或 npm。
`reference/primitives.rs` 保存上游原始输出的 IEEE 位表示以及输入文件 SHA256。
覆盖样本仅针对本次翻译的风险，不进行多种子全链生成。

复现方式（从仓库根目录运行；`<prefix>` 为临时 npm 目录）：

```text
npm install --prefix <prefix> --no-audit --no-fund --ignore-scripts --save-exact simplex-noise@4.0.3
node --experimental-import-meta-resolve crates/civ-atlas-terrain/reference/primitives.mjs <pinned-upstream-checkout> <prefix> crates/civ-atlas-terrain/reference/primitives.rs
cargo test --manifest-path crates/civ-atlas-terrain/Cargo.toml -j 2
```

参照生成使用 Node v24.18.0。当前 Windows x64 上样本全部逐位匹配，包括周期纹理；
这不是任意平台超越函数逐位相同的承诺。周期纹理的方向初始化调用系统 sin/cos，
测试用上游所有方向角的独立 sin/cos 位参照检查宿主；不匹配时仅跳过周期纹理的精确
金样比较，有限性和尺寸契约仍执行，随机整数、舍入和 simplex 代数断言始终执行。
此 canary 仅在测试中存在；根项目集成后仍使用其既有 `audited_float_platform()`。
不能静默重钉世界指纹。

## 粗地形生成移植（2026-11-01）

- `src/tectonics.rs`、`src/tectonics/continents.rs`：固定上游 `tectonics.ts` 的默认
  生成分支，包括多源板块生长、多尺度扭曲、碎片合并、大陆/微大陆/群岛选择、
  欧拉轴运动、汇聚与张裂距离场、岛弧、热点、大陆架/海沟、抬升与高原底座。
  所有作者控制保留；不移植草图、板块编辑状态或地形编辑器。
- `src/inland_sea.rs`：同提交 `inlandSea.ts`，按原顺序填平内海或凿通海峡；
  未保留无人消费的草图保护和 noFill 分支。
- `src/erosion.rs`：同提交 `erosion.ts` 的 Barnes (2014) Priority-Flood 与
  Braun & Willett (2013) 隐式流水功率求解；保留原 shoreSeeds、邻接顺序和
  Float32Array 写回语义，复用一次建立的岸边种子与边长。
- `src/util.rs`：同提交 `util.ts` 的插值和最小堆；相等优先级的入堆、选子节点
  和出堆顺序不变。`orderByKey` 用 Rust 稳定数值排序实现相同的 key/id 次序，
  包括正负零相等；未移植仅用于 JavaScript 数组加速的整数编码层。
- `src/initial_climate.rs`、`src/initial_currents.rs`：同提交 `climate.ts` 与
  `currents.ts`，完整保留初始化降水依赖、季节风带与洋流参数化。缓存只属于一次
  生成，构造一次球面几何、种子噪声与风带；不保存编辑器弱引用缓存。
  这些字段保持 crate-private，不作为 Sekai 最终气候或最终水文发布。
- `src/terrain.rs`：同提交 `world.ts` 的两段侵蚀、作者初态米制换算、高原峡谷、
  裂谷/随机洼地、湖泊、湖心岛、初始化径流和河道链。删除回放历史、编辑器与
  文明分支；海冰、creeks 和其他细地形实际消费者由后续 raster 任务迁移。
  分位换算及高峰膝点是上游作者初态规则（规格修订 A1），不得复用于 Sekai
  后续物理反馈，不能将本移植称为守恒的地质历史。
- `src/params.rs`：同提交 `params.ts` 的全部实际 WorldParams 作者控制，公开
  参数域取自 `ui/genesisSliders.ts` 与 `ui/ParamField.tsx` 的大陆按钮及 cap。
  离线 237 格夹具不扩展公开支持域；公开请求在分配网格前验证。

新增取消检查与输出有限性/尺寸校验，未引入新算法依赖、物理阈值或替代地形公式。
`UPSTREAM_REVISION`、`UPSTREAM_PACKAGE_VERSION` 与 `UPSTREAM_GENERATOR_VERSION`
为根项目来源身份的唯一代码定义；后续集成应引用这些定义。

初版移植的生成器身份常量曾误记为7；固定 `src/gen/edits.ts` 的实际声明是57。
本次只更正该元数据事实，并由消费者提升 Atlas 模型身份；不更改原数值算法或重钉数值。
共享来源 pin 另记录 `edits.ts` 身份元数据哈希，离线回归读取该实际声明与 Rust 常量比较，
不再仅验证常量自身的序列化往返。历史 oracle 数值和已有来源注释保持原样。

### 粗地形离线参考

`reference/terrain.mjs` 使用离线安装的 tsx 加载器运行原固定 TypeScript 世界生成，
只将 `world.ts` 的模块路径改为固定源码的绝对地址，并在既有语句之后插入数组快照。
没有重写上游算法；原回放、编辑器模块仍由上游正常加载，但不作为 Rust 运行时依赖。
运行前引用 `source-pins.mjs` 校验实际算法源码、源 UI 域和噪声/三角剖分依赖入口；
新脚本不复制字面哈希。`terrain.mjs`、`fine-world.mjs` 与 `fine.mjs` 使用同一个前置校验：
从原 `noise.ts` / `geometry.ts` 的实际 ESM 导入位置解析，再沿 d3-delaunay → delaunator →
robust-predicates 的实际父模块解析，要求 realpath 与已校验的 prefix 文件相同。
两个纯 re-export 入口也单独钉哈希，避免入口转发绕开原四个数值实现文件。
`root/node_modules` 指向 prefix 的 junction 可通过；更近的或嵌套的另一份依赖即使字节
相同也拒绝。此项约束已列出的数值实现及转发路径，不声称审计整个 npm 依赖树。
校验不加载世界模块、不重算 fixture；原数值、算法和生成器版本不因本次校验加固改变。
原始上游输出写入 `reference/terrain.rs`，测试不从 Rust 反钉。

```text
node --experimental-import-meta-resolve --import <prefix>/node_modules/tsx/dist/loader.mjs crates/civ-atlas-terrain/reference/terrain.mjs <pinned-upstream-checkout> <prefix> crates/civ-atlas-terrain/reference/terrain.rs
cargo test --manifest-path crates/civ-atlas-terrain/Cargo.toml -j 2
```

上述命令使用本轮 Node 24 工具链；另两个原 TS 入口使用相同 flags。
`primitives.mjs` 与 `geometry.mjs` 也使用同一依赖校验助手，但从真实的 prefix 临时
`.mjs` 模块处解析；它们分别校验实际消费的 simplex 或完整几何依赖链。
`--experimental-import-meta-resolve` 使 Node 真正采用导入者参数；助手检查该行为，缺失时
在执行算法前报错。导入者必须保持 ESM package scope，且不得启用 `--preserve-symlinks`。
普通哈希检查为 `node --test crates/civ-atlas-terrain/reference/source-pins.test.mjs`；
路径回归先将 `ATLAS_ORACLE_ROOT` / `ATLAS_ORACLE_PREFIX` 设为已有离线目录，再运行
`node --experimental-import-meta-resolve --test crates/civ-atlas-terrain/reference/resolution.test.mjs`。
该回归只复制少量来源与依赖文件并查询解析结果，包含三个生成入口的拒绝检查，不运行世界。

默认小夹具覆盖板块、海陆、海底、抬升、高原、两次降水、两段侵蚀、米制高程、
最终水体、私有季节气候/洋流/汇流及全部原河道链。另一个同 seed/同小网格的组合
作者控制场景覆盖非默认大陆数量、聚拢、破碎度、硬度和半球偏向分支；不扫描种子。
有限性与取消契约始终运行；精确上游浮点断言复用球面运算 canary，不匹配时说明跳过。

## simplex-noise MIT 许可

包内 LICENSE 声明 Copyright (c) 2018 Jonas Wagner；移植的 4.0.3 实现文件声明
Copyright (c) 2024 Jonas Wagner。两份原始声明均保留如下，许可正文相同：

```text
MIT License

Copyright (c) 2018 Jonas Wagner
Copyright (c) 2024 Jonas Wagner

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## 球面网格与几何移植（2026-11-01）

- `src/mesh.rs`、`src/geometry.rs`：上述固定 Civ Atlas 的 `src/gen/mesh.ts` 与
  `src/gen/geometry.ts`，保留 Poisson 撒点、立体投影、跨缝三角形稳定分区、
  d3 邻接顺序、球面距离/局部运动/噪声、分组质心和气候风带查询。
  数值参数均为该上游实现的作者先验或数值算法参数，不冒充地球物理常量。
- `src/triangulation.rs`：从 `delaunator` Rust crate 1.1.0 的 `src/lib.rs`
  移植必需的 Delaunator 内核，并依锁定的 JavaScript `delaunator@5.1.0`
  恢复 `ceil(sqrt(n))` hull hash 大小、JS quicksort 同距顺序、首种子选择与
  duplicate epsilon；加入插点循环取消检查，删除无消费者公共 API/no_std 支路。
  原 Rust 源：https://github.com/mourner/delaunator-rs （crate 1.1.0）。
  原 JS 源：https://github.com/mapbox/delaunator （npm 5.1.0）。
  两份原 ISC 许可证与各自的 Mapbox 版权年保留于 `licenses/delaunator-*-ISC.txt`。
- `src/mesh.rs` 的 incoming-halfedge 建表及邻居遍历来自 `d3-delaunay@6.0.4`
  `src/delaunay.js` 与 Civ Atlas `sphereDelaunay`。保留 Observable/Mapbox 的 ISC
  许可证于 `licenses/d3-delaunay-ISC.txt`。
- 唯一数值依赖 `robust@1.2.0` 为实际三角剖分消费者提供 Shewchuk 自适应 orientation
  谓词（Georust 的 Rust 实现，MIT/Apache-2.0 双许可，此处选择 MIT）。未复制
  `robust-predicates` JavaScript 谓词；其锁定源码仅供离线 oracle 使用。
  `licenses/robust-MIT.txt` 保留原 MIT 许可及 Spade/GeoRust 版权声明。
- `thiserror@2` 仅用于 `TerrainError` 的参数域/取消错误；它不参与生成数值。

### 球面离线参考

`reference/geometry.mjs` 直接读取原 TypeScript，通过 Node transform 模式移除类型，
仅改写模块导入路径。运行前校验五个原文件与三个 Delaunay 依赖入口的 SHA256；
原始文件内容及校验和进入生成的 `reference/geometry.rs`。不是重新写一份 JS 算法。

```text
node --experimental-import-meta-resolve crates/civ-atlas-terrain/reference/geometry.mjs <pinned-upstream-checkout> <prefix> crates/civ-atlas-terrain/reference/geometry.rs
```

`<prefix>` 是包含锁定的 `simplex-noise@4.0.3`、`d3-delaunay@6.0.4`
（依赖 `delaunator@5.1.0`）的离线 npm 目录，运行时不需要 Node。
夹具使用 seed 42/requested cells 256，实际 237 格；全量坐标与邻接、三角形顺序逐项比对。
另有一个 25 点同距平面网格，只验证 Delaunator quicksort/halfedge 顺序，未扩大世界语料。
精确球面金样以独立的上游 round24 三角函数 canary 保护；不匹配时带说明跳过精确
比对，拓扑/有限性/最近点等不变式仍然执行。当前 Windows 主机全部精确比对通过。
