# 产品候选整合验证记录

状态：本地 `integration/sekai-current` 可运行候选；常规回归已分批覆盖，用户 UI 验收与科学研究仍未完成。
用户已授权按顺序推进、合并可采纳成果并安全清理。代理因额度中断后由协调者接手，
未更换账户绕过限制；续接前所有未提交差异和独有文件已备份到
`D:/hemifuture/sekai-backup-20261101-resume/`，原任务终端已关闭，不遗留并行编辑者。

## 1. 海岸和性能链

- `73f53cf` 合入海岸 UI 修补 `5d8beb5`；当时构建输入与该源提交相同，复用其限定验证，
  不把纯文档整合重复构建。此前并行 GPU 复跑超时保留为异常记录，后续 GPU 测试单线程。
- `33b0ed4` 合入 prewarm（含校验去重/A9 前段），保留 LOD-1 R3 水岸与实际安装批次拾取；
  修正新档位标签返回 String 后的测试借用。默认严格 Clippy、WASM lib check、fmt 通过；
  Release app 108 项与几何/身份/水面直接消费者 52 项通过。共享链路全量留在最终整合后运行。
- `475684c` 合入既有启动内存预热与按需 LOD 的时延口径澄清；保留当前初态授权和测试纪律。
  A9 改变气候工作尺度，不声称它对旧主线逐位等价；新世界不得继续冒用旧青岚导出身份。

## 2. Phase A 作者闭环

以 `0112048` 为输入，保留作者输入事务、撤销重做、请求 token、实际 worker 退出后
才结算、CPU/GPU 最终原子发布、配方原子保存及显式升级副本。

冲突处理的语义：

- 作者输入只保留 `AuthoringSession` 一份，不复活旧 app 科学字段。
- 海岸菜单/两种重建按钮按本帧 editing 输入判定；真正发起请求按已提交 draft 再预检。
- 预热继续复用完整 `FormationGeometry`，不保留第二份旧 profile surface cache。
  取消/失败后可回收种子无关几何，但不得覆盖较新选择的预热；这不发布失败世界。
- 只有已提交的几何 key 改变才启动新预热，不因每次绘制/种子编辑重复启动。
- 当前 mesh 的 context/pick batches 与世界绑定；清空项目时一并释放，项目偏好不持久化拾取点。
- 程序化预设在 canonical recipe 中追加独立 tag，不改变已有预设的 bytes；新增该预设的
  身份/序列化断言。保留现有生成器兼容纪元与工作域/阶段身份检查。
- PreparedFormation 路径保留输入、图构建、呈现的计时边界，不悄悄退回完整重建几何。

验证：默认 workspace/all-targets/all-features Clippy（`-D warnings`）、WASM lib check、
fmt 通过；Release app 203 passed / 2 ignored；ignored
`phase_a_recipe_reopen_formation` 单独运行 1 passed，约77秒测试耗时（不是性能基准）。
后者真实执行生成、修改活动配方、保存、销毁会话/缓存/GPU发布、重开自动一次生成，
恢复后的 recipe 与发布源身份一致。原始输出：
`D:/hemifuture-probes/consolidation-20261031/integration/phase-a-reopen.log`。

## 3. 站点导出与事实源整理

站点导出按 `485efef` 接入当前发布文档，不读取未发布的作者草稿；原冻结青岚 JSON 保留原样。
合并时修复面板折叠/发布发生于控件绘制后造成旧足迹残留的路径：世界绑定缓存统一由
`synchronize_world` 在发布、面板刷新和清空项目时失效，保留会话内区域编辑偏好。
一个纯状态测试覆盖缓存所有权，不为测试添加导出反序列化 API；site 三项窄集成通过。

区域描边颜色归 view/palette，青岚范围作者初值归 world；站点引入的 Celsius/Kelvin
转换量与原有环流转换合为 world 中唯一 SI 定义。数值和浮点类型不变，不增加物理过程、
容差或依赖；出处为 BIPM SI Brochure 第9版 §2.3.1，原始Qinglan取值出处保持不变。

## 4. 最终回归的有界续跑（2026-11-01）

原单条全量命令的1200秒预算到达后停止；核对没有遗留cargo/rustc/test进程。
日志显示编译482秒，55个已结束套件执行合计约649秒，943 passed / 0 failed / 34 ignored。
其中旧 circulation_steady/transient/comparison 合计约463秒；没有扩大这些夹具或门限。
停止时刚进入 global_circulation_generation，不能把整轮记为通过。

