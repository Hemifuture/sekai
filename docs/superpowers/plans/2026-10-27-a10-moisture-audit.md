# A10 水汽输送回查计划

> 本轮由已派发工作者按 executing-plans 在当前工作树执行，不再派代理。

**目标：** 对 §19 陆地偏湿作可复核的来源分解，检验有一手依据的原型修正；不以调阈值达标。

**架构：** 只修改 `#[cfg(test)]` 的 `reduced_climate/prototype.rs` 与其 `probe.rs`。
在相同生产终点强迫下复用现有原型求解器，输出面积加权账本及敏感性；不新增产品路径。

**技术：** Rust、现有 serde_json、Release 忽略探针；离线绘图，无新增项目依赖。

**规格：** `docs/superpowers/specs/2026-10-25-a10-reduced-order-climate-design.md`
§16–19；本轮追加 §20 R7 草案，不冻结。

## 约束与审查重点

- T8 前生产链逐位不变；不开始 T4–T12、不 push、不 merge。
- 保留 Budyko 与块体 E_pot；不修改门槛、常量或身份，不落盘世界快照。
- 原始证据在 `D:/hemifuture-probes/a10-impl/moisture/`，只保存诊断报告而非可加载世界。
- 沿用 17 seed × Draft/Standard，只有跨 seed 质量门使用全语料；局部算子用小夹具。
- 不用 cos(latitude) 近似立方球格面积；混合岸格的海面蒸发须单列，不能冒称陆地再循环。
- 通量分解是有符号 Euler 账本，不是水分子来源追踪；逆向分量允许负值。
- 固定 RH 的 MSE 与求解水汽 q 不同，不能仅凭相同质量因子宣称潜热输送逐边一致。
- 只根据纬度的 Hadley 权重不含陆地系数；检验的是二维局地 MSE 推广，不拟合陆地权重。
- 平流的共同迎风值依赖总流向，分项解释须保留这一离散约定。

## M1：精确诊断与出处回查

文件：本计划；`prototype/probe.rs`。

- [ ] 读交接、两份 AGENTS、§16–19、Siler et al. (2018) 原文 Eq. 2–9/Table 1。
- [ ] 增加 `moisture_budget`：全球/陆地分数/海洋分数加权的 P、总 E、当地陆面 E、
  岸格海面 E、涡动/Hadley/纬向辐合、局地账差；输出真实格面积和参考高度 RH。
- [ ] 同强迫输出 r4、final（T3b Budyko）对照；逐格闭合与全球各通量零和复用原型输出验证。
- [ ] 执行 17 seed 统计，报告中位、范围及量纲；不能把中位分项之和当中位总量。
- [ ] 完成检查并提交 M1。

## M2：有出处的最小原型修正

文件：`prototype.rs`、`prototype/probe.rs`。

- [ ] 检验 Siler 纬向平均 Hadley：先以纬带面积均值重建各格 MSE，再沿边取梯度；
  `hadley_lower_branch` 复用同一 gross moist stability 与全部物理常量。
- [ ] 小夹具先证伪：在保持纬带均值不变时，添加带内陆海 MSE 异常，纬向平均分支的通量不应改变；
  常值 MSE 不应产生 Hadley 质量流。无需多 seed 证明这个算子契约。
- [ ] 用现有无 qdiff 配置作为质量因子敏感性，明确它不是被采纳的修法；不另创系数。
- [ ] 在 17 seed 同强迫比较 final、纬向平均 Hadley 与质量因子敏感性，保留温度/TOA/水量/收敛检查。
- [ ] 申请独占计时窗口，按原有两次求值、投影、强迫口径记录 Draft 五次中位。
- [ ] 执行 fmt/clippy/wasm 与最小针对测试，检查差异只在 cfg(test)；提交 M2。

## M3：证据、草案与后续 UI

文件：规格 §20；中文报告与绘图脚本（外部证据目录）。

- [ ] 生成 r4/final/本轮对照图：相同权威格掩膜的陆/洋降水、工作域来源分解、纬带曲线。
- [ ] 报告所有未过门、模型适用域、账本与一手出处；不得将敏感性当修正或将旧模型当真值。
- [ ] 附生产链不变证据、命令、提交、重现入口，提交草案与完成记录。
- [ ] **未来 UI 接入（本轮不执行）：** 用户批准 R7 后才安排 T8 生产升版与接入；
  使用独立预览分支展示 Draft/Standard seed 42 的降水、温度、风及点选，
  比较海岸降水条纹、陆海分配、极区/热带温度、L0 到显示叶片的一致性；
  用户本人上手验收后才能称为算法交付。预览不能添加产品开关。

## 每项承重技术的出处

- 水汽连续方程、经向/纬向平均通量分解：Peixoto & Oort (1992), *Physics of Climate*, ch. 12；
  全球水量对照：Trenberth et al. (2007), https://doi.org/10.1175/JHM600.1。
- MEBM 质量因子、固定 RH 适用边界、纬向平均 Hadley：Siler, Roe & Armour (2018),
  https://doi.org/10.1175/JCLI-D-18-0081.1，Eq. 2–9、Table 1；
  [作者全文](https://earthweb.ess.washington.edu/roe/GerardWeb/Publications_files/SilerEtal_HydroCycle_JCIim18.pdf)。
- 陆地湿度不能直接套海洋固定 RH：Byrne & O'Gorman (2016),
  https://doi.org/10.1175/JCLI-D-16-0351.1；仅用于解释局限，不移植其拟合系数。
- Budyko、块体 E_pot、Anderson 及流函数迎风离散沿用规格 §17.3、§19.3/19.6，
  本轮不改变公式、数值常量或阈值；纬带线性重建为已有分段线性流函数采样的空间离散类比。
