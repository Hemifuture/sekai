# T8 实际原生界面证据

2026-09-30 UTC，生产 HEAD `49e20840db736a233876e1028dedf2cc5b0c70b6`。
精确生产库 `target/release/deps/libsekai-7716a1b8ffc33b4e.rlib`，
SHA256 `A3F6D1F3CCD4B48070B8AB5A085F76BD86E49125D8D945A438B7CEA819844299`。
启动器、源码身份见 `native-identity.json`、`production-source-manifest.json`。
实际适配器 RTX 4080 SUPER / NVIDIA 591.86 / Vulkan；窗口客户区 1280×900。

仅自己的 PID 69756 / HWND 1248108，正常授权环境，独立本目录 RON，
`with_active(false)`；没有全屏截图、焦点抢占、全局输入或定时重绘。
`SEKAI_FRAME_STATS=1`，无科学 scenario；默认 Formation / Draft / seed 42 未变。
所有 PNG 均通过 PrintWindow 客户区捕获并逐张 view_image 检查，UTC/SHA 见
`screenshots-index.json`。13:54:56.1452172Z 已确认该 PID 正常退出。

## 实际操作

- owned HWND InvalidateRect 成功，命令文件随后消费且真实菜单展开，见
  `wake-first.json`、`wake-consumed.json`、`wake-first-menu.png`。
  默认受限执行环境看不到这个正常环境窗口；同启动环境的探测成功。
- [3D 转动](t8-3d-rotated.png)，13:38:56.3153972Z。
  实际拖动后保存的相机四元数为
  `[0.0415938893, 0.1850369704, -0.0121362406, 0.9817759315]`，区别于初始单位四元数。
  控件清楚显示初始板块数 12；T5 旧图疑点在本次未复现，不推断旧图原因。
- 实际保存新文件 `C:\Users\DYZ\Documents\Codex\2026-09-30\task\Sekai演示\海蓝星.sekai.json`，
  创建前核对不存在。保存内容仅配方/身份/视图，无世界快照；重开后文件哈希未变。
  实际输入 plate_count=12，见 `saved-demo-recipe.json` 及配方原样副本。
- [首次兼容 Open 自动生成](t8-open-autogenerating.png)，13:42:23.9827614Z；
  没有手动点重建。该次观测门控关闭，不能提供其构建计时，之后只补一次必要开启观测的 Open。
- [2D 实际缩放](t8-2d-zoom-controls.png)、[切换年均气温字段](t8-field-temperature.png)。
  都通过真实 UI 输入操作，不修改科学参数。
- [取消后保留旧有效世界](t8-cancel-retained-world.png)，13:47:48.9383653Z。
  request 3 结果为 Cancelled；旧地图和字段仍可见。直到约 90 秒后的明确 request 4
  都没有新 BuildOrigin，之后显式重建才再次发布。
- [显式热重建发布和采样](t8-retry-published-sampled.png)，13:49:47.9428222Z。
  真正禁用→开启生产采样器后再重建，同一 10 秒窗口覆盖 GPU 安装和发布。
- 最后必要的开启观测 Open：真实 read/decode 后自动启动且仅记录一次
  `origin=open`，token epoch 2 / request 5 / revision 0，终态 Published；
  [恢复后的实际 3D 状态](t8-observed-open-state.png)。

## 原始数据

| 实际操作 | 耗时 |
|---|---:|
| 启动默认世界 Published | 52.344039 s |
| 保存请求 / worker 编码校验与写入 | 0.008060 / 0.0074275 s |
| 观测 Open 读取请求 / worker 读取解码 | 0.007852 / 0.0070838 s |
| Open 前置阶段 / 完整发布 | 0.024922 / 69.724806 s |
| Open Graph / GpuInstall | 66.682725 / 0.060270 s |
| Open FirstSubmittedFrame 起点 / 本阶段 | 69.724761 / 0.000044 s |
| 显式热重建 Published / GpuInstall | 2.163288 / 0.092688 s |
| 热重建 FirstSubmittedFrame 起点 / 本阶段 | 2.163174 / 0.000112 s |
| 取消接受→反馈更新 / 观察 worker 退出 / 槽释放 | 0.0016165 / 0.999387 / 0.999937 s |

| 生产 10 秒窗口 | 样本 | 平均 FPS / 1% low | p50 / p95 / p99 / max，ms |
|---|---:|---|---|
| 启动生成前段，未含发布 | 1440 | 143.985 / 43.159 | 1.375 / 21.209 / 21.507 / 28.094 |
| 首次 Open 发布后的 3D | 1194 | 119.351 / 28.347 | 2.817 / 24.510 / 26.316 / 65.799 |
| 显式热重建，包含安装和发布 | 1433 | 143.202 / 31.443 | 1.806 / 21.251 / 24.994 / 100.690 |

完整日志为 `native.stdout.log` / `native.stderr.log`，摘要 `runtime-summary.log`；
操作发起 UTC 见 `issued-commands.jsonl` / `action-receipts.log`，退出见 `exit.json`。
helper 写文件/唤醒时刻不等于应用接受时刻；取消三个延迟以生产内部接受记录为起点。

## 限制和未完成项

这些是 inactive native 窗口、逐事件临时驱动和生产 update/submission 代理，
不是 OS present FPS，也没有精确原生 worker 退出测量。
FirstSubmittedFrame 仅是应用提交绘制，不是 OS 已显示。
临时驱动每个原生更新仍检查命令文件；额外重绘仅服务实际输入队列，不强制定时 idle 节奏。
首个 Open 缺计时、第二个窗口晚于发布已如实保留；第三窗口仅覆盖热重建发布，
不能归属最后一次冷 Open。窗口采样不是 stock idle 性能或冷全链预算测试。
未实际检查不可兼容升级、不同目录/新进程重开、精确候选待发布瞬间取消，
也未完整验收河线、拾取、接缝与 LOD。用户本人最终 UI 验收仍待完成。
