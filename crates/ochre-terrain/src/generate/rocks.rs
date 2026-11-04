//! Boulders and termite mounds: single objects set by a hash of position, built block by block with a chunk, like trees.

use super::trees::{hash, TreeGround};
use bevy::math::{I64Vec2, I64Vec3, IVec3};
use ochre_base::biome::Modifier;
use ochre_base::block::BlockType;
use ochre_base::terrain_source::TerrainSource;
use ochre_base::WATER_LEVEL;

// Objects stand on a grid of cells this many blocks across, at most one in each.
const CELL: i64 = 48;
/// The widest cell, in blocks, of a LOD mesh that shows rocks: past it one is under a pixel.
pub const ROCK_MAX_CELL: i64 = 4;
/// How far out from its centre any rock reaches.
pub const ROCK_REACH: i64 = 6;
const MAX_GRADIENT: f64 = 0.4;
const MAX_SLOPE: f64 = 0.9;
const SEA_CLEARANCE: i64 = 3;

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
    seed: u64,
}

// The kind of object a biome has and the share of its cells that hold one.
fn share(biome: Option<&str>) -> Option<(RockKind, f64)> {
    match biome? {
        "tundra" => Some((RockKind::Erratic, 0.20)),
        "taiga" => Some((RockKind::Erratic, 0.16)),
        "savanna" => Some((RockKind::Mound, 0.30)),
        _ => None,
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
    // dry under it and within its spread of height, or its foot would hang or drown
    for (dx, dz) in [(0, 0), (3, 0), (-3, 0), (0, 3), (0, -3)] {
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
    let growth = ((seed >> 32) % 1000) as f64 / 1000.0;
    let (radius, height) = match kind {
        RockKind::Erratic => (2.0 + 3.0 * growth, 0.0),
        RockKind::Mound => (2.5 + 1.5 * growth, 3.0 + 4.0 * growth),
    };
    Some(Rock {
        kind,
        base: I64Vec3::new(x, ground, z),
        radius,
        height,
        seed,
    })
}

impl Rock {
    /// The block this rock is made of.
    pub fn block(&self) -> BlockType {
        match self.kind {
            RockKind::Erratic => BlockType::Stone,
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
                    (dx / across).powi(2) + (dy / (across * 0.7)).powi(2) + (dz / across).powi(2);
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
        assert_eq!(BlockType::Stone, r.block());
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
