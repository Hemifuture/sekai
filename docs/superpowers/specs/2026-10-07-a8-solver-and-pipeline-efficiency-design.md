# 生成时延里程碑 A8 设计：求解器与流水线的逐位等价提速

日期：2026-10-07（实测 2026-10-07 至 2026-10-08）
状态：**实施完成（分支 `a8-bitwise-solver-efficiency`，6 个提交，未推送），待合并与
用户 UI 验收**。用户 2026-10-07 授权自行评估提速与数值策略（“你自己评估决策就行，
只要正确、快速就没问题”）。偏离按 §9 修订条目记录。

上位：`AGENTS.md`；A1 `2026-09-02-generation-latency-design.md`（目标 Draft 10–20 s、
Standard ≤ 60 s）；A6 `2026-09-29-a6-generation-latency-recovery-design.md`（§4.4
成对计时与 2 % 保留门）；A7 `2026-10-06-a7-gravity-wave-time-splitting-design.md`
（本里程碑的基线 `9e87d39`）。

## 1. 范围与边界

- **只做逐位等价的提速。** 任何改动都不得改变数值：全部已发布引擎产物的
  `ContentHash`、`BuildResultHash`、字段文档与呈现哈希，以及 P4 起终点探针快照，
  必须与基线逐字节一致。改变数值的杠杆（如终点 FAS 复用起点校正）不在本文件，
  见 §8。
- 方程、离散、积分器、步长规划、模型指纹、发布语义、引擎契约
  （`StoredArtifact::new_cancellable` 的发布时完整校验）均不变。
- **信任边界原则。** 每个不可变对象只在其信任边界完整校验一次——生产者的最终
  校验、公开 API 入口，或引擎产物发布——此后在同一进程内信任（King 2019，
  “Parse, don't validate”；先例 `222be5e`）。删除一处校验的前提是能指出**同一
  不可变对象**对**同一对照**的更早校验（提交正文逐条引用文件:行）；只缺廉价的
  绑定/身份检查时保留或补上该检查。公开入口（语料/证据调用者、外部测试）保持
  完整校验，并委托到 crate 私有的受信路径，不复制校验体（SSOT）。
- 产品失败的错误码映射：对生产链能产生的任何输入不变；只有生产链无法产生的
  输入的报错先后或报错位置可能变化，逐项记在 §4、§5。
- 保留门：A6 §4.4，每项改动对其**受影响项**（目标阶段或包装项，按实测）收益
  不足 2 % 不保留；同时报告其在全图中的占比。

## 2. 验证装置

1. **探针快照**：`stage_timing_probe`（`causal.rs`，ignored，Release）以
   `SEKAI_PROBE_TAG` 写出 Draft / Standard 的 P4 起点与终点气候快照 JSON，与
   `9e87d39` 的基线快照 `cmp` 逐字节比对；同时核对冻结摘要（Draft 起点
   5/60/239/0.2231、终点 5/60/239/0.2232；Standard 4/48/206/0.1675、
   4/48/206/0.1667，依次为循环/步/快子步/终残差）。
2. **全图装置**（临时补丁，不提交）：ignored 测试 `a8_meas_full_graph` 走 UI 路径
   `build_spherical_formation_candidate_for_view`（P1 表面 → 外部产物 →
   `BuildEngine::build` → 字段文档 → 呈现），种子 42，打印分段耗时
   （`engine_scope` = P1 表面 + 外部产物 + 引擎构建；`ui_scope` 再加文档与呈现）
   与 13 行哈希：8 个外部产物与 2 个阶段输出的 `ContentHash`、`BuildResultHash`、
   文档哈希（目录载荷、诊断、面积汇总、呈现来源、质量档、色标锚点）与呈现哈希。
   基线取 `14a13da`。
3. **计时纪律**：Release，同机，确认 CPU 空闲后基线与新版交替成对运行（Draft
   4 对、Standard 2 对），取最小值；受影响项另用临时 `Instant` 计时（不提交）。
   任何时刻只跑一个 cargo 构建或测试。

## 3. 引擎开销实测（`14a13da`，种子 42）

在一次性 worktree 上给调度器、阶段适配器、产物发布、`graph.rs`、`causal.rs`、
工作域阶段与 UI 文档/呈现加计时（未提交；检测构建的探针摘要与冻结值一致）。
生成器 P2–P5 在探针与全图中速度相同（各行相差约 1 %），差距全部在其外围。

