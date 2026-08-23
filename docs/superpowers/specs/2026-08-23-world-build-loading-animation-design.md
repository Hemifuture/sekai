# 世界构建加载动画设计规格

## 1. 状态与裁定

本规格冻结 2026-08-23 经用户在网页原型中逐轮验收的世界构建加载动画。
最终裁定如下：

- 加载图形使用程序同款 Equal Earth 二维投影轮廓。
- 图形由 `WORLD_LOADING_PIECE_COUNT` 个大小不一、边界共享、互不重叠的凸多边形组成。
- 块从中心向外依次淡入并轻微移入；停留后按相同顺序淡出并轻微移出。
- 世界重新生成开始后，旧地图立即从视窗消失；侧边栏计时与取消仍保留，但不再是唯一反馈。
- 临时网页原型只用于验收，egui 实现成为唯一运行时事实源；原型在落地后删除，避免几何、色板与时序出现第二份定义。

## 2. 运行时语义

`TemplateApp::world_build.is_some()` 是加载视图唯一状态来源：

1. 初次异步构建和球面世界重建都进入加载视图。
2. pending 期间不调用 `interact_spherical_canvas`，也不排队
   `SphericalPaintCallback`；旧 publication 继续留在内存中，但不可见、不可交互。
3. 构建成功后，沿既有原子发布路径安装 candidate，再恢复地图视图。
4. 构建失败或取消后，既有错误处理清除 pending；若此前有 publication，旧地图重新出现，形成回滚；初次构建失败则显示既有“尚未发布”状态。
5. pending 期间第二次重建请求仍沿用既有“忽略”语义，不新增队列、进度协议或构建状态机。

本功能不改变生成器、Artifact、缓存、指纹、worker 或 GPU publication 语义。
旧平面兼容链的同步构建不在本次范围内。

## 3. 视觉几何

### 3.1 投影轮廓

加载轮廓直接调用生产侧 `SphericalProjectionKind::EqualEarth`，不得复制 Equal Earth
公式或静态轮廓点。轮廓按 `WORLD_LOADING_OUTLINE_LATITUDE_STEPS` 采样，映射到
`WORLD_LOADING_VIEWBOX_WIDTH` × `WORLD_LOADING_VIEWBOX_HEIGHT` 的设计平面，并保留网页验收稿的留白比例。

出处：Šavrič、Patterson 与 Jenny（2018/2019），DOI
`10.1080/13658816.2018.1504949`；项目现有权威实现为
`src/view/spherical_projection.rs`。

### 3.2 凸胞元

`WORLD_LOADING_VERTICES` 是共享顶点唯一事实源，`WORLD_LOADING_PIECES` 只保存顶点索引和色调索引。二十块按离投影中心由近到远排序，因此数组顺序同时是动画顺序。

移动后的胞元通过 Sutherland–Hodgman 逐边裁剪到生产投影轮廓。该算法针对凸裁剪窗，保持输入凸多边形的凸性，适配 egui 的 `Shape::convex_polygon`。

出处：I. E. Sutherland 与 G. W. Hodgman，*Reentrant Polygon Clipping*，
Communications of the ACM 17(1), 32–42, 1974，DOI
`10.1145/360767.360802`。

### 3.3 色板

全部加载颜色只定义在 `src/view/palette.rs` 的 `WORLD_LOADING_PALETTE`：背景、表面、轮廓、正文、弱化文字、强调色及七个块色。UI 调用点只能引用该常量，不能硬编码第二份颜色。

七个块色及相邻块异色关系来自用户确认的最终网页原型；几何测试固定“共享边的块色调不同”这一行为，不固定绘制调用的内部结构。

## 4. 动效时序

生产常量与用户确认稿一致：

- `WORLD_LOADING_PIECE_COUNT = 20`。
- `WORLD_LOADING_TRANSITION_SECONDS = 0.3`。
- `WORLD_LOADING_STAGGER_WINDOW_SECONDS = 0.6`。
- `WORLD_LOADING_HOLD_SECONDS = 0.3`。
- `WORLD_LOADING_CYCLE_SECONDS = 2.1`，由前述阶段求和得出，不另写魔法值。
- `WORLD_LOADING_TRAVEL_VIEWBOX_UNITS = 18.0`，即设计平面高度的 3.6%。

