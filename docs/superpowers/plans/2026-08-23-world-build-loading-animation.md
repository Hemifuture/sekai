# 世界构建加载动画实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在异步球面世界构建期间立即隐藏旧地图，并在 egui CentralPanel 中播放用户确认的二十块 Equal Earth 拼合加载动画。

**Architecture:** `TemplateApp` 继续以 `world_build` 作为唯一 pending 状态，只负责在地图与加载视图之间路由。新建 `ui::world_loading`，集中静态共享拓扑、投影裁剪、纯时序函数与 egui 绘制；加载色板仍只由 `view::palette` 提供。旧 publication 留在内存中供失败/取消回滚，但 pending 期间不排队 GPU callback。

**Tech Stack:** Rust 1.85、egui/eframe 0.31、现有 `SphericalProjection`、egui `Painter`/`Shape::convex_polygon`、serde 持久化；无新依赖。

**Spec:** `docs/superpowers/specs/2026-08-23-world-build-loading-animation-design.md`

## Global Constraints

- `world_build.is_some()` 是加载状态唯一事实源；不新增第二个 loading bool。
- pending 期间保留旧 publication 但不绘制；成功后显示新 publication，失败或取消后恢复旧 publication。
- Equal Earth 数学只调用 `src/view/spherical_projection.rs`，不得复制公式或静态轮廓。
- 色板只定义于 `src/view/palette.rs`；UI 调用点不得重复 RGB 值。
- 静态拼图只保存一份共享顶点表和一份索引拓扑；不得逐块重复共享边坐标。
- 所有行为修改严格执行 RED → GREEN；测试断言真实帧输出，不检查源码文本。
- 不新增第三方依赖，不修改生成算法、Artifact、缓存、指纹或 GPU 管线。
- 临时网页原型在 egui 版本转绿后删除，生产 Rust 实现成为唯一事实源。
- 每项任务独立提交，提交信息使用祈使句主题并在正文解释动机。

---

## 文件结构

创建：

- `src/ui/world_loading.rs`：加载拼图共享拓扑、Equal Earth 轮廓采样与凸裁剪、纯动效帧、egui 绘制和模块测试。
- `docs/superpowers/specs/2026-08-23-world-build-loading-animation-design.md`：冻结的用户裁定与出处。
- `docs/superpowers/plans/2026-08-23-world-build-loading-animation.md`：本计划及执行证据。

修改：

- `src/ui/mod.rs`：注册 crate 内加载 UI 模块。
- `src/view/palette.rs`：增加 `WorldLoadingPalette` 与 `WORLD_LOADING_PALETTE` 唯一色板。
- `src/view/mod.rs`：仅向 crate 内重导出加载色板。
- `src/app.rs`：持久化 reduced-motion 选择；pending 时路由加载视图；更新旧地图 callback 回归测试。

删除：

- `prototypes/world-loading/`：已完成使命的网页验收稿，避免几何、颜色和时序重复。

---

### Task 1: 冻结规格与实施计划

**Files:**

- Create: `docs/superpowers/specs/2026-08-23-world-build-loading-animation-design.md`
- Create: `docs/superpowers/plans/2026-08-23-world-build-loading-animation.md`

**Interfaces:**

- Consumes: 用户于 2026-08-23 确认的最终网页原型。
- Produces: 后续任务引用的冻结状态语义、常量名、模块边界、验收步骤与技术出处。

- [x] **Step 1: 写入冻结规格**

记录二十块凸胞元、Equal Earth 轮廓、中心向外顺序、双向微位移、reduced motion、失败回滚和非目标。

- [x] **Step 2: 写入任务化计划**

逐项列出 RED、GREEN、受影响测试、门禁、提交和用户验收步骤；计划末尾列出每项承重技术出处。

- [x] **Step 3: 自审规格覆盖与占位符**

运行：

```powershell
rg -n "T[B]D|T[O]DO|implement l[a]ter|fill in d[e]tails|类似 T[a]sk|适当处[理]" docs/superpowers/specs/2026-08-23-world-build-loading-animation-design.md docs/superpowers/plans/2026-08-23-world-build-loading-animation.md
```

