use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::util::{
    canonical_for_compare, is_inside, valid_dimension_id, validate_snapshot_relative,
};

const DEFAULT_STYLE: &str = "styles/default.toml";
const DEFAULT_MAX_LEVEL: u32 = 8;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub output: OutputConfig,
    #[serde(default)]
    pub render: RenderConfig,
    #[serde(default = "default_style")]
    pub style: PathBuf,
    pub markers: Option<PathBuf>,
    pub dimensions: BTreeMap<String, DimensionConfig>,
}

fn default_style() -> PathBuf {
    PathBuf::from(DEFAULT_STYLE)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputConfig {
    pub dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RenderConfig {
    pub threads: Option<usize>,
    pub max_level: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DimensionConfig {
    /// Optional snapshot-relative region directory. Auto-detected when absent.
    pub path: Option<PathBuf>,
    pub layer: LayerConfig,
    /// Optional clip in block coordinates: [minX, minZ, maxXExclusive, maxZExclusive].
    pub bounds: Option<[i32; 4]>,
    pub max_level: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum LayerConfig {
    Named(String),
    BelowY { below_y: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArchiveDimension {
    Overworld,
    Nether,
    End,
}

impl ArchiveDimension {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "world" | "overworld" => Some(Self::Overworld),
            "world_nether" | "nether" | "the_nether" => Some(Self::Nether),
            "world_the_end" | "end" | "the_end" => Some(Self::End),
            _ => None,
        }
    }

    pub fn modern_region(self) -> &'static str {
        match self {
            Self::Overworld => "dimensions/minecraft/overworld/region",
            Self::Nether => "dimensions/minecraft/the_nether/region",
            Self::End => "dimensions/minecraft/the_end/region",
        }
    }

    pub fn legacy_region(self) -> &'static str {
        match self {
            Self::Overworld => "region",
            Self::Nether => "DIM-1/region",
            Self::End => "DIM1/region",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    Modern,
    Legacy,
    Explicit,
}

impl LayoutKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Modern => "modern",
            Self::Legacy => "legacy",
            Self::Explicit => "explicit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Surface,
    BelowY(i32),
}

impl Layer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Surface => "surface",
            Self::BelowY(_) => "below_y",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DimensionSpec {
    pub id: String,
    pub archive: ArchiveDimension,
    pub region_dir: PathBuf,
    pub layout: LayoutKind,
    pub layer: Layer,
    pub bounds: Option<[i32; 4]>,
    pub max_level: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub config: Config,
    pub config_dir: PathBuf,
    pub snapshot: PathBuf,
    pub output_dir: PathBuf,
    pub style_path: PathBuf,
    pub markers_path: Option<PathBuf>,
    pub dimensions: Vec<DimensionSpec>,
}

impl Project {
    pub fn load(snapshot: &Path, config_path: &Path) -> Result<Self> {
        let config_text = std::fs::read_to_string(config_path)
            .map_err(|source| Error::io(config_path, source))?;
        let config: Config = toml::from_str(&config_text).map_err(|source| Error::Toml {
            path: config_path.to_path_buf(),
            source: Box::new(source),
        })?;
        if config.version != 1 {
            return Err(Error::Config(format!(
                "不支持的配置版本 {}，当前只支持 1",
                config.version
            )));
        }
        if let Some(threads) = config.render.threads {
            if threads == 0 || threads > 1024 {
                return Err(Error::Config("render.threads 必须在 1..=1024".to_owned()));
            }
        }
        if let Some(max_level) = config.render.max_level {
            validate_level(max_level, "render.max_level")?;
        }

        let config_path = canonical_for_compare(config_path);
        let config_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let snapshot = snapshot
            .canonicalize()
            .map_err(|source| Error::io(snapshot, source))?;
        if !snapshot.is_dir() {
            return Err(Error::Snapshot(format!(
                "快照目录不存在：{}",
                snapshot.display()
            )));
        }

        let output_dir = resolve_from(&config_dir, &config.output.dir);
        if is_inside(&output_dir, &snapshot) || is_inside(&snapshot, &output_dir) {
            return Err(Error::Config(format!(
                "输出目录不能与快照目录重叠：{}",
                output_dir.display()
            )));
        }
        let style_path = resolve_from(&config_dir, &config.style);
        if !style_path.is_file() {
            return Err(Error::Config(format!(
                "样式文件不存在：{}",
                style_path.display()
            )));
        }
        let markers_path = config
            .markers
            .as_ref()
            .map(|path| resolve_from(&config_dir, path));
        if let Some(path) = &markers_path {
            if !path.is_file() {
                return Err(Error::Config(format!("标记文件不存在：{}", path.display())));
            }
        }
        if config.dimensions.is_empty() {
            return Err(Error::Config("至少需要配置一个维度".to_owned()));
        }

        let mut seen_archives = BTreeSet::new();
        let mut dimensions = Vec::with_capacity(config.dimensions.len());
        for (id, dimension) in &config.dimensions {
            if !valid_dimension_id(id) {
                return Err(Error::Config(format!(
                    "维度 ID 只能包含字母、数字、下划线和连字符：{id}"
                )));
            }
            let archive = ArchiveDimension::from_id(id).ok_or_else(|| {
                Error::Config(format!(
                    "未知维度 ID {id}；当前支持 world、world_nether、world_the_end（以及 overworld、nether、the_end 别名）"
                ))
            })?;
            if !seen_archives.insert(archive) {
                return Err(Error::Config(format!(
                    "维度 ID {id} 与另一个配置项指向同一个存档维度；不要重复配置"
                )));
            }
            if let Some(bounds) = dimension.bounds {
                if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
                    return Err(Error::Config(format!(
                        "维度 {id} 的 bounds 必须是 [minX,minZ,maxXExclusive,maxZExclusive] 且宽高为正"
                    )));
                }
            }
            if let Some(max_level) = dimension.max_level {
                validate_level(max_level, &format!("维度 {id} 的 max_level"))?;
            }
            let layer = match &dimension.layer {
                LayerConfig::Named(name) => match name.as_str() {
                    "surface" => Layer::Surface,
                    other => {
                        return Err(Error::Config(format!(
                            "维度 {id} 的 layer 只支持 surface 或 {{ below_y = <Y> }}，收到 {other}"
                        )));
                    }
                },
                LayerConfig::BelowY { below_y } => {
                    if !(-2048..=2047).contains(below_y) {
                        return Err(Error::Config(format!(
                            "维度 {id} 的 below_y 超出有效高度范围：{below_y}"
                        )));
                    }
                    Layer::BelowY(*below_y)
                }
            };
            let (region_dir, layout) = match &dimension.path {
                Some(path) => {
                    validate_snapshot_relative(path)?;
                    let dir = snapshot.join(path);
                    let dir = dir.canonicalize().map_err(|source| {
                        Error::Snapshot(format!(
                            "维度 {id} 的路径无法访问（{}）：{source}",
                            dir.display()
                        ))
                    })?;
                    if !dir.is_dir() {
                        return Err(Error::Snapshot(format!(
                            "维度 {id} 的路径不是目录：{}",
                            dir.display()
                        )));
                    }
                    if !dir.starts_with(&snapshot) {
                        return Err(Error::Config(format!(
                            "维度 {id} 的路径越出快照目录：{}",
                            path.display()
                        )));
                    }
                    (dir, LayoutKind::Explicit)
                }
                None => detect_layout(&snapshot, archive, id)?,
            };
            dimensions.push(DimensionSpec {
                id: id.clone(),
                archive,
                region_dir,
                layout,
                layer,
                bounds: dimension.bounds,
                max_level: dimension.max_level,
            });
        }

        Ok(Self {
            config,
            config_dir,
            snapshot,
            output_dir,
            style_path,
            markers_path,
            dimensions,
        })
    }

    pub fn effective_max_level(&self, dimension: &DimensionSpec) -> u32 {
        dimension
            .max_level
            .or(self.config.render.max_level)
            .unwrap_or(DEFAULT_MAX_LEVEL)
    }

    pub fn threads(&self) -> usize {
        self.config
            .render
            .threads
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
    }
}

fn resolve_from(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn validate_level(level: u32, field: &str) -> Result<()> {
    if level > 12 {
        return Err(Error::Config(format!(
            "{field} 不能超过 12（当前 {level}）"
        )));
    }
    Ok(())
}

fn detect_layout(
    snapshot: &Path,
    archive: ArchiveDimension,
    id: &str,
) -> Result<(PathBuf, LayoutKind)> {
    let modern = snapshot.join(archive.modern_region());
    let legacy = snapshot.join(archive.legacy_region());
    let modern_exists = modern.is_dir();
    let legacy_exists = legacy.is_dir();
    match (modern_exists, legacy_exists) {
        (true, true) => Err(Error::Snapshot(format!(
            "维度 {id} 同时存在 26.1 与旧版目录（{}、{}）；请用 path 显式选择，不能自动合并",
            archive.modern_region(),
            archive.legacy_region()
        ))),
        (true, false) => {
            let canonical = modern
                .canonicalize()
                .map_err(|source| Error::io(&modern, source))?;
            Ok((canonical, LayoutKind::Modern))
        }
        (false, true) => {
            let canonical = legacy
                .canonicalize()
                .map_err(|source| Error::io(&legacy, source))?;
            Ok((canonical, LayoutKind::Legacy))
        }
        (false, false) => Err(Error::Snapshot(format!(
            "维度 {id} 找不到区域目录；期望 {} 或 {}",
            archive.modern_region(),
            archive.legacy_region()
        ))),
    }
}
