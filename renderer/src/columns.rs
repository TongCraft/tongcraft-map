/// Two words per output column, matching the CPU/GPU input contract:
/// - `data0`: material id (low 16), water depth (next 8), flags (high 8)
/// - `data1`: signed height
///
/// Missing and generated-but-empty columns are distinct. Neither may be
/// represented as height 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Column {
    data0: u32,
    data1: u32,
}

const FLAG_GENERATED: u8 = 1 << 0;
const FLAG_VALID: u8 = 1 << 1;

impl Column {
    pub const fn missing() -> Self {
        Self { data0: 0, data1: 0 }
    }

    pub const fn empty() -> Self {
        Self {
            data0: (FLAG_GENERATED as u32) << 24,
            data1: 0,
        }
    }

    pub fn solid(material: u16, water_depth: u8, height: i32) -> Self {
        let flags = FLAG_GENERATED | FLAG_VALID;
        Self {
            data0: material as u32 | ((water_depth as u32) << 16) | ((flags as u32) << 24),
            data1: height as u32,
        }
    }

    #[allow(dead_code)]
    pub fn is_missing(self) -> bool {
        self.flags() & FLAG_GENERATED == 0
    }

    pub fn is_valid(self) -> bool {
        self.flags() & FLAG_VALID != 0
    }

    pub fn material(self) -> u16 {
        (self.data0 & 0xFFFF) as u16
    }

    pub fn water_depth(self) -> u8 {
        ((self.data0 >> 16) & 0xFF) as u8
    }

    pub fn height(self) -> i32 {
        self.data1 as i32
    }

    fn flags(self) -> u8 {
        (self.data0 >> 24) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::Column;

    #[test]
    fn missing_and_empty_are_not_confused_with_ground() {
        let missing = Column::missing();
        let empty = Column::empty();
        let ground = Column::solid(7, 2, 64);

        assert!(missing.is_missing());
        assert!(!missing.is_valid());
        assert!(!empty.is_missing());
        assert!(!empty.is_valid());
        assert!(!ground.is_missing());
        assert!(ground.is_valid());
        assert_eq!(ground.material(), 7);
        assert_eq!(ground.water_depth(), 2);
        assert_eq!(ground.height(), 64);
    }
}
