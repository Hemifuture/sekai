# P4 近地面风场纬向不对称设计（里程碑 A2）

日期：2026-09-02
状态：**实施中**（用户 2026-09-02 指示修正"风场完全对称"）

上位：`AGENTS.md`；P4 设计 `2026-08-17-global-atmosphere-ocean-p4-design.md`；
时延里程碑 `2026-09-02-generation-latency-design.md` §6 开放问题 7。
实施计划：`../plans/2026-09-02-p4-zonal-asymmetry.md`。

## 1. 病征与实测

用户在规则格点箭头下看出盛行风"完全对称、放大也完全规则"。Draft seed 42
年平均近地面风（5° 纬带，面积等权）：

| 量 | 实测 | 地球 |
| --- | ---: | --- |
| 偏离纬向平均的方差占比 | 1.4 % | 与纬向平均同量级（副高、季风、风暴路径） |
| 陆 / 海偏离均方根 | 0.93 / 0.92 m/s | 显著不同 |
| 南北半球镜像 | `u` 相等、`v` 反号，差 < 0.1 m/s | 因陆地分布而不对称 |
| 18° 东风峰 | −12.4 m/s | 年均信风约 −6 ~ −7 m/s |
| 极区经向风 | 3.5 m/s | < 1 m/s |

## 2. 成因（代码事实）

1. 平衡温度是纯纬度函数：`forcing.rs` 按日均日射 × 反照率取灰体平衡再减地形
   递减率；陆海只差反照率 0.16。
2. 低层大气动量方程 `−g′∇η − f k×u − r u + Γ∇T + ∇·(K∇u)` 中 **没有地形**：
   6 km 的层在 5 km 高原上厚度不变，山脉不偏转气流。
3. Rayleigh 摩擦 `r = 1/天` 陆海相同，没有地表粗糙度对比。
4. 斜压 Reynolds 应力闭合按轴对称拟合构造，只给纬向平均分量。
5. 一轮成形只有 12 × 7200 s，全程 6–9 模式日；温度场停在轴对称初值上，非对称
   性只能来自强迫与边界，不能靠动力学自己长出来。

本里程碑只动 2 与 3：它们是有标准出处、几天内就起作用、且不改变时间结构的
边界机制。1、4、5 归 P4 水热校正。

## 3. 度量（Task 0）

新增 `near-surface-wind-non-zonal-variance-fraction`：年平均近地面风逐格
`(u, v)`，按 5° 纬带做面积加权纬向平均，偏离纬带平均的方差占总方差的份额。
初版**只记录不设门**（`(None, None, false)`）：地球参考值需再分析资料的
定量结果（Peixoto & Oort 1992 第 7 章只给出定常涡动与纬向平均同量级），按
先测后钉原则在有数据前不冒充包络。

## 4. 低层地形（Task 1）

带地形的浅水方程（Vallis 2017, *Atmospheric and Oceanic Fluid Dynamics*
2nd ed., §3.1）：

```text
∂u/∂t + … = −g ∇η,         h = η − η_b,        ∂h/∂t + ∇·(h u) = 0
```

压力梯度只由层顶 `η` 决定，**层厚**由层顶减去地形。对低层大气：

```text
h_lower(cell) = H_ref − z_b(cell) + η(cell)
z_b = land_fraction · max(elevation − sea_level, 0)，钳制 H_ref − z_b ≥ H_ref / 6
```

只改连续方程的施主厚度（快路径 `gradient_and_donor_layer_thickness_tendency_into_…`
与慢路径 `conservative_layer_thickness_tendency`），压力梯度、热力耦合与其他
层不变。静止态（`η = 0`，层顶平）仍是精确平衡解。钳制值是数值安全界（地球最
高高原约 5 km，6 km 层下至少留 1 km），不是物理参数。上层大气以低层层顶为
底，不受地形影响。

## 5. 陆海摩擦对比（Task 2）

体块拖曳 `C_D`：开阔海面约 1.2 × 10⁻³，陆面 3 × 10⁻³ ~ 10⁻²（草地到森林）
（Garratt 1992, *The Atmospheric Boundary Layer*, §4.1；Stull 1988 §7）。线性
Rayleigh 率 `r = C_D |U| / H_bl` 与 `C_D` 成正比，故

```text
r_lower(cell) = r_sea · (1 + (ρ − 1) · land_fraction)，r_sea = 1/天（既有）
```

`ρ = C_D,land / C_D,sea` 钉在 **3**（草地量级，取保守下限），先测后钉：实施
计划记录 2 / 3 / 4 的扫描。

## 6. 验证

- Draft seed 42：每个任务前后的 §3 度量与 §1 表格各量；
- P4 全部测试二进制；17 seed 气候证据的既有门（全球降水 2.81 ± 7 %、TOA、
  水循环闭合、东风/西风占比等）必须继续通过；
- 32 seed 冷启动扫描无失败；产品级时延不变（两项都是每格常量项）。

## 7. 出处

| 项 | 出处 |
| --- | --- |
| 带地形浅水方程 | Vallis, G. K. (2017) *AOFD* 2nd ed., §3.1 |
| 地面拖曳系数陆海对比 | Garratt, J. R. (1992) *The Atmospheric Boundary Layer*, §4.1；Stull, R. B. (1988) *An Introduction to Boundary Layer Meteorology*, §7 |
| 地面风纬向不对称同量级 | Peixoto, J. P. & Oort, A. H. (1992) *Physics of Climate*, ch. 7 |
