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
use crate::features::ReliefKind;
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

// What a biome changes about the ground: its blocks in place of the base terrain's, and tree density.
pub trait Biome: Send + Sync {
    fn name(&self) -> &'static str;

    // The chance, 0..1, of a tree on each spot that could grow one, or None for the base terrain's.
    fn tree_density(&self) -> Option<f64> {
        None
    }

    // The block to use where the base terrain would put `base`, `above_sea` blocks up, on this gradient.
    fn block(&self, base: BlockType, _above_sea: i64, _gradient: f64) -> BlockType {
        base
    }

    // What a spring's rim and pool bed are made of, where a river rises.
    fn spring_block(&self) -> BlockType {
        BlockType::Gravel
    }

    // Added to the temperature and humidity that tint grass and flora, so a biome reads lusher or drier.
    fn climate_shift(&self) -> (f32, f32) {
        (0.0, 0.0)
    }

    fn tree_scale(&self) -> TreeScale {
        TreeScale::default()
    }

    // Share of the frost the cold would give its ground that it keeps, 0..1: tundra and taiga thaw it.
    fn frost(&self) -> f32 {
        1.0
    }

    fn relief(&self) -> &'static [ReliefKind] {
        &[]
    }
}

/// What trees keep of their density in a clearing, unless the biome says otherwise (`TreeScale::clearings`).
pub const CLEARING_KEEPS: f64 = 0.4;

/// Multipliers on a tree's trunk height, crown spread and crown squash (1 is the usual).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct TreeScale {
    pub height: f64,
    pub spread: f64,
    pub squash: f64,
    /// The share of broadleaf trees, 0..1, that grow as emergent giants, standing well over the rest.
    pub giants: f64,
    /// The share of its density a tree keeps in a clearing, 0..1: 1 leaves no clearings.
    pub clearings: f64,
}

impl Default for TreeScale {
    fn default() -> Self {
        Self {
            height: 1.0,
            spread: 1.0,
            squash: 1.0,
            giants: 0.0,
            clearings: CLEARING_KEEPS,
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

// Below this height over the sea the tundra is bare ground, not snow as the cold would make it.
const TUNDRA_THAW_LINE: i64 = 350;
const TUNDRA_SCREE_GRADIENT: f64 = 0.38;
// Keep taiga lowlands forested and green, with snow remaining on higher ground.
const TAIGA_THAW_LINE: i64 = 250;
const TAIGA_SCREE_GRADIENT: f64 = 0.6;
// How much of the frost the cold gives its ground a thawed biome keeps.
const TUNDRA_FROST: f32 = 0.5;
const TAIGA_FROST: f32 = 0.6;

impl Biomes {
    pub fn new(_seed: u32) -> Self {
        Self::with(vec![
            Box::new(Glacier),
            Box::new(Desert),
            Box::new(Forest),
            Box::new(
                Plain::new("tundra", 0.01)
                    .shifted(-0.10, -0.20)
                    .with_thaw(TUNDRA_THAW_LINE, TUNDRA_SCREE_GRADIENT)
                    .with_frost(TUNDRA_FROST)
                    .with_relief(&[ReliefKind::Pingos, ReliefKind::PatternedGround, ReliefKind::TundraHills]),
            ),
            Box::new(
                Plain::new("taiga", 0.3)
                    .with_thaw(TAIGA_THAW_LINE, TAIGA_SCREE_GRADIENT)
                    .with_frost(TAIGA_FROST)
                    .scaled(1.2, 0.8, 1.0)
                    .with_relief(&[ReliefKind::Drumlins]),
            ),
            Box::new(
                Plain::new("steppe", 0.004)
                    .shifted(0.0, -0.10),
            ),
            Box::new(
                Plain::new("savanna", 0.03)
                    .shifted(0.05, -0.15)
                    .with_relief(&[ReliefKind::Kopjes, ReliefKind::Gullies]),
            ),
            Box::new(
                Plain::new("jungle", 1.0)
                    .shifted(0.0, 0.15)
                    .with_giants(0.1)
                    .with_clearings(0.9)
                    .with_relief(&[ReliefKind::Mogotes, ReliefKind::Ledges]),
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

    pub fn tree_scale(&self, biome: BiomeId) -> TreeScale {
        self.entry(biome)
            .map_or_else(TreeScale::default, |e| e.biome.tree_scale())
    }

    /// How much of the frost the cold would give the ground in `biome` it keeps, 0 to 1.
    pub fn frost(&self, biome: BiomeId) -> f32 {
        self.entry(biome).map_or(1.0, |e| e.biome.frost())
    }

    pub fn relief(&self, biome: BiomeId) -> &'static [ReliefKind] {
        self.entry(biome).map_or(&[], |e| e.biome.relief())
    }

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

// Grass at the climate square's four corners, sRGB: parched straw hot and dry, lush green hot and wet, duller cold.
const GRASS_HOT_DRY: [f32; 3] = [184.0, 164.0, 96.0];
const GRASS_HOT_WET: [f32; 3] = [40.0, 130.0, 12.0];
const GRASS_COLD_DRY: [f32; 3] = [140.0, 128.0, 88.0];
const GRASS_COLD_WET: [f32; 3] = [30.0, 96.0, 24.0];

// The mean linear value of the greyscale grass tile, which vertex colours tint.
const GRASS_TEXTURE_MEAN: f32 = 0.5;

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
        // a jungle has almost no clearings and a forest the usual
        assert!(biomes.tree_scale(biomes.id("jungle").unwrap()).clearings >= 0.9);
        assert_eq!(CLEARING_KEEPS, biomes.tree_scale(forest).clearings);
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

    #[test]
    fn test_thawed_biomes_keep_part_of_the_frost_and_modifiers_pass_it_through() {
        let biomes = Biomes::new(1);
        let (tundra, taiga, forest) = (biomes.id("tundra").unwrap(), biomes.id("taiga").unwrap(), biomes.id("forest").unwrap());
        assert_eq!((0.5, 0.6, 1.0), (biomes.frost(tundra), biomes.frost(taiga), biomes.frost(forest)));
        assert_eq!(1.0, biomes.frost(BiomeId::BASE));
        let mountain = biomes.with_modifier(tundra, Modifier::Mountain);
        assert_eq!(0.5, biomes.frost(mountain));
    }
}