| 范围 | Draft | Standard |
| --- | ---: | ---: |
| 探针全链 | 21.58 s | 48.64 s |
| 全图，引擎范围 | 24.31 s（+2.73） | 60.03 s（+11.39） |
| 全图，UI 范围 | 25.06 s（+3.48） | 63.09 s（+14.45） |

| # | 项 | Draft s | Standard s |
| --- | --- | ---: | ---: |
| 1 | `CausalFormationOutput::validate`：重验生成器已校验过的五个快照 | 0.87 | 3.60 |
| 2 | `from_output`：质量报告与捆绑组装 | 1.05 | 4.32 |
| 3 | 阶段内 `ProfileSurfaceBuilder::complete`：重建调用方已建过的控制表面与映射 | 0.38 | 1.56 |
| 4 | 捆绑 `StoredArtifact::new_cancellable`（校验 0.09/0.34 + 哈希 0.32/1.21） | 0.41 | 1.55 |
| 5 | 外部产物（表面校验 + 发布校验 + 哈希） | 0.15 | 0.59 |
| 6 | 工作域阶段包装（阶段内与发布时各校验一次 + 哈希） | 0.09 | 0.21 |
| 7 | 引擎簿记（依赖/外部/输出哈希、缓存、来源、适配器） | ≈0 | ≈0 |
| | 引擎范围合计 | 2.95 | 11.83 |
| 8 | `SphericalFormationFieldDocument::from_build_outcome`（仅 UI） | 0.57 | 2.34 |
| 9 | `assemble_spherical_candidate`（仅 UI：定位、投影图、球面网格） | 0.17 | 0.70 |

- 第 2 行：P3 质量 0.64/2.67（重验表面、P2、基底、地形量测，并在内部重算一遍
  P2 报告 0.11/0.46）；P2 质量 0.11/0.46；气候质量 0.09/0.37；
  `NaturalFormationBundle::new`（含完整校验）0.12/0.48；`validate_product`
  0.09/0.34。捆绑完整校验每次构建共 4 次（`new`、`from_output` 末尾、发布、UI 文档）。
- 第 8 行：表面 0.036/0.14、捆绑 0.09/0.34、P2 兼容性 0.04/0.15、P5 0.025/0.10、
  地形 `validate_against_authoring` 0.25/1.03、基底 0.10/0.43。
- **分类合计**：全部 BLAKE3 哈希（含序列化）Draft 0.45 s（全图 1.8 %）、Standard
  1.64 s（2.6 %）；对上游已校验对象的重复校验与重算约 2.4 s / 9.5 s，是差距主体。
  哈希的成本在逐次写入（JSON 87.1 MB 分 2240 万次写入 / 335.7 MB 分 8700 万次），
  BLAKE3 本身约 6 GB/s。
- **更正**：A1 计划 `plans/2026-09-02-generation-latency.md` Task 5 把“剩余的
  1.8 s 引擎开销”归为“束产物 JSON 序列化 + blake3 身份哈希（来源契约，不动）”。
  该归因不成立：哈希只占 0.45 / 1.64 s，其余是校验与重算，且后者并非来源契约
  要求。计划原行已加更正并指向本节。

## 4. 第 1 轮：求解器内核与工作域（`70791e5`、`5c7fc00`、`14a13da`）

计时以探针阶段行为受影响项；起点求解、P5 + 终点段为 A6 §4.4 的“阶段”。

### 4.1 `70791e5` 工作域在因果链内只完整校验一次

- **改动**：同一 `ClimateWorkDomainSnapshot` 对同一表面原被完整校验五次（起点强迫
  构建、起点求解、P5 `validate_inputs`、终点强迫构建、终点求解）。保留起点公开
  `GlobalClimateForcingBuilder::build` 为链内唯一完整校验；起终点求解改调
  `generate_from_validated`；P5 与 crate 私有的 `build_for_formation_terrain` 只查
  常数时间的 `validate_binding_against`（`validate_common_inputs` 改收一个校验闭包，
  未新增函数）。公开 `build` / `generate` / `generate_with_phase_observer` 保持完整
  校验，顺序与错误变体不变。
