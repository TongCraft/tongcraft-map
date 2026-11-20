use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::util::read_json;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marker {
    pub id: String,
    pub dimension: String,
    pub name: String,
    pub x: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    pub z: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub products: Vec<String>,
}

pub struct Markers {
    pub markers: Vec<Marker>,
    pub warnings: Vec<String>,
}

pub fn load_markers(path: Option<&Path>, dimensions: &HashSet<String>) -> Result<Markers> {
    let Some(path) = path else {
        return Ok(Markers {
            markers: Vec::new(),
            warnings: Vec::new(),
        });
    };
    let value: serde_json::Value = read_json(path)?;
    let Some(array) = value.as_array() else {
        return Err(crate::error::Error::Config(format!(
            "标记文件 {} 的根必须是数组",
            path.display()
        )));
    };

    let mut markers = Vec::new();
    let mut warnings = Vec::new();
    let mut ids = HashSet::new();
    for (index, value) in array.iter().enumerate() {
        let Some(object) = value.as_object() else {
            warnings.push(format!("标记[{index}] 不是对象，已跳过"));
            continue;
        };
        let id = object.get("id").and_then(|value| value.as_str());
        let dimension = object.get("dimension").and_then(|value| value.as_str());
        let name = object.get("name").and_then(|value| value.as_str());
        let x = object.get("x").and_then(|value| value.as_f64());
        let z = object.get("z").and_then(|value| value.as_f64());
        let (Some(id), Some(dimension), Some(name), Some(x), Some(z)) = (id, dimension, name, x, z)
        else {
            warnings.push(format!("标记[{index}] 缺少 id/dimension/name/x/z，已跳过"));
            continue;
        };
        if !x.is_finite() || !z.is_finite() {
            warnings.push(format!("标记 {id} 的 x/z 不是有限数字，已跳过"));
            continue;
        }
        if !dimensions.contains(dimension) {
            warnings.push(format!(
                "标记 {id} 的维度 {dimension} 不在本次地图中，已跳过"
            ));
            continue;
        }
        if !ids.insert(id.to_owned()) {
            warnings.push(format!("标记 ID {id} 重复，已跳过后者"));
            continue;
        }
        let y = match object.get("y") {
            None | Some(serde_json::Value::Null) => None,
            Some(value) => match value.as_f64() {
                Some(y) if y.is_finite() => Some(y),
                _ => {
                    warnings.push(format!("标记 {id} 的 y 不是有限数字，已忽略高度"));
                    None
                }
            },
        };
        let products = object
            .get("products")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        markers.push(Marker {
            id: id.to_owned(),
            dimension: dimension.to_owned(),
            name: name.to_owned(),
            x,
            y,
            z,
            products,
        });
    }
    markers.sort_by(|left, right| {
        left.dimension
            .cmp(&right.dimension)
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(Markers { markers, warnings })
}
