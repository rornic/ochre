mod desert;
mod forest;
mod glacier;
mod modifier;
mod plain;
mod swamp;

use bevy::color::{ColorToComponents, LinearRgba, Srgba};
use bevy::math::Vec3;

use std::sync::Arc;

use crate::block::BlockType;
pub use desert::Desert;
pub use forest::Forest;
pub use glacier::Glacier;
pub use modifier::Modifier;
pub use plain::Plain;
pub use swamp::Swamp;

// How hot and how wet a column is, each 0..1. See `TerrainSource::climate`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Climate {
    pub temperature: f32,
    pub humidity: f32,
}

impl Default for Climate {
    fn default() -> Self {
        Self {
            temperature: 0.5,
            humidity: 0.5,
        }
    }
}

// What a biome changes about the ground and what grows on it: the blocks it uses in place of
// the base terrain's, and how thickly trees grow.
pub trait Biome: Send + Sync {
    fn name(&self) -> &'static str;

    // The chance, 0..1, of a tree on each spot one could grow, or None to leave the base
    // terrain's. Trees only grow on grass.
    fn tree_density(&self) -> Option<f64> {
        None
    }

    // The block to use where the base terrain would put `base`, `above_sea` blocks up, on
    // ground with the given gradient (see `terrain_block`).
    fn block(&self, base: BlockType, _above_sea: i64, _gradient: f64) -> BlockType {
        base
    }

    // What a spring's rim and pool bed are made of, where a river rises.
    fn spring_block(&self) -> BlockType {
        BlockType::Gravel
    }

    // Added to the temperature and humidity grass and flora are tinted by, so a biome reads lusher or
    // drier than its climate.
    fn climate_shift(&self) -> (f32, f32) {
        (0.0, 0.0)
    }

    // How big the biome's trees grow against the usual.
    fn tree_scale(&self) -> TreeScale {
        TreeScale::default()
    }
}

/// Multipliers on a tree's trunk height, crown spread and crown squash (1 is the usual).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TreeScale {
    pub height: f64,
    pub spread: f64,
    pub squash: f64,
    /// The share of broadleaf trees, 0..1, that grow as emergent giants, standing well over the rest.
    pub giants: f64,
}

impl Default for TreeScale {
    fn default() -> Self {
        Self {
            height: 1.0,
            spread: 1.0,
            squash: 1.0,
            giants: 0.0,
        }
    }
}

// Which biome a column is, as an index into `Biomes`, or the base terrain.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
pub struct BiomeId(u8);

impl BiomeId {
    pub const BASE: BiomeId = BiomeId(0);
}

// The base terrain's scattering of lone trees, see `Biome::tree_density`.
pub(super) const BASE_TREE_DENSITY: f64 = 0.02;

// A biome in the table, or a climate biome with a modifier on it.
struct Entry {
    biome: Arc<dyn Biome>,
    // the biome it is a modifier of (0 for the base terrain), or its own id
    base: u8,
    modifier: Modifier,
}

// Every biome a world map can name, and each of them under each modifier.
pub struct Biomes {
    entries: Vec<Entry>,
}

impl Biomes {
    pub fn new(_seed: u32) -> Self {
        Self::with(vec![
            Box::new(Glacier),
            Box::new(Desert),
            Box::new(Forest),
            Box::new(Plain::new("tundra", 0.01).shifted(-0.10, -0.05)),
            Box::new(Plain::new("taiga", 0.3).scaled(1.2, 0.8, 1.0)),
            Box::new(Plain::new("steppe", 0.004).shifted(0.0, -0.10)),
            Box::new(
                Plain::new("savanna", 0.03)
                    .shifted(0.05, -0.15)
                    .scaled(0.8, 1.2, 0.5),
            ),
            Box::new(
                Plain::new("jungle", 0.6)
                    .shifted(0.0, 0.15)
                    .scaled(1.3, 1.0, 1.0)
                    .with_giants(0.1),
            ),
            Box::new(Swamp),
        ])
        .with_modifiers()
    }

