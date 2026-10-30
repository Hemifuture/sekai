# A10 修订 R9 候选：同参考高度的能水守恒耦合

日期：2026-10-30。状态：**研究草案，未冻结；源头与独立审阅通过，后续实现仍按任务派发**。
上游：`2026-10-25-a10-reduced-order-climate-design.md` §20–21（R7/R8）。
起点 `a10-impl@e7a7f84`；用户已授权继续模型定义、最小受控夹具与两档 seed 42 对照。
当前完成定义、储量量级统计与 M1a/三个受控代数夹具；**未实现完整候选、压力B助手、
周期块扫或外层求解，未求耦合世界，未修改生产/UI**。
最终绿色 Release 编译发生 rustc 访问冲突，尚未验证；替代的默认 Debug 直接套件16项通过，
不能将此写成 Release 通过。原始证据与身份区别见证据根 REPORT。
湖线优先产品/UI 闭环；气候编译、探针和计时均先申请独占窗口。

## 1. 承重决定与适用边界

推荐先检验**同高度 MSE、固定有效输送质量、显式季节水汽储量**，热力诊断采用下面 B，
A 作最小消融。保留年 Budyko、暂定块体 E_pot 的过程定义，不拟合陆地 RH、D 或 GMS。
这是对 Siler 纬向平均近海平面模型的山地/二维类比候选，不是该论文对本模型的保证。

三个可实施的压力/参考柱选项共用 §2–5 的能水账本，避免把热力近似和守恒定义混在一起：

- **A：固定参考柱。** q_sat 继续使用 `P4_LOWER_LAYER_REFERENCE_PRESSURE_PA`，
  bulk 系数继续使用 `P4_REFERENCE_AIR_DENSITY_KG_M3`。q/RH 明确是固定参考压力的
  等效近地面值；Mq、Mcol、Hadley 速度换算质量也都是固定参考柱系数，不能冒称实际 p_s。
  改动最小，是分离新耦合与压力修正的基线；高山实际 RH 解释不足。
- **B：诊断标准大气压力，固定有效质量（推荐）。** 从既有 H 计算标准大气 p_std(H)、
  rho_std(H)，q_sat/海陆 bulk/凝结/降尺度使用同一压力诊断；Mq、Mcol 和 Hadley 参考
  质量保持固定有效系数。O(N) 几何/热力预计算，无新垂直自由度。它不声称这些质量
  等于 p_std/g；保留固定垂直形状与输送系数是待受控/两例检验的降阶近似。
- **C：诊断压力并按压力缩放参考柱。** 以固定 sigma 形状类比令 Mq(H)、Mcol(H)、
  空气热容和 Hadley 层质量随 p_std/p_ref 变；W 必须仍为守恒变量，所有扩散/平流
  系数改读 W/Mq(H)。这需要重新定义变质量的干空气连续与上支返回，不能只改一个
  Mq 倍率。没有增加垂直层，但装配与验证面明显扩大，当前不优先实施。

B 的选择理由是以一次 O(N) 诊断处理 R8 暴露的参考压力标签，同时保留现有参考剖面
作为有效闭合；若 A/B 质量或适用域不足，先回查该近似，再决定是否有收益做 C。
不得因为某选项陆雨更接近参照便追认其物理有效性。

## 2. T、q、h、高程与压力的唯一含义

H 复用 `ReducedClimateForcing.reference_height_m`：从当前终点地形和水面几何计算
权威格参考高度，再保守重映射的 scheme-C 高度，不使用旧的含海底平均高程替代。
H 在一次气候求解内固定；每个 P5 起点/终点评估各有自己的 H。

在同一工作格参考高度 H 定义：

```text
T_K = T_air_c + 273.15
Phi = STANDARD_GRAVITY_M_S2 * H
s = C_p T_K + Phi
q = W / Mq
h = s + L q
theta_c = T_air_c + Gamma H
```

C_p、L、g、Gamma、Mq 均复用现 SSOT。theta 仅为既有投影/温度降尺度坐标，
**不把 q 代入 q_sat(theta) 来替换已解 q**，也不把 theta 叫作热力学位温。
同一 h 若用 theta 写，必须保留 `(g-C_p Gamma)H`；Gamma 不是干绝热线 g/C_p。
Phi 的零点固定在该世界海面，近地面 gH 采用标准 MSE 的常重力近似。[S1,S2]

