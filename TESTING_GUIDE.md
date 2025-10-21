# 地形生成系统测试指南

## 📋 测试清单

### 前提条件
由于网络限制可能无法下载依赖，建议在有网络连接的环境中测试。

## 🧪 自动化测试

### 1. 运行所有单元测试
```bash
# 运行所有库测试（包括内嵌的单元测试）
cargo test --lib

# 只运行 terrain 模块的测试
cargo test --lib terrain::

# 运行具体的测试
cargo test --lib test_noise_output_range
```

### 2. 运行集成测试
```bash
# 运行地形验证集成测试
cargo test --test terrain_validation

# 运行特定的集成测试
cargo test --test terrain_validation test_noise_generator_basic

# 显示详细输出（包括 println!）
cargo test --test terrain_validation -- --nocapture
```

### 3. 性能基准测试
```bash
# 运行 Delaunay 基准测试
cargo bench --bench delaunay
```

## 🔍 手动验证步骤

### 步骤 1: 检查代码编译
```bash
# 检查所有代码是否可以编译（不运行）
cargo check

# 检查并显示警告
cargo check --all-targets

# 发布模式检查
cargo check --release
```

预期结果：
- ✅ 无编译错误
- ✅ 无严重警告
- ⚠️ 可能有一些未使用变量的警告（这是正常的）

### 步骤 2: 运行应用
```bash
# 开发模式运行（较慢但有调试信息）
cargo run

# 发布模式运行（优化性能）
cargo run --release
```

预期行为：
1. 应用启动后自动生成初始地形
2. 控制台输出：
   ```
   MapSystem: Generated XXXX height values for XXXX grid points
   HeightMapRenderer: Initialized with XXXXXX vertices
   ```

### 步骤 3: 验证地形显示
在应用界面中：

1. **观察初始地形**
   - ✅ 看到彩色的 Voronoi 单元（不是全黑或全白）
   - ✅ 颜色从深蓝（海洋）到绿色（平原）到棕色（山脉）到白色（雪峰）
   - ✅ 地形有明显的高低变化

2. **测试交互**
   - 使用鼠标滚轮缩放 → ✅ 地形平滑缩放
   - 按住空格+拖拽或中键拖拽 → ✅ 地形平移
   - 观察边缘 → ✅ 无明显的撕裂或空洞

### 步骤 4: 测试重新生成
在顶部菜单栏：

1. **点击 "🎲 Regenerate Terrain" 按钮**
   - ✅ 地形立即变化
   - ✅ 生成完全不同的地形形状
   - ✅ 顶部显示新的 Seed 值

2. **多次点击（测试5-10次）**
   - ✅ 每次都生成不同的地形
   - ✅ 无崩溃或卡顿
   - ✅ Octaves 值保持不变（默认 6）

3. **观察控制台输出**
   ```
   Regenerating terrain...
   MapSystem: Regenerated XXXX height values with seed XXXXXXX
   HeightMapRenderer: Updated with XXXXXX vertices
   ```

### 步骤 5: 性能验证
1. **观察帧率**
   - ✅ 缩放/平移流畅（无明显卡顿）
   - ✅ 重新生成地形耗时 < 1 秒

2. **内存使用**
   - ✅ 无明显内存泄漏（多次重新生成后内存稳定）

## 📊 预期测试结果

