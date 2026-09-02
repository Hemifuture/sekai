# P4 陆面蒸散与越山抬升设计（里程碑 A3）

日期：2026-09-02
状态：**已实施**（用户 2026-09-02 指示继续解决 A2 暴露的两个问题；实测见 §5.1）

上位：`AGENTS.md`；A2 `2026-09-02-p4-zonal-asymmetry-design.md` §6.1；P4 设计
`2026-08-17-global-atmosphere-ocean-p4-design.md`；P5 完成文档
`2026-08-18-coupled-geomorphic-formation-p5-completion.md`。
实施计划：`../plans/2026-09-02-p4-land-evapotranspiration.md`。

## 1. 病征

A2 把信风峰从 −12.4 降到 −8.4 m/s 后，17 seed 语料的全球蒸发 2.91 → 2.66、
降水 2.80 → 2.56 mm/day（GPCP 2.81）；地形雨影度量失败 1/17 → 9/17。

## 2. 成因（代码事实）

1. **陆面蒸发恒为零。** `surface_moisture_availability = 1 − land_fraction`，体块蒸发
   `E = ρ C_E |U_lower| (q_sat(T_s) − q) · availability` 只在海面起作用。地球陆面蒸散
   约 65 500 km³/yr，占陆地降水 111 000 km³/yr 的 59 %（Oki & Kanae 2006），
   折合全球均值约 0.4 mm/day；模型全靠偏强一倍的海面风把这一块补回来，A2 让风
   变真实后缺口就露出来了。P5 的水量分配（`FORMATION_RUNOFF_MIN_FRACTION +
   FORMATION_RUNOFF_PERMEABILITY_RANGE · (1 − 渗透率)` 为径流）隐含"其余降水
   蒸散回大气"，但 P4 从未把这部分还给大气：两层之间的水量账不闭合。
2. **地形抬升用的是被阻挡的低层风。** A2 之后低层作为薄层绕山而行，`upslope =
   u_lower · ∇z` 减小；而真实的地形雨由**越过**山脊的气流产生。分流线理论
   （Sheppard 1956；Hunt & Snyder 1980）：`h_c = h (1 − Fr)` 以下的空气绕行、以上
   的空气翻越；两层模式里上层就是翻越的那部分。

## 3. 陆面蒸散：稳态水量平衡（Task 1）

Manabe (1969) 桶模式在稳态下 `E = P − R`；Budyko 框架给出同一结论。P5 已经把
`R = runoff_fraction(渗透率) · P` 钉死，因此与 P5 闭合的陆面蒸散是

```text
E_land(cell) = land_fraction · (1 − runoff_fraction(κ)) · P(cell)
runoff_fraction(κ) = FORMATION_RUNOFF_MIN_FRACTION + FORMATION_RUNOFF_PERMEABILITY_RANGE · (1 − κ)
```

`κ` 为 P3 发布的相对渗透率，公式收敛到 `src/world/natural/surface_formation.rs`
一处（P5 水文与 P4 强迫共用）。逐格系数 `f = land_fraction · (1 − runoff_fraction)`
在强迫构造时从权威面守恒重映射到工作网格，进入强迫指纹（v4）。

在 `apply_moisture` 中：先按现有路径算出无回流的凝结 `P₀`，取 `E_land = f · P₀`
加进湿度后再算最终凝结（一次 Picard 迭代；`f ≤ 0.85` 保证收敛且 `E_land < P`）。
`E_land` 计入发布的蒸发场与水循环账（`E − P` 闭合门不变），其潜热从**低层大气**
扣除而不是从虚拟的混合层：陆面上辐射本就记在大气层，局地回流的蒸发与凝结
在大气内互相抵消。

地球参考：陆地 `E/P ≈ 0.59`（Oki & Kanae 2006）；P5 公式在 `κ = 0.5` 给 0.5、
`κ = 0.8` 给 0.71。

## 4. 越山抬升风（Task 2）

C2 剖面下地形凝结项的 `upslope` 与风速改用**上层大气**的风（越山流），湿度与
温度仍取低层（含水的那层）。C1 无上层，保持不变。出处：Smith (1979) 线性
山地波理论用上游风廓线驱动抬升；分流线以上气流翻越（Hunt & Snyder 1980）。

### 4.1 度量的一致性

`orographic-rain-shadow-leeward-drying` 与 `orographic-uplift-enrichment-ratio`
在山顶格按风向挑上游 / 下游邻格。低层风绕山后二者沿脊分布，度量失去物理
含义；改为按越山风（C2 发布的上层风）分类，与抬升项用同一支风。这是让度量
跟上被度量的物理，不是改包络：包络（≥ 0.02、≥ 1.20）不动。

## 5. 验证

- seed 42 探针增加全球降水 / 蒸发、雨影与增湿度量的打印；
- 17 seed 证据：降水回到 GPCP ±7 % 包络、雨影失败数回落、硬门全过；
- 32 seed 冷启动扫描；全量回归；产品级时延不变。

## 5.1 实测结论（2026-09-02）

| 指标（17 seed） | A2 态 | A3 态 | 参考 |
| --- | ---: | ---: | --- |
| 全球降水 mm/day | 2.56 | **2.66** | GPCP 2.81（±7 %） |
| 全球蒸发 mm/day | 2.66 | 2.76 | ≈ 2.8 |
| 雨影度量失败 | 10/17 | **4/17** | A0 1/17 |
| 增湿比失败 | 7/17 | 7/17 | A0 6/17 |
| 非纬向方差占比 | 0.047 | 0.046 | — |
| TOA W/m² | 6.8 | 7.2 | 0.9，门 ±10 |

seed 42 分解：海面 E 3.47 → 3.10、陆面 E 0 → 0.70 mm/day（陆地 E/P 0.28，地球
0.59）。陆面回流被海面饱和差的收缩部分抵消：单层湿度 + 相对湿度阈值降雨、
没有下沉干燥的水循环是"饱和差限制"的。剩余的 GPCP 缺口（−5 %）、TOA 的上漂
（4.95 → 7.2）与陆面 E/P 偏低，属 P4 水热校正（湿度垂直结构与降水闭合）。

## 6. 出处

| 项 | 出处 |
| --- | --- |
| 桶模式稳态 `E = P − R` | Manabe, S. (1969) *Mon. Wea. Rev.* 97, 739–774 |
| 全球陆面水量 | Oki, T. & Kanae, S. (2006) *Science* 313, 1068–1072 |
| 分流线 / 翻越与绕行 | Sheppard, P. A. (1956) *QJRMS* 82, 528–529；Hunt, J. C. R. & Snyder, W. H. (1980) *J. Fluid Mech.* 96, 671–704 |
| 线性山地波抬升 | Smith, R. B. (1979) *Adv. Geophys.* 21, 87–230 |
