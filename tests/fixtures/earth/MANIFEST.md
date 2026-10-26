# 地球参照掩膜夹具（G4 规格 §7.2）

- 数据：ETOPO1 1 Arc-Minute Global Relief Model，冰面版，格点配准，2 字节整数二进制
  （Amante & Eakins 2009, NOAA Tech. Memo. NESDIS NGDC-24）。
  - URL：<https://www.ngdc.noaa.gov/mgg/global/relief/ETOPO1/data/ice_surface/grid_registered/binary/etopo1_ice_g_i2.zip>
    （服务器 Last-Modified 2011-05-05，322,504,559 字节，下载于 2026-10-26）
  - `etopo1_ice_g_i2.zip` SHA-256 `877cbe01350b009583fd3b9c5ea4231e269484fbdaeb513b17b3a1bfdccce1ce`
  - `etopo1_ice_g_i2.bin`（21601 × 10801，LSB i16，首行 90°N）SHA-256
    `19ea64969e52bc980281772507b6a9e7aabb8963777c85f088d66db713652034`
  - 数据不入库；本机副本在 `D:/hemifuture-probes/etopo1/`。
- 处理（`tests/continent_shape_probe.rs::probe_earth_reference_masks`）：取 1′ 网格每第 10 行、
  第 10 列（10′ 采样，舍去与 180°W 重复的末列）；每个采样点按质心最近原则归入权威面格元，
  以 cos(纬度) 加权求格均值高程；掩膜为格均值严格大于阈值的格元。
- 掩膜：`> −200 m`（主掩膜，Q1）与 `> 0 m`（记录项）。Cogley (1984) 地壳块无可复核的栅格来源，
  未生成（规格 R2：记“未测”）。
- 文件格式：16 字节魔数 `sekai.earthmask1`、32 字节 `SurfaceRef` 指纹、u32 格数、u32 边数、
  按格号 LSB 优先的位图。载入时逐项校验 `SurfaceRef`，不符即失败。

| 文件 | 网格 | SurfaceRef 指纹（blake3 hex） | 格 / 边 | 掩膜内格数 | 面积占比 | 文件 blake3 |
| --- | --- | --- | --- | --- | --- | --- |
| `draft-minus200m.bin` | 草稿权威面 | `0d09df7aa131d120490202741b0fd3184919ea9681f16537a14f81f0e5806f2e` | 20252 / 60750 | 6721 | 0.3298 | `b72b2374325863151698cbcb0585a383812a17ee8bf4b42b2ba9f29020e06de5` |
| `draft-0m.bin` | 草稿权威面 | 同上 | 同上 | 5929 | 0.2906 | `571f08de6b0451a433a7f495489e380a744ceb54b19b6e9a181503edcd831943` |
| `standard-minus200m.bin` | 标准权威面 | `4247616b841cc415e4647576d4c445b501f053afc9da4bf33255fda1e9c09e84` | 79212 / 237630 | 26730 | 0.3355 | `3cb148886a05b8f80cd2a7dd05a903b201c40ddd75fdfc365b80db622a427e2a` |
| `standard-0m.bin` | 标准权威面 | 同上 | 同上 | 23338 | 0.2923 | `65a5f5ac8e4e14ef82dc287cb518e6f00644855c0c76305787342686823ffeb6` |

权威面的 `SurfaceRef` 指纹只在审计浮点平台上逐位稳定（`AGENTS.md` 测试范围纪律）；在其他平台
载入会因指纹不符而报错，消费者应按 `audited_float_platform()` 跳过。
