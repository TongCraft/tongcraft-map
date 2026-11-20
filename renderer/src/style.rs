use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    fn parse(value: &str, context: &str) -> Result<Self> {
        let hex = value.strip_prefix('#').unwrap_or(value);
        if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::Config(format!(
                "{context} 必须是 #RRGGBB 颜色，收到 {value}"
            )));
        }
        let r = u8::from_str_radix(&hex[0..2], 16)
            .map_err(|_| Error::Config(format!("{context} 颜色无效：{value}")))?;
        let g = u8::from_str_radix(&hex[2..4], 16)
            .map_err(|_| Error::Config(format!("{context} 颜色无效：{value}")))?;
        let b = u8::from_str_radix(&hex[4..6], 16)
            .map_err(|_| Error::Config(format!("{context} 颜色无效：{value}")))?;
        Ok(Self { r, g, b })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StyleFile {
    version: String,
    fallback: String,
    water: String,
    #[serde(default = "default_water_alpha_min")]
    water_alpha_min: u8,
    #[serde(default = "default_water_alpha_step")]
    water_alpha_step: u8,
    #[serde(default = "default_water_alpha_max")]
    water_alpha_max: u8,
    blocks: BTreeMap<String, String>,
    #[serde(default)]
    skip: Vec<String>,
    #[serde(default)]
    water_blocks: Vec<String>,
}

fn default_water_alpha_min() -> u8 {
    90
}

fn default_water_alpha_step() -> u8 {
    18
}

fn default_water_alpha_max() -> u8 {
    220
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Skip,
    Water,
    Known(u16),
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Style {
    pub version: String,
    pub fallback: Color,
    pub water: Color,
    pub water_alpha_min: u8,
    pub water_alpha_step: u8,
    pub water_alpha_max: u8,
    pub water_material: u16,
    materials: Vec<Color>,
    names: HashMap<String, Class>,
    skip: HashSet<String>,
    water_blocks: HashSet<String>,
}

impl Style {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| Error::io(path, source))?;
        let file: StyleFile = toml::from_str(&text).map_err(|source| Error::Toml {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        if file.version.trim().is_empty() {
            return Err(Error::Config("样式版本不能为空".to_owned()));
        }
        if file.water_alpha_min > file.water_alpha_max {
            return Err(Error::Config(format!(
                "样式 water_alpha_min ({}) 不能大于 water_alpha_max ({})",
                file.water_alpha_min, file.water_alpha_max
            )));
        }
        if file.blocks.is_empty() {
            return Err(Error::Config("样式 blocks 不能为空".to_owned()));
        }
        if file.blocks.len() + 2 > u16::MAX as usize {
            return Err(Error::Config(format!(
                "样式方块数 {} 超过 16 位材质号上限",
                file.blocks.len()
            )));
        }

        let fallback = Color::parse(&file.fallback, "fallback")?;
        let water = Color::parse(&file.water, "water")?;
        let mut materials = Vec::with_capacity(file.blocks.len() + 2);
        let mut names = HashMap::new();
        for (index, (name, value)) in file.blocks.iter().enumerate() {
            let name = normalize_name(name);
            if names.contains_key(&name) {
                return Err(Error::Config(format!("样式重复定义方块 {name}")));
            }
            let color = Color::parse(value, &format!("blocks.{name}"))?;
            materials.push(color);
            names.insert(name, Class::Known(index as u16));
        }
        materials.push(fallback);
        let water_material = materials.len() as u16;
        materials.push(water);

        let mut skip = HashSet::new();
        for name in ["air", "cave_air", "void_air"] {
            skip.insert(format!("minecraft:{name}"));
        }
        for name in file.skip {
            skip.insert(normalize_name(&name));
        }
        let mut water_blocks = HashSet::new();
        for name in ["water", "flowing_water", "bubble_column"] {
            water_blocks.insert(format!("minecraft:{name}"));
        }
        for name in file.water_blocks {
            water_blocks.insert(normalize_name(&name));
        }

        for name in &skip {
            names.remove(name);
        }
        for name in &water_blocks {
            names.insert(name.clone(), Class::Water);
        }

        Ok(Self {
            version: file.version,
            fallback,
            water,
            water_alpha_min: file.water_alpha_min,
            water_alpha_step: file.water_alpha_step,
            water_alpha_max: file.water_alpha_max,
            water_material,
            materials,
            names,
            skip,
            water_blocks,
        })
    }

    pub fn classify(&self, name: &str, snowy: bool) -> Class {
        let name = normalize_name(name);
        if snowy && name == "minecraft:grass_block" {
            if let Some(Class::Known(id)) = self.names.get("minecraft:snow_block") {
                return Class::Known(*id);
            }
        }
        if self.skip.contains(&name) {
            return Class::Skip;
        }
        if self.water_blocks.contains(&name) {
            return Class::Water;
        }
        self.names.get(&name).copied().unwrap_or(Class::Unknown)
    }

    pub fn fallback_material(&self) -> u16 {
        self.materials.len() as u16 - 2
    }

    pub fn color(&self, material: u16) -> Color {
        self.materials
            .get(material as usize)
            .copied()
            .unwrap_or(self.fallback)
    }

    pub fn is_water_material(&self, material: u16) -> bool {
        material == self.water_material
    }
}

fn normalize_name(name: &str) -> String {
    let name = name.trim();
    if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    }
}
