//! Boulders and termite mounds: single objects set by a hash of position, built block by block with a chunk, like trees.

use super::trees::{hash, TreeGround};
use bevy::math::{I64Vec2, I64Vec3, IVec3};
use ochre_base::biome::Modifier;
use ochre_base::block::BlockType;
use ochre_base::terrain_source::{RockColumn, TerrainSource};
use ochre_base::WATER_LEVEL;

// Objects stand on a grid of cells this many blocks across, at most one in each.
const CELL: i64 = 24;
/// The widest cell, in blocks, of a LOD mesh that shows rocks: past it even a giant is under a pixel.
pub const ROCK_MAX_CELL: i64 = 8;
// The widest cell that still shows the ledges, mounds and taiga boulders on the `CELL` grid, and the one that shows small rocks.
const MEDIUM_MAX_CELL: i64 = 4;
const SMALL_MAX_CELL: i64 = 2;
/// How far out from its centre any rock reaches: a giant's arch.
pub const ROCK_REACH: i64 = 20;
// How far a rock on the `CELL` grid reaches: the widest ledge.
const MEDIUM_REACH: i64 = 12;
const MAX_GRADIENT: f64 = 0.4;
const MAX_SLOPE: f64 = 0.9;
const SEA_CLEARANCE: i64 = 3;

// The tundra's rocks on the `CELL` grid are outcrops, wide low ledges of rock, this share of the cells it once held boulders in,
// between these radii and this much of their width tall.
const OUTCROP_SHARE: f64 = 0.12;
const OUTCROP_RADIUS: (f64, f64) = (5.5, 11.0);
const OUTCROP_SQUASH: f64 = 0.32;
// How far height may differ over an outcrop's foot before it would hang or be buried.
const OUTCROP_SPREAD: i64 = 6;

// The tundra's small rocks: a finer grid, thickened and thinned by the same fields as the ledges, so they come in drifts.
const SMALL_CELL: i64 = 12;
const SMALL_REACH: i64 = 3;
const SMALL_SHARE: f64 = 0.3;
const SMALL_RADIUS: (f64, f64) = (0.9, 2.3);

// Giants, the tundra's rare landmarks, on a coarse grid: a share of its cells hold one, a lone tall boulder or an arch of two
// pillars with a gap between and a capstone over it, a passage to walk through and build into.
const GIANT_CELL: i64 = 96;
const GIANT_SHARE: f64 = 0.054;
const GIANT_ARCH_SHARE: f64 = 0.22;
const GIANT_RADIUS: (f64, f64) = (7.0, 11.0);
// How far apart an arch's pillars stand, edge to edge, and how high over the ground the capstone's underside is, in blocks.
const ARCH_GAP: (f64, f64) = (3.5, 4.5);
const ARCH_CLEARANCE: f64 = 4.5;
// How far height may differ over a giant's feet.
const GIANT_SPREAD: i64 = 8;

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
    // Outcrops narrow across one axis, sometimes turned diagonally.
    aspect: f64,
    direction: u8,
    // how far the rock's middle is raised above where a half-sunk one's is, for an arch's capstone
    lift: f64,
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
    let (cx, cz) = (x.div_euclid(FIELD), z.div_euclid(FIELD));
    let fade = |v: i64| {
        let t = v.rem_euclid(FIELD) as f64 / FIELD as f64;
        t * t * (3.0 - 2.0 * t)
    };
    let sample = |x, z| (hash(seed ^ 0xF1E1_D5ED, x, z) >> 11) as f64 / (1u64 << 53) as f64;
    let lerp = |a, b, t| a + (b - a) * t;
    let (u, v) = (fade(x), fade(z));
    let p = lerp(lerp(sample(cx, cz), sample(cx + 1, cz), u), lerp(sample(cx, cz + 1), sample(cx + 1, cz + 1), u), v);
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

