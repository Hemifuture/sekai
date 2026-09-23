# 河网显示选择的直接出处与效果复核

## 目标与边界

将 R5 的「河级、河宽」上游排序替换为可复核的 GRASS GIS
`r.stream.order -a` Horton 主流追踪规则：Strahler 级优先，同级取汇水面积较大者。
只消费已验证 P5 河网与汇水面积，不改变水文、河道几何、河宽或水体分类。
设计真相为河网完整性规格 R6；不宣称本轮审计证明整个河流生成链的科学性。

## 任务

- [x] 核对原始报告、GRASS 手册和固定版本源码，勘正错误作者及依据范围。
- [x] 绑定 P5 汇水面积，替换河宽代理；用最小分叉图验证主流选择与连通性。
- [x] 接入既有二维／三维 UI，同配置生成并检查全球、放大和缩放连续性。
- [x] 运行显示选择及 app 直接消费者 Release 测试、fmt、Clippy、WASM check；
  重建 native 与 Trunk。此次不改共享科学算子，无需多 seed 全生成回归。
- [x] 记录效果与限制、提交本任务，提供用户验证步骤。
- [ ] 用户本人最终 UI 验收。

## 验证记录

- `cargo test --release --lib river_`：21 通过；
  `cargo test --release --lib app::amplified_mesh::tests`：11 通过。
  两次过滤有重叠，不将结果相加当作不同测试数量。新增的一个选择器测试及修改的
  app 分叉 fixture 证明排序规则和实际绑定；没有新增全世界生成测试。
- fmt、workspace/all-targets/all-features Clippy `-D warnings`、WASM lib check
  均通过。桌面 Release 与 Trunk Release 均重建成功；网页产物位于
  `target/river-science/web`，本轮不宣称浏览器运行时已经验收。
- 首次测试编译因 fixture 对 `UnitVector3` 误用数组方法而失败，改用已有
  `components()` 后通过。首次 native 链接被仍运行的旧程序占用；关闭该窗口后
  重建成功。两者均已解决，没有放宽门禁。
- 原版／新版均使用保存的 Standard、多大陆、seed `781909846035989120`、
  陆壳比例驱动生成。两版 UI 均发布 79,212 格，P/E 显示 2.578/2.499 mm/day，
  TOA −1.66 W/m²。此处只是 UI 摘要一致性，不将舍入数字当作逐位指纹证据。
- 同视角放大图可见部分上游保留支路变化，已检查的干流、汇流及湖岸连接保持连续。
  新版完成缩小、全球重置与二维→三维→二维检查，stderr 为空。截图为
  `target/river-science/before-detail.png`、`after-detail.png`、`after-zoom-out.png`、
  `after-global.png`、`after-globe.png`，同目录保存 UI 树与构建／测试日志。

## 效果边界与用户验证

可确认的是主流追踪换成有直接实现依据的规则，局部反例和实际显示链路均消费
汇水面积；不能据此宣称整张地图的自然感已经改善。粗格岸线仍呈多边形，细化
河线的形状也未改变。规则不保证选择最长河道或最大流量的支流，出处的已知
限制见 R6；河源仍只表示当前模型已发布河网的起点。

启动 `target/release/sekai.exe`，保留当前参数，填色选“当前地表高程”。
点“重置地图”，放大陆地，沿河线检查源头、汇流和水岸；缩小后主流应仍连通，
放大时支流逐步恢复。切换“三维球体”检查相同水体和河网。程序已留给本人操作，
最终视觉验收待用户确认。

## 每项承重技术的出处

- Horton 主流追踪：GRASS GIS `r.stream.order` 手册 Horton 小节及 `-a`，
  Jarek Jasiewicz；`OSGeo/grass-addons` 提交
  `a0e37beaf0ce8a4cc6ec615b6d3d40e39f9d725f`，
  `src/raster/r.stream.order/stream_order.c` 的 `horton()`。源码核对的是算法规则，
  Rust 实现复用本项目图索引，不复制 GRASS 代码。
- 河级：Strahler (1957)。球面非等面积单元用 P5 已积累的面积，避免格数偏差；
  不重新计算水文。精确并列按 CellId 保持确定性，只是工程约定，不代表物理优先级。
- 下游追踪与制图连通：Gary et al. (2009, revised May 2010), USGS SIR 2009–5202,
  pp. 7–10。引用限于既有有向网络的下游追踪，不照搬依赖 GNIS 名称和人工选源的流程。
- 显示层级仍是既有 UI 细节层级到河级的映射，不是物理定律。R6 单列其适用边界；
  无新增物理常量、科学阈值或经验调参。