第 `i` 块的错峰量为
`i × WORLD_LOADING_STAGGER_WINDOW_SECONDS / (WORLD_LOADING_PIECE_COUNT - 1)`。
进入进度和退出进度都使用 Khronos GLSL `smoothstep`：先把线性进度夹到
`[0, 1]`，再求 `t²(3 − 2t)`。最终：

- `opacity = enter × (1 − exit)`；
- `travel = 1 − enter + exit`。

Windows Fluent Motion 把 167/250/333 ms 列为直接进入/既有元素的标准时长，并要求直接退出与淡出结合；本规格采用网页实测并由用户确认的 300 ms，位于该工业区间内。循环时钟只对
`WORLD_LOADING_CYCLE_SECONDS` 取模，不随真实构建时长累计漂移。

## 5. 可访问性与反馈

- 侧边栏新增持久化的“减少加载位移动效”开关，具备真实消费者。
- 开启后保留块的顺序淡入淡出与文字进度，把所有 `travel` 置零；状态信息不依赖位移表达。
- 加载视图包含实际 egui 文本控件：“世界正在成形”、经过时间和当前处理摘要，供 AccessKit 暴露；装饰性品牌和网格不承担状态语义。
- 每个加载帧用 `Context::request_repaint_after` 请求约一个显示帧后的刷新；停止 pending 后不再主动刷新。

出处：W3C WCAG 2.2 SC 2.3.3 要求交互触发的非必要位移动画可禁用；Apple HIG Motion 同样要求 motion 可选、简短且不作为唯一反馈。

## 6. 布局与文案

加载视图占据整个 CentralPanel：深色背景、微弱环境光、中央 Equal Earth 拼图、投影轮廓与下方状态文案。窗口缩小时地图按可用宽高等比缩放，不改变块几何、动画顺序或时钟。

产品文案在 `src/ui/world_loading.rs` 只定义一次：

- 标题：`世界正在成形`；取消请求后为 `正在取消构建`。
- 眉题：`WORLD FORMATION ENGINE`。
- 过程：`构筑地表 · 求解海陆 · 发布新世界`。
- 经过时间使用 `MM:SS`，分钟不在 59 截断。

## 7. 验收与门禁

自动门禁：

- 20 个胞元、每块简单且凸、共享边拼合、相邻色调不同。
- 进入、停留、退出、循环无漂移及 reduced-motion 行为。
- pending 球面重建帧排队零个 GPU 地图 callback，并绘制加载状态；pending 清除后地图 callback 恢复为一个。
- 既有异步成功、失败、取消和 publication 原子性测试保持通过。
- `cargo fmt --all -- --check`。
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`。
- `cargo check --target wasm32-unknown-unknown --all-features --lib`。
- 完整调试回归。

用户验收：

1. 运行 `cargo run --release`。
2. 等首个世界发布后点击左侧“按当前参数重建”或“新种子并重建”。
3. 预期旧地图立即消失，中央出现二十块 Equal Earth 拼图；块依次移入淡入、停留、再移出淡出并循环。
4. 点击“取消”，预期加载视图维持到 worker 确认取消，随后旧地图恢复并显示取消状态。
5. 勾选“减少加载位移动效”后再次重建，预期仍有顺序淡入淡出，但块不再平移。

## 8. 非目标

- 不增加真实阶段进度百分比或预测剩余时间。
- 不修改世界生成算法、调度器、缓存、Artifact 或 GPU 渲染管线。
- 不为旧平面同步链新增 worker。
- 不新增第三方依赖、纹理、图片或 shader。
- 不把网页原型作为生产资产保留。

## 9. 修订记录

- R0（2026-08-23）：网页原型；验证按钮触发、六秒模拟和重复刷新。
- R1（2026-08-23）：用户裁定 Equal Earth 拼合形状、十三块自然凹板块与双向位移。
- R2（2026-08-23）：用户最终裁定恢复凸多边形并增加至二十块。
- R3（2026-08-23）：用户确认最终网页效果，授权 egui 落地、验证并提交；本规格冻结。
