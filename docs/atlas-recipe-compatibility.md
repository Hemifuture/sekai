# Atlas 配方与来源兼容

## Atlas 模型 4：水类尺度和月度账本

按世界真值规格 R2，格元分类直接供给站点和格元字段，干比例取干样点面积，湖泊记录保留
上游初始水面和逐月通量。模型 2、3 配方打开时均经已有身份比较提示升级副本。站点导出的
字段格式仍为 V2，新行星的构建指纹标识其来源。边求积并行单独与模型 3 串行路径作位级对照。

## Atlas 模型 3：上游水体分类（world-truth-scope R1）

`ATLAS_TERRAIN_MODEL_VERSION` 升为 3：水体拓扑改取上游粗格元分类，格元量改为固定扇形
求积，汇流改在源站点图上进行（规格 `specs/2026-11-03-world-truth-scope-design.md` R1）。
同一种子与参数生成的海岸、湖泊、水量、气候与河网都会变化，模型 2 配方读取后显示
身份差异并走升级副本流程，不判为“完全兼容”。配方字段 `support_depth` 保留原名，
现表示 `ATLAS_SUPPORT_DEPTH` 定义的格元求积层。作者输入、种子与来源语义不变。

## 来源身份事实更正

初版工程候选误把固定上游 `src/gen/edits.ts::GENERATOR_VERSION` 记为 7；实际声明为 57。
现已更正唯一事实源 `UPSTREAM_GENERATOR_VERSION`，并提升 `ATLAS_TERRAIN_MODEL_VERSION`，
因为该值进入上游内容身份流，进而改变 Atlas artifact / forcing / 构建身份。
细、粗地形数值算法与既有数值 fixture 没有修改，`FINE_TERRAIN_VERSION` 未变。

旧候选保存的上游7/Atlas模型1配方保持原文件和 checksum，读取后必须显示生成器身份
差异并显式升级副本，不能判为“完全兼容”。旧配方记录的是历史错误标签，不把它改写成
已经由当前模型生成的世界。此前 UI 和性能证据仍归属各自实际运行的候选身份。
本更正不改变旧 Formation/Foundation 来源的生成语义，也不等于时延或科学质量已达标。

## 传输与作者输入

新配方格式为 V2，作者 canonical encoding 为 V2，本地自动恢复写入 schema V2。
V1 配方仍使用原 canonical encoding 校验原 checksum；读入时只补齐未生效的 Atlas
形状默认值，保留原来源、u64 种子和旧参数。旧本地恢复缺少来源时按旧 Formation 解释，
新建世界才默认 Civ Atlas。保存的新项目写当前格式；算法不兼容仍通过现有升级副本流程。

根 `world_seed` 是唯一可编辑种子。Atlas 请求用 checked u32 转换，手输越界报错，随机
按钮生成有效 u32 后提升为根 u64。来源形状由纯包 `TerrainShape` 持有，默认值与校验
复用原 `TerrainParams`；后者保留上游扁平序列化。旧来源的完整 u64 语义不变。

Atlas 配方记录来源 stage 身份，显式保存固定上游提交、包版本、生成器版本、细地形版本、
Sekai Atlas 模型版本及固定 scale/support；当前气候/水文模型和 C2 quality 保存内容哈希。
形状与根种子属于作者输入身份。相机 LOD 不改变地形尺度、床面、水体或气候身份。
运行时发布身份仍来自真实 BuildOutcome，不使用旧 Formation/P2/P3 占位。

站点导出 V2 增加真实地形来源和可选固定地形尺度，quality 表示 C2 工作域。
既有 V1 导出保持历史 V1，不声称新导出是同一个世界；本项目没有为迁移新增无消费者读取器。
新源站点高程为来源格 exposed 面积均值，水类为同一 snapshot 的格中心采样；月降水和
蒸发沿用实际 source-cell 月均日率与已有归约、单位和能力声明。

验收：保存 Atlas 配方，关闭后打开，确认来源、种子、形状和固定版本；再打开 V1 旧配方，
确认仍为原来源。生成期间切换来源、取消或重开项目，旧 epoch/request 的晚到结果不得发布。
完整世界只在最终当前态、显示几何与 GPU 候选全部准备成功后原子替换。