B 的压力是诊断标准大气，既不是新的气压动力方程，也不随逐月 T 波动重新造总空气柱。
对流层 `T_std=T_ref-Gamma H`，`R_ref=p_ref/(rho_ref T_ref)`；
`p_std=p_ref (T_std/T_ref)^(g/(R_ref Gamma))`，`rho_std=p_std/(R_ref T_std)`。
R_ref 由现有标准大气三元组推导，不另拍气体常数或指数；超出对流层时按同一
ISO/USSA 分层静力公式处理，界面由来源给出，不把一段直减率无限外推。[S3]

现有资料：两例工作格 H 最大为 Draft 2585.603 m、Standard 2038.758 m，均在首段内。
项目绝对高程范围不能直接当 H 范围：H 相对海面，`SurfaceWaterGeometry` 的海面校验
只保证有限，当前 forcing 没有统一高度上限。B 在原型入口必须核查所有参与计算/投影
的 H 是否落入所实现标准大气段；遇到未支持域显式失败，不 clamp 或静默退回 A。
初次两例只实现其实际覆盖的首段；生产前必须明确覆盖所有声明支持层级的段域。

q_sat 的压力形式须从现有 Bolton 单一实现扩参，p_ref 包装保持旧生产行为；不能复制
一份饱和曲线。bulk E/E_pot 仍是现块体交换律，B 只统一其压力/密度输入；Budyko
仍用同一实际 E_pot/P 年比。混合岸格仍是一份等效大气，海陆源分别按面积记账；
此近似不声称解析格内海面到山坡的垂直边界层结构。[S4,上游§20.2]

固定 C_p/L 是现有 MEBM 的标准热力近似，不是精确湿空气焓；本候选不为无消费者的
CAPE/完整热力精度引入新组分。Yano & Ambaum 的精确式也说明不能只把 L 换成另一
常量便宣称精确；本轮保留同一 L 用于库存、通量和相变，显式承认近似。[S1,S2]

## 3. 每边只定义一份水通量与配对能量

边正方向统一为 cell i→j，所有格残差用同一有符号关联矩阵：
`C_i(F)=-Σ outward F/A_i`。令 `K_e=Mcol D l_e/d_e`；质量因子是参考输送闭合，
不是实际 p_std/g。每个分项的潜热都直接读同一份 Fq：

```text
eddy:  Fq_E=(1-w) K_e (q_i-q_j)
       Fs_E=(1-w) K_e (s_i-s_j)
       Fh_E=Fs_E+L Fq_E
```

不得先算一份固定 RH 湿导度给 E1，再为 Q1 算另一份 q 梯度。年均冻结的湿割线导度
不再是候选 E1 的事实源；干静能扩散与显式 L Fq 分开装配，最后检验二者之和。

### 3.1 Hadley：先定参考环流，再记真实上下支

采用已有 hmean 的真实面积纬带重建，以**同一已解 h** 的纬向平均驱动参考环流：
`psi_e=-w K_e Δh_zonal / Gamma_band`，
`Gamma_band=h_upper-h_band`，`h_upper=alpha max_band(h_band)`。
alpha/w/D 仍为现有 Siler SSOT，固定其源论文的 Kelvin/海面参考零点；不新拟合稳定度。
上支取 q_upper=0、s_upper=h_upper，并返回与下支等量反号的质量流。[S1,S5]

q 的离散仍保留当前 Hadley+纬向总流向所选的共同下支迎风格 u；能量中的 s 与 q
也必须使用它。上支为原型中的诊断返回，不能额外充当第二份下支风：

```text
Fq_H = psi q_u
Fs_H = psi s_u - psi h_upper
Fh_H = Fs_H + L Fq_H = psi (h_u-h_upper)
```

