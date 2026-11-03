use super::{Biome, TreeScale};
use crate::block::BlockType;

// Ground this far over the sea or lower is mud flat, not grass.
const MUD_BELOW: i64 = 2;

/// Flat, low, wet ground by fresh water: a fair number of broadleaf trees, grass that reads wetter than
/// the climate, mud on the lowest flats and clay where a spring rises.
pub struct Swamp;

impl Biome for Swamp {
    fn name(&self) -> &'static str {
        "swamp"
    }

    fn tree_density(&self) -> Option<f64> {
        Some(0.12)
    }

    fn spring_block(&self) -> BlockType {
        BlockType::Clay
    }

    fn climate_shift(&self) -> (f32, f32) {
        (0.0, 0.20)
    }

    fn tree_scale(&self) -> TreeScale {
        TreeScale::default()
    }

    fn block(&self, base: BlockType, above_sea: i64, _gradient: f64) -> BlockType {
        match base {
            BlockType::Grass if above_sea <= MUD_BELOW => BlockType::Dirt,
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_lowest_flats_are_mud_and_higher_ground_keeps_its_grass() {
        assert_eq!(BlockType::Dirt, Swamp.block(BlockType::Grass, 1, 0.0));
        assert_eq!(BlockType::Dirt, Swamp.block(BlockType::Grass, 2, 0.0));
        assert_eq!(BlockType::Grass, Swamp.block(BlockType::Grass, 6, 0.0));
        assert_eq!(BlockType::Water, Swamp.block(BlockType::Water, 1, 0.0));
        assert_eq!(BlockType::Clay, Swamp.spring_block());
    }
}
