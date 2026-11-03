use super::Biome;
use crate::block::BlockType;

// Ground gentler than this is ice, gentler than the next snow, and rougher ice again.
const ICE_GRADIENT: f64 = 0.8;
const SNOW_GRADIENT: f64 = 1.6;

pub struct Glacier;

impl Biome for Glacier {
    fn name(&self) -> &'static str {
        "glacier"
    }

    fn spring_block(&self) -> BlockType {
        BlockType::Stone
    }

    fn tree_density(&self) -> Option<f64> {
        Some(0.0)
    }

    fn block(&self, base: BlockType, _above_sea: i64, gradient: f64) -> BlockType {
        match base {
            BlockType::Grass | BlockType::Snow if gradient < ICE_GRADIENT => BlockType::Ice,
            BlockType::Grass if gradient < SNOW_GRADIENT => BlockType::Snow,
            // steep faces, banks and shores are ice too: the ice under them is what a glacier is made of
            BlockType::Grass | BlockType::Sand | BlockType::Stone => BlockType::Ice,
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glacier_ground_follows_the_slope() {
        assert_eq!(BlockType::Ice, Glacier.block(BlockType::Grass, 20, 0.1));
        assert_eq!(BlockType::Snow, Glacier.block(BlockType::Grass, 20, 1.0));
        assert_eq!(BlockType::Ice, Glacier.block(BlockType::Grass, 20, 2.5));
        assert_eq!(BlockType::Ice, Glacier.block(BlockType::Sand, 2, 0.1));
        assert_eq!(BlockType::Ice, Glacier.block(BlockType::Stone, 20, 2.5));
        assert_eq!(BlockType::Water, Glacier.block(BlockType::Water, -5, 0.0));
    }
}