// The cells of a grid of `cell` blocks that could hold something reaching `reach` into the columns `min..=max` (x, z).
fn cells_near(min: I64Vec2, max: I64Vec2, cell: i64, reach: i64) -> impl Iterator<Item = (i64, i64)> {
    let first = (min - reach).div_euclid(I64Vec2::splat(cell));
    let last = (max + reach).div_euclid(I64Vec2::splat(cell));
    (first.y..=last.y).flat_map(move |cz| (first.x..=last.x).map(move |cx| (cx, cz)))
}

// Every rock that could reach into the columns `min..=max` (x, z), as seen from cells `scale` blocks across: a LOD node leaves out
// what would be under a pixel.
pub fn rocks_near(
    noise: &dyn TerrainSource,
    heights: &mut impl TreeGround,
    min: I64Vec2,
    max: I64Vec2,
    scale: i64,
) -> Vec<Rock> {
    let mut rocks = Vec::new();
    if scale <= SMALL_MAX_CELL {
        for (cx, cz) in cells_near(min, max, SMALL_CELL, SMALL_REACH) {
            rocks.extend(small_in_cell(noise, heights, cx, cz));
        }
    }
    if scale <= MEDIUM_MAX_CELL {
        for (cx, cz) in cells_near(min, max, CELL, MEDIUM_REACH) {
            rocks.extend(rock_in_cell(noise, heights, cx, cz));
        }
    }
    for (cx, cz) in cells_near(min, max, GIANT_CELL, ROCK_REACH) {
        rocks.extend(giant_in_cell(noise, heights, cx, cz));
    }
    rocks
}

// 0 to 1 from 16 bits of `h` from bit `shift` up.
fn frac(h: u64, shift: u32) -> f64 {
    ((h >> shift) & 0xffff) as f64 / 65536.0
}

// The way a rock's long axis runs, as `direction` says, as a unit step in x and z.
fn axis(direction: u8) -> (f64, f64) {
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    match direction {
        0 => (1.0, 0.0),
        1 => (0.0, 1.0),
        2 => (diagonal, diagonal),
        _ => (diagonal, -diagonal),
    }
}

// A small rock on the tundra's ground, its size leaning to the least, or none: the ledges and giants are what stand out.
fn small_in_cell(
    noise: &dyn TerrainSource,
    heights: &mut impl TreeGround,
    cx: i64,
    cz: i64,
) -> Option<Rock> {
    let seed = hash(noise.seed() as u64 ^ 0x5A11_70C4, cx, cz);
    let x = cx * SMALL_CELL + ((seed >> 16) % SMALL_CELL as u64) as i64;
    let z = cz * SMALL_CELL + ((seed >> 24) % SMALL_CELL as u64) as i64;
    let biome = heights.biome(x, z);
    if noise.biomes().name(biome) != Some("tundra") || noise.biomes().modifier(biome) != Modifier::None {
        return None;
    }
    let share = (SMALL_SHARE * field(noise.seed() as u64, x, z)).min(1.0);
    if frac(seed, 0) >= share {
        return None;
    }
    let (gradient_x, gradient_z) = heights.gradient(x, z);
    if gradient_x.abs() + gradient_z.abs() > MAX_SLOPE || heights.wet(x, z) {
        return None;
    }
    let ground = heights.height(x, z).round() as i64;
    if ground < WATER_LEVEL + SEA_CLEARANCE {
        return None;
    }
    let size = frac(seed, 32);
    Some(Rock {
        kind: RockKind::Erratic,
        base: I64Vec3::new(x, ground, z),
        radius: SMALL_RADIUS.0 + (SMALL_RADIUS.1 - SMALL_RADIUS.0) * size * size,
        height: 0.0,
        squash: 0.5 + 0.4 * frac(seed, 42),
        aspect: 1.0,
        direction: ((seed >> 60) & 3) as u8,
        lift: 0.0,
        stone: stone_of(RockKind::Erratic, &mut *noise.rock_column(x, z), ground),
        seed,
    })
}