    pub fn with(biomes: Vec<Box<dyn Biome>>) -> Self {
        assert!(biomes.len() < u8::MAX as usize / 3);
        let entries = biomes
            .into_iter()
            .enumerate()
            .map(|(i, b)| Entry {
                biome: Arc::from(b),
                base: i as u8 + 1,
                modifier: Modifier::None,
            })
            .collect();
        Self { entries }
    }

    // Adds each biome, and the base terrain, under the mountain and volcanic modifiers.
    fn with_modifiers(mut self) -> Self {
        let plain: Vec<(u8, Arc<dyn Biome>)> =
            std::iter::once((0, Arc::new(modifier::Grassland) as Arc<dyn Biome>))
                .chain(self.entries.iter().map(|e| (e.base, e.biome.clone())))
                .collect();
        for modifier in [Modifier::Mountain, Modifier::Volcanic] {
            for (base, inner) in &plain {
                self.entries.push(Entry {
                    biome: Arc::new(modifier::Modified {
                        inner: inner.clone(),
                        modifier,
                    }),
                    base: *base,
                    modifier,
                });
            }
        }
        self
    }

    // The entry for `biome`, None for the base terrain.
    fn entry(&self, biome: BiomeId) -> Option<&Entry> {
        biome.0.checked_sub(1).map(|i| &self.entries[i as usize])
    }

    /// The biome called `name`, not under a modifier.
    pub fn id(&self, name: &str) -> Option<BiomeId> {
        self.entries
            .iter()
            .position(|e| e.modifier == Modifier::None && e.biome.name() == name)
            .map(|i| BiomeId(i as u8 + 1))
    }

    /// `biome` with `modifier` on it (`Modifier::None` gives the biome itself).
    pub fn with_modifier(&self, biome: BiomeId, modifier: Modifier) -> BiomeId {
        let base = self.entry(biome).map_or(0, |e| e.base);
        if modifier == Modifier::None {
            return BiomeId(base);
        }
        self.entries
            .iter()
            .position(|e| e.base == base && e.modifier == modifier)
            .map(|i| BiomeId(i as u8 + 1))
            .expect("every biome has each modifier")
    }

    /// What the ground has become on top of `biome`.
    pub fn modifier(&self, biome: BiomeId) -> Modifier {
        self.entry(biome).map_or(Modifier::None, |e| e.modifier)
    }

    /// How thickly trees grow in `biome`, 0..1.
    pub fn tree_density(&self, biome: BiomeId) -> f64 {
        self.entry(biome)
            .and_then(|e| e.biome.tree_density())
            .unwrap_or(BASE_TREE_DENSITY)
    }

    /// What to add to the (temperature, humidity) the grass and flora of `biome` are tinted by.
    pub fn climate_shift(&self, biome: BiomeId) -> (f32, f32) {
        self.entry(biome)
            .map_or((0.0, 0.0), |e| e.biome.climate_shift())
    }

    /// How big trees grow in `biome` against the usual.
    pub fn tree_scale(&self, biome: BiomeId) -> TreeScale {
        self.entry(biome)
            .map_or_else(TreeScale::default, |e| e.biome.tree_scale())
    }

    /// What a spring's rim and pool bed are made of in `biome`.
    pub fn spring_block(&self, biome: BiomeId) -> BlockType {
        self.entry(biome)
            .map_or(BlockType::Gravel, |e| e.biome.spring_block())
    }

    // The climate biome's name, or None for the base terrain, whatever modifier is on it.
    pub fn name(&self, biome: BiomeId) -> Option<&'static str> {
        self.entry(biome)
            .filter(|e| e.base != 0)
            .map(|e| e.biome.name())
    }

    // The block `biome` uses in place of the base terrain's `base`.
    pub fn block(
        &self,
        biome: BiomeId,
        base: BlockType,
        above_sea: i64,
        gradient: f64,
    ) -> BlockType {
        self.entry(biome)
            .map_or(base, |e| e.biome.block(base, above_sea, gradient))
    }
}