### 单元测试（33个）
```
terrain::noise::tests::test_noise_output_range ... ok
terrain::noise::tests::test_seed_determinism ... ok
terrain::noise::tests::test_different_seeds_produce_different_results ... ok
terrain::noise::tests::test_octaves_increase_detail ... ok
terrain::noise::tests::test_frequency_affects_scale ... ok
terrain::noise::tests::test_custom_range_generation ... ok
terrain::noise::tests::test_preset_configs ... ok

terrain::height_generator::tests::test_height_count_matches_grid_points ... ok
terrain::height_generator::tests::test_height_values_in_valid_range ... ok
terrain::height_generator::tests::test_heights_are_non_uniform ... ok
terrain::height_generator::tests::test_same_config_produces_same_heights ... ok
terrain::height_generator::tests::test_different_seeds_produce_different_heights ... ok
terrain::height_generator::tests::test_generate_at_single_point ... ok
terrain::height_generator::tests::test_generate_at_normalized ... ok
terrain::height_generator::tests::test_adjacent_points_vary ... ok
terrain::height_generator::tests::test_empty_grid ... ok
terrain::height_generator::tests::test_large_grid_performance ... ok

terrain::color_map::tests::test_interpolate_boundary_values ... ok
terrain::color_map::tests::test_interpolate_middle_value ... ok
terrain::color_map::tests::test_interpolate_clamping ... ok
terrain::color_map::tests::test_multiple_stops_interpolation ... ok
terrain::color_map::tests::test_single_stop ... ok
terrain::color_map::tests::test_empty_color_map ... ok
terrain::color_map::tests::test_unordered_stops_are_sorted ... ok
terrain::color_map::tests::test_interpolate_u8 ... ok
terrain::color_map::tests::test_earth_style_preset ... ok
terrain::color_map::tests::test_grayscale_preset ... ok
terrain::color_map::tests::test_fantasy_style_preset ... ok
terrain::color_map::tests::test_color_smoothness ... ok

test result: ok. 33 passed; 0 failed; 0 ignored
```

### 集成测试（7个）
```
test test_noise_generator_basic ... ok
test test_noise_generator_statistics ... ok
test test_height_generator_basic ... ok
test test_height_color_map ... ok
test test_noise_seed_consistency ... ok
test test_different_seeds_produce_different_results ... ok
test test_integration_full_pipeline ... ok

test result: ok. 7 passed; 0 failed; 0 ignored
```

## 🐛 常见问题排查

### 问题 1: 地形全黑或全白
**可能原因**：
- 高度生成失败
- 颜色映射错误

**排查步骤**：
1. 检查控制台是否有错误日志
2. 查看 Seed 值是否更新
3. 运行单元测试验证核心逻辑

### 问题 2: 点击按钮无反应
**可能原因**：
- UI 事件处理错误
- 渲染器更新失败

**排查步骤**：
1. 检查控制台是否有 "Regenerating terrain..." 日志
2. 查看 Seed 值是否变化
3. 尝试缩放/平移测试渲染器是否正常

### 问题 3: 编译错误
**可能原因**：
- 缺少依赖
- 网络问题

**解决方案**：
```bash
# 清理并重新构建
cargo clean
cargo build

# 如果是网络问题，等待网络恢复后：
cargo update
cargo build
```

### 问题 4: 测试失败
**可能原因**：
- 随机性导致的边界情况
- 浮点精度问题

**排查步骤**：
1. 查看具体失败的测试和错误信息
2. 运行多次确认是否稳定失败
3. 检查测试的断言是否合理

## 📝 测试报告模板

测试完成后，请填写：

```
测试日期: YYYY-MM-DD
测试环境: [Windows/Linux/macOS]
Rust 版本: [rustc --version]

编译测试:
  cargo check: [✅/❌]
  cargo check --release: [✅/❌]

单元测试:
  cargo test --lib: [XX/33 passed] [✅/❌]

集成测试:
  cargo test --test terrain_validation: [XX/7 passed] [✅/❌]

手动验证:
  应用启动: [✅/❌]
  初始地形显示: [✅/❌]
  地形颜色正确: [✅/❌]
  缩放/平移正常: [✅/❌]
  重新生成功能: [✅/❌]
  性能流畅: [✅/❌]

发现的问题:
  1. [描述]
  2. [描述]

建议:
  [您的建议]
```

## 🎯 成功标准

全部功能正常的标志：
- ✅ 所有单元测试通过（33/33）
- ✅ 所有集成测试通过（7/7）
- ✅ 应用正常启动并显示地形
- ✅ 地形颜色从蓝到绿到棕到白
- ✅ 重新生成功能正常工作
- ✅ 交互流畅无卡顿

如果以上都通过，则实现完全正确！ 🎉
