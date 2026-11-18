use crate::biome::{BiomeId, Biomes, Climate};

/// What covers a column's ground with water.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum WaterKind {
    #[default]
    None,
    Ocean,
    Lake,
    River,
}

/// The water over a column: its surface height in blocks and how deep it is there.
#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub struct Water {
    pub kind: WaterKind,
    pub surface: f32,
    pub depth: f32,
}

// A column's height, with how far inland it is (negative out to sea), the biome owning it
// and how thickly trees grow there.
#[derive(Debug, Default, Copy, Clone)]
pub struct TerrainSample {
    pub continentalness: f64,
    pub height: f64,
    pub biome: BiomeId,
    pub trees: f64,
    /// Water surface elevation. `water_kind` identifies wet columns: zero is a valid sea level.
    pub water_surface: f32,
    pub water_kind: WaterKind,
    pub spring: SpringPart,
    /// Bare rock on top of a relief feature, which a block rule that reads only slope would leave as grass.
    pub rocky: bool,
    /// A column of the sheet of water over a river's fall, deeper than the river's own bed.
    pub fall: bool,
}

/// The part of a spring, where a river rises, a column is in.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum SpringPart {
    #[default]
    None,
    /// The ring of ground round the pool.
    Rim,
    /// The pool the river rises in.
    Pool,
}

/// Where the ground, its biomes and its water come from. Chunk generation and LOD read terrain
/// only through this.
/// The rocks down one column.
pub trait RockColumn {
    /// The blocks `y` is among that are all one rock, as `start..end`, and the block stone is drawn as in them.
    fn bed(&mut self, y: i64) -> (i64, i64, crate::block::BlockType);

    /// The block stone is drawn as at height `y`.
    fn at(&mut self, y: i64) -> crate::block::BlockType {
        self.bed(y).2
    }
}

struct StoneColumn;

impl RockColumn for StoneColumn {
    fn bed(&mut self, _y: i64) -> (i64, i64, crate::block::BlockType) {
        (i64::MIN, i64::MAX, crate::block::BlockType::Rock)
    }
}

pub trait TerrainSource: Send + Sync {
    fn seed(&self) -> u32;

    fn bounds(&self) -> crate::WorldBounds {
        crate::WorldBounds::default()
    }

    fn biomes(&self) -> &Biomes;

    /// The terrain as a cell `cell` blocks wide, sampled at its centre, should see it: features
    /// too fine for cells that wide fade to their average. The same as `sample` for single blocks.
    fn sample_for_cell(&self, x: f64, z: f64, cell: f64) -> TerrainSample;

