//! Boulders and termite mounds: single objects set by a hash of position, built block by block with a chunk, like trees.

use super::trees::{hash, TreeGround};
use bevy::math::{I64Vec2, I64Vec3, IVec3};
use ochre_base::biome::Modifier;
use ochre_base::block::BlockType;
use ochre_base::terrain_source::{RockColumn, TerrainSource};
use ochre_base::WATER_LEVEL;

// Objects stand on a grid of cells this many blocks across, at most one in each.
const CELL: i64 = 24;
/// The widest cell, in blocks, of a LOD mesh that shows rocks: past it one is under a pixel.
pub const ROCK_MAX_CELL: i64 = 4;
/// How far out from its centre any rock reaches.
pub const ROCK_REACH: i64 = 12;
const MAX_GRADIENT: f64 = 0.4;
const MAX_SLOPE: f64 = 0.9;
const SEA_CLEARANCE: i64 = 3;

// Some of a biome's boulders are outcrops instead, wide low ledges of rock, this share of them where the biome has any,
// between these radii and this much of their width tall.
const OUTCROP_SHARE: f64 = 0.5;
const OUTCROP_RADIUS: (f64, f64) = (5.5, 11.0);
const OUTCROP_SQUASH: f64 = 0.32;
// How far height may differ over an outcrop's foot before it would hang or be buried.
const OUTCROP_SPREAD: i64 = 6;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum RockKind {
    /// A lone boulder a glacier left, half sunk in the ground.
    Erratic,
    /// A cone of earth a colony of termites built.
    Mound,
}

#[derive(Debug, Copy, Clone)]
pub struct Rock {
    pub kind: RockKind,
    /// The first air block over the ground at the centre.
    pub base: I64Vec3,
    radius: f64,
    height: f64,
    // how tall an erratic is against how wide: 0.7 for a boulder, much less for a ledge of an outcrop
    squash: f64,
    // what it is made of: the rock the geology has under it, for a boulder, see `rock_in_cell`
    stone: BlockType,
    seed: u64,
}

// The kind of object a biome has and the share of its cells that hold one.
fn share(biome: Option<&str>) -> Option<(RockKind, f64)> {
    match biome? {
        // the tundra is strewn with boulder fields, where a taiga has one in a hundred
        "tundra" => Some((RockKind::Erratic, 0.9)),
        "taiga" => Some((RockKind::Erratic, 0.04)),
        "savanna" => Some((RockKind::Mound, 0.075)),
        _ => None,
    }
}

// Boulders come in fields: the share of cells that hold one is scaled by a factor that varies over patches this many blocks
// across, from `FIELD_FACTOR`'s first, near bare ground, to its second, where nearly every cell has one.
const FIELD: i64 = 96;
const FIELD_FACTOR: (f64, f64) = (0.1, 2.0);

// How much of the biome's `share` the cell at (x, z) keeps: a field of boulders or open ground between.
fn field(seed: u64, x: i64, z: i64) -> f64 {
    let p = (hash(seed ^ 0xF1E1_D5ED, x.div_euclid(FIELD), z.div_euclid(FIELD)) >> 11) as f64 / (1u64 << 53) as f64;
    FIELD_FACTOR.0 + (FIELD_FACTOR.1 - FIELD_FACTOR.0) * p * p
}

// What a rock of `kind` standing at `ground` is made of: a boulder is the rock under it, so it matches the cliffs and the ground round
// it and not a stone of its own, and a mound is earth.
fn stone_of(kind: RockKind, column: &mut dyn RockColumn, ground: i64) -> BlockType {
    match kind {
        RockKind::Erratic => column.at(ground),
        RockKind::Mound => BlockType::Dirt,
    }
}

// Every rock that could reach into the columns `min..=max` (x, z).
pub fn rocks_near(
    noise: &dyn TerrainSource,
    heights: &mut impl TreeGround,
    min: I64Vec2,
    max: I64Vec2,
) -> Vec<Rock> {
    let first = (min - ROCK_REACH).div_euclid(I64Vec2::splat(CELL));
    let last = (max + ROCK_REACH).div_euclid(I64Vec2::splat(CELL));
    let mut rocks = Vec::new();
    for cz in first.y..=last.y {
        for cx in first.x..=last.x {
            if let Some(rock) = rock_in_cell(noise, heights, cx, cz) {
                rocks.push(rock);
            }
        }
    }
    rocks
}

