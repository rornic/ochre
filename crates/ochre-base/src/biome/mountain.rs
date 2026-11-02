use super::Biome;
use crate::block::BlockType;
use crate::math::smoothstep_f64;

// Ground steeper than this is bare rock, the limit falling with height from the first gradient at the
// first height over the sea to the second at the second.
const ROCK_GRADIENT: (f64, f64) = (1.6, 1.0);
const ROCK_ABOVE: (f64, f64) = (150.0, 600.0);

/// High, steep ground: meadow with bare rock on the steeper faces, more of it the higher it gets,
/// scree for the sand and a few scattered trees.
pub struct Mountain;

impl Biome for Mountain {
    fn name(&self) -> &'static str {
        "mountain"
    }

    fn spring_block(&self) -> BlockType {
        BlockType::Stone
    }

    fn tree_density(&self) -> Option<f64> {
        Some(0.04)
    }

    fn block(&self, base: BlockType, above_sea: i64, gradient: f64) -> BlockType {
        match base {
            BlockType::Grass if gradient >= rock_gradient(above_sea) => BlockType::Stone,
            BlockType::Sand => BlockType::Gravel,
            other => other,
        }
    }
}

fn rock_gradient(above_sea: i64) -> f64 {
    let t = smoothstep_f64(ROCK_ABOVE.0, ROCK_ABOVE.1, above_sea as f64);
    ROCK_GRADIENT.0 + (ROCK_GRADIENT.1 - ROCK_GRADIENT.0) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_steep_ground_is_rock_sooner_the_higher_it_is_and_gentle_ground_stays_meadow() {
        assert_eq!(BlockType::Stone, Mountain.block(BlockType::Grass, 120, 1.7));
        assert_eq!(BlockType::Grass, Mountain.block(BlockType::Grass, 300, 0.2));
        assert_eq!(BlockType::Grass, Mountain.block(BlockType::Grass, 300, 0.9));
        assert_eq!(BlockType::Stone, Mountain.block(BlockType::Grass, 800, 1.1));
        assert_eq!(BlockType::Grass, Mountain.block(BlockType::Grass, 200, 0.2));
        assert_eq!(BlockType::Grass, Mountain.block(BlockType::Grass, 120, 0.3));
        assert_eq!(BlockType::Gravel, Mountain.block(BlockType::Sand, 120, 0.3));
        assert_eq!(BlockType::Snow, Mountain.block(BlockType::Snow, 220, 0.3));
        assert_eq!(BlockType::Water, Mountain.block(BlockType::Water, 5, 0.0));
    }
}