    /// The rock down the column at (x, z), `Stone` throughout where the source has no geology.
    fn rock_column(&self, _x: i64, _z: i64) -> Box<dyn RockColumn + '_> {
        Box::new(StoneColumn)
    }

    /// How hot and wet the column at (x, z) is.
    fn climate(&self, x: f64, z: f64) -> Climate;

    /// How hot the column at (x, z) is, 0..1.
    fn temperature(&self, x: f64, z: f64) -> f32;

    /// How much more 3D relief than usual the ground at (x, z) has, 0..1: it lengthens the reach of its cliffs.
    fn cliff_boost(&self, _x: f64, _z: f64) -> f32 {
        0.0
    }

    /// How much of a valley's wall the column at (x, z) is, 0..1: 0 away from valleys.
    fn valley_wall(&self, _x: f64, _z: f64) -> f32 {
        0.0
    }

    /// How far the column at (x, z) stands over the ground round it, 0..1: 0.5 on level ground, up on crests, down in hollows.
    fn exposure(&self, _x: f64, _z: f64) -> f32 {
        0.5
    }

    /// The climate grass and flora are tinted by at (x, z): `climate` shifted by the lie of the land, so ridges, hollows and banks read differently.
    fn tint_climate(&self, x: f64, z: f64) -> Climate {
        self.climate(x, z)
    }

    /// How much lusher (above 0) or thinner (below 0) the lie of the land makes cover at (x, z), in humidity: banks and hollows up, crests and high ground down. Grass tint and tree density both read it.
    fn vigour(&self, _x: f64, _z: f64) -> f32 {
        0.0
    }

    /// How many times the map's tree density a tree cell at (x, z) keeps for the lie of the land: more on banks and in hollows, fewer on crests and high ground. 1 where it makes no difference.
    fn tree_lie(&self, _x: f64, _z: f64) -> f64 {
        1.0
    }

    /// How close the column at (x, z) is to a river, lake or pond, 0..1: 1 on the water, falling to 0 within about 100 blocks.
    fn water_closeness(&self, _x: f64, _z: f64) -> f32 {
        0.0
    }

    /// Whether the column at (x, z) is steppe, whose grass ground draws its own rougher texture.
    fn steppe(&self, _x: f64, _z: f64) -> bool {
        false
    }

    /// The nearest lowland column to the world centre.
    fn find_spawn(&self) -> (i64, i64, f64);

    /// The way the prevailing wind blows over the world, as an x, z direction of any length: the
    /// wind its climate carries rain on, and its weather drifts with.
    fn prevailing_wind(&self) -> [f32; 2] {
        [1.0, 0.0]
    }

    /// The most tree density the column at (x, z) can have, without sampling its terrain, so
    /// callers can rule out trees cheaply. 1 if it can't say.
    fn tree_bound(&self, _x: f64, _z: f64) -> f64 {
        1.0
    }

    fn sample(&self, x: f64, z: f64) -> TerrainSample {
        self.sample_for_cell(x, z, 1.0)
    }

    /// Column height in blocks at a block position.
    fn get(&self, pos: bevy::math::I64Vec2) -> f64 {
        self.height(pos.x as f64, pos.y as f64)
    }

    fn height(&self, x: f64, z: f64) -> f64 {
        self.sample(x, z).height
    }

    fn biome(&self, x: f64, z: f64) -> BiomeId {
        self.sample(x, z).biome
    }

    /// The water over the column at (x, z).
    fn water(&self, x: f64, z: f64) -> Water {
        let sample = self.sample(x, z);
        if sample.water_kind != WaterKind::None {
            Water {
                kind: sample.water_kind,
                surface: sample.water_surface,
                depth: sample.water_surface - sample.height as f32,
            }
        } else {
            Water::default()
        }
    }
}

impl<T: TerrainSource + ?Sized> TerrainSource for std::sync::Arc<T> {
    fn bounds(&self) -> crate::WorldBounds {
        (**self).bounds()
    }
    fn seed(&self) -> u32 {
        (**self).seed()
    }

    fn biomes(&self) -> &Biomes {
        (**self).biomes()
    }

    fn sample_for_cell(&self, x: f64, z: f64, cell: f64) -> TerrainSample {
        (**self).sample_for_cell(x, z, cell)
    }

    fn rock_column(&self, x: i64, z: i64) -> Box<dyn RockColumn + '_> {
        (**self).rock_column(x, z)
    }

    fn climate(&self, x: f64, z: f64) -> Climate {
        (**self).climate(x, z)
    }

    fn temperature(&self, x: f64, z: f64) -> f32 {
        (**self).temperature(x, z)
    }

    fn cliff_boost(&self, x: f64, z: f64) -> f32 {
        (**self).cliff_boost(x, z)
    }

    fn valley_wall(&self, x: f64, z: f64) -> f32 {
        (**self).valley_wall(x, z)
    }

    fn exposure(&self, x: f64, z: f64) -> f32 {
        (**self).exposure(x, z)
    }

    fn water_closeness(&self, x: f64, z: f64) -> f32 {
        (**self).water_closeness(x, z)
    }

    fn tint_climate(&self, x: f64, z: f64) -> Climate {
        (**self).tint_climate(x, z)
    }

    fn vigour(&self, x: f64, z: f64) -> f32 {
        (**self).vigour(x, z)
    }

    fn tree_lie(&self, x: f64, z: f64) -> f64 {
        (**self).tree_lie(x, z)
    }

    fn steppe(&self, x: f64, z: f64) -> bool {
        (**self).steppe(x, z)
    }

    fn find_spawn(&self) -> (i64, i64, f64) {
        (**self).find_spawn()
    }

    fn prevailing_wind(&self) -> [f32; 2] {
        (**self).prevailing_wind()
    }

    fn tree_bound(&self, x: f64, z: f64) -> f64 {
        (**self).tree_bound(x, z)
    }
}