fn rock_in_cell(
    noise: &dyn TerrainSource,
    heights: &mut impl TreeGround,
    cx: i64,
    cz: i64,
) -> Option<Rock> {
    let seed = hash(noise.seed() as u64 ^ 0x0C45_70C4, cx, cz);
    let x = cx * CELL + ((seed >> 16) % CELL as u64) as i64;
    let z = cz * CELL + ((seed >> 24) % CELL as u64) as i64;
    let chance = (seed & 0xffff) as f64 / 65536.0;
    let biome = heights.biome(x, z);
    let (kind, share) = share(noise.biomes().name(biome))?;
    // a mountain's slopes and its screes are not where a glacier dropped one or termites build
    let share = match kind {
        RockKind::Erratic => (share * field(noise.seed() as u64, x, z)).min(1.0),
        RockKind::Mound => share,
    };
    if chance >= share || noise.biomes().modifier(biome) != Modifier::None {
        return None;
    }
    let (gradient_x, gradient_z) = heights.gradient(x, z);
    // a boulder lies on a slope, sunk to its low side, and a mound wants level ground
    let (steepest, spread) = match kind {
        RockKind::Erratic => (MAX_SLOPE, 3),
        RockKind::Mound => (MAX_GRADIENT, 1),
    };
    if gradient_x.abs() + gradient_z.abs() > steepest {
        return None;
    }
    let (mut low, mut high) = (i64::MAX, i64::MIN);
    let growth = ((seed >> 32) % 1000) as f64 / 1000.0;
    // an outcrop, where the biome has them: wide, low and on ground that may vary under it
    let outcrop = kind == RockKind::Erratic
        && noise.biomes().name(biome) == Some("tundra")
        && ((seed >> 52) & 0xff) as f64 / 256.0 < OUTCROP_SHARE;
    let (spread, foot) = if outcrop {
        (OUTCROP_SPREAD, ((OUTCROP_RADIUS.0 + (OUTCROP_RADIUS.1 - OUTCROP_RADIUS.0) * growth) * 0.5).round() as i64)
    } else {
        (spread, 3)
    };
    // dry under it and within its spread of height, or its foot would hang or drown
    for (dx, dz) in [(0, 0), (foot, 0), (-foot, 0), (0, foot), (0, -foot)] {
        if heights.wet(x + dx, z + dz) {
            return None;
        }
        let ground = heights.height(x + dx, z + dz).round() as i64;
        (low, high) = (low.min(ground), high.max(ground));
    }
    if high - low > spread || low < WATER_LEVEL + SEA_CLEARANCE {
        return None;
    }
    let ground = if kind == RockKind::Erratic {
        low
    } else {
        heights.base_height(x, z)
    };
    let stone = stone_of(kind, &mut *noise.rock_column(x, z), ground);
    let (radius, height) = match kind {
        _ if outcrop => (OUTCROP_RADIUS.0 + (OUTCROP_RADIUS.1 - OUTCROP_RADIUS.0) * growth, 0.0),
        RockKind::Erratic => (2.0 + 3.0 * growth, 0.0),
        RockKind::Mound => (2.5 + 1.5 * growth, 3.0 + 4.0 * growth),
    };
    Some(Rock {
        kind,
        base: I64Vec3::new(x, ground, z),
        radius,
        height,
        squash: if outcrop { OUTCROP_SQUASH } else { 0.7 },
        stone,
        seed,
    })
}

impl Rock {
    /// The block this rock is made of.
    pub fn block(&self) -> BlockType {
        match self.kind {
            RockKind::Erratic => self.stone,
            RockKind::Mound => BlockType::Dirt,
        }
    }

    /// Whether the rock's block goes where `existing` is: a mound only into air, a boulder also sinks into the ground.
    pub fn fits_into(&self, existing: BlockType) -> bool {
        existing == BlockType::Air
            || (self.kind == RockKind::Erratic
                && !matches!(
                    existing,
                    BlockType::Water | BlockType::Ice | BlockType::Log | BlockType::Leaves
                ))
    }