这显式改变旧模型“E1 直接强钉 wKΔh，W2 用另一当月 h 构造 psi”的关系：
**参考 psi 仍来自 Siler 类比；E1 实际 HC 总能量由上下支 pair 回算，不能同时强钉旧
wKΔh。**局地下支与纬带平均不同，故回算值通常不等于参考驱动值。每轮记录此差、
GMS、上下支质量与潜热/干静能补偿；Gamma_band 非正/非有限、实际下支超出声明的
正 GMS 闭合，或数值残差不能满足既有精度时停止该候选，不设自创的 GMS 下限或 clamp。
正 GMS 定义不是二维山地环流保证。

### 3.2 纬向：进入总能量，采用同一等效含湿层

保留 R4 非辐散流函数面积流 Fz，固定参考模型将 Mq 作为含湿层的等效输送质量：

```text
Fq_Z = Fz W_u
Fs_Z = Fz Mq s_u
Fh_Z = Fs_Z + L Fq_Z
```

其余参考空气质量的上层取空间均匀干静能 h_upper；因固定质量、h_upper 常值且
Fz 逐格非辐散，该上层纬向项辐合严格为零，可不计算。将“水汽剖面等效质量”压缩成
含湿输送层是明确的两支/两库类比近似，不把 Mq 宣称为实测边界层深度或总空气柱。
这也是 C 不能只替换 Mq 的原因：空间变质量后上述零辐合不再自动成立。[S1,S5]

最终 `Fq=Fq_E+Fq_H+Fq_Z`、`Fs=Fs_E+Fs_H+Fs_Z`、`Fh=Fs+L Fq`。
旧 Q1 的共同 donor 约定是本轮比较的离散选择，不用分项各迎风改变数值扩散。

## 4. 库存、相变与周期月份离散

保留现有有效热容 C（空气与海洋/海冰部分的现有 SSOT）。在固定地形的一次气候评估中，
将温度库存记为 `C T`、水汽潜热库存记为 `L W`；固定 Phi 的库存时间导数为零。
这定义的是降阶柱的声明账本，不冒称解析完整垂直湿空气焓、降水焓/冰相与动能。

推荐候选保留现有 12 个月 W 未知量，但把原来 12 个独立准稳态问题改成周期库存问题：

```text
D_t W = E-P+C(Fq)
C D_t T + L D_t W = ASR-OLR+C(Fh)
```

总 MSE 方程不另加 L(P-E)。等价的 DSE 分裂式是
`C D_t T = ASR-OLR+C(Fs)+L(P-E)`；两式必须由同一个离散 Q1 相减得到，
不能各自装配一套相变量。E/P 是表面到大气的内部相变交换，保留 Budyko 与块体 E_pot。

首个有限周期候选采用**同一等月间隔的隐式后向差分**
`D_t X_m=(X_m-X_(m-1))/SECONDS_PER_CLIMATOLOGICAL_MONTH`，索引周期环绕。
选它是为了给刚性的凝结/蒸发与水库存一个共同、可有界求解的时间算子；现 E1 的 CN
谐波算子不能与此混用。BE 的阶次/季节幅相误差在受控单谐波夹具与旧 CN 基线间测量，
未测前不称季节精度无损；不增加时间步轨迹、子步或发布字段。[S6]

省略库存的消融必须同时置 `D_t W=0` 并在 E1 去掉 `L D_t W`，明确是诊断大气近似。
不能一侧动态、一侧静态后宣称相同能水预算。2026-10-30 现默认场月采样差分实测：
纯陆潜热库存变化 RMS 为 Draft 1.444、Standard 1.322 W/m²，对应完整纯陆热库存变化
RMS 7.692/9.605 W/m²；比值约 18.8%/13.8%。这是量级而非 E1 精确残差，
也不是新验收门。现有证据不足以将该项默认为零，故先受控比较含/省略。

### 4.1 选定算法：冻结系数的逐月空间块与外层 Picard

**不沿用旧 CN/DFT 求解。** `SymmetricSystem` 只适用于对称空间导度；现
`solve_bicgstab(grid, diag, off, rhs, initial)` 才能处理单月四邻点非对称行。
R9 的 HC/zonal 迎风项非对称、各月系数不同，不能宣称旧复数谐波求解直接适用。

