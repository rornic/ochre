use super::Biome;
use crate::block::BlockType;

// Ground steeper than this is bare rock: on volcanic ground meadow clings only to the gentle slopes.
const BARE_GRADIENT: f64 = 0.5;

/// High ground on a volcanic province, cones and ranges of lava: the rock there is basalt, bare on
/// everything but the gentlest slopes, with ash for the sand and hardly a tree.
pub struct Volcanic;

impl Biome for Volcanic {
    fn name(&self) -> &'static str {
        "volcanic"
    }

    fn spring_block(&self) -> BlockType {
        BlockType::Stone
    }

    fn tree_density(&self) -> Option<f64> {
        Some(0.01)
    }

    fn block(&self, base: BlockType, _above_sea: i64, gradient: f64) -> BlockType {
        match base {
            BlockType::Grass if gradient >= BARE_GRADIENT => BlockType::Stone,
            BlockType::Sand => BlockType::Gravel,
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volcanic_slopes_are_bare_rock_and_gentle_ground_keeps_its_meadow() {
        assert_eq!(BlockType::Stone, Volcanic.block(BlockType::Grass, 200, 0.8));
        assert_eq!(BlockType::Grass, Volcanic.block(BlockType::Grass, 200, 0.2));
        assert_eq!(BlockType::Gravel, Volcanic.block(BlockType::Sand, 200, 0.2));
        assert_eq!(BlockType::Snow, Volcanic.block(BlockType::Snow, 900, 0.8));
        assert_eq!(BlockType::Water, Volcanic.block(BlockType::Water, 5, 0.0));
    }
}
