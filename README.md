# TongCraft Map

把定期导出的 Minecraft 世界快照做成二维网页地图，让玩家找到服务器里的机器和建筑。项目目标是替换 TongCraft 现用的 BlueMap 平面地图，沿用 Fabric 服务器的存档导出流程，并保留 `map.weiuou.top` 上已有的坐标链接。游戏服务器不需要安装地图模组。

**当前进度：CPU 纵向切片已可运行。** 已在可公开的合成快照上跑通“读取 region/NBT → 提取地表 → CPU 渲染 256 像素 PNG 瓦片 → 金字塔 → 清单/覆盖索引/标记 → Leaflet 网页与旧链接定位”。尚未用真实 TongCraft 存档验证，GPU、Tauri 桌面端、增量缓存和正式发布流程仍未实现。合成样本只验证代码路径，不代表任何真实 Minecraft 版本的兼容性。

## 现在能做什么

- `inspect` 识别 26.1 的 `dimensions/minecraft/.../region` 与旧版 `region/`、`DIM-1/region/`、`DIM1/region/` 布局；两者同时存在时拒绝自动合并。读取 region 时会报出 DataVersion、Status、压缩类型和外置 `.mcc` 数量。
- `render` 只实现 `cpu` 后端：读取 1.18+ 根 `sections` 的 paletted block states，提取主世界/末地地表或下界 `below_y` 层，按 `styles/default.toml` 着色，写出 256×256 RGBA PNG、2×2 预乘透明平均的父层、`manifest.json`、coverage 和 markers。未知方块用洋红回退并计入私有报告。
- `verify` 检查清单、路径越界、coverage 与瓦片文件的一致性、抽样 PNG 尺寸和标记字段，能区分“声明了但缺文件”和“文件损坏”。
- `web/` 用 TypeScript + Vite + Leaflet 1.9 读取 `current.json` 或 `?release=<id>` 预览；按 coverage 跳过空瓦片请求；支持维度切换、机器选择、坐标显示和 `#world_nether:X:Y:Z:200:0:0:0:1:flat` 旧链接（Y 不参与平面定位）。

暂不支持：LZ4 区块压缩、旧版 `Level` 区块结构、GPU 后端、桌面客户端、增量缓存、对象存储上传和正式 `current.json` 发布命令。

## 快速验证（合成样本）

要求 Rust 1.85+、Python 3.10+、Node.js 20+。

```powershell
# 在仓库根目录生成可公开的合成快照，输出到已忽略的 .local-tests/
python fixtures/generate_synthetic_snapshot.py --out .local-tests/synthetic

cargo run -p tc-map -- inspect `
  --snapshot .local-tests/synthetic/snapshot `
  --config .local-tests/synthetic/map.toml

cargo run -p tc-map -- render `
  --snapshot .local-tests/synthetic/snapshot `
  --config .local-tests/synthetic/map.toml `
  --backend cpu --release demo-001

cargo run -p tc-map -- verify `
  --release-dir .local-tests/synthetic/output/staging/demo-001

cd web
npm ci
npm run build
node scripts/preview.mjs --map-root ../.local-tests/synthetic/output --port 4173
```

打开 `http://127.0.0.1:4173/?release=demo-001#world:5:65:5:200:0:0:0:1:flat`。样本包含跨原点的负坐标、下界屋顶、末地空区块、gzip 与 zlib、外置 `.mcc`、需要父层的远距离区块和未知方块回退。预览服务器优先读取 `output/releases/<id>`，找不到时读取 `output/staging/<id>`。

本地检查：

```powershell
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cd web; npm run build
```

这些检查目前不包含真实存档兼容性或 GPU 验证；CI 也尚未建立。

## 真实快照

复制根目录的 `map.example.toml` 为本地配置，按实际快照填写维度路径、输出目录、样式和标记文件。`tc-map` 的 `--snapshot` 指向快照根目录；配置里的 `path` 是相对快照的 region 目录，省略时按维度自动探测。不要提交私有存档、输出目录或标记源文件。

```toml
version = 1
style = "styles/default.toml"
# markers = "markers.public.json"

[output]
dir = "output"

[render]
threads = 4

[dimensions.world]
layer = "surface"

[dimensions.world_nether]
layer = { below_y = 100 }

[dimensions.world_the_end]
layer = "surface"
```

当前只支持网站维度 ID `world`、`world_nether`、`world_the_end`（也接受 `overworld`、`nether`、`the_end` 别名）。下界必须显式选择 `surface` 或 `below_y`，程序不会猜测交通层。

## 下一步

1. 用真实 TongCraft 导出抽查三个维度和负坐标，记录 DataVersion、压缩方式、下界层和已知机器位置；未过关不进入 GPU。
2. 在真实渲染机上比较 CPU/GPU 解码像素、端到端墙钟和峰值内存；GPU 未验证前不承诺加速倍率。
3. 补 Tauri 桌面端、增量缓存与原子发布，再决定 R2 或继续静态目录。

## 设计入口

| 想了解什么 | 从哪里读 |
| --- | --- |
| 首版承诺什么、怎么验收 | [产品意图与首版范围](docs/product.md) |
| 从世界快照到玩家网页的数据流 | [架构概览（含总览图）](docs/architecture.md) |
| 存档格式、坐标、瓦片和发布协议 | [v1 详细设计](docs/design-v1.md) |
| 已定的桌面与核心分工 | [桌面客户端和 CLI 共用 Rust 渲染核心](docs/decisions/2026-11-20-desktop-renderer.md) |
| 待定的离线技术方案 | [离线地图技术选型提案](docs/decisions/2026-11-20-offline-map-stack.md) |

参与开发前请读[协作约定](AGENTS.md)。私有世界快照放在仓库外或已忽略的 `snapshots/`，本地渲染产物放在 `output/`，不要把这些文件提交到仓库。
