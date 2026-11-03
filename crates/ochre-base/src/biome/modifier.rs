use std::sync::Arc;

use super::{Biome, TreeScale, BASE_TREE_DENSITY};
use crate::block::BlockType;
use crate::math::smoothstep_f64;

/// What the ground has become on top of its climate biome: high steep ground, or a volcanic province's.
/// A mountain desert and a mountain taiga differ because they are the desert and the taiga underneath.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Modifier {
    #[default]
    None,
    /// High, steep ground: meadow with bare rock on the steeper faces, more of it the higher it gets,
    /// scree for the sand and a few scattered trees.
    Mountain,
    /// High ground on a volcanic province: the rock there is basalt, bare on everything but the
    /// gentlest slopes, with ash for the sand and hardly a tree.
    Volcanic,
}

// Mountain ground steeper than this is bare rock, the limit falling with height from the first gradient
// at the first height over the sea to the second at the second.
const ROCK_GRADIENT: (f64, f64) = (1.6, 1.0);
const ROCK_ABOVE: (f64, f64) = (150.0, 600.0);
// Volcanic ground steeper than this is bare rock.
const BARE_GRADIENT: f64 = 0.5;
// How thickly trees grow on mountain and on volcanic ground.
const MOUNTAIN_TREES: f64 = 0.04;
const VOLCANIC_TREES: f64 = 0.01;

fn rock_gradient(above_sea: i64) -> f64 {
    let t = smoothstep_f64(ROCK_ABOVE.0, ROCK_ABOVE.1, above_sea as f64);
    ROCK_GRADIENT.0 + (ROCK_GRADIENT.1 - ROCK_GRADIENT.0) * t
}

/// The base terrain with nothing on it, the climate biome under a modifier where the map names none.
pub(super) struct Grassland;

impl Biome for Grassland {
    fn name(&self) -> &'static str {
        "grassland"
    }
}

/// A climate biome with a modifier on it: its own blocks, trees and springs, then the modifier's changes.
pub(super) struct Modified {
    pub(super) inner: Arc<dyn Biome>,
    pub(super) modifier: Modifier,
}

impl Biome for Modified {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn tree_density(&self) -> Option<f64> {
        match self.modifier {
            Modifier::None => self.inner.tree_density(),
            Modifier::Mountain => Some(
                self.inner
                    .tree_density()
                    .unwrap_or(BASE_TREE_DENSITY)
                    .min(MOUNTAIN_TREES),
            ),
            Modifier::Volcanic => Some(VOLCANIC_TREES),
        }
    }

    fn block(&self, base: BlockType, above_sea: i64, gradient: f64) -> BlockType {
        let block = self.inner.block(base, above_sea, gradient);
        let bare_above = match self.modifier {
            Modifier::None => return block,
            Modifier::Mountain => rock_gradient(above_sea),
            Modifier::Volcanic => BARE_GRADIENT,
        };
        match block {
            BlockType::Grass if gradient >= bare_above => BlockType::Stone,
            BlockType::Sand => BlockType::Gravel,
            other => other,
        }
    }

    fn spring_block(&self) -> BlockType {
        match self.modifier {
            Modifier::None => self.inner.spring_block(),
            _ => BlockType::Stone,
        }
    }

    fn climate_shift(&self) -> (f32, f32) {
        self.inner.climate_shift()
    }

    fn tree_scale(&self) -> TreeScale {
        self.inner.tree_scale()
    }
}

#[cfg(test)]
mod tests {
    use super::super::Biomes;
    use super::*;

    #[test]
    fn test_steep_ground_is_rock_sooner_the_higher_it_is_and_gentle_ground_stays_meadow() {
        let biomes = Biomes::new(1);
        let grass = biomes.with_modifier(super::super::BiomeId::BASE, Modifier::Mountain);
        let rock = |above, gradient| biomes.block(grass, BlockType::Grass, above, gradient);
        assert_eq!(BlockType::Stone, rock(120, 1.7));
        assert_eq!(BlockType::Grass, rock(300, 0.2));
        assert_eq!(BlockType::Grass, rock(300, 0.9));
        assert_eq!(BlockType::Stone, rock(800, 1.1));
        assert_eq!(BlockType::Grass, rock(200, 0.2));
        assert_eq!(
            BlockType::Gravel,
            biomes.block(grass, BlockType::Sand, 120, 0.3)
        );
        assert_eq!(
            BlockType::Snow,
            biomes.block(grass, BlockType::Snow, 220, 0.3)
        );
        assert_eq!(
            BlockType::Water,
            biomes.block(grass, BlockType::Water, 5, 0.0)
        );
    }

    #[test]
    fn test_volcanic_slopes_are_bare_rock_and_gentle_ground_keeps_its_meadow() {
        let biomes = Biomes::new(1);
        let volcanic = biomes.with_modifier(super::super::BiomeId::BASE, Modifier::Volcanic);
        assert_eq!(
            BlockType::Stone,
            biomes.block(volcanic, BlockType::Grass, 200, 0.8)
        );
        assert_eq!(
            BlockType::Grass,
            biomes.block(volcanic, BlockType::Grass, 200, 0.2)
        );
        assert_eq!(
            BlockType::Gravel,
            biomes.block(volcanic, BlockType::Sand, 200, 0.2)
        );
        assert_eq!(
            BlockType::Snow,
            biomes.block(volcanic, BlockType::Snow, 900, 0.8)
        );
    }

    #[test]
    fn test_a_modifier_sits_on_its_climate_biome() {
        let biomes = Biomes::new(1);
        let desert = biomes.id("desert").unwrap();
        let mountain_desert = biomes.with_modifier(desert, Modifier::Mountain);
        assert_ne!(desert, mountain_desert);
        // still the desert: its name, its sand (to scree on a mountain), no trees to speak of
        assert_eq!(Some("desert"), biomes.name(mountain_desert));
        assert_eq!(Modifier::Mountain, biomes.modifier(mountain_desert));
        assert_eq!(
            BlockType::Gravel,
            biomes.block(mountain_desert, BlockType::Grass, 10, 0.0)
        );
        // and a forest keeps its trees, held to a mountain's few
        let forest = biomes.id("forest").unwrap();
        let mountain_forest = biomes.with_modifier(forest, Modifier::Mountain);
        assert!(biomes.tree_density(forest) > biomes.tree_density(mountain_forest));
        assert_eq!(Some("forest"), biomes.name(mountain_forest));
        // no modifier is the biome itself
        assert_eq!(
            forest,
            biomes.with_modifier(mountain_forest, Modifier::None)
        );
        // and the lookup by name only finds the biomes themselves
        assert_eq!(Some(forest), biomes.id("forest"));
        assert!(biomes.id("mountain").is_none() && biomes.id("grassland").is_none());
    }
}