选择下述分块固定点，复用该实数 BiCGSTAB，不造通用 solver/stencil 框架。[S7]
每个冻结外层内，月空间算子保持四邻点，周期前月的库存只进入 RHS：

```text
water: (A/dt + Lq_m) W_m = bq_m + A W_(m-1)/dt
heat:  (A C/dt + A B + Ls_m) T_K,m
          = bs_m + A L(P_m-E_m) + A C T_K,(m-1)/dt
```

Lq 是原水汽边输送和冻结的线性蒸发/凝结源汇；Ls 是干扩散与**同一共同 donor** 的
HC/zonal 干静能迎风项，系数包含 C_p。bs 包含原 ASR、线性 OLR 的截距、固定
gH 边项与上支 `-psi h_upper`；使用 Kelvin 未知量避免遗漏常量焓平流。
固定有效质量使 heat 行仍只有同月四邻格，不需要额外远邻。水块求得的同一 E/P
向量送入 heat 的 L(P−E)，热块内不另按新 T 重算一套相变量。

**冻结系数的周期扫：** 从上一轮12个月数组开始，按 m=0..11 顺序更新；m>0 用本扫
已更新的前月，m=0 用上一扫的12月。每月调用现有实数 BiCGSTAB，继承其 Jacobi
预条件、线性精度和单月迭代边界。完成整扫后，必须用本扫最终12月值重算1月行，
并重算全部12个月的固定系数循环行残差；不能用“每个月各自解过”代替周期收敛。
water 周期块收敛后冻结其 E/P，再以同样顺序扫 heat 周期块。
任一月 BiCGSTAB 返回未满足现线性精度、非有限值或 breakdown 时，该冻结块报失败，
不能把返回的临时向量当作成功月份继续验收。

不首选联合12月 BiCGSTAB：那需要新增时间邻点的矩阵应用/预条件，现四邻点接口
不能承载；目前没有证据证明其成本优于上述复用路径。块扫若不收敛，记录失败与
残差，重新评审算法，不静默扩大为新求解框架。

**外层更新顺序：** 初值可用旧原型的 T/W 与已用蒸散份额，只作为私有 warm start。
每轮 k 严格执行：

1. 从 T^k/W^k 重建同高度 h、纬带量、h_upper、psi、Fz、共同 donor 与风；
   按现年温度规则定冰掩膜并装配 ASR/C/OLR；压力/地形诊断固定。冻结本轮 eps^k、
   蒸发/凝结活跃集、上坡凝结系数及这些通量系数。
2. 解 water 的12月周期块，得到 W' 和**同一冻结源汇定义**下的 E/P；
   解 heat/DSE 周期块，得到 T'。此处的线性残差只证明冻结块求解，不能验收模型。
3. 从 T'/W' 重新构造所有物理诊断（含 psi/donor/风、冰、凝结/bulk E、E_pot/P），
   计算 eps_target=Budyko(E_pot,annual P)。先按 §4.2 验证最终状态；若失败，令
   T^(k+1)=T'、W^(k+1)=W'、eps^(k+1)=eps_target，进入下一轮。

第一版为不加阻尼/加速的普通 Picard。既有 Budyko-only Anderson 不自动保证这个联合
固定点收敛，不在这里另套一个已收敛 Budyko 内循环或发明混合系数。冰/活跃集翻转、
周期扫或外层不收敛都如实失败；外层/周期扫次数是受控阶段的有界实验资源预算，
在派发前明确，不以预算用尽冒称达到最终态。

### 4.2 最终验收必须重建同一状态的残差

候选状态为 T'/W'、本轮实际使用的 eps^k，以及从它们**重新诊断**的 psi/Fz/donor、
风、冰/ASR/C、活跃集和 E/P/E_pot。不要输出旧冻结 psi，或先把 eps 换成 target
却不重新平衡 T/W。O1 洋流不反馈这些方程，只在该状态验收后由最终风求一次。

所有物理残差都从重建状态计算，而不是读缓存的旧 RHS：

```text
r_q = D_t W-E+P-C(Fq)
r_h = C D_t T+L D_t W-ASR+OLR-C(Fh)
r_s = C D_t T-ASR+OLR-C(Fs)-L(P-E)
r_B = eps^k-Budyko(E_pot,annual P)
```

