# Atlas 配方与来源兼容

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