- **逐位**：只删只读校验调用，快照无内部可变性。
- **实测**：单次完整校验 Draft 0.22–0.23 s、Standard 0.84–1.26 s。成对最小值 P5 +
  终点段 Draft 11.786 → 10.988 s（−6.8 %），Standard 26.418 → 22.713 s（−14 %）；
  Standard 起点求解 −5.6 %。Draft 起点求解的成对差只有 −1.4 %，保留依据是直接计时
  0.229 s / 9.7 s = 2.4 %。合计约省 Draft 0.9 s、Standard 3.9 s。
- **失败语义**：域内部损坏但绑定正确时只由起点强迫构建捕获——生产链里那本来就是
  第一次完整校验。`build_for_formation_terrain` 的“调用方已校验表面”是承重前提
  （唯一生产调用方在 `validate_inputs` 校验表面之后）。取消轮询次数减少，强迫的
  取消测试（≥ 8 次观察）仍过。

### 4.2 `5c7fc00` Verlet 小步连续方程每条边的通量只算一次

- **改动**：`thickness_tendency_into` 先按边算一次 `edge_volume_flux_m2_s × 面深度`
  入 f64 缓冲（每界面一个通量，LeVeque 2002 ch. 4），再按各格元原有边序、0 起始、
  同一符号规则聚合，最后除面积；缓冲由 `time_split_stage` 每阶段分配一次。
- **逐位**：乘积存为 f64 无损；纯函数，同一边两侧原本就得到同值；取负精确；求和
  顺序不变；Rust 不做隐式 FMA。
- **实测**（与 `70791e5` 成对）：起点求解 Draft 9.385 → 8.905 s（−5.1 %）、
  Standard 16.243 → 15.413 s（−5.1 %）；终点段 −4.5 % / −4.6 %；探针全链 Draft
  23.381 → 22.369 s、Standard 51.704 → 49.939 s。
- **未保留**：小步梯度复用缓冲（组件 Draft 0.65 → 0.54 s，约起点行 1.2 %；成对
  Standard 起点 +0.9 %）；阶段末合成跳过被覆盖的大气 h/u（约 1.3 % / 1.0 %，且需
  新接口、改变失败语义）；踢步中提出层查表（< 1 %）。

### 4.3 `14a13da` 边的中点位移与插值权重改为网格静态度量

- **改动**：`SphericalEdge` 在构网时以原表达式存下两侧中心到中点的切向位移与两项
  距离比权重（同 MPAS 的边度量，Ringler et al. 2010），二阶面重构与
  `interpolate_vector_f64` 直接取用；原自由函数 `edge_displacement_m` 移入
  `grid.rs`。`interpolate_scalar_f64` 保留除法形式。
- **逐位**：同一表达式、同一输入、同一运算顺序；边只在 `new_impl` 构造、此后不变；
  网格指纹不含派生量。内存每边 +64 B（Draft 约 0.44 MB）。
- **实测**（与 `5c7fc00`、`9e87d39` 三方成对）：起点求解 Draft 8.903 → 8.441 s
  （−5.2 %）、Standard 15.445 → 14.830 s（−4.0 %）；终点段 −4.5 % / −2.6 %；
  探针全链 Draft 22.314 → 21.370 s、Standard 49.921 → 48.747 s。微基准：动量输运
  0.51 → 0.33 ms/次，其中面重构 0.17 → 0.09 ms。
- **未保留**（按每次 Draft 求解约 1293 次细网格当量快张量评估折算）：应变黏性提出
  层查找等（约起点行 1.0 %）；海洋温度输运供体通量按边只算一次（约 1.1 %）；
  `from_tendency` 与 `add` 融合（整项仅 1.1 %，全删也不足 2 %）。前两项合计约
  2.1 %，若日后改按杠杆而非按项判门，可合为一个提交重新成对计时。

第 1 轮累计（探针全链，对 `9e87d39`）：Draft 24.202 → 21.370 s（−11.7 %），
Standard 55.480 → 48.747 s（−12.1 %）。

## 5. 第 2 轮：引擎与 UI 包装（`08e64be`、`943b319`、`4f32353`）

计时用 §2 全图装置；受影响项为对应包装项。

### 5.1 `08e64be` 产物内容哈希经 64 KiB 缓冲再喂给 BLAKE3

- **改动**：`stream_hash` 与 `stream_hash_cancellable` 外包
  `BufWriter::with_capacity(HASH_BUFFER_BYTES = 64 KiB)`；序列化后显式 `flush`
  并以 `serde_json::Error::io` 传播其错误，再 `finalize`。可取消版：取消轮询仍在
  内层 `CancellableHasherWriter`（约每 64 KiB），首次与末次显式检查保留，末次检查
  移到 flush 之后。
