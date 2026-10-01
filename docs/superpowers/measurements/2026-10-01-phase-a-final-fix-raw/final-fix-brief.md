# Final whole-branch fix wave（一次集中修补）

BASE: 516c23c64eddb7f083caf4ab4a8d2eb6d6805bdf；branch codex/phase-a-authoring。
批准R2与计划在docs/superpowers/specs/2026-09-30-phase-a-authoring-loop-design.md和plans/2026-09-30-phase-a-authoring-loop.md。用户已正式批准实施全部八项，历史design-only文字不限制此次修补。AGENTS唯一事实源。
实施及独立审查模型均 gpt-6.1-sol/high。禁止派子代理。Root安排唯一 scoped review；只有你写产品源，不与别的app.rs writer并行。

## 原独立全分支发现（完整定位/风险/最小修正）

Important I1 — src/app.rs:2572–2573、2601–2602、2786：两个百分比编辑器显示38%，共用解析器却只接受裸f64，没有百分号或百分比单位转换。输入合法40%被拒绝；输入界面单位下的40则由egui夹为0.75，随后提交为不同作者意图的配方。统一百分比显示/解析单位，明确接受的文本格式；确认输入时校验实际领域值。最低成本实际egui回归：百分比输入、Enter、Escape、无改动时配方身份不变；包含一个非整百分比的已保存值，避免精度损失。

Important I2 — src/app.rs:2533–2536、2753–2779、2786–2797：板块数解析器只检查能否解析和有限性。输入0或999，egui失焦/Enter先夹为2或64；作者校验看到夹值，推进历史并撤销构建权限。违反R2§4.2无效控件文本不提交。在控件转换/夹值之前拒绝域外文本，复用领域范围事实，保留原文并显示错误；不得变成另一合法值提交。实际板块控件覆盖域外/非整数输入、revision/token保持、纠正和Escape，无需科学全链。

Minor M1（既有T3延期）— project_file.rs:146、163、366：未知Tectonic schema错误统一转换Input，而其他schema为UnsupportedSchema。严格拒绝已实现，未放行/覆盖/静默升级；独立review认定非阻断。此wave不扩展codec/schema；root明确继续延期并在最终报告披露。

Minor M2 — app.rs:1581–1602；R2§6：生成状态只有正在生成/取消、elapsed和取消按钮；静态链说明/加载动画未显示实际运行阶段，未有免除裁定。显示真实粗边界当前阶段，如地表准备、自然世界构建、显示准备/发布；只保留当前请求阶段，不新增通用框架，不虚构独立调度P2–P5。

## 必须完成的最小范围与验证

1. 先添加实际egui控件回归，在原代码真实运行I1/I2失败并记录实际数量/exit/log。不得使用只调用parser或AuthorInputs等于自己的伪端到端证明。复用已有keyboard_frame/rendered_text/focus和token helpers。格式化/解析必须按显示的百分比单位往返；保留非整百分比已有精度和原始输入的无改动Enter/no-op；非法板块和百分比域外文本不得由egui夹值后提交/推进history/撤销activeBuildToken。合法纠正才提交，Escape恢复，保存被未提交非法文本阻挡。范围/整数验证引用真实领域校验事实，不复制科学范围或创造常量。
2. 最小修I1/I2；合法drag/Slider操作、u64 seed精确输入、临时文本/noop/Enter/blur/Escape和旧世界/异步发布调用顺序必须保留。未知输入拒绝原则不变。不得修改求解器、FAS/CFL/cycles/质量profile、冻结门禁、T1候选或任意数组后处理。
3. 对M2先一个最低成本实际渲染状态失败→绿（观察关闭也应有真实阶段）；在生产已有Surface/Graph/Presentation粗边界赋当前request阶段，只为真实UI消费者。复用现有类型/边界，避免通用进度框架/假的独立P2P5进度/百分比或ETA。phase state不得persist、不得成为输入SSOT，不能让旧token阶段污染新项目。取消中及actualworker尚未退出应真实表示，原单槽/guard不改。
4. 测试优先Release热现存依赖，不默认Debug重建或全链。CARGO_INCREMENTAL=0；当前space约1.20GB且波动，任何compile前严格budget与已无own activity确认；至少1GB操作reserve，单crate热额外峰值保守估计并记录。如不满足/出现cold dependency rebuild/低于reserve，停仅你自己的命令，报告，不删除/停止未知进程/动main或其他缓存。父全盘cleanup另协调，任何cache delete目前hold。
5. 跑对应新红绿与直接keyboard/token regressions；必要fmt、clippy all-target/all-feature -Dwarnings（新源码需要），wasm lib（生产app新增字段/路径需实际证据），无未改动科学全链/20s预算/旧T1重跑。给真实数量/exit/raw，不把compile失败算behaviorRED，零筛选不PASS。
6. Source changes后required fresh生产rlib与source manifest/sha用于真UI阶段截图；root只在BENCH-IDLE复用native_screenshots helper，并提供新phase/status截图及必要实际键盘输入操作，不能把49e旧截图当修后证明。不要你再扩工具/默认main启动(user autosave风险)。你给root currentrlib/path/hash/commit及BENCH-IDLE，root接手helper；等待期间仅小docs。
7. 最终报告final-fix-report.md包括I1/I2/M2各Addressed证据、真实红绿/工程checks、source/commit/diff、scope成本/关切；本地commit允许正常require_escalated共用gitdir，不push/merge/deploy。最终仅状态/commits/测试一句/关切。

## 不可改变的批准范围

配方only，不持久世界/阶段/历史世界/轨迹；runtime旧有效世界允许留内存。兼容Open自动一次、普通编辑显式重建；fail/cancel/stale保留旧世界且不自重试；同一实际fullworker退出前单槽不能释放；epoch/request/revision/token正确。版本/算法/依赖/精度不兼容明确失败/显式升级副本保护原件。原生desktop优先，wasm lib兼容。graph真实ClimateWorkDomainStage＋bundled CausalNaturalFormationStage(P2–P5)。Draft10–20s/Standard≤60s待验证，原最后58.83/91.25均FAILED；已知T1 .0139>.01 ignored红独立保留。T7候选撤回，未测gradient/strain/RKcopy不能强造优化。用户本人UI验收仍pending，代理窗口/FirstSubmitted不是OSpresent。

## 已验证前置/环境

T8当前源码checkpoint49e2084，加doc-only516c23c；整链1PASS/app191PASS/consumer43PASS/最新clippy26.54s exit0，全部raw在docs/superpowers/measurements/2026-09-30-phase-a-performance-raw/t8和native-ui/t8。
fresh旧UIrlib7716 SHA A3F6D1F3CCD4B48070B8AB5A085F76BD86E49125D8D945A438B7CEA819844299仅代表49e，修后必须重新编译/标注。当前source/beforeafterT7 probe/旧截图/报告保留。磁盘曾完全耗尽导致working plan0，已从HEAD同目录tmp验证完整恢复，当前37157bytes；所有写小temp完整核验后atomic替换，禁止满盘原地整文件Set-Content。现无source writer。