预期：无输出。再逐节核对“旧地图消失、二十块凸拼图、双向位移、reduced motion、失败回滚、无新依赖、UI 验收”都有任务承接。

- [x] **Step 4: 提交设计文档**

```powershell
git add docs/superpowers/specs/2026-08-23-world-build-loading-animation-design.md docs/superpowers/plans/2026-08-23-world-build-loading-animation.md
git commit -m "docs: freeze the world build loading animation" -m "Record the approved geometry, motion, rollback semantics, accessibility path, and implementation gates before production code changes."
```

---

### Task 2: 实现二十块 Equal Earth 加载视图

**Files:**

- Create: `src/ui/world_loading.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/view/palette.rs`
- Modify: `src/view/mod.rs`
- Modify: `docs/superpowers/specs/2026-08-23-world-build-loading-animation-design.md`

**Interfaces:**

- Consumes:
  - `crate::view::SphericalProjection` 与 `SphericalProjectionKind::EqualEarth`。
  - `crate::view::WORLD_LOADING_PALETTE`。
  - `elapsed: std::time::Duration`、`reduce_motion: bool`、`cancelling: bool`。
- Produces:

```rust
pub(crate) fn show_world_loading(
    ui: &mut egui::Ui,
    elapsed: std::time::Duration,
    reduce_motion: bool,
    cancelling: bool,
);
```

模块内纯函数契约：

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
struct LoadingPieceFrame {
    opacity: f32,
    travel: f32,
}

#[derive(Debug, Clone, Copy)]
struct LoadingPiece {
    vertices: &'static [usize],
    tone: usize,
}

fn loading_frame(elapsed_seconds: f64, reduce_motion: bool)
    -> [LoadingPieceFrame; WORLD_LOADING_PIECE_COUNT];
fn equal_earth_outline() -> Vec<[f32; 2]>;
fn clipped_piece_to_outline(
    piece_index: usize,
    travel: f32,
    outline: &[[f32; 2]],
) -> Vec<[f32; 2]>;
fn format_elapsed(elapsed: std::time::Duration) -> String;
```

- [x] **Step 1: 写动效与几何 RED 测试**

在 `src/ui/world_loading.rs` 先写模块测试并在 `src/ui/mod.rs` 注册模块。测试必须断言：

```rust
fn signed_area(points: &[[f32; 2]]) -> f32 {
    points
        .iter()
        .enumerate()
        .map(|(index, &[x, y])| {
            let [next_x, next_y] = points[(index + 1) % points.len()];
            x * next_y - next_x * y
        })
        .sum::<f32>()
        * 0.5
}

fn piece_is_convex(piece: &LoadingPiece) -> bool {
    let points: Vec<_> = piece
        .vertices
        .iter()
        .map(|&index| WORLD_LOADING_VERTICES[index])
        .collect();
    let mut direction = 0.0_f32;
    for index in 0..points.len() {
        let [ax, ay] = points[index];
        let [bx, by] = points[(index + 1) % points.len()];
        let [cx, cy] = points[(index + 2) % points.len()];
        let turn = (bx - ax) * (cy - by) - (by - ay) * (cx - bx);
        if turn == 0.0 {
            continue;
        }
        if direction != 0.0 && direction.signum() != turn.signum() {
            return false;
        }
        direction = turn;
    }
    direction != 0.0
}

fn adjacent_pieces_use_distinct_tones() -> bool {
    let mut owners = std::collections::BTreeMap::new();
    for piece in WORLD_LOADING_PIECES {
        for index in 0..piece.vertices.len() {
            let first = piece.vertices[index];
            let second = piece.vertices[(index + 1) % piece.vertices.len()];
            let key = if first < second {
                (first, second)
            } else {
                (second, first)
            };
            if owners.insert(key, piece.tone).is_some_and(|tone| tone == piece.tone) {
                return false;
            }
        }
    }
    true
}

