# 合成快照

`generate_synthetic_snapshot.py` 生成一份**不是真实 Minecraft 存档**的小样本，用于在仓库内复现读档、渲染和网页流程。它使用现代 `dimensions/minecraft/.../region` 目录和 1.18+ 根 `sections` 结构，DataVersion 写为 3955 仅作示例，不代表任何版本兼容性。

样本特意覆盖：

- 跨原点的负 chunk 与负瓦片索引（`r.-1.-1.mca`、tile `-1,-1`）。
- 主世界地表、水面、雪顶、树、负 section Y 和一个未知模组方块（洋红回退）。
- 下界 `below_y` 忽略 y=127 的基岩屋顶。
- 末地缺少一个区块，保持透明；末地区块使用外置 `c.0.0.mcc`。
- 一个需要 gzip 解压的下界区块。
- 一个远距离区块，触发父层 2×2 下采样。
- 三个测试标记和一条旧格式坐标链接所需的维度。

生成到已忽略的 `.local-tests/`：

```powershell
python fixtures/generate_synthetic_snapshot.py --out .local-tests/synthetic
```

脚本会写入 `snapshot/`、`map.toml`、`markers.json`。完整命令见根目录 [README](../README.md)。不要把 `.local-tests/` 或任何真实存档提交到仓库。
