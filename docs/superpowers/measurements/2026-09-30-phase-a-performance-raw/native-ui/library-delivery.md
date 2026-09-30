# T5真实应用截图交付（2026-09-30 UTC）

构建：f2ad407507668f9d02727c3b235920738940a8fc + T6未提交源码；精确身份见 docs/superpowers/measurements/2026-09-30-phase-a-performance-raw/t6-source-manifest.json。实际生产TemplateApp运行于隔离storage的原生窗口，PrintWindow仅截PID58168客户区域1280×900；控制者已逐张view确认可读。截图程序已退出后才恢复基准。

| 中文说明 | UTC时间 | library_file_id | 返回版本 |
| --- | --- | --- | --- |
| 生成中：真实11秒进度，可取消；不是最终生成耗时 | 11:27:05 | libfile_d5b52c29a81c8191809fa49ea6d9a3e6 | 0 |
| 文件菜单：新建、打开、保存、另存为、退出 | 11:27:33 | libfile_8da60aa42374819191850f5ba6ba42b9 | 0 |
| 已发布的真实二维地图与地理/水热诊断；初始板块数控件疑似尾字裁切，T8待核验 | 11:28:50 | libfile_e8ea64e4ec908191ab74e8745d497d5e | 0 |
| 新建后仅保存默认配方：明确世界尚未发布、重开需重新生成 | 11:30:18 | libfile_cac1b76d1e5881919aa66dcba1a5742e | 0 |

未完成：真实3D、重开自动一次生成、取消保留旧世界、编辑状态完整操作检查，安排T8；用户本人UI验收始终待进行。截图不能证明科学门禁已过或性能达标。

Library当前云技能helper先在tools/list连接失败，正常环境重试发现prepare_uploads不可用；没有prepare/upload/finalize发生，按prepared不可用规则转有序direct create batch。四文件全部返回succeeded。Windows Python无os.setxattr，四次官方metadata写回均失败，身份/版本/xattrs与原始本地路径完整保存在library-upload-receipt.json及library-xattrs-{0,1,2,3}.json；不声称扩展属性成功，不重传创建重复项。
