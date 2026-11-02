# Civ Atlas 地形结合产品许可说明

本构建实际链接 `crates/civ-atlas-terrain`，其中地形实现直接移植自
guaner-334/civ-atlas，固定提交
`176cc9168a69be30d1e09bfd85db84a5f3fb99da`。该实现采用 AGPL-3.0-only；
结合它的 Sekai 产品按 AGPL-3.0-only 提供，完整许可证见 `LICENSE-AGPL-3.0`。
这不改变原有独立代码和第三方组件的许可证或版权归属。

原 Sekai/eframe 模板的 `LICENSE-MIT` 和 `LICENSE-APACHE` 均保留。
移植出处、原作者及各几何依赖的 MIT/ISC 声明见
`crates/civ-atlas-terrain/NOTICE.md` 与该目录的 `licenses/`。
实际构建对应源码包括本仓库及该路径包；TypeScript 参照仅用于离线验证，
产品生成过程不依赖网页服务、JavaScript 或磁盘世界缓存。

本次工作仅在本地实现和验证，不构成网站部署或对外发布。