fn convex_polygon_contains(polygon: &[[f32; 2]], point: [f32; 2]) -> bool {
    let orientation = signed_area(polygon).signum();
    polygon.iter().enumerate().all(|(index, &[ax, ay])| {
        let [bx, by] = polygon[(index + 1) % polygon.len()];
        ((bx - ax) * (point[1] - ay) - (by - ay) * (point[0] - ax)) * orientation
            >= -f32::EPSILON
    })
}

fn opacities(frame: &[LoadingPieceFrame; WORLD_LOADING_PIECE_COUNT]) -> [f32; 20] {
    std::array::from_fn(|index| frame[index].opacity)
}

#[test]
fn loading_map_has_twenty_convex_shared_edge_pieces() {
    assert_eq!(WORLD_LOADING_PIECES.len(), 20);
    assert!(WORLD_LOADING_PIECES.iter().all(|piece| {
        piece.vertices.len() >= 3
            && piece
                .vertices
                .iter()
                .all(|&index| index < WORLD_LOADING_VERTICES.len())
    }));
    assert!(WORLD_LOADING_PIECES.iter().all(piece_is_convex));
    assert!(adjacent_pieces_use_distinct_tones());
}

#[test]
fn blocks_enter_hold_exit_and_loop_without_drift() {
    let entering = loading_frame(0.25, false);
    assert!(entering[0].opacity > entering[1].opacity);
    assert!(entering[0].travel < entering[1].travel);
    let exiting = loading_frame(1.35, false);
    assert!(exiting[0].opacity < exiting[1].opacity);
    assert!(exiting[0].travel > exiting[1].travel);
    assert_eq!(loading_frame(2.35, false), entering);
}

#[test]
fn reduced_motion_preserves_fades_and_removes_displacement() {
    let full = loading_frame(0.25, false);
    let reduced = loading_frame(0.25, true);
    assert_eq!(opacities(&reduced), opacities(&full));
    assert!(reduced.iter().all(|piece| piece.travel == 0.0));
}

#[test]
fn moved_pieces_are_clipped_to_the_production_equal_earth_outline() {
    let outline = equal_earth_outline();
    for index in 0..WORLD_LOADING_PIECE_COUNT {
        let points = clipped_piece_to_outline(index, 1.0, &outline);
        assert!(points.len() >= 3);
        assert!(points
            .into_iter()
            .all(|point| convex_polygon_contains(&outline, point)));
    }
}
```

生产变更若把块数改回十三、重新引入凹折、让相邻块同色、移除退出位移、改变循环周期或绕过投影裁剪，至少一条测试必须失败。

- [x] **Step 2: 运行 RED**

运行：

```powershell
cargo test --lib ui::world_loading::tests -- --nocapture
```

预期：编译失败，因为加载常量、纯函数、色板和绘制入口尚不存在。

- [x] **Step 3: 增加唯一加载色板**

在 `src/view/palette.rs` 增加 crate 内只读结构：

```rust
pub(crate) struct WorldLoadingPalette {
    pub(crate) background: [u8; 3],
    pub(crate) surface: [u8; 3],
    pub(crate) outline: [u8; 3],
    pub(crate) ink: [u8; 3],
    pub(crate) muted: [u8; 3],
    pub(crate) accent: [u8; 3],
    pub(crate) tones: [[u8; 3]; 7],
}