    /// Calls `set` with the place of each cell of the rock inside `bounds` (local, inclusive), for a chunk or LOD node at
    /// `origin` whose cells are `scale` blocks across. A cell is the rock if its middle is.
    pub fn plant(
        &self,
        origin: I64Vec3,
        scale: i64,
        bounds: (IVec3, IVec3),
        mut set: impl FnMut(IVec3, BlockType),
    ) {
        let reach = self.radius.ceil() as i64 + 1;
        let up = match self.kind {
            RockKind::Erratic => reach,
            RockKind::Mound => self.height.ceil() as i64 + 1,
        };
        let low = self.base - I64Vec3::new(reach, reach, reach);
        let high = self.base + I64Vec3::new(reach, up, reach);
        let first = ((low - origin).div_euclid(I64Vec3::splat(scale)))
            .as_ivec3()
            .max(bounds.0);
        let last = ((high - origin).div_euclid(I64Vec3::splat(scale)))
            .as_ivec3()
            .min(bounds.1);
        for cy in first.y..=last.y {
            for cz in first.z..=last.z {
                for cx in first.x..=last.x {
                    let cell = origin + I64Vec3::new(cx as i64, cy as i64, cz as i64) * scale;
                    if self.holds(cell, scale) {
                        set(IVec3::new(cx, cy, cz), self.block());
                    }
                }
            }
        }
    }

