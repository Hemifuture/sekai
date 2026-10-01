# 最终修正：实际原生 UI 验证

生产构建为 `50e379cfe2b918bdd8a50445d318a7a7f082d13d`。精确链接 `libsekai-5c9dba06e85c9e1f.rlib`，SHA256 `135FB169A9DF61EA216573DE9A0EBED52959088DF7C4DECBA54EB017A430A756`，匹配 eframe `34ef0bb63ee25fb3`。完整身份见 [native-identity.json](native-identity.json)；250 文件交接 manifest SHA256 `04F5F53F0FBFF94A527905B160DCCDD85A8E01EDD7A1E9BF765E91A874358655`。退出后的只读核对发现后续工作区 `src/app.rs` 已改变；这些图片明确对应上述交接构建，见 [final-verification.json](final-verification.json)。

仅独立 PID 4916 / HWND 7805320，1280×900 客户区；task 内独立 RON，窗口 inactive。输入由 owned-window 命令与 InvalidateRect 唤醒，无全桌面输入、无额外常驻重绘。四张均用 PrintWindow 捕获并逐张 view_image 确认可读。图片 UTC、SHA 与说明见 [screenshots-index.json](screenshots-index.json)。

| 实际图片 | UTC | 可见结果 |
| --- | --- | --- |
| [自然世界构建](current-build-phase.png) | 2026-10-01 00:47:38.6587863Z | 默认配方唯一一次自动生成，真实粗阶段可见 |
| [键盘接受 40%](percentage-40-accepted.png) | 2026-10-01 00:49:29.1736860Z | UI 选择陆地占比，Ctrl+A 输入 `40%`，Enter 后失焦保留 40% |
| [板块 0 保留错误](invalid-plate-zero-retained.png) | 2026-10-01 00:51:52.2706494Z | 输入 `0` 后 Enter/失焦仍显示 0 和错误，旧有效地图保留 |
| [Escape 恢复默认](escape-restored-default.png) | 2026-10-01 00:53:30.7437058Z | Escape 后板块 12，错误清除，默认配方真实保存成功 |

通过真实保存新文件 `Sekai演示/输入校验.sekai.json` 确认接受值为 `target_land_fraction=0.4`，见 [accepted-percentage-receipt.json](accepted-percentage-receipt.json)。随后实际 Undo 两次恢复原政策和目标，再输入无效板块 0。无效状态实际 Save 未改变已保存文件 SHA，plate 仍为 12，见 [invalid-save-blocked.json](invalid-save-blocked.json)。实际 Escape 后再次 Save：plate 12、WaterInventory、target 0.38、seed 42、Formation Draft 等默认输入恢复，见 [restored-default-receipt.json](restored-default-receipt.json)。保存的原始配方副本也保留在本目录。

只产生初始默认地图；40% 验证期间保留旧图，未点击重建或生成变更配方。旧 `海蓝星.sekai.json` 与 T8 保存回执 SHA 相同，T8 三个原 helper 字节不变。动作原始记录见 [issued-commands.jsonl](issued-commands.jsonl) 和 [action-receipts.log](action-receipts.log)。

实际 Close 后 PID 已退出，2026-10-01 00:53:31.6649202Z，见 [exit.json](exit.json)，已向 root 通知 BENCH-IDLE。未测量 OS present 或性能分位；本轮 UI 范围无剩余阻碍，最终用户验收由 root 汇总。