pub(crate) const WORLD_LOADING_PALETTE: WorldLoadingPalette = WorldLoadingPalette {
    background: [4, 10, 16],
    surface: [5, 16, 24],
    outline: [194, 220, 210],
    ink: [233, 241, 236],
    muted: [99, 119, 112],
    accent: [184, 217, 196],
    tones: [
        [66, 110, 125],
        [118, 160, 145],
        [190, 196, 149],
        [243, 230, 190],
        [140, 170, 148],
        [86, 127, 133],
        [105, 150, 138],
    ],
};
```

实际值逐项抄自最终网页原型的 CSS；`src/view/mod.rs` 只作 crate 内重导出，UI 不再出现 RGB 字面量。

- [x] **Step 4: 实现共享拓扑、生产投影轮廓与凸裁剪**

把最终原型的二十块几何转写为一份 `WORLD_LOADING_VERTICES` 和一份索引拓扑。`equal_earth_outline` 调用生产投影，`clipped_piece_to_outline` 用 Sutherland–Hodgman 逐边裁剪移动后的胞元；不增加几何 crate 或运行时随机数。绘制和测试都传入同一帧已计算的轮廓，避免逐块重复投影。

- [x] **Step 5: 实现纯时序与 egui 绘制**

使用规格常量和 Khronos `smoothstep` 公式生成每块 `opacity/travel`。绘制顺序为背景与环境光 → 投影表面 → 二十块凸多边形 → 轮廓 → 实际 egui 状态文案；pending 时调用
`ui.ctx().request_repaint_after(Duration::from_millis(16))`。

- [x] **Step 6: 运行 GREEN 与格式检查**

运行：

```powershell
cargo test --lib ui::world_loading::tests -- --nocapture
cargo fmt --all -- --check
```

预期：加载模块测试全部通过，格式检查退出 0。

执行证据（2026-08-23）：RED 因加载常量和纯函数尚不存在而产生 32 个编译错误；实现后 7 个模块测试全部通过。裁剪调试先定位到设计平面尺度下 `-0.000038146973` 的 `f32` 叉积舍入，再以机器精度和设计平面尺度推导容差；最终几何、时序、reduced-motion、计时和真实 egui 文本/二十块绘制断言均为 GREEN，格式检查退出 0。为保持任务逐提交且让本提交独立通过 `-D warnings`，模块入口和两项色板事实临时用 `cfg_attr(not(test), allow(dead_code))` 标记；Task 3 接入唯一真实消费者时立即删除这些属性。

- [x] **Step 7: 提交加载视图**

```powershell
git add src/ui/world_loading.rs src/ui/mod.rs src/view/palette.rs src/view/mod.rs docs/superpowers/specs/2026-08-23-world-build-loading-animation-design.md docs/superpowers/plans/2026-08-23-world-build-loading-animation.md
git commit -m "feat: draw the Equal Earth build animation" -m "Render the approved twenty-piece convex loading choreography from one shared topology and the canonical projection and palette sources."
```

---

### Task 3: 用真实构建状态替换旧地图

**Files:**

- Modify: `src/app.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/ui/world_loading.rs`
- Modify: `src/view/palette.rs`
- Delete: `prototypes/world-loading/index.html`
- Delete: `prototypes/world-loading/styles.css`
- Delete: `prototypes/world-loading/app.js`
- Delete: `prototypes/world-loading/animation.js`
- Delete: `prototypes/world-loading/animation.test.mjs`
- Delete: `prototypes/world-loading/map-geometry.js`
- Delete: `prototypes/world-loading/map-geometry.test.mjs`
- Delete: `prototypes/world-loading/simulation.js`
- Delete: `prototypes/world-loading/simulation.test.mjs`
- Delete: `prototypes/world-loading/server-compat.test.mjs`
- Delete: `prototypes/world-loading/package.json`

**Interfaces:**

- Consumes: `ui::world_loading::show_world_loading` 与既有 `PendingWorldBuild`。
- Produces:
  - `TemplateApp::reduce_loading_motion: bool`，`#[serde(default)]` 持久化。
  - pending CentralPanel 零地图 callback；pending 清除后现有地图 callback 恢复。

- [x] **Step 1: 把旧 callback 断言改为 RED**

拆分 `packet_changing_app_actions_queue_only_the_current_callback_in_the_same_frame`：非重建 action 继续要求一个当前 callback；新增测试构造一个未完成的 `PendingWorldBuild`，调用
`show_active_canvas_after_actions` 后断言：