// Grass at the four corners of the climate square, in sRGB: hot and dry is parched
// yellow, hot and wet lush green, and the cold side duller versions of both.
const GRASS_HOT_DRY: [f32; 3] = [200.0, 170.0, 60.0];
const GRASS_HOT_WET: [f32; 3] = [40.0, 130.0, 12.0];
const GRASS_COLD_DRY: [f32; 3] = [140.0, 140.0, 60.0];
const GRASS_COLD_WET: [f32; 3] = [30.0, 96.0, 24.0];

// The mean linear value of the greyscale grass and leaves tiles in blocks.png, which vertex
// colours tint: leaves share grass's tint, and their darker tile makes them a deeper green.
const GRASS_TEXTURE_MEAN: f32 = 0.5;
const LEAF_TEXTURE_MEAN: f32 = 0.25;

// Linear colour of grass in this climate.
pub fn grass_colour(climate: Climate) -> Vec3 {
    let srgb = |c: [f32; 3]| Vec3::from(c) / 255.0;
    let (t, h) = (
        climate.temperature.clamp(0.0, 1.0),
        climate.humidity.clamp(0.0, 1.0),
    );
    let dry = srgb(GRASS_COLD_DRY).lerp(srgb(GRASS_HOT_DRY), t);
    let wet = srgb(GRASS_COLD_WET).lerp(srgb(GRASS_HOT_WET), t);
    let colour = dry.lerp(wet, h);
    LinearRgba::from(Srgba::rgb(colour.x, colour.y, colour.z)).to_vec3()
}

// Linear colour of leaves in this climate.
pub fn leaf_colour(climate: Climate) -> Vec3 {
    grass_tint(climate) * LEAF_TEXTURE_MEAN
}

// What to multiply the grass texture by to draw `grass_colour`.
pub fn grass_tint(climate: Climate) -> Vec3 {
    grass_colour(climate) / GRASS_TEXTURE_MEAN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_biomes_are_found_by_name_and_know_their_trees() {
        let biomes = Biomes::new(1);
        let forest = biomes.id("forest").unwrap();
        assert_eq!(Some("forest"), biomes.name(forest));
        assert_eq!(None, biomes.name(BiomeId::BASE));
        assert!(biomes.tree_density(forest) > biomes.tree_density(BiomeId::BASE) * 10.0);
        assert_eq!(0.0, biomes.tree_density(biomes.id("desert").unwrap()));
        assert!(biomes.id("atlantis").is_none());
    }

    #[test]
    fn test_a_biome_owns_the_blocks_it_swaps() {
        let biomes = Biomes::new(1);
        let desert = biomes.id("desert").unwrap();
        assert_eq!(
            BlockType::Sand,
            biomes.block(desert, BlockType::Grass, 10, 0.0)
        );
        assert_eq!(
            BlockType::Grass,
            biomes.block(BiomeId::BASE, BlockType::Grass, 10, 0.0)
        );
    }

    fn climate(temperature: f32, humidity: f32) -> Climate {
        Climate {
            temperature,
            humidity,
        }
    }

    // How much yellower than green, as red over green.
    fn yellowness(colour: Vec3) -> f32 {
        colour.x / colour.y
    }

    #[test]
    fn test_hot_dry_grass_is_yellower_than_wet() {
        let dry = grass_colour(climate(1.0, 0.0));
        let wet = grass_colour(climate(1.0, 1.0));
        assert!(
            yellowness(dry) > 0.8,
            "dry grass should be yellow, was {dry}"
        );
        assert!(
            yellowness(wet) < 0.2,
            "wet grass should be green, was {wet}"
        );
    }

    #[test]
    fn test_drier_and_hotter_is_always_yellower() {
        for step in 0..10 {
            let x = step as f32 / 10.0;
            let y = x + 0.1;
            assert!(
                yellowness(grass_colour(climate(0.5, x)))
                    > yellowness(grass_colour(climate(0.5, y)))
            );
            assert!(
                yellowness(grass_colour(climate(y, 0.3)))
                    > yellowness(grass_colour(climate(x, 0.3)))
            );
        }
    }

    #[test]
    fn test_tint_draws_grass_colour_from_texture() {
        let c = climate(0.3, 0.8);
        assert!((grass_tint(c) * GRASS_TEXTURE_MEAN - grass_colour(c)).length() < 1e-6);
    }
}