不延长原命令，不重跑已完成套件。以 Cargo metadata 的实际测试目标减去有完成记录的目标，
仅续跑原队列中94个未完成集成套件，固定两批各47个、每批600秒、单线程、无编译参数变化。
这是根据实测成本缩窄补证范围，不增加seed/profile/迭代；若续跑仍超时则停止并保留未完成。
同一代码的首批/续跑结果共同构成覆盖证据，原命令超时记录不删除。

续跑两批均在各自上限内结束：第一批261 passed / 1 failed / 21 ignored，第二批
245 passed / 0 failed / 18 ignored。唯一失败是旧工作域的 T1 精确身份期待值；实测
恰好等于既有 A20 审计基线，并与保留的 a10-impl 相同生产输入一致。按显式修订 A22
恢复该基线，未改科学阈值、浮点金丝雀或忽略标记；仅重跑失败断言，1 passed。
因此149个既有套件的常规契约合计1450通过，73按原设计忽略。原始失败保留在第一批日志。

随后只追加低成本气候建议文案测试（1 passed）和一个 ignored 联合场景（单独运行1 passed）：
`integrated_procedural_coast_site_uses_published_identity` 在最终工作域生成同一个 Draft42
程序化目标25%世界，复用完整链/细岸报告，并直接调用真实 UI 取样适配器导出第一个陆格。
不另找好气候锚点，不追加第二次世界生成。当前导出与发布 build/P4 身份一致；文件在
`D:/hemifuture-probes/consolidation-20261031/integration/current-procedural-site.json`，
不覆盖旧青岚文件。测试日志和 `integration-target25.json` 位于同目录。
导入的历史 `orographic-zero/probe-output.txt` 保留原 Git 字节（包括末尾空行），
不为消除该文件的 whitespace 提示而重写测量日志；其余变更的 diff 检查通过。

最终场景仍有风跳变和季节相位软失败（实测4.70294309、0.54762765）；预算、身份和
直接消费者通过不能替代完整气候真实性。UI 摘要现显示生产报告中的失败建议，中文标签
集中在本地化表，值与上下界直接来自报告；没有第二套阈值，没有 clamp 或输出平滑。
标准/高档实验准入仍未解除，原旧工作域候选只保留为历史证据。

最终 fmt、默认严格 workspace/all-targets/all-features Clippy、Release lib/tests Clippy、
WASM lib check、Trunk Release 与 native Release 构建均通过。新增 UI 文案后又通过默认
严格 Clippy、WASM 检查与该窄测试并重建两种产物；未再次重跑已覆盖科学套件。

## 5. 未完成的整合验收

- [x] site 站点导出适配当前作者输入与 P4 身份；保留旧冻结导出，不覆盖旧世界。
- [x] 合入海岸响应诊断记录，对最终工作域复跑最小程序化场景。
- [x] 最终 Release 常规契约分批覆盖、工程门禁、native/Trunk 构建；原全量命令超时如上保留。
- [ ] README/PROGRESS 更新、最终备份与已包含分支/工作区清理。
- [ ] 用户 UI：启动当前候选，编辑/撤销/重做，生成中取消，保存/重开一次生成，旧配方显式升级；
  检查二维/三维、岸线/河口/点选和站点导出身份。用户本人确认前不标记产品整体交付。

湖 R7、A10 R9、G4、P2 和占位字段的旧预览均未整体合入。对应有限研究测试通过
不等于完整模型、全档位或所有 LOD 物理有效。当前已知季节相位、包括整合后 Draft 的程序化风场、
特定湖直岸、未解析海峡仍必须明确报告，不能以整合授权掩盖。

## 每项承重技术的出处

- 预热、校验一次与映射复用：A9/预热冻结规格及源提交的逐位对照；本轮不新增近似。
- 输入事务、token/epoch 与原子发布：Phase A R2/A1 及其现有契约测试；Rust 所有权保证
  移交后旧资源不再被发布，几何缓存与世界状态分责。
- 配方保存：Phase A 项目格式/兼容身份契约及 Microsoft ReplaceFileW 工业接口。
- 水岸/拾取：既有 T1/LOD-1 同源字段契约；不新引入平滑或经验修形。
- 清理：Git bundle verify、祖先包含与 Orca 活动所有权核对，恢复记录在外部备份目录。
