use super::{Biome, TreeScale};
use crate::block::BlockType;
use crate::features::ReliefKind;

/// A biome that sets how thickly trees grow, how big they get and how lush or dry the grass reads: a
/// world map picks it, and the ground keeps the base terrain's blocks.
pub struct Plain {
    name: &'static str,
    trees: f64,
    shift: (f32, f32),
    scale: TreeScale,
    relief: &'static [ReliefKind],
    // the height over the sea below which snow thaws, and the gradient from which the ground is gravel, not grass
    thaw: Option<(i64, f64)>,
    frost: f32,
}

impl Plain {
    pub fn new(name: &'static str, trees: f64) -> Self {
        Self {
            name,
            trees,
            shift: (0.0, 0.0),
            scale: TreeScale::default(),
            relief: &[],
            thaw: None,
            frost: 1.0,
        }
    }

    /// Reads `temperature` warmer and `humidity` wetter, in grass and flora tint.
    pub fn shifted(mut self, temperature: f32, humidity: f32) -> Self {
        self.shift = (temperature, humidity);
        self
    }

    /// Grows trees `height`, `spread` and `squash` times the usual.
    pub fn scaled(mut self, height: f64, spread: f64, squash: f64) -> Self {
        self.scale = TreeScale {
            height,
            spread,
            squash,
            giants: self.scale.giants,
        };
        self
    }

    /// Lays `relief` over the biome's ground.
    pub fn with_relief(mut self, relief: &'static [ReliefKind]) -> Self {
        self.relief = relief;
        self
    }

    /// Bares the ground below `line` blocks over the sea that the climate would snow over: grass, and gravel on ground
    /// with a gradient of `scree` or more. Snow stays on the heights.
    pub fn with_thaw(mut self, line: i64, scree: f64) -> Self {
        self.thaw = Some((line, scree));
        self
    }

    /// Keeps `share` of the frost the cold would give the ground, 0 to 1.
    pub fn with_frost(mut self, share: f32) -> Self {
        self.frost = share;
        self
    }

    /// Lets `share` of the biome's broadleaf trees grow as emergent giants.
    pub fn with_giants(mut self, share: f64) -> Self {
        self.scale.giants = share;
        self
    }
}

impl Biome for Plain {
    fn name(&self) -> &'static str {
        self.name
    }

    fn tree_density(&self) -> Option<f64> {
        Some(self.trees)
    }

    fn climate_shift(&self) -> (f32, f32) {
        self.shift
    }

    fn tree_scale(&self) -> TreeScale {
        self.scale
    }

    fn frost(&self) -> f32 {
        self.frost
    }

    fn relief(&self) -> &'static [ReliefKind] {
        self.relief
    }

    fn block(&self, base: BlockType, above_sea: i64, gradient: f64) -> BlockType {
        match (self.thaw, base) {
            (Some((line, scree)), BlockType::Snow) if above_sea < line => {
                if gradient >= scree {
                    BlockType::Gravel
                } else {
                    BlockType::Grass
                }
            }
            _ => base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_thawed_biome_has_grass_and_scree_low_down_and_keeps_its_snow_high_up() {
        let tundra = Plain::new("tundra", 0.0).with_thaw(300, 0.45);
        assert_eq!(BlockType::Grass, tundra.block(BlockType::Snow, 30, 0.1));
        assert_eq!(BlockType::Gravel, tundra.block(BlockType::Snow, 30, 0.6));
        assert_eq!(BlockType::Snow, tundra.block(BlockType::Snow, 300, 0.1));
        // only snow thaws
        assert_eq!(BlockType::Rock, tundra.block(BlockType::Rock, 30, 3.0));
        assert_eq!(BlockType::Snow, Plain::new("glacier", 0.0).block(BlockType::Snow, 30, 0.1));
    }
}