逐格逐月检查 `r_h-L r_q=r_s`、同边 `Fh-Fs-L Fq=0` 与成对反号；
1月的 D_t 必须读取最终12月值。固定块周期扫另记录环闭缺陷
`A(X_11,after-X_11,before)/dt`（heat 再乘 C），以及用最终12月值重算的1月行残差。
数组索引环绕本身不能证明周期块已求解。

数值闭合的规范化明确为：每个格月取 `sigma_q=|D_tW|+|E|+|P|+|C(Fq)|`、
`sigma_h=|C D_tT|+|L D_tW|+|ASR|+|OLR|+|C(Fh)|`，令 rho=r/sigma；
检查 `sqrt(Σ_(i,m) A_i rho² /(12 Σ_i A_i))` 和 `max_(i,m)|rho|`。
分母为零时仅零残差定义为0，非零定义为失败。边恒等式尺度为
`|Fh|+|Fs|+|L Fq|`，格月残差恒等式用 `sigma_h+L sigma_q+sigma_s`，
其中 sigma_s 是 DSE 各项绝对量之和；冻结块行用该行左右各项绝对量之和。
r_q/r_h、固定块循环行和上述代数恒等式使用
现 `REDUCED_CLIMATE_LINEAR_RELATIVE_TOLERANCE` 作为研究数值闭合精度；同时输出
最大绝对水/能量残差，不能只用全球零和掩盖局地失败。这不是新的最终地图质量阈值。

r_B 复用现 Budyko 份额精度；冰掩膜和蒸发/凝结活跃集必须与最终状态的原判定完全
一致，共同 donor 必须来自最终 psi+Mq Fz 的流向。psi/风由该状态确定性重建，检查
其定义一致性。若这些检查或任一非有限/负水库存/声明有效域检查失败，继续外层或
按资源边界报失败；不能只因 T/W 的相邻迭代差小、旧 active-set 标志不变就停止。

## 5. 最小受控实验与两例推进规则

先用同一私有边核构造以下三个小夹具，不靠多 seed 证明代数：

1. **两格、不同 H 的静力/边账本。** 指定相同 h、不同 s/q，检查坐标变换、
   Fh=Fs+L Fq、反号守恒及潜热/干静能抵消；平坦 H=0 回到源模型的定义。
   包含 psi 与 Fz 反号情形，验证共同 donor 和上下支质量反号。
2. **闭合两格、周期水/相变。** 用指定有符号 E−P 与周期库存，检查水式与
   总 MSE 式减去 L×水式恰好得到 DSE 式；常值状态零漂移，全年库存变化和为零。
   同一夹具分别跑含/省略库存，不能将两者数学不同叫作代码回归。
3. **单谐波辐射/热容响应。** 对无输送的受控库使用已知离散解析响应，测共同 BE
   的幅相、循环首尾和误差，再与现 CN 结果对照；不先钉新的季节误差阈值。

代数误差复用现有线性精度与尺度归一化，不自创科学容差。新耦合轮数、停止预算与
科学质量包络先在夹具测量，由实现任务给有界执行预算；未收敛即记失败，不扩大超时。

夹具成立后只在同一生产终点强迫上先 Draft42、再 Standard42 做：旧 R8、B 含库存、
B 省略库存的对照；A 仅在确认压力解释需要分离时作同两例对照，不默认全组合。
报告温度/P/E/Budyko、q/RH 标签、真实格面积区域账本、每月局地能水残差、
同边恒等式、GMS/参考驱动与实际 HC 差、有效域、求解耗时/内存。
先看机制/守恒和目标尺度最终字段，再决定有无收益；未成立不扩 17 seed 语料。

## 6. 消费者、配置、身份与 UI

- `forcing.rs` 的 H/地形身份保持 SSOT；`prototype.rs::solve_reduced_climate`、
  `solve_moist_energy_balance`、`hadley_lower_branch`、`solve_monthly_water` 是候选边界。
- 原 q_sat/大尺度凝结/上坡凝结/bulk E/E_pot 和 probe 的 RH 保持投影，必须统一压力
  解释；不能只修 h 后让发布 q 的 RH 用另一压力。B 保持现有 T/q 字段，不加产品旋钮。