- **逐位**：喂给 BLAKE3 的字节序列不变，`BufWriter` 只改分块；钉住哈希的测试
  （`result_hash_uses_the_exact_v1_output_only_byte_frame` 等）未改且通过。
- **实测**：发布时可取消哈希 Draft 0.370 → 0.166 s、Standard 1.379 → 0.611 s
  （−55 %）；外部产物段 Draft 0.147 → 0.099 s、Standard 0.605 → 0.400 s。全图
  `engine_scope` Draft 24.560 → 24.314 s（−1.0 %），Standard 60.264 → 59.242 s
  （−1.7 %），与 §3 的估计（0.26 / 0.93 s）一致。
- **失败语义**：新增错误源只有显式 flush，走与其他写入错误相同的 Serialization /
  Cancelled 分支；内层写入器只会因取消失败，故错误码不变。两次轮询间最坏工作量
  从约 64 KiB 升到约 128 KiB（远小于取消测试的 250 ms 界）；取消测试的 8 次观察
  窗口仍有约 128 次轮询的余量。日后若再加快哈希，应复核这一余量。

### 5.2 `943b319` 因果形成产物在阶段内只校验一次，质量证据复用已算出的 P2 报告

- **改动与对应的更早校验**（生产路径 `generate_working` → `from_output`）：
  - 删除 `CausalFormationOutput::validate`。P2 在 `tectonics/publication.rs:412`、
    基底在 `geologic_substrate.rs:140`、地形在 `formation/primary_relief.rs:105`、
    终点 P4 在 `global_circulation/generation.rs:381`（多出的表面校验已在
    `formation/evolved_tectonics.rs:78` 完成）、P5 在
    `surface_formation/generation.rs:1261`，均以同一权威表面（及同一上游兄弟）
    结束校验。
  - 质量：新增 crate 私有 `evaluate_evolved_tectonic_quality_from_validated` 与
    `evaluate_primary_relief_quality_from_validated`；P3 质量直接复用 `graph.rs`
    刚算出的 P2 报告，并以 `p2.surface_ref() == evolved.surface_ref()` 做廉价
    绑定检查。公开 `evaluate_*` 保持完整校验并委托私有路径。
  - 捆绑：`from_output` 改用 `NaturalFormationBundle::from_validated_siblings`，
    只查模式头、跨兄弟身份与质量报告绑定（各兄弟的 `validate()` 是上列生产者
    `validate_against` 的第一步）；末尾 `validate_product` 改为只校验四份质量
    报告（P2/P4/P5 报告无更早校验，且 P4 硬闭合失败可由生产链产生，留在这里才能
    维持 `causal-formation.invalid-input` 产品错误码）。发布时完整校验与反序列化
    的完整校验 `new()` 不变。
- **逐位**：只删只读校验；复用的 P2 报告与重算的是同一函数作用于同一输入。
- **实测**：受影响项（`CausalFormationOutput::validate` + `from_output`）Draft
  1.925 → 0.132 s、Standard 7.97 → 0.524 s（−93 %）。按组看，重复的
  `validate_against`（0.87/3.60）、P3 质量输入重验（约 0.53/2.2，含两次各约
  0.035/0.14 的表面校验——单看这两次低于 2 %，按组保留）、重算的 P2 报告
  （0.11/0.46）、P2 质量输入重验（约 0.1/0.4）、两次捆绑完整校验（各 0.09/0.34）
  都远超受影响项的 2 %。与父提交 `08e64be` 成对：`ui_scope` Draft 25.101 →
  23.227 s（−7.5 %），Standard 62.757 → 55.735 s（−11.2 %）；`engine_scope`
  Draft 24.347 → 22.475 s、Standard 59.669 → 52.603 s。提交正文里的
  “24.91 → 23.04 / 62.35 → 55.15”是修订前首版的成对值，以本节为准。
- **失败语义**：生产输入的错误码与顺序不变。兄弟失败（不可达）改由发布报同一
  `causal-formation.invalid-input`；新 P2 绑定错误走 Quality → invalid-input，
  生产不可达。公开 `evaluate_primary_relief_quality` 对通过输入校验、但 P2 评估与
  量测收集都失败的输入，改为先报 P2 评估错误。`InvalidFinalCandidate` 只剩离线
  参考测试使用，改为 `cfg(test)`。P3 私有入口的 P2 报告绑定只比表面引用，调用方
  须传入由同一 `evolved` 算出的报告（唯一调用方在两行之前算出它）。

