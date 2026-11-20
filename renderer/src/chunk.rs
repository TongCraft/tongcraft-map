use std::collections::BTreeMap;

use fastnbt::Value;

use crate::columns::Column;
use crate::error::{chunk_error, Result};
use crate::style::{Class, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkState {
    Missing,
    NotFull,
    Full,
}

#[derive(Debug)]
pub struct ChunkColumns {
    pub state: ChunkState,
    pub columns: [Column; 256],
    pub unknown: BTreeMap<String, u64>,
}

impl ChunkColumns {
    pub fn missing() -> Self {
        Self {
            state: ChunkState::Missing,
            columns: [Column::missing(); 256],
            unknown: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
struct Section {
    y: i32,
    palette: Vec<(String, bool)>,
    classes: Vec<Class>,
    data: Option<Vec<i64>>,
    bits: u32,
    entries_per_long: usize,
}

pub fn extract_columns(
    dimension: &str,
    chunk_x: i32,
    chunk_z: i32,
    nbt: &BTreeMap<String, Value>,
    style: &Style,
    max_y: i32,
) -> Result<ChunkColumns> {
    let _data_version = nbt
        .get("DataVersion")
        .and_then(Value::as_i64)
        .ok_or_else(|| {
            chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                "缺少 DataVersion；不能假定所有区块使用同一格式",
            )
        })? as i32;

    let status = match nbt.get("Status") {
        Some(Value::String(value)) => value.as_str(),
        Some(_) => {
            return Err(chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                "Status 不是字符串",
            ));
        }
        None => {
            return Err(chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                "缺少 Status；旧格式区块尚未支持",
            ));
        }
    };
    if status != "minecraft:full" {
        return Ok(ChunkColumns {
            state: ChunkState::NotFull,
            columns: [Column::missing(); 256],
            unknown: BTreeMap::new(),
        });
    }

    let sections_value = match nbt.get("sections") {
        Some(Value::List(sections)) => sections,
        Some(_) => {
            return Err(chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                "sections 不是列表",
            ));
        }
        None => {
            let message = if nbt.contains_key("Level") {
                "检测到旧版 Level 区块结构；当前只支持 1.18+ 根 sections"
            } else {
                "缺少 sections；无法确定区块内容"
            };
            return Err(chunk_error(dimension, chunk_x, chunk_z, message));
        }
    };
    if sections_value.len() > 256 {
        return Err(chunk_error(
            dimension,
            chunk_x,
            chunk_z,
            format!("sections 数量异常：{}", sections_value.len()),
        ));
    }

    let mut sections = Vec::with_capacity(sections_value.len());
    for (index, value) in sections_value.iter().enumerate() {
        let Value::Compound(section) = value else {
            return Err(chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                format!("sections[{index}] 不是 compound"),
            ));
        };
        let y = section.get("Y").and_then(Value::as_i64).ok_or_else(|| {
            chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                format!("sections[{index}] 缺少 Y"),
            )
        })?;
        if !(-2048..=2047).contains(&y) {
            return Err(chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                format!("sections[{index}] 的 Y 超出范围：{y}"),
            ));
        }
        let y = y as i32;
        if sections.iter().any(|section: &Section| section.y == y) {
            return Err(chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                format!("section Y={y} 重复"),
            ));
        }

        let Some(block_states_value) = section.get("block_states") else {
            sections.push(Section {
                y,
                palette: Vec::new(),
                classes: Vec::new(),
                data: None,
                bits: 0,
                entries_per_long: 1,
            });
            continue;
        };
        let Value::Compound(block_states) = block_states_value else {
            return Err(chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                format!("section Y={y} 的 block_states 不是 compound"),
            ));
        };
        let palette_value = block_states.get("palette").ok_or_else(|| {
            chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                format!("section Y={y} 缺少 block_states.palette"),
            )
        })?;
        let Value::List(palette_values) = palette_value else {
            return Err(chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                format!("section Y={y} 的 palette 不是列表"),
            ));
        };
        if palette_values.is_empty() || palette_values.len() > 4096 {
            return Err(chunk_error(
                dimension,
                chunk_x,
                chunk_z,
                format!(
                    "section Y={y} 的 palette 数量异常：{}",
                    palette_values.len()
                ),
            ));
        }

        let mut palette = Vec::with_capacity(palette_values.len());
        let mut classes = Vec::with_capacity(palette_values.len());
        for (palette_index, value) in palette_values.iter().enumerate() {
            let Value::Compound(entry) = value else {
                return Err(chunk_error(
                    dimension,
                    chunk_x,
                    chunk_z,
                    format!("section Y={y} 的 palette[{palette_index}] 不是 compound"),
                ));
            };
            let name = entry.get("Name").and_then(|value| match value {
                Value::String(name) => Some(name.as_str()),
                _ => None,
            });
            let Some(name) = name else {
                return Err(chunk_error(
                    dimension,
                    chunk_x,
                    chunk_z,
                    format!("section Y={y} 的 palette[{palette_index}] 缺少 Name"),
                ));
            };
            let snowy = entry
                .get("Properties")
                .and_then(|value| match value {
                    Value::Compound(properties) => properties.get("snowy"),
                    _ => None,
                })
                .and_then(|value| match value {
                    Value::String(value) => Some(value == "true"),
                    _ => None,
                })
                .unwrap_or(false);
            palette.push((name.to_owned(), snowy));
            classes.push(style.classify(name, snowy));
        }

        let bits = if palette.len() <= 1 {
            0
        } else {
            (usize::BITS - (palette.len() - 1).leading_zeros()).max(4)
        };
        let entries_per_long = if bits == 0 { 1 } else { 64 / bits as usize };
        let expected_len = if bits == 0 {
            0
        } else {
            4096_usize.div_ceil(entries_per_long)
        };
        let data = match block_states.get("data") {
            Some(Value::LongArray(data)) => {
                if bits != 0 && data.len() < expected_len {
                    return Err(chunk_error(
                        dimension,
                        chunk_x,
                        chunk_z,
                        format!(
                            "section Y={y} 的 data 长度 {} 小于期望 {expected_len}",
                            data.len()
                        ),
                    ));
                }
                if data.len() > 8192 {
                    return Err(chunk_error(
                        dimension,
                        chunk_x,
                        chunk_z,
                        format!("section Y={y} 的 data 异常过长：{}", data.len()),
                    ));
                }
                Some(data.to_vec())
            }
            Some(_) => {
                return Err(chunk_error(
                    dimension,
                    chunk_x,
                    chunk_z,
                    format!("section Y={y} 的 data 不是 LongArray"),
                ));
            }
            None => {
                if bits != 0 {
                    return Err(chunk_error(
                        dimension,
                        chunk_x,
                        chunk_z,
                        format!("section Y={y} 有多个 palette 但缺少 data"),
                    ));
                }
                None
            }
        };

        sections.push(Section {
            y,
            palette,
            classes,
            data,
            bits,
            entries_per_long,
        });
    }

    sections.sort_by_key(|section| std::cmp::Reverse(section.y));

    let mut columns = [Column::empty(); 256];
    let mut resolved = [false; 256];
    let mut water_count = [0_u16; 256];
    let mut water_top = [0_i32; 256];
    let mut unknown = BTreeMap::new();

    for section in &sections {
        if section.palette.is_empty() {
            continue;
        }
        let section_base = section.y * 16;
        let local_max = (max_y - section_base).clamp(-1, 15);
        if local_max < 0 {
            continue;
        }
        for local_y in (0..=local_max).rev() {
            let world_y = section_base + local_y;
            for local_z in 0..16 {
                for local_x in 0..16 {
                    let column_index = (local_z * 16 + local_x) as usize;
                    if resolved[column_index] {
                        continue;
                    }
                    let block_index = ((local_y * 16 + local_z) * 16 + local_x) as usize;
                    let palette_index = section_value(section, block_index).ok_or_else(|| {
                        chunk_error(
                            dimension,
                            chunk_x,
                            chunk_z,
                            format!(
                                "section Y={} 的 palette 索引越界（block {block_index}）",
                                section.y
                            ),
                        )
                    })?;
                    if palette_index >= section.palette.len() {
                        return Err(chunk_error(
                            dimension,
                            chunk_x,
                            chunk_z,
                            format!(
                                "section Y={} 的 palette 索引 {} 超出调色板长度 {}",
                                section.y,
                                palette_index,
                                section.palette.len()
                            ),
                        ));
                    }
                    let class = section.classes[palette_index];
                    match class {
                        Class::Skip => {}
                        Class::Water => {
                            if water_count[column_index] == 0 {
                                water_top[column_index] = world_y;
                            }
                            water_count[column_index] = water_count[column_index].saturating_add(1);
                        }
                        Class::Known(material) => {
                            let depth = water_count[column_index].min(255) as u8;
                            columns[column_index] = Column::solid(material, depth, world_y);
                            resolved[column_index] = true;
                        }
                        Class::Unknown => {
                            let (name, _) = &section.palette[palette_index];
                            *unknown.entry(name.clone()).or_insert(0) += 1;
                            let depth = water_count[column_index].min(255) as u8;
                            columns[column_index] =
                                Column::solid(style.fallback_material(), depth, world_y);
                            resolved[column_index] = true;
                        }
                    }
                }
            }
        }
    }

    for index in 0..256 {
        if !resolved[index] && water_count[index] > 0 {
            columns[index] = Column::solid(
                style.water_material,
                water_count[index].min(255) as u8,
                water_top[index],
            );
        }
    }

    Ok(ChunkColumns {
        state: ChunkState::Full,
        columns,
        unknown,
    })
}

fn section_value(section: &Section, block_index: usize) -> Option<usize> {
    if section.palette.len() == 1 {
        return Some(0);
    }
    let data = section.data.as_ref()?;
    let entries_per_long = section.entries_per_long;
    let long_index = block_index / entries_per_long;
    let offset = (block_index % entries_per_long) * section.bits as usize;
    let value = (*data.get(long_index)? as u64 >> offset) & ((1_u64 << section.bits) - 1);
    Some(value as usize)
}
