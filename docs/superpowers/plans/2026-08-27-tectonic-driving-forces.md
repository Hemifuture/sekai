# G1d 构造驱动力与终态陆壳形态 实现计划

> **给代理：** 规格是 `docs/superpowers/specs/2026-08-27-tectonic-driving-forces-design.md`。已冻结（2026-08-27）。

**目标：** 形成链发布态上五种预设可辨。Continents 为被洋盆隔开的若干陆壳块；Supercontinent 一块主导。角速度由边界力矩解出；俯冲启动按 Stern 自发/诱导。

**架构：** 力矩与接触分类留在 `foundation/tectonics`。预设相位只扩展已有 `FormationTectonicRecipe`（裂谷是否完成洋化），不新写威尔逊引擎。Cortial 步进与消耗算子保留。

**技术栈：** 现有 Rust 生成器；常量先测后钉，归 `src/world/`。

---

### 任务 0：冻结规格与路线图

**文件：**
- 修订 `docs/superpowers/specs/2026-08-26-natural-geography-short-horizon-roadmap.md`
- 修订 `docs/superpowers/specs/2026-08-26-g1-continental-crust-on-plates-design.md` §11

- [x] 用户确认 G1d 规格冻结
- [x] 提交

```bash
git add docs/superpowers/specs/2026-08-27-tectonic-driving-forces-design.md \
  docs/superpowers/specs/2026-08-26-natural-geography-short-horizon-roadmap.md \
  docs/superpowers/specs/2026-08-26-g1-continental-crust-on-plates-design.md \
  docs/superpowers/plans/2026-08-27-tectonic-driving-forces.md
git commit -m "$(cat <<'EOF'
docs: 冻结 G1d 构造驱动力与终态陆壳形态规格

随机固定欧拉极会在 256 Myr 内把 G1 多核陆壳缝回超大陆。用边界力矩与
Stern 俯冲启动接管演化中的角速度和接触分类，把预设钉在威尔逊相位上。
EOF
)"
```

---

### 任务 1：边界力矩解 \(\omega\)

**文件：**
- 新增 `src/generators/natural/foundation/tectonics/torques.rs`（或同等内聚模块）
- 修改 `runner.rs` 演化循环：力矩更新步写回各板 `rotation`
- `src/world/` 增加有出处注释的占位系数（实现期用探针替换为实测钉值）

- [x] 纯函数测试：玩具海沟 → 下插板指向海沟；有陆板更慢
- [x] `cargo test` 目标模块；`cargo fmt`；`clippy -D warnings`
- [x] 提交

系数未测前只进规格允许的排序（拉力 ≫ 脊推，陆拖曳 > 洋拖曳），数值用探针
对照后再钉，禁止为凑形态改符号。

---

### 任务 2：Stern 接触分类

**文件：**
- `contacts.rs`：洋–陆汇聚不再无条件 `OceanicSubduction`
- 裂谷/扩张路径给新洋–陆缘打被动缘标签（最小状态，不新增发布 schema）

- [x] 分类测试：被动缘 + 脊推量级 ≠ 俯冲；洋内老–幼 = 俯冲；碰撞邻接可诱导
- [x] 提交

---

### 任务 3：预设相位（裂谷是否完成洋化）

**文件：**
- `FormationTectonicRecipe` / 裂谷过程：Continents、Archipelago 完成洋化；
  Supercontinent 与 GreatIsland 主块保持陆内减薄

- [ ] 开局 G1 核数测试仍过
- [ ] 提交

---

### 任务 4：终态窄集成、身份、UI

**文件：**
- 抬 `natural.spherical-tectonics` 与 `natural.causal-formation` version
- 窄集成：草稿、种子 42 与 3、Continents vs Supercontinent、12 与 22 板
- 高成本探针 `#[ignore]` + Release，复用 G0 语料

- [ ] Continents 主要块不缝成一块主导；Supercontinent 一块主导
- [ ] 形成链地壳种类图可辨（用户 UI 验收）
- [ ] 完整调试回归（任务收尾）
- [ ] 提交

---

## 每项承重技术的出处

见规格 §8。本计划不新增无出处杠杆。