### 5.3 `4f32353` 形成字段文档信任已验证构建的产物

- **改动**：`SphericalFormationFieldDocument::build` 不再对已校验的产物重跑
  `surface.validate()`、`bundle.validate()`、P2 兼容性、P5、地形
  `validate_against_authoring` 与基底 `validate_against_surface`；
  `SurfaceRef::for_spherical`（会再校验表面，`surface_ref.rs:121`）改为 crate 私有
  `from_validated_spherical(...).expect(...)`。
- **更早校验**：唯一构造入口 `from_build_outcome` 先经
  `BuildOutcome::verified_provenance`（`engine/scheduler.rs:337-363`）绑定报告与
  产物集哈希；表面在外部产物插入时经 `StoredArtifact::new` 校验
  （`spherical_stage.rs:102`，含指纹重算，故 `expect` 在生产不会触发）；捆绑在
  发布时经 `validate_product`（`graph.rs:176` → `formation_bundle.rs:104`）；对照
  表面与地形规格的校验由形成阶段的生产者完成（阶段从同一产物集取表面与规格，
  `graph.rs:290`、`:303`；权威表面是其克隆，`profile_surface.rs:109`）。
- **保留的廉价绑定**：P5 快照 `surface_ref` 等于结果表面的 `SurfaceRef`（指纹覆盖
  半径、顶点、格元面积与边界，相等即内容相同）；地形对结果 `ReliefSpecArtifact`
  的 `validate_authored_policy`（改为 `pub(crate)`；按位比较，`WaterInventory` 时
  加一次面积求和）。二者挡住“别的构建的捆绑配上不同表面或规格”。
- **逐位**：文档内容不变，文档与呈现哈希逐位一致。
- **实测**（与 `14a13da` 成对）：文档构建 Draft 0.570 → 0.0011 s、Standard
  2.351 → 0.0039 s（−99.8 %）；占 UI 范围 Draft 2.3 %、Standard 3.7 %。
- **失败语义**：被删校验对生产链能产生的输入从不失败，产品错误码不变。不可达的
  `SphericalFormationDisplayError::{Surface, Tectonic, Formation, FormationBundle}`
  删除（仓内无匹配；该枚举为 pub 且无 `#[non_exhaustive]`，严格说是公开 API 变更，
  仓外无下游）。两项都失败时现在只报策略错误。

## 6. 当前总计（种子 42，Release，Windows）

| 口径 | 提交 | Draft | Standard |
| --- | --- | ---: | ---: |
| 探针全链 | `9e87d39`（A7） | 24.202 s | 55.480 s |
| 探针全链 | `14a13da`（第 1 轮后） | 21.370 s | 48.747 s |
| 探针全链 | `4f32353`（HEAD，单次） | 21.490 s | 48.657 s |
| 全图 `engine_scope` | `14a13da` | 24.424 s | 60.211 s |
| 全图 `engine_scope` | `4f32353` | 22.495 s | 51.714 s |
| 全图 `ui_scope` | `14a13da` | 25.201 s | 63.291 s |
| 全图 `ui_scope` | `4f32353` | 22.671 s | 52.420 s |

- 全图值为 §5.3 成对运行的最小值（Draft 4 次、Standard 2 次）。第 2 轮不改探针
  覆盖的路径，HEAD 探针为单次运行，与 `14a13da` 的差在噪声内。
- 探针与全图的差距：引擎范围 Draft 2.73 → 1.0 s、Standard 11.39 → 3.06 s；UI 范围
  3.48 → 1.18 s、14.45 → 3.76 s（剩余主要是 P1 表面、§8 的
  `ProfileSurfaceBuilder::complete`、仍在的哈希与呈现组装）。
- 对 A1 目标：Standard 52.4 s 达标；Draft 22.7 s 仍高于 20 s。
- 本表止于 A8 的逐位提交。A8b（终点 P4 复用起点 FAS 校正）之后的总计以 A8b 规格
  §9.4.1 为准：成对中位探针全链 Draft 18.692 s、Standard 43.544 s，全图 `ui_scope`
  Draft 19.703 s、Standard 47.739 s（单对）。