- 海冰、OLR、热容读取 T 的原物理高度：海洋温度在海面、空气/陆地温度在 H；
  theta 只服务现 scheme-C 投影，候选不能让一项修正静默改变其他模块的坐标。
- 发布降水/风/洋流会影响 P5、水文及最终地形；发布 T/q 影响显示、质量、点击与
  LOD 派生。受控阶段不做第二套 UI，不占湖线产品窗口。
- 当前 test-only 统计/夹具不改变发布身份。未来 T8 若采纳，必须提升
  reduced equations/profile 身份、对应阶段实现版本及 recipe 兼容版本；若 schema
  字段未变，不为方程变化伪造 schema 字段。不得只重钉旧金样。
- UI 接入任务在后续获批预览分支执行：`cargo run --release --bin sekai`，
  Continents、Draft/Standard、seed42；二维年降水/温度/风、三维极区与海岸/山地/内陆，
  L0 到支持叶片点选/显示同源，明确 B 的 RH 压力解释。用户本人验收前不称算法交付。

## 7. 每项承重技术的出处

- **S1** Siler, Roe & Armour (2018), DOI 10.1175/JCLI-D-18-0081.1，Eq.2、4–9、Table1。
  [作者全文](https://earthweb.ess.washington.edu/roe/GerardWeb/Publications_files/SilerEtal_HydroCycle_JCIim18.pdf)。
  支持近地面 MSE 梯度参数化、固定参考柱、HC 上干下湿分配；山地二维与等效层压缩是类比。
- **S2** Yano & Ambaum (2017), DOI 10.1002/qj.3121，§2 Eq.1–3、§3；
  [作者接受稿](https://www.met.reading.ac.uk/~sws97mha/Publications/qj3121.pdf)。
  支持 gz 与参考常量/标准热力近似的区分；不将该文当作固定 L 的精确性保证。
- **S3** COESA, *U.S. Standard Atmosphere 1976*, NOAA-S/T-76-1562，静力/理想气体分层压力；
  [NASA 原始记录](https://ntrs.nasa.gov/citations/19770009539)；
  ISO2533:1975 §2.7 Eq.12–13，与已有 Gamma/Tref/pref/rhoref 常量来源相同。
- **S4** Bolton (1980), DOI 10.1175/1520-0493(1980)108<1046:TCOTEP>2.0.CO;2，Eq.10；
  现 SSOT 的饱和蒸气压转比湿，压力作为显式输入。B 不另写饱和曲线。
- **S5** Neelin & Held (1987), DOI 10.1175/1520-0493(1987)115<0003:MTCBOT>2.0.CO;2；
  Raymond et al. (2009), DOI 10.3894/JAMES.2009.1.9，§2、§4.1，
  [一手全文](https://agupubs.onlinelibrary.wiley.com/doi/10.3894/james.2009.1.9)。
  支持 GMS 与上下支的近似性质，尤其说明仅有代数预算不能保证环流气候合理。
- **S6** 隐式后向差分采用既有工业时间积分实现的同一方法定义：
  [PETSc TSBEULER](https://petsc.org/release/manualpages/TS/TSBEULER/)（theta=1），
  仅作为离散候选；季节精度需本计划受控测量，不把一阶当自动可接受。
- **S7** 月空间块使用现有 Jacobi-BiCGSTAB：van der Vorst (1992),
  [DOI 10.1137/0913035](https://epubs.siam.org/doi/abs/10.1137/0913035)；
  周期月份块顺序扫与普通 Picard：Saad (2003), *Iterative Methods for Sparse Linear
  Systems*, 2nd ed., §4.1 的块 Gauss–Seidel/固定点矩阵分裂，
  [作者版](https://www-users.cse.umn.edu/~saad/IterMethBook_2ndEd.pdf)。
  这些来源说明算法，不保证本非线性气候候选收敛；最终残差必须实测。
- Budyko、块体 E_pot、R4 流函数非辐散与 SSOT 数值域沿用 R7/R8 及其出处；
  本草案没有新增经验系数或气候质量阈值。