// What stands in a cell of the giants' grid, if anything: the rocks that make it, a lone tall boulder or the three of an arch.
fn giant_in_cell(
    noise: &dyn TerrainSource,
    heights: &mut impl TreeGround,
    cx: i64,
    cz: i64,
) -> Vec<Rock> {
    let seed = hash(noise.seed() as u64 ^ 0x61A7_70C4, cx, cz);
    // kept off the cell's edge, so neighbours' giants stay apart
    let margin = ROCK_REACH;
    let span = (GIANT_CELL - 2 * margin) as u64;
    let x = cx * GIANT_CELL + margin + ((seed >> 16) % span) as i64;
    let z = cz * GIANT_CELL + margin + ((seed >> 24) % span) as i64;
    let biome = heights.biome(x, z);
    if frac(seed, 0) >= GIANT_SHARE
        || noise.biomes().name(biome) != Some("tundra")
        || noise.biomes().modifier(biome) != Modifier::None
    {
        return Vec::new();
    }
    let lumps = if frac(seed, 8) < GIANT_ARCH_SHARE { arch(x, z, seed) } else { vec![lone_giant(x, z, seed)] };
    // each foot dry, over the sea's reach, and the lot on ground that doesn't vary too much to stand on
    let (mut low, mut high) = (i64::MAX, i64::MIN);
    let mut grounds = Vec::new();
    for lump in &lumps {
        let foot = (lump.radius * 0.5).round() as i64;
        let (lx, lz) = (lump.base.x, lump.base.z);
        let mut here = i64::MAX;
        for (dx, dz) in [(0, 0), (foot, 0), (-foot, 0), (0, foot), (0, -foot)] {
            if heights.wet(lx + dx, lz + dz) {
                return Vec::new();
            }
            let ground = heights.height(lx + dx, lz + dz).round() as i64;
            here = here.min(ground);
            (low, high) = (low.min(ground), high.max(ground));
        }
        grounds.push(here);
    }
    let (gradient_x, gradient_z) = heights.gradient(x, z);
    if high - low > GIANT_SPREAD || low < WATER_LEVEL + SEA_CLEARANCE || gradient_x.abs() + gradient_z.abs() > 1.2 {
        return Vec::new();
    }
    // a capstone stands over the lower pillar's ground, and a pillar or boulder on its own
    let capstone = grounds.iter().copied().min().unwrap_or(low);
    lumps
        .into_iter()
        .zip(grounds)
        .map(|(mut lump, ground)| {
            let ground = if lump.lift > 0.0 { capstone } else { ground };
            lump.base.y = ground;
            lump.stone = stone_of(RockKind::Erratic, &mut *noise.rock_column(lump.base.x, lump.base.z), ground);
            lump
        })
        .collect()
}

// A tall boulder: round, not squashed, as high as it is wide.
fn lone_giant(x: i64, z: i64, seed: u64) -> Rock {
    Rock {
        kind: RockKind::Erratic,
        base: I64Vec3::new(x, 0, z),
        radius: GIANT_RADIUS.0 + (GIANT_RADIUS.1 - GIANT_RADIUS.0) * frac(seed, 32),
        height: 0.0,
        squash: 0.85 + 0.3 * frac(seed, 48),
        aspect: 0.7 + 0.3 * frac(seed, 20),
        direction: ((seed >> 60) & 3) as u8,
        lift: 0.0,
        stone: BlockType::Rock,
        seed,
    }
}

