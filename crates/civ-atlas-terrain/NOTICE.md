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
node crates/civ-atlas-terrain/reference/primitives.mjs <pinned-upstream-checkout> <prefix> crates/civ-atlas-terrain/reference/primitives.rs
cargo test --manifest-path crates/civ-atlas-terrain/Cargo.toml -j 2
```

参照生成使用 Node v24.18.0。当前 Windows x64 上样本全部逐位匹配，包括周期纹理；
这不是任意平台超越函数逐位相同的承诺。周期纹理的方向初始化调用系统 sin/cos，
测试用上游所有方向角的独立 sin/cos 位参照检查宿主；不匹配时仅跳过周期纹理的精确
金样比较，有限性和尺寸契约仍执行，随机整数、舍入和 simplex 代数断言始终执行。
此 canary 仅在测试中存在；根项目集成后仍使用其既有 `audited_float_platform()`。
不能静默重钉世界指纹。

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
node crates/civ-atlas-terrain/reference/geometry.mjs <pinned-upstream-checkout> <prefix> crates/civ-atlas-terrain/reference/geometry.rs
```

`<prefix>` 是包含锁定的 `simplex-noise@4.0.3`、`d3-delaunay@6.0.4`
（依赖 `delaunator@5.1.0`）的离线 npm 目录，运行时不需要 Node。
夹具使用 seed 42/requested cells 256，实际 237 格；全量坐标与邻接、三角形顺序逐项比对。
另有一个 25 点同距平面网格，只验证 Delaunator quicksort/halfedge 顺序，未扩大世界语料。
精确球面金样以独立的上游 round24 三角函数 canary 保护；不匹配时带说明跳过精确
比对，拓扑/有限性/最近点等不变式仍然执行。当前 Windows 主机全部精确比对通过。