    // Whether the cell `scale` blocks across with its corner at `at` is part of the rock, by its middle.
    fn holds(&self, at: I64Vec3, scale: i64) -> bool {
        let half = scale as f64 / 2.0;
        let (dx, dz) = (
            (at.x as f64 + half) - (self.base.x as f64 + 0.5),
            (at.z as f64 + half) - (self.base.z as f64 + 0.5),
        );
        let y = at.y as f64 + half;
        match self.kind {
            RockKind::Erratic => {
                // half sunk, squashed, and nibbled at the rim so it is not a smooth ball
                let dy = y - (self.base.y as f64 + self.radius * 0.25);
                let across = self.radius;
                let d2 =
                    (dx / across).powi(2) + (dy / (across * self.squash)).powi(2) + (dz / across).powi(2);
                let nibble = (hash(self.seed, at.x ^ (at.y << 20), at.z) % 1000) as f64 / 1000.0;
                d2 <= 1.0 - 0.3 * nibble
            }
            RockKind::Mound => {
                let t = 1.0 - libm::sqrt(dx * dx + dz * dz) / self.radius;
                // the cell's bottom is on or over the ground and under the cone's height there
                if t <= 0.0 || at.y < self.base.y {
                    return false;
                }
                let blocks = (self.height * libm::pow(t, 1.2)).round();
                (at.y - self.base.y) as f64 + half < blocks
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rock(kind: RockKind, radius: f64, height: f64) -> Rock {
        Rock {
            kind,
            base: I64Vec3::new(40, 100, -20),
            radius,
            height,
            squash: 0.7,
            stone: BlockType::Rock,
            seed: 3,
        }
    }

    fn blocks(rock: &Rock) -> Vec<IVec3> {
        let mut out = Vec::new();
        let size = IVec3::splat(200);
        rock.plant(I64Vec3::new(0, 0, -100), 1, (IVec3::ZERO, size), |at, _| {
            out.push(at)
        });
        out
    }

    #[test]
    fn an_erratic_is_stone_within_its_radius_and_half_sunk() {
        let r = rock(RockKind::Erratic, 4.0, 0.0);
        let found = blocks(&r);
        assert!(found.len() > 30, "{} blocks", found.len());
        for at in &found {
            assert!((at.x - 40).abs() <= 5 && (at.z - 80).abs() <= 5, "{at}");
        }
        // some of it under the ground and some over
        assert!(found.iter().any(|at| at.y < 100) && found.iter().any(|at| at.y >= 100));
        assert_eq!(r.stone, r.block());
    }

    #[test]
    fn a_mound_is_a_cone_of_earth_on_the_ground_that_narrows_as_it_rises() {
        let r = rock(RockKind::Mound, 3.0, 6.0);
        let found = blocks(&r);
        assert!(
            found.iter().all(|at| at.y >= 100),
            "a mound dug into the ground"
        );
        let top = found.iter().map(|at| at.y).max().unwrap();
        assert!(top >= 104, "only {} high", top - 99);
        let wide = |y: i32| found.iter().filter(|at| at.y == y).count();
        assert!(
            wide(100) > wide(top),
            "{} low, {} at the top",
            wide(100),
            wide(top)
        );
        assert_eq!(BlockType::Dirt, r.block());
    }

    #[test]
    fn the_tundra_is_strewn_with_boulders_and_a_taiga_has_few() {
        let density = |biome| share(Some(biome)).unwrap().1 / (CELL * CELL) as f64;
        assert_eq!(RockKind::Erratic, share(Some("tundra")).unwrap().0);
        assert!(density("tundra") > 8.0 * density("taiga"));
        // up to about one in a square 25 blocks across, where its fields are thickest
        assert!((1.0 / density("tundra")).sqrt() < 30.0);
        // and the fields vary: patches hold between a tenth and twice the share
        let factors: Vec<f64> = (0..200).map(|i| field(3, i * FIELD, 0)).collect();
        assert!(factors.iter().cloned().fold(f64::MAX, f64::min) < 0.3 && factors.iter().cloned().fold(0.0, f64::max) > 1.5);
    }

    #[test]
    fn an_outcrop_is_a_wide_low_ledge_where_a_boulder_is_a_ball() {
        let width = |found: &[IVec3]| {
            let (lo, hi) = found.iter().fold((i32::MAX, i32::MIN), |(lo, hi), at| (lo.min(at.x), hi.max(at.x)));
            (hi - lo + 1) as f64
        };
        let height = |found: &[IVec3]| {
            let (lo, hi) = found.iter().fold((i32::MAX, i32::MIN), |(lo, hi), at| (lo.min(at.y), hi.max(at.y)));
            (hi - lo + 1) as f64
        };
        let ball = blocks(&rock(RockKind::Erratic, 4.0, 0.0));
        let ledge = blocks(&Rock { squash: OUTCROP_SQUASH, ..rock(RockKind::Erratic, 9.0, 0.0) });
        assert!(width(&ledge) >= 17.0, "{} wide", width(&ledge));
        assert!(height(&ledge) / width(&ledge) < 0.7 * height(&ball) / width(&ball), "{} by {}", width(&ledge), height(&ledge));
        // the biggest outcrop still fits inside the reach neighbouring chunks look for rocks in
        assert!(OUTCROP_RADIUS.1 + 1.0 <= ROCK_REACH as f64);
    }

    // A boulder is the rock the geology has at its foot, not a plain stone of its own, and a mound is earth whatever is under it.
    #[test]
    fn a_boulder_is_the_rock_the_geology_has_at_its_foot() {
        struct Beds;
        impl ochre_base::terrain_source::RockColumn for Beds {
            fn bed(&mut self, y: i64) -> (i64, i64, BlockType) {
                if y < 100 {
                    (i64::MIN, 100, BlockType::Granite)
                } else {
                    (100, i64::MAX, BlockType::Limestone)
                }
            }
        }
        assert_eq!(BlockType::Limestone, stone_of(RockKind::Erratic, &mut Beds, 130));
        assert_eq!(BlockType::Granite, stone_of(RockKind::Erratic, &mut Beds, 60));
        assert_eq!(BlockType::Dirt, stone_of(RockKind::Mound, &mut Beds, 130));
        // and it is what the rock is made of
        let boulder = Rock { stone: BlockType::Basalt, ..rock(RockKind::Erratic, 4.0, 0.0) };
        assert_eq!(BlockType::Basalt, boulder.block());
        assert_eq!(BlockType::Dirt, rock(RockKind::Mound, 3.0, 6.0).block());
    }

    #[test]
    fn blocks_outside_the_chunk_bounds_are_not_set() {
        let r = rock(RockKind::Erratic, 4.0, 0.0);
        let mut out = Vec::new();
        r.plant(
            I64Vec3::new(40, 100, -20),
            1,
            (IVec3::ZERO, IVec3::splat(15)),
            |at, _| out.push(at),
        );
        assert!(out
            .iter()
            .all(|at| at.cmpge(IVec3::ZERO).all() && at.cmple(IVec3::splat(15)).all()));
        assert!(!out.is_empty());
    }

    #[test]
    fn coarser_cells_hold_about_the_same_volume_of_rock() {
        for r in [
            rock(RockKind::Erratic, 4.0, 0.0),
            rock(RockKind::Mound, 3.5, 6.0),
        ] {
            let count = |scale: i64| {
                let mut n = 0;
                r.plant(
                    I64Vec3::new(0, 0, -100),
                    scale,
                    (IVec3::ZERO, IVec3::splat(200)),
                    |_, _| n += 1,
                );
                (n * scale.pow(3)) as f64
            };
            let (fine, coarse) = (count(1), count(2));
            assert!(
                (coarse / fine - 1.0).abs() < 0.4,
                "{:?} holds {fine} blocks at 1 and {coarse} at 2",
                r.kind
            );
        }
    }
}