```rust
fn spherical_callback_count(output: &egui::FullOutput) -> usize {
    output
        .shapes
        .iter()
        .filter(|shape| matches!(shape.shape, egui::epaint::Shape::Callback(_)))
        .count()
}

fn collect_text(shape: &egui::epaint::Shape, output: &mut Vec<String>) {
    match shape {
        egui::epaint::Shape::Text(text) => output.push(text.galley.text().to_owned()),
        egui::epaint::Shape::Vec(shapes) => {
            for shape in shapes {
                collect_text(shape, output);
            }
        }
        _ => {}
    }
}

fn test_raw_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    }
}

let (sender, receiver) = std::sync::mpsc::channel();
app.world_build = Some(PendingWorldBuild {
    receiver,
    cancellation: crate::engine::BuildCancellation::new(),
    started_at: std::time::Instant::now(),
    replacement: true,
});
let output = context.run(test_raw_input(), |context| {
    app.show_active_canvas_after_actions(context, Vec::new());
});
assert_eq!(spherical_callback_count(&output), 0);
let mut texts = Vec::new();
for shape in &output.shapes {
    collect_text(&shape.shape, &mut texts);
}
assert!(texts.iter().any(|text| text == "世界正在成形"));
drop(sender);
app.world_build = None;
let restored = context.run(test_raw_input(), |context| {
    app.show_active_canvas_after_actions(context, Vec::new());
});
assert_eq!(spherical_callback_count(&restored), 1);
```

另在持久化测试中把 `reduce_loading_motion = true` 往返序列化，证明开关不是临时测试字段。

- [x] **Step 2: 运行 RED**

运行：

```powershell
cargo test --lib app::natural_app_tests::pending_world_build_hides_the_old_map_until_rollback_or_publication -- --exact --nocapture
cargo test --lib app::natural_app_tests::loading_motion_preference_roundtrips -- --exact --nocapture
```

预期：第一条仍看到旧地图 callback 或缺少加载文案；第二条因字段不存在而编译失败。

- [x] **Step 3: 最小接入 app 状态与侧边栏**

在 `TemplateApp` 增加 `#[serde(default)] reduce_loading_motion: bool`。重建按钮附近增加
`ui.checkbox(&mut self.reduce_loading_motion, "减少加载位移动效")`；保留既有 spinner、已用秒数与取消按钮。

- [x] **Step 4: pending 时只绘制加载 CentralPanel**

`show_active_canvas_after_actions` 先应用 actions，再读取 `world_build`。若 pending：

```rust
let elapsed = pending.started_at.elapsed();
let cancelling = pending.cancellation.is_cancelled();
egui::CentralPanel::default().show(ctx, |ui| {
    crate::ui::world_loading::show_world_loading(
        ui,
        elapsed,
        self.reduce_loading_motion,
        cancelling,
    );
});
return;
```

否则原样进入 legacy/spherical canvas。不得清空 `spherical_presentation` 或 renderer；回滚仍由 pending 清除自然恢复。

- [x] **Step 5: 运行 GREEN 与受影响回归**

运行：

```powershell
cargo test --lib ui::world_loading::tests -- --nocapture
cargo test --lib app::natural_app_tests::pending_world_build_hides_the_old_map_until_rollback_or_publication -- --exact --nocapture
cargo test --lib app::natural_app_tests::packet_changing_app_actions_queue_only_the_current_callback_in_the_same_frame -- --exact --nocapture
cargo test --lib app::natural_app_tests::failed_spherical_startup_is_visible_and_retries_standalone_without_planar_fallback -- --exact --nocapture
cargo test --lib app::natural_app_tests::gpu_failed_spherical_startup_is_visible_and_retry_publishes_once -- --exact --nocapture
```

预期：全部通过；失败/取消语义与 publication 原子性不变。

执行证据（2026-08-23）：callback RED 实际得到 `left: 1, right: 0`；持久化 RED 因 `TemplateApp` 缺少 `reduce_loading_motion` 产生 3 个编译错误。最小接入后两条目标测试转绿，且取消中的帧保持零 callback 并显示“正在取消构建”；7 个加载视图测试、同帧 packet callback、无 GPU 启动失败重试、GPU 准备失败重试均通过。接入真实消费者后删除 Task 2 的临时 dead-code 属性，并按 Clippy 反馈删除只被测试调用的裁剪包装函数，让绘制与测试直接复用同一个带轮廓参数的生产助手。

- [x] **Step 6: 删除临时网页原型**

用 `apply_patch` 删除 `prototypes/world-loading/` 中全部文件。确认 `rg --files prototypes/world-loading` 无输出，生产几何、时序与色板只剩 Rust 事实源。