## 7. 验证（最小充分证据）

- 每个提交：§2 探针快照与 `9e87d39` 逐字节一致；第 2 轮每个提交另跑全图装置，
  Draft 4 次、Standard 2 次全部与 `14a13da` 的 13 行哈希一致。
- 第 1 轮：`global_circulation::tendency`、`formation::global_circulation`、
  `circulation`、`surface_formation`、`global_circulation::forcing`、
  `formation::causal` 库测试；集成测试 `causal_formation_generation`、
  `circulation_grid`、`circulation_operators`、`circulation_second_order_transport`、
  `circulation_steady`、`circulation_transient`。
- 第 2 轮：`engine::`、`rules::` 库测试；集成测试 `build_cancellation`、
  `builtin_rules`、`diagnostics_and_provenance`、`engine_execution`（含两项钉住
  哈希的测试）、`rule_*`、`spherical_foundation_build`、
  `spherical_natural_stage_graph`、`primary_relief_quality`、
  `evolved_tectonic_quality`、`causal_formation_generation`；形成、质量、捆绑库测试
  95 项；ignored 的 Standard 高成本参考对照
  `compare_production_split_with_high_cost_reference`（865 s，覆盖 §9 R1 保留的
  绑定）；wgpu 相关的 `app::natural_app_tests` 等库测试 55 项与
  `spherical_presentation_integration` 31 项（`--test-threads=1`）。
- 每个提交 `cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets
  --all-features -- -D warnings` 干净；`4f32353` 另跑 wasm32 lib check 通过。
- 待做：合并前 CI；用户在 UI 上验收生成耗时。

## 8. 推迟与未处理项

- **阶段内 `ProfileSurfaceBuilder::complete`**（Draft 0.38 s / Standard 1.56 s）：
  调用方已建好完整 `ProfileSurfaceBundle`，却只发布权威表面，阶段内再重建控制
  表面与保守映射。去掉它需要把完整剖面束变成外部或缓存产物，改变图的外部产物集
  与缓存键，单独决策。
- **终点 P4 复用起点 FAS 校正**：改变数值，另立规格
  `2026-10-08-a8b-endpoint-fas-reuse-design.md`（A8b，含参考对照门）。
- 未动的小项：工作域阶段内与发布时各校验一次（每次 0.019 / 0.035 s）；应用层
  外部产物插入前的 `surface.validate()` 与 `StoredArtifact::new` 重复（约
  0.035 / 0.14 s）；P5 `validate_inputs` 对 `initial_climate` 的完整重验（未测）；
  UI 呈现组装 0.17 / 0.70 s（重算，未剖析）；`SphericalNaturalFieldDocument::build`
  未查；`validate_against_authoring` 与 `validate_against_surface` 两个公开 API
  函数体相同（既有重复，未改）。

## 9. 修订日志

- **R1（2026-10-08）`943b319` 审查修正。** 首版把离线参考路径
  `generate_resample_boundary_reference` 上的 `CausalFormationOutput::validate` 一并
  删去；该路径的基底只在重采样观察器里对边界快照校验过，与随后单独发布的最终
  P2 快照之间没有更早校验，故在那里恢复
  `geologic_substrate.validate_against(surface, &evolved_tectonics)`，映射到
  `InvalidFinalCandidate { role: "geologic_substrate" }`；同时为 P3 私有质量入口补
  P2 报告的表面绑定检查（§5.2）。

## 10. 每项承重技术的出处

| 技术 | 出处 | 用法 |
| --- | --- | --- |
| 在边界校验一次、之后信任 | King, A. (2019), “Parse, don't validate”；仓内先例 `222be5e` | §1、§4.1、§5.2、§5.3 |
| 每界面一个通量的有限体积聚合 | LeVeque, R. J. (2002), *Finite Volume Methods for Hyperbolic Problems*, ch. 4 | §4.2 |
| 静态边度量（中点位移、插值权重） | Ringler, T. D. et al. (2010), *J. Comput. Phys.* 229, 3065–3090（MPAS dvEdge/weightsOnEdge） | §4.3 |
| 缓冲写入后流式哈希 | Rust `std::io::BufWriter`；BLAKE3 规范（O'Connor et al. 2020） | §5.1 |
| 成对计时、2 % 保留门 | A1 §3.4、A6 §4.4 | §1、§2 |
