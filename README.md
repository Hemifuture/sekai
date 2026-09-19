# Sekai

[![CI](https://github.com/Hemifuture/sekai/actions/workflows/rust.yml/badge.svg)](https://github.com/Hemifuture/sekai/actions/workflows/rust.yml)
[![GitHub Pages](https://github.com/Hemifuture/sekai/actions/workflows/pages.yml/badge.svg)](https://github.com/Hemifuture/sekai/actions/workflows/pages.yml)

Sekai 是一个用 Rust、egui 和 wgpu 构建的幻想世界生成器。项目面向世界观作者，以前工业、中世纪幻想为默认语境，生成可检查、可复现且因果关系清晰的**当前时间切片**。

当前里程碑聚焦自然世界底座。历史时间线、事件演化和历史回放不属于现阶段实现；系统只用当前板块、地幔、气候和地表过程之间的因果关系，使最终快照显得经历过自然塑形。

> 当前状态：自然底座里程碑已合入 `main`。完整进度、验证证据和未完成范围见 [项目进度](docs/PROGRESS.md)。

## 当前可以做什么

- 用固定根种子生成确定性的平面 Voronoi 世界，桌面默认规模为 20,000 个单元。
- 选择随机、多大陆、群岛、超级大陆、大岛与卫星岛、火山群岛六种世界形成预设。
- 生成彼此分责的板块、地壳、构造边界、地幔热点、地貌、地质、初步气候、水文与侵蚀字段。
- 根据当前板块相对运动形成汇聚、离散和走滑边界响应，并在 Relief 阶段形成相应高程变化。
- 在热点和洋—洋俯冲的因果支撑内生成海山、火山岛组和断续岛弧；多尺度噪声只负责塑形，不会凭空在全球海床上造陆。
- 保持有限平面地图的外边缘和东西可见边框为海洋，避免尚未实现无缝球面拓扑时出现截断大陆。
- 通过中文字段目录查看当前地表高程、海陆、地壳、板块、构造、火山、气候、水文、侵蚀和地质潜势等中间结果。
- 在原生桌面和 WASM 上运行同一套领域生成逻辑。

## 设计边界

Sekai 把“预设”“物理生成”“显示”保持为三个正交层次：

- 世界形成预设只提供宏观地壳初态和少量显式先验，不替代板块运动，也不直接绘制最终高度图。
- Tectonics 拥有板块速度与边界分类，Mantle 独立拥有热点，Relief 是把这些当前因果场解释为正式地形分量的唯一阶段。
- 气候、水文、侵蚀和显示层只消费上游公开产物；GPU 颜色、UI 状态和缓存都不是世界真值。
- 每个随机机制使用独立的确定性子流；阶段产物有版本、验证和缓存失效边界。
- 当前生成的是一个自洽快照，不生成地质年龄、热点年代、王朝、战争或其他历史事件。

这些边界及第一阶段总体目标以 [当前切片世界生成总体设计](docs/superpowers/specs/2026-07-28-sekai-current-slice-world-design.md) 为准。

## 快速开始

需要 Rust 1.85 或更高版本。运行桌面 release 构建：

```powershell
cargo run --release
```

应用左侧可以设置根种子、目标单元数、世界形态和构造活动度；重建后可在字段目录中切换不同的自然字段。

Web 版本使用 [Trunk](https://trunkrs.dev/)：

```powershell
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
$env:RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
$env:RUSTDOCFLAGS='--cfg getrandom_backend="wasm_js"'
trunk serve --release
```

也可以访问由 `main` 自动构建的 [GitHub Pages 版本](https://hemifuture.github.io/sekai/)。

## 验证

提交前的核心质量门如下：

```powershell
cargo fmt --all -- --check
cargo check --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --release --test natural_display_golden
```

CI 执行 fmt、workspace clippy、wasm32 检查、全套件测试、WASM/Trunk 构建、GPU 离屏参考测试和多平台 release 构建。性能预算与大语料证据是 `#[ignore]` 的 Release 探针，离线运行，不在 CI 里。Golden 变化必须经过人工视觉审阅，不能只机械更新图片。

## 文档

- [项目进度与当前边界](docs/PROGRESS.md)
- [当前切片世界生成总体设计](docs/superpowers/specs/2026-07-28-sekai-current-slice-world-design.md)
- [独立字段显示系统设计](docs/superpowers/specs/2026-07-29-field-display-system-design.md)
- [世界形成预设与海洋边框设计](docs/superpowers/specs/2026-08-02-world-formation-presets-design.md)
- [自然地图可信度修正设计](docs/superpowers/specs/2026-08-02-natural-map-polish-design.md)
- [因果岛屿与多尺度地貌噪声设计](docs/superpowers/specs/2026-08-03-causal-island-relief-design.md)

`docs/superpowers/specs/` 保存已确认的设计契约，`docs/superpowers/plans/` 保存相应实施与验收记录。`docs/` 下较早的通用架构、算法和地形研究文档保留作历史参考；若内容冲突，以当前切片总体设计和后续日期更晚的专项设计为准。

## 尚未实现

自然底座里程碑不代表第一阶段完整产品已经完成。目前仍未交付的主要范围包括完整土壤与生态、魔法地理、物种与社会当前切片、作者编辑与项目存储，以及通往外部城镇/村庄生成器的确定性链接。它们会继续沿用现有的类型化阶段、单一职责和只读显示边界；历史系统则留待未来独立设计。
