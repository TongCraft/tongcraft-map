use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::util::{read_json, write_json};

#[derive(Debug, Clone, Default)]
pub struct Coverage {
    levels: BTreeMap<u32, BTreeSet<(i32, i32)>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoverageFile {
    schema: u32,
    dimension: String,
    levels: BTreeMap<String, BTreeMap<String, Vec<[i32; 2]>>>,
}

impl Coverage {
    pub fn insert(&mut self, level: u32, tx: i32, tz: i32) {
        self.levels.entry(level).or_default().insert((tx, tz));
    }

    #[allow(dead_code)]
    pub fn contains(&self, level: u32, tx: i32, tz: i32) -> bool {
        self.levels
            .get(&level)
            .is_some_and(|tiles| tiles.contains(&(tx, tz)))
    }

    pub fn level_tiles(&self, level: u32) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.levels
            .get(&level)
            .into_iter()
            .flat_map(|tiles| tiles.iter().copied())
    }

    pub fn level_count(&self, level: u32) -> usize {
        self.levels.get(&level).map_or(0, BTreeSet::len)
    }

    pub fn max_level(&self) -> u32 {
        self.levels.keys().next_back().copied().unwrap_or(0)
    }

    pub fn tile_count(&self) -> usize {
        self.levels.values().map(BTreeSet::len).sum()
    }

    pub fn level0_bounds(&self) -> Option<[i32; 4]> {
        let tiles = self.levels.get(&0)?;
        if tiles.is_empty() {
            return None;
        }
        let min_x = tiles.iter().map(|(x, _)| *x).min()?;
        let max_x = tiles.iter().map(|(x, _)| *x).max()?;
        let min_z = tiles.iter().map(|(_, z)| *z).min()?;
        let max_z = tiles.iter().map(|(_, z)| *z).max()?;
        Some([
            min_x * 256,
            min_z * 256,
            (max_x + 1) * 256,
            (max_z + 1) * 256,
        ])
    }

    pub fn to_json(&self, dimension: &str) -> serde_json::Value {
        let mut levels = BTreeMap::new();
        for (level, tiles) in &self.levels {
            let mut rows: BTreeMap<String, Vec<[i32; 2]>> = BTreeMap::new();
            let mut by_z: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
            for (tx, tz) in tiles {
                by_z.entry(*tz).or_default().push(*tx);
            }
            for (tz, mut xs) in by_z {
                xs.sort_unstable();
                let mut runs = Vec::new();
                let mut start = xs[0];
                let mut previous = start;
                for x in xs.into_iter().skip(1) {
                    if x == previous + 1 {
                        previous = x;
                        continue;
                    }
                    runs.push([start, previous + 1]);
                    start = x;
                    previous = x;
                }
                runs.push([start, previous + 1]);
                rows.insert(tz.to_string(), runs);
            }
            levels.insert(level.to_string(), rows);
        }
        serde_json::json!({
            "schema": 1,
            "dimension": dimension,
            "levels": levels,
        })
    }

    pub fn write(&self, path: &std::path::Path, dimension: &str) -> Result<()> {
        write_json(path, &self.to_json(dimension))
    }

    pub fn read(path: &std::path::Path) -> Result<(String, Self)> {
        let file: CoverageFile = read_json(path)?;
        if file.schema != 1 {
            return Err(Error::Verify(format!(
                "{} 的 coverage schema 不是 1",
                path.display()
            )));
        }
        let mut coverage = Self::default();
        for (level, rows) in file.levels {
            let level: u32 = level.parse().map_err(|_| {
                Error::Verify(format!(
                    "{} 的 coverage level 不是非负整数：{level}",
                    path.display()
                ))
            })?;
            for (tz, runs) in rows {
                let tz: i32 = tz.parse().map_err(|_| {
                    Error::Verify(format!("{} 的 coverage tz 不是整数：{tz}", path.display()))
                })?;
                let mut previous_end: Option<i32> = None;
                for [start, end] in runs {
                    if start >= end {
                        return Err(Error::Verify(format!(
                            "{} 的 coverage 区间无效：[{start},{end})",
                            path.display()
                        )));
                    }
                    if let Some(previous_end) = previous_end {
                        if start < previous_end {
                            return Err(Error::Verify(format!(
                                "{} 的 coverage 区间重叠或未排序",
                                path.display()
                            )));
                        }
                    }
                    previous_end = Some(end);
                    for tx in start..end {
                        coverage.insert(level, tx, tz);
                    }
                }
            }
        }
        Ok((file.dimension, coverage))
    }
}