// Two pillars with a gap between them a player walks through, and a long capstone laid over the gap and resting on both, as
// `ARCH_CLEARANCE` over the ground at its underside. Their `base.y` is for the caller to set from the ground.
fn arch(x: i64, z: i64, seed: u64) -> Vec<Rock> {
    let direction = ((seed >> 60) & 3) as u8;
    let (ux, uz) = axis(direction);
    let pillar = |index: u64| GIANT_RADIUS.0 - 1.0 + 3.0 * frac(seed, 24 + 8 * index as u32);
    let (r1, r2) = (pillar(0), pillar(1));
    let gap = ARCH_GAP.0 + (ARCH_GAP.1 - ARCH_GAP.0) * frac(seed, 40);
    // centres along the axis so the pillars' edges are a gap apart
    let apart = r1 + r2 + gap;
    let (a, b) = (-apart / 2.0 + (r1 - r2) / 2.0, apart / 2.0 + (r1 - r2) / 2.0);
    let at = |along: f64| I64Vec3::new(x + (ux * along).round() as i64, 0, z + (uz * along).round() as i64);
    let make = |along: f64, radius: f64, squash: f64, k: u64| Rock {
        kind: RockKind::Erratic,
        base: at(along),
        radius,
        height: 0.0,
        squash,
        aspect: 1.0,
        direction,
        lift: 0.0,
        stone: BlockType::Rock,
        seed: seed ^ k,
    };
    let first = make(a, r1, 1.05 + 0.15 * frac(seed, 4), 1);
    let second = make(b, r2, 1.05 + 0.15 * frac(seed, 12), 2);
    // long enough to rest on both pillars, narrower across than along, thin
    let cap_radius = apart / 2.0 + 0.55 * r1.min(r2);
    let cap_squash = 0.28;
    let mut cap = make((a + b) / 2.0, cap_radius, cap_squash, 3);
    cap.aspect = 0.55;
    // a half-sunk rock's middle is a quarter radius up, and its underside a semi-axis under that
    cap.lift = ARCH_CLEARANCE + cap_radius * cap_squash - cap_radius * 0.25;
    vec![first, second, cap]
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
        // the tundra's `CELL` grid holds only the ledges now, a share of what it once held as boulders
        RockKind::Erratic if noise.biomes().name(biome) == Some("tundra") => (share * OUTCROP_SHARE * field(noise.seed() as u64, x, z)).min(1.0),
        RockKind::Erratic => {
            let p = (hash(noise.seed() as u64 ^ 0xF1E1_D5ED, x.div_euclid(FIELD), z.div_euclid(FIELD)) >> 11) as f64 / (1u64 << 53) as f64;
            (share * (FIELD_FACTOR.0 + (FIELD_FACTOR.1 - FIELD_FACTOR.0) * p * p)).min(1.0)
        }
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
    let outcrop = kind == RockKind::Erratic && noise.biomes().name(biome) == Some("tundra");
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
        aspect: if outcrop { 0.45 + 0.3 * ((seed >> 12) & 255) as f64 / 255.0 } else { 1.0 },
        direction: ((seed >> 60) & 3) as u8,
        lift: 0.0,
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
            RockKind::Erratic => reach + self.lift.ceil() as i64,
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
                let dy = y - (self.base.y as f64 + self.radius * 0.25 + self.lift);
                let across = self.radius;
                let (major, minor) = match self.direction {
                    0 => (dx, dz),
                    1 => (dz, dx),
                    2 => ((dx + dz) * std::f64::consts::FRAC_1_SQRT_2, (dx - dz) * std::f64::consts::FRAC_1_SQRT_2),
                    _ => ((dx - dz) * std::f64::consts::FRAC_1_SQRT_2, (dx + dz) * std::f64::consts::FRAC_1_SQRT_2),
                };
                let d2 =
                    (major / across).powi(2) + (dy / (across * self.squash)).powi(2) + (minor / (across * self.aspect)).powi(2);
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
            aspect: 1.0,
            direction: 0,
            lift: 0.0,
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
    fn the_tundra_is_strewn_with_small_rocks_and_a_taiga_has_few_boulders() {
        let small = SMALL_SHARE / (SMALL_CELL * SMALL_CELL) as f64;
        let taiga = share(Some("taiga")).unwrap().1 / (CELL * CELL) as f64;
        assert_eq!(RockKind::Erratic, share(Some("tundra")).unwrap().0);
        assert!(small > 8.0 * taiga);
        // up to about one in a square 16 blocks across, where its fields are thickest
        assert!((1.0 / (small * 2.0)).sqrt() < 16.0);
        // and the fields vary: patches hold between a tenth and twice the share
        let factors: Vec<f64> = (0..200).map(|i| field(3, i * FIELD, 0)).collect();
        assert!(factors.iter().cloned().fold(f64::MAX, f64::min) < 0.3 && factors.iter().cloned().fold(0.0, f64::max) > 1.5);
        for x in -200..200 {
            assert!((field(3, x, 17) - field(3, x + 1, 17)).abs() < 0.05);
        }
        // small rocks stay small, the ledges and giants are what stand out, and a giant fits the reach rocks are looked for in
        assert!(SMALL_RADIUS.1 * 2.0 < OUTCROP_RADIUS.0);
        assert!(OUTCROP_RADIUS.1 + 1.0 <= MEDIUM_REACH as f64 && MEDIUM_REACH <= ROCK_REACH);
        assert!(GIANT_RADIUS.0 > OUTCROP_RADIUS.0);
    }

    // An arch's pillars are far enough apart that a player can walk between them, under a capstone that rests on both.
    #[test]
    fn an_arch_has_a_passage_between_its_pillars_under_a_capstone() {
        for seed in 1..40u64 {
            let (x, z) = (40, -20);
            let lumps: Vec<Rock> = arch(x, z, seed << 60 | seed).into_iter().map(|mut r| { r.base.y = 100; r }).collect();
            assert_eq!(3, lumps.len());
            let mut solid = std::collections::HashSet::new();
            let size = IVec3::splat(200);
            for lump in &lumps {
                lump.plant(I64Vec3::new(0, 0, -100), 1, (IVec3::ZERO, size), |at, _| {
                    solid.insert(at);
                });
            }
            // the cells of the middle of the gap, between the pillars: the axis the pillars lie along is `direction`'s, and the
            // middle of the arch is where it crosses the walk-through
            let (ux, uz) = axis(((seed << 60 | seed) >> 60) as u8 & 3);
            let (vx, vz) = (-uz, ux);
            let middle = |along: f64, across: f64| {
                IVec3::new((x as f64 + ux * along + vx * across).round() as i32, 0, (z as f64 + uz * along + vz * across).round() as i32 + 100)
            };
            let free_between = |y: i32| (-10..=10).all(|v| !solid.contains(&(middle(0.0, v as f64) + IVec3::Y * y)));
            // a body two blocks high walks through it at the ground, from one side to the other
            assert!(
                (1..=3).all(|up| free_between(100 + up)),
                "seed {seed}: no way through at the ground"
            );
            // under a capstone across the gap, and the pillars either side
            assert!(
                (100 + ARCH_CLEARANCE as i32..110).any(|y| solid.contains(&(middle(0.0, 0.0) + IVec3::Y * y))),
                "seed {seed}: nothing over the gap"
            );
            let pillars = [&lumps[0], &lumps[1]];
            for p in pillars {
                assert!(solid.contains(&IVec3::new(p.base.x as i32, 102, p.base.z as i32 + 100)), "seed {seed}: no pillar");
            }
            // the capstone overlaps both pillars, so it rests on them instead of floating over the gap
            let cells = |rock: &Rock| {
                let mut out = std::collections::HashSet::new();
                rock.plant(I64Vec3::new(0, 0, -100), 1, (IVec3::ZERO, size), |at, _| {
                    out.insert(at);
                });
                out
            };
            let cap = cells(&lumps[2]);
            for p in pillars {
                assert!(cells(p).iter().any(|c| cap.contains(c)), "seed {seed}: the capstone floats");
            }
            // every lump within the reach neighbouring chunks look for rocks in
            for lump in &lumps {
                let d = ((lump.base.x - x) as f64).hypot((lump.base.z - z) as f64) + lump.radius;
                assert!(d <= ROCK_REACH as f64, "seed {seed}: {d} out");
            }
        }
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
        let ledge = blocks(&Rock { squash: OUTCROP_SQUASH, aspect: 0.5, ..rock(RockKind::Erratic, 9.0, 0.0) });
        assert!(width(&ledge) >= 17.0, "{} wide", width(&ledge));
        let depth = ledge.iter().map(|p| p.z).max().unwrap() - ledge.iter().map(|p| p.z).min().unwrap() + 1;
        assert!((depth as f64) < width(&ledge) * 0.7);
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