- [x] **Step 7: 提交 app 集成**

```powershell
git add src/app.rs src/ui/mod.rs src/ui/world_loading.rs src/view/palette.rs docs/superpowers/plans/2026-08-23-world-build-loading-animation.md
git commit -m "feat: replace rebuilding maps with the loading stage" -m "Hide the retained publication while a worker build is pending, preserve rollback semantics, expose reduced motion, and retire the duplicate web prototype."
```

---

### Task 4: 完整门禁、执行证据与交付

**Files:**

- Modify: `docs/superpowers/plans/2026-08-23-world-build-loading-animation.md`

**Interfaces:**

- Consumes: Tasks 1–3 的已提交实现。
- Produces: 可复核的测试输出、提交列表和用户 UI 验收步骤；不执行合并。

- [ ] **Step 1: 跑格式、Clippy 与 wasm 门禁**

运行：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check --target wasm32-unknown-unknown --all-features --lib
```

预期：全部退出 0，无 warning。

- [ ] **Step 2: 跑完整调试回归**

运行：

```powershell
cargo test --workspace --all-features --no-fail-fast
```

预期：非 ignored 测试全部通过；若出现失败，先按系统化调试定位，不以本任务无关为由跳过。

- [ ] **Step 3: 核对差异与事实源**

运行：

```powershell
git diff --check
git status --short
rg -n "#426e7d|WORLD_LOADING_TRANSITION_SECONDS|WORLD_LOADING_VERTICES" src prototypes
```

预期：无空白错误；只存在计划内修改；色值只在 `src/view/palette.rs`，时序和几何只在
`src/ui/world_loading.rs`，`prototypes/world-loading` 不存在。

- [ ] **Step 4: 记录执行证据并提交**

把各命令的测试数量、退出码与提交哈希写入本计划“执行证据”，然后运行：

```powershell
git add docs/superpowers/plans/2026-08-23-world-build-loading-animation.md
git commit -m "docs: record loading animation verification" -m "Capture the native, wasm, lint, regression, and handoff evidence for the committed egui implementation."
```

- [ ] **Step 5: 向用户交付但不合并**

报告提交哈希与以下验证流程：`cargo run --release` → 等首图 → 点击“按当前参数重建” → 检查旧图立即消失和二十块循环 → 取消检查回滚 → 勾选 reduced motion 再试。明确当前分支保持不合并，等待用户通知。

---

## 每项承重技术的出处

1. **Equal Earth 投影轮廓**：Bojan Šavrič、Tom Patterson、Bernhard Jenny，*The Equal Earth map projection*，International Journal of Geographical Information Science 33(3), 454–465，DOI `10.1080/13658816.2018.1504949`。生产侧复用现有 `SphericalProjection`，不复制公式。
2. **凸窗多边形裁剪**：Ivan E. Sutherland、Gary W. Hodgman，*Reentrant Polygon Clipping*，Communications of the ACM 17(1), 32–42, 1974，DOI `10.1145/360767.360802`。
3. **缓动公式**：Khronos Group，GLSL 4.60.8 `smoothstep`，`t = clamp(...)`、`t²(3−2t)` 的标准定义。
4. **进入/退出时序与淡出组合**：Microsoft Fluent Design “Motion in Windows”，直接进入/既有元素采用 167/250/333 ms，直接退出要求与 fade-out 组合；本项目采用用户在网页原型确认的 300 ms。
5. **错峰编排**：IBM Carbon Design System “Motion” 要求产品团队按整体 UI 进入/退出关系协调 choreography；本项目的 0.6 秒错峰窗口与中心向外顺序由用户在最终原型确认。
6. **减少动态效果**：W3C WCAG 2.2 SC 2.3.3 “Animation from Interactions” 与 Apple Human Interface Guidelines “Motion”；位移可禁用，状态同时通过淡入淡出与文本表达。
7. **按帧刷新**：egui 0.31 `Context::request_repaint_after` 契约；只在 pending 视图请求下一帧，构建结束后停止。

## 执行证据

执行时逐任务记录，不预填结果。
