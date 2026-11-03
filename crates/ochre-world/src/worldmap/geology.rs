//! Coarse rock provinces and tilted sedimentary beds. These are geological inputs, not textures.

use super::climate::blur_box;
use super::{par_rows, tectonics, WorldMap};
use ochre_base::block::BlockType;
use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{fbm, unit};

/// The rock at a point. Clay and shale are the soft sedimentary rocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rock {
    Stone,
    Basalt,
    Marble,
    Limestone,
    Clay,
    Sandstone,
    Granite,
    Shale,
}

impl Rock {
    /// Fraction of a droplet's erosion demand taken from this material.
    pub fn hydraulic_erodibility(self) -> f32 {
        match self {
            Self::Stone | Self::Limestone => 0.8,
            Self::Basalt => 0.55,
            Self::Marble => 0.7,
            Self::Clay => 1.0,
            Self::Sandstone => 0.85,
            Self::Granite => 0.45,
            Self::Shale => 0.95,
        }
    }

    /// Relative rate at which streams cut this material. These are artistic tuning values.
    pub fn stream_erodibility(self) -> f32 {
        match self {
            Self::Stone => 1.0,
            Self::Basalt => 0.65,
            Self::Marble => 0.8,
            Self::Limestone => 1.0,
            Self::Clay => 1.5,
            Self::Sandstone => 1.15,
            Self::Granite => 0.55,
            Self::Shale => 1.3,
        }
    }

    /// The block this rock is drawn as.
    pub fn block(self) -> BlockType {
        match self {
            Self::Stone => BlockType::Stone,
            Self::Basalt => BlockType::Basalt,
            Self::Marble => BlockType::Marble,
            Self::Limestone => BlockType::Limestone,
            Self::Clay => BlockType::Clay,
            Self::Sandstone => BlockType::Sandstone,
            Self::Granite => BlockType::Granite,
            Self::Shale => BlockType::Shale,
        }
    }

    // The share of a bed's thickness its face climbs over, the rest being a flat bench: soft clay slopes, hard rock cliffs.
    fn riser(self) -> f64 {
        match self {
            Self::Clay => 0.95,
            Self::Shale => 0.3,
            _ => 0.1,
        }
    }
}

// The province describes a vertical column. Sedimentary provinces contain beds;
// volcanic and metamorphic provinces are treated as massive rock for this first iteration.
const GENERIC: u8 = 0;
const SEDIMENTARY: u8 = 1;
const VOLCANIC: u8 = 2;
const METAMORPHIC: u8 = 3;
// Where a metamorphic belt is uplifted this much (blocks) its core is granite, the marble being the margin.
const INTRUSIVE: u8 = 4;
const GRANITE_UPLIFT: f32 = 450.0;
// Beds average this thick, each boundary wandering by up to `BED_WANDER` of it from an even spacing.
const BED: f64 = 160.0;
const BED_WANDER: f64 = 0.5;
// The share of beds that are soft (half clay, half shale) runs between these over a few hundred kilometres: mostly
// hard benches at the first (mesa country), mostly clay and shale slopes at the second (badlands). The rest are
// limestone or sandstone as the facies noise favours.
const SOFT_SHARE: (f64, f64) = (0.30, 0.70);
const BADLAND_WAVELENGTH: f64 = 12000.0;
// The facies noise is over this many blocks, and sways from sandstone to limestone between these two values.
const FACIES_WAVELENGTH: f64 = 9000.0;
const FACIES_SWAY: (f64, f64) = (-0.1, 0.1);
// Soft beds thin out between these heights (blocks above the sea) until the highest ground is all hard rock, as peaks are capped.
const SOFT_FADE: (f64, f64) = (150.0, 450.0);
// A sedimentary province's edge fades the bedded terraces out over this many cells either side.
const BEDDED_FADE: usize = 4;
// Lava flows stack thinner than beds, are all basalt, and fade out at a volcanic province's edge as beds do.
const FLOW: f64 = 50.0;
const FLOW_RISER: f64 = 0.1;
// Provinces are the most common one this many cells round, so a range is one rock and not a patchwork of boundary types.
const PROVINCE_SMOOTH: usize = 40;
// A face is footed by a scree apron climbing this share of the bed's non-riser share, steepening into the cliff.
const TALUS: f64 = 0.3;
// Bedded ground is kept this share of the way between its ledges, so they don't sit on dead flats.
const KEEPS: f64 = 0.2;

struct Bed {
    rock: Rock,
    bottom: f64,
    top: f64,
}

fn boundary(seed: u32, scale: f64, k: i32) -> f64 {
    (k as f64 + (unit(seed ^ 0xBED5, k, 0) - 0.5) * BED_WANDER) * scale
}

// The number and bounds of the layer at `y` in a stack of layers about `scale` thick.
fn layer(seed: u32, scale: f64, y: f64) -> (i32, f64, f64) {
    let mut k = (y / scale).floor() as i32;
    let (mut bottom, mut top) = (boundary(seed, scale, k), boundary(seed, scale, k + 1));
    if y < bottom {
        k -= 1;
        (bottom, top) = (boundary(seed, scale, k), bottom);
    } else if y >= top {
        k += 1;
        (bottom, top) = (top, boundary(seed, scale, k + 1));
    }
    (k, bottom, top)
}

/// The share of soft beds in the stack at (`x`, `z`) before the highest ground hardens them: it drifts between
/// mesa country and badlands, so a world's sedimentary lands differ from place to place.
pub(super) fn base_soft(seed: u32, x: f64, z: f64) -> f64 {
    let badland = smoothstep(
        0.4,
        0.6,
        (fbm(seed ^ 0x7AB2, x, z, BADLAND_WAVELENGTH, 2) + 1.0) / 2.0,
    );
    SOFT_SHARE.0 + (SOFT_SHARE.1 - SOFT_SHARE.0) * badland
}

// The bed at height `y` of a stack: uneven in thickness, and clay or hard rock by a hash of its number alone, so
// the stack never repeats, the hard rock being limestone where `facies` is high and sandstone where it is low.
fn bed(seed: u32, facies: f64, y: f64, phase: f64, base_soft: f64) -> Bed {
    let (k, bottom, top) = layer(seed, BED, y);
    let r = unit(seed ^ 0xBED6, k, 0);
    let soft = base_soft * (1.0 - smoothstep(SOFT_FADE.0, SOFT_FADE.1, bottom + phase));
    let rock = if r < soft {
        if unit(seed ^ 0x54A1, k, 0) < 0.5 {
            Rock::Clay
        } else {
            Rock::Shale
        }
    } else if (r - soft) / (1.0 - soft) < smoothstep(FACIES_SWAY.0, FACIES_SWAY.1, facies) {
        Rock::Limestone
    } else {
        Rock::Sandstone
    };
    Bed { rock, bottom, top }
}

pub(super) fn generate(map: &mut WorldMap) {
    let config = map.config;
    // each plate is one rock, and a range takes the rock of the older plate it stands on, so a range is not split
    let model = tectonics::Tectonics::new(map.seed, config.size, config.tectonics);
    let mut land = vec![0usize; config.tectonics.plates as usize];
    for i in 0..map.height.len() {
        if map.height[i] >= 0.0 {
            if let Some(count) = land.get_mut(map.plate[i] as usize) {
                *count += 1;
            }
        }
    }
    let rocks = model.plate_rocks(&land);
    let mut geology = vec![(GENERIC, 0.0f32, 0.0f32); map.height.len()];
    par_rows(map.dim(), &mut geology, |row, cells| {
        for (col, output) in cells.iter_mut().enumerate() {
            let i = row * config.dim() + col;
            let (x, z) = map.cell_centre(col, row);
            let province = if map.continentalness[i] < -0.2 {
                VOLCANIC
            } else {
                match rocks.get(map.plate[i] as usize) {
                    Some(&tectonics::ROCK_SHIELD) => METAMORPHIC,
                    Some(&tectonics::ROCK_VOLCANIC) => VOLCANIC,
                    _ => SEDIMENTARY,
                }
            };
            let phase = fbm(map.seed ^ 0x73A9_8C4D, x, z, 18_000.0, 2) as f32 * 400.0;
            let facies = fbm(map.seed ^ 0xFAC1_E5ED, x, z, FACIES_WAVELENGTH, 3) as f32;
            *output = (province, phase, facies);
        }
    });
    for (i, (province, phase, facies)) in geology.into_iter().enumerate() {
        map.geology[i] = province;
        map.strata_offset[i] = phase;
        map.facies[i] = facies;
    }
    smooth_provinces(map);
    // after the smoothing, which would otherwise move the rock of a range's flanks to a neighbour's
    one_rock_per_range(map);
    for i in 0..map.geology.len() {
        if map.geology[i] == METAMORPHIC && map.uplift[i] > GRANITE_UPLIFT {
            map.geology[i] = INTRUSIVE;
        }
    }
    let sedimentary: Vec<f32> = map
        .geology
        .iter()
        .map(|&p| (p == SEDIMENTARY) as u8 as f32)
        .collect();
    map.bedded = blur_box(&sedimentary, map.dim(), BEDDED_FADE);
    let volcanic: Vec<f32> = map
        .geology
        .iter()
        .map(|&p| (p == VOLCANIC) as u8 as f32)
        .collect();
    map.flowed = blur_box(&volcanic, map.dim(), BEDDED_FADE);
}

// High ground this many blocks over the sea, before erosion, is taken as a range for rock: the landform comes later.
const RANGE_PROXY: f32 = 200.0;

// Each connected stretch of high ground is the rock most of it is, so a range stands on one rock and is not cut
// between two where plates' belts meet or an edge of a province crosses it.
fn one_rock_per_range(map: &mut WorldMap) {
    let dim = map.dim();
    let high = |i: usize| map.height[i] >= RANGE_PROXY && map.continentalness[i] >= -0.2;
    let mut seen = vec![false; dim * dim];
    for start in 0..dim * dim {
        if seen[start] || !high(start) {
            continue;
        }
        let (mut stack, mut cells) = (vec![start], Vec::new());
        seen[start] = true;
        while let Some(i) = stack.pop() {
            cells.push(i);
            let (x, z) = (i % dim, i / dim);
            for (dx, dz) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let (nx, nz) = (x as i64 + dx, z as i64 + dz);
                if nx < 0 || nz < 0 || nx >= dim as i64 || nz >= dim as i64 {
                    continue;
                }
                let n = nz as usize * dim + nx as usize;
                if !seen[n] && high(n) {
                    seen[n] = true;
                    stack.push(n);
                }
            }
        }
        let mut votes = [0usize; 5];
        for &i in &cells {
            votes[map.geology[i] as usize] += 1;
        }
        let rock = (0..votes.len())
            .max_by_key(|&p| (votes[p], std::cmp::Reverse(p)))
            .unwrap_or(0);
        for i in cells {
            map.geology[i] = rock as u8;
        }
    }
}

fn smooth_provinces(map: &mut WorldMap) {
    let dim = map.dim();
    let share = |province: u8| {
        let layer: Vec<f32> = map
            .geology
            .iter()
            .map(|&p| (p == province) as u8 as f32)
            .collect();
        blur_box(&layer, dim, PROVINCE_SMOOTH)
    };
    let (sedimentary, volcanic, metamorphic) =
        (share(SEDIMENTARY), share(VOLCANIC), share(METAMORPHIC));
    for i in 0..map.geology.len() {
        map.geology[i] = if volcanic[i] > sedimentary[i] && volcanic[i] >= metamorphic[i] {
            VOLCANIC
        } else if metamorphic[i] > sedimentary[i] {
            METAMORPHIC
        } else {
            SEDIMENTARY
        };
    }
}

// `height` pulled `weight` of the way to the ledge of the layer it is in, `phase` blocks up and between `bounds`.
fn stepped(height: f64, phase: f64, weight: f64, bounds: (f64, f64), riser: f64) -> f64 {
    let thickness = bounds.1 - bounds.0;
    let t = (height - phase - bounds.0) / thickness;
    let apron = TALUS * (1.0 - riser);
    let rise = if t < 1.0 - riser {
        apron * (t.max(0.0) / (1.0 - riser)).powi(2)
    } else {
        apron + (1.0 - apron) * smoothstep(1.0 - riser, 1.0, t)
    };
    let ledge = phase + bounds.0 + thickness * rise;
    height + (ledge + (height - ledge) * KEEPS - height) * weight
}

impl WorldMap {
    /// Rock at a map cell and elevation, including beds below the surface.
    pub fn rock_at_cell(&self, i: usize, y: f32) -> Rock {
        let (x, z) = self.cell_centre(i % self.dim(), i / self.dim());
        self.rock_in(i, y, self.strata_offset[i], base_soft(self.seed, x, z))
    }

    // The rock at `y` in cell `i`'s province, its beds `phase` blocks up.
    fn rock_in(&self, i: usize, y: f32, phase: f32, soft: f64) -> Rock {
        match self.geology[i] {
            VOLCANIC => Rock::Basalt,
            METAMORPHIC => Rock::Marble,
            INTRUSIVE => Rock::Granite,
            SEDIMENTARY => {
                bed(
                    self.seed,
                    self.facies[i] as f64,
                    (y - phase) as f64,
                    phase as f64,
                    soft,
                )
                .rock
            }
            _ => Rock::Stone,
        }
    }

    /// `height` at (`x`, `z`), `above_sea` blocks up, stepped through the beds of the stack there where the ground
    /// is sedimentary: hard beds as benches with cliff faces, clay as slopes. It is a function of height and the
    /// slow drift of the stack alone, so rising ground climbs through the steps in turn and never falls back.
    pub(super) fn terraced(&self, x: f64, z: f64, height: f64, above_sea: f64) -> f64 {
        let high = smoothstep(25.0, 110.0, above_sea);
        let phase = self.bilinear(&self.strata_offset, x, z) as f64;
        let beds = self.bilinear(&self.bedded, x, z) as f64 * high;
        let flows = self.bilinear(&self.flowed, x, z) as f64 * high;
        let mut height = height;
        if beds > 0.0 {
            // the bed's rock is its shape, so the facies need not be known
            let bed = bed(
                self.seed,
                0.0,
                height - phase,
                phase,
                base_soft(self.seed, x, z),
            );
            height = stepped(height, phase, beds, (bed.bottom, bed.top), bed.rock.riser());
        }
        if flows > 0.0 {
            let (_, bottom, top) = layer(self.seed ^ 0xF10E, FLOW, height - phase);
            height = stepped(height, phase, flows, (bottom, top), FLOW_RISER);
        }
        height
    }

    /// Rock at world coordinates: the province of the nearest map cell, its beds drifting smoothly with the map's stack offset.
    pub fn rock_at(&self, x: f64, y: f32, z: f64) -> Rock {
        let (i, phase) = self.rock_cell(x, z);
        self.rock_in(i, y, phase, base_soft(self.seed, x, z))
    }

    // The nearest map cell to a point, and the offset of its beds there.
    fn rock_cell(&self, x: f64, z: f64) -> (usize, f32) {
        let (u, v) = self.to_cell(x, z);
        let max = (self.dim() - 1) as f64;
        let i = self.index(
            u.round().clamp(0.0, max) as usize,
            v.round().clamp(0.0, max) as usize,
        );
        (i, self.bilinear(&self.strata_offset, x, z))
    }

    /// The rocks down the column at (`x`, `z`), as blocks.
    pub fn rock_column(&self, x: f64, z: f64) -> RockColumn<'_> {
        let (i, phase) = self.rock_cell(x, z);
        let soft = base_soft(self.seed, x, z);
        let fixed = (self.geology[i] != SEDIMENTARY).then(|| self.rock_in(i, 0.0, phase, soft));
        RockColumn {
            map: self,
            i,
            soft,
            phase: phase as f64,
            bed: (f64::NAN, f64::NAN, fixed.unwrap_or(Rock::Stone)),
            fixed: fixed.is_some(),
        }
    }
}

/// A column's rocks, remembering the bed last asked for, as ground is read up or down a column in runs.
pub struct RockColumn<'a> {
    map: &'a WorldMap,
    i: usize,
    soft: f64,
    phase: f64,
    bed: (f64, f64, Rock),
    fixed: bool,
}

impl ochre_base::terrain_source::RockColumn for RockColumn<'_> {
    fn bed(&mut self, y: i64) -> (i64, i64, BlockType) {
        let rock = self.rock(y as f64 + 0.5).block();
        if self.fixed {
            return (i64::MIN, i64::MAX, rock);
        }
        // the blocks whose middles are in the bed
        let (bottom, top) = (self.bed.0 + self.phase, self.bed.1 + self.phase);
        (
            (bottom - 0.5).ceil() as i64,
            (top - 0.5).ceil() as i64,
            rock,
        )
    }
}

impl RockColumn<'_> {
    pub fn rock(&mut self, y: f64) -> Rock {
        if !self.fixed && !(self.bed.0 <= y - self.phase && y - self.phase < self.bed.1) {
            let b = bed(
                self.map.seed,
                self.map.facies[self.i] as f64,
                y - self.phase,
                self.phase,
                self.soft,
            );
            self.bed = (b.bottom, b.top, b.rock);
        }
        self.bed.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    // the share of soft beds before tableland varied it
    const DEFAULT_SOFT: f64 = 0.35;

    // far below the sea, where soft beds are at their full share
    const LOW: f64 = -1.0e6;

    fn column(offset: f32) -> WorldMap {
        let mut map = WorldMap::empty(
            1,
            WorldConfig {
                size: 256,
                ..Default::default()
            },
        );
        map.geology.fill(SEDIMENTARY);
        map.bedded.fill(1.0);
        map.strata_offset.fill(offset);
        map
    }

    // The two points of a wide sweep with the least and the most soft rock.
    fn softest_and_hardest(seed: u32) -> ((f64, f64), (f64, f64)) {
        let points: Vec<(f64, f64)> = (0..60)
            .flat_map(|i| {
                (0..60).map(move |j| (i as f64 * 1100.0 - 32000.0, j as f64 * 1100.0 - 32000.0))
            })
            .collect();
        let soft = |p: &&(f64, f64)| base_soft(seed, p.0, p.1);
        let key = |a: &&(f64, f64), b: &&(f64, f64)| soft(a).total_cmp(&soft(b));
        (
            *points.iter().max_by(key).unwrap(),
            *points.iter().min_by(key).unwrap(),
        )
    }

    #[test]
    fn the_soft_share_stays_in_its_range_and_varies_across_the_world() {
        let (badlands, mesas) = softest_and_hardest(1);
        let (hi, lo) = (
            base_soft(1, badlands.0, badlands.1),
            base_soft(1, mesas.0, mesas.1),
        );
        assert!(
            hi <= SOFT_SHARE.1 + 1e-9 && lo >= SOFT_SHARE.0 - 1e-9,
            "{lo} {hi}"
        );
        assert!(hi - lo > 0.25, "badland {hi} against mesa {lo}");
    }

    #[test]
    fn terraced_ground_never_falls_back_in_mesa_or_badland_country() {
        let map = column(0.0);
        for (x, z) in [
            softest_and_hardest(map.seed).0,
            softest_and_hardest(map.seed).1,
        ] {
            let rise = 0.5;
            let profile: Vec<f64> = (0..8000)
                .map(|i| map.terraced(x, z, 300.0 + i as f64 * rise, 200.0 + i as f64 * rise))
                .collect();
            let worst_fall = profile.windows(2).map(|w| w[0] - w[1]).fold(0.0, f64::max);
            assert!(worst_fall <= 1e-9, "fell {worst_fall} at {x},{z}");
        }
    }

    #[test]
    fn a_columns_rock_agrees_with_the_rock_at_a_point() {
        let map = column(7.0);
        let mut disagree = 0;
        for k in 0..1000 {
            let (x, z) = (
                unit(3, k, 1) * 60000.0 - 30000.0,
                unit(3, k, 2) * 60000.0 - 30000.0,
            );
            let y = unit(3, k, 3) * 600.0;
            if map.rock_at(x, y as f32, z) != map.rock_column(x, z).rock(y) {
                disagree += 1;
            }
        }
        assert_eq!(0, disagree);
    }

    #[test]
    fn beds_are_uneven_continuous_and_never_repeat_on_a_cycle() {
        let map = column(0.0);
        let mut thicknesses = Vec::new();
        let mut y = -1000.0;
        while y < 3000.0 {
            let b = bed(map.seed, 0.0, y, LOW, DEFAULT_SOFT);
            assert!(
                b.bottom <= y && y < b.top,
                "{y} outside {}..{}",
                b.bottom,
                b.top
            );
            // the next bed starts where this one ends
            assert_eq!(bed(map.seed, 0.0, b.top, LOW, DEFAULT_SOFT).bottom, b.top);
            thicknesses.push(b.top - b.bottom);
            y = b.top;
        }
        let (min, max) = thicknesses
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), &t| (lo.min(t), hi.max(t)));
        assert!(min < BED * 0.8 && max > BED * 1.2, "thickness {min}..{max}");
        let rocks = |y: f64| {
            (0..8)
                .map(|i| bed(map.seed, 0.0, y + i as f64 * BED, LOW, DEFAULT_SOFT).rock)
                .collect::<Vec<_>>()
        };
        assert_ne!(rocks(0.0), rocks(4.0 * BED));
    }

    #[test]
    fn facies_make_swathes_of_limestone_or_sandstone_with_soft_beds_between() {
        let share = |facies: f64, rock: Rock| {
            let n = 2000;
            (0..n)
                .filter(|&i| bed(1, facies, i as f64 * BED + 3.0, LOW, DEFAULT_SOFT).rock == rock)
                .count() as f64
                / n as f64
        };
        assert!(share(1.0, Rock::Limestone) > 0.6 && share(1.0, Rock::Sandstone) == 0.0);
        assert!(share(-1.0, Rock::Sandstone) > 0.6 && share(-1.0, Rock::Limestone) == 0.0);
        for facies in [-1.0, 1.0] {
            assert!((0.3..0.4).contains(&(share(facies, Rock::Clay) + share(facies, Rock::Shale))));
        }
    }

    #[test]
    fn a_cliff_is_footed_by_a_rising_apron_and_the_ground_never_falls_back() {
        let steps: Vec<f64> = (0..=1000)
            .map(|i| stepped(i as f64 * 0.1, 0.0, 1.0, (0.0, 100.0), 0.1))
            .collect();
        assert!(steps.windows(2).all(|w| w[1] >= w[0]));
        assert!(
            steps[800] > steps[400] + 5.0,
            "no apron: {} vs {}",
            steps[800],
            steps[400]
        );
        assert!(
            steps[900] - steps[800] < 40.0,
            "the apron is a slope, not the cliff"
        );
    }

    #[test]
    fn sedimentary_beds_continue_below_zero_and_follow_the_offset() {
        let mut map = column(0.0);
        map.facies.fill(1.0);
        let at = |map: &WorldMap, y: f32| map.rock_at_cell(0, y);
        let rocks: Vec<_> = (-40..40).map(|i| at(&map, i as f32 * 50.0)).collect();
        assert!(rocks.contains(&Rock::Limestone) && rocks.contains(&Rock::Clay));
        let shifted = column(80.0);
        let mut shifted = shifted;
        shifted.facies.fill(1.0);
        for i in -40..0 {
            let y = i as f32 * 50.0;
            assert_eq!(at(&map, y), at(&shifted, y + 80.0));
        }
    }

    #[test]
    fn bedded_ground_climbs_through_benches_and_faces_and_never_falls_back() {
        let map = column(0.0);
        let rise = 0.5;
        let profile: Vec<f64> = (0..8000)
            .map(|i| {
                map.terraced(
                    100.0,
                    100.0,
                    300.0 + i as f64 * rise,
                    200.0 + i as f64 * rise,
                )
            })
            .collect();
        let worst_fall = profile.windows(2).map(|w| w[0] - w[1]).fold(0.0, f64::max);
        assert!(worst_fall <= 1e-9, "fell {worst_fall}");
        // benches near flat, and faces steeper than the slope
        let slopes: Vec<f64> = profile.windows(2).map(|w| (w[1] - w[0]) / rise).collect();
        assert!(slopes.iter().any(|&s| s < 0.3), "no benches");
        assert!(slopes.iter().any(|&s| s > 1.5), "no cliffs");
    }

    #[test]
    fn lava_flows_step_the_ground_thinner_than_beds_and_never_fall_back() {
        let mut map = column(0.0);
        map.bedded.fill(0.0);
        map.flowed.fill(1.0);
        let rise = 0.25;
        let profile: Vec<f64> = (0..8000)
            .map(|i| {
                map.terraced(
                    100.0,
                    100.0,
                    300.0 + i as f64 * rise,
                    200.0 + i as f64 * rise,
                )
            })
            .collect();
        let worst_fall = profile.windows(2).map(|w| w[0] - w[1]).fold(0.0, f64::max);
        assert!(worst_fall <= 1e-9, "fell {worst_fall}");
        let slopes: Vec<f64> = profile.windows(2).map(|w| (w[1] - w[0]) / rise).collect();
        assert!(slopes.iter().any(|&s| s < 0.3), "no benches");
        assert!(slopes.iter().any(|&s| s > 1.5), "no cliffs");
        // about 2000 blocks of rise at a flow in 50: well over twice the beds' 12
        let faces = slopes
            .windows(2)
            .filter(|w| w[0] <= 1.0 && w[1] > 1.0)
            .count();
        assert!(faces > 25, "{faces} faces");
    }

    #[test]
    fn soft_beds_are_clay_or_shale_and_shale_cliffs_between_the_two() {
        let found: Vec<Rock> = (0..400)
            .map(|k| bed(3, 0.0, k as f64 * BED, LOW, DEFAULT_SOFT).rock)
            .collect();
        assert!(found.contains(&Rock::Clay) && found.contains(&Rock::Shale));
        assert!(Rock::Shale.riser() > Rock::Limestone.riser());
        assert!(Rock::Shale.riser() < Rock::Clay.riser());
    }

    #[test]
    fn granite_is_the_uplifted_core_of_a_metamorphic_belt() {
        let mut cores = 0;
        for seed in [1, 42, 1337] {
            let config = WorldConfig {
                size: 16_384,
                ..Default::default()
            };
            let mut map = super::super::tectonics::generate(seed, config);
            generate(&mut map);
            for i in 0..map.geology.len() {
                if map.geology[i] == INTRUSIVE {
                    cores += 1;
                    assert!(map.uplift[i] > GRANITE_UPLIFT);
                    assert_eq!(Rock::Granite, map.rock_at_cell(i, 100.0));
                }
            }
        }
        assert!(cores > 0, "no granite on three seeds");
    }

    #[test]
    fn clay_slopes_where_hard_rock_cliffs() {
        let map = column(0.0);
        let steepest = |rock: Rock| {
            (0..20_000)
                .map(|i| 300.0 + i as f64 * 0.1)
                .filter(|&h| bed(map.seed, 0.0, h, 0.0, DEFAULT_SOFT).rock == rock)
                .map(|h| {
                    let at = |h: f64| map.terraced(50.0, 50.0, h, 300.0);
                    (at(h + 0.1) - at(h)) / 0.1
                })
                .fold(0.0, f64::max)
        };
        assert!(steepest(Rock::Limestone) > 1.2 * steepest(Rock::Clay));
    }

    #[test]
    fn terraces_fade_out_off_sedimentary_ground_and_off_the_high_ground() {
        let mut map = column(0.0);
        assert_ne!(map.terraced(50.0, 50.0, 700.0, 300.0), 700.0);
        assert_eq!(map.terraced(50.0, 50.0, 700.0, 10.0), 700.0);
        map.bedded.fill(0.0);
        assert_eq!(map.terraced(50.0, 50.0, 700.0, 300.0), 700.0);
    }

    #[test]
    fn plates_share_the_land_between_basin_shield_and_volcanic_rock() {
        for seed in [1u32, 42, 1337, 77] {
            let model = tectonics::Tectonics::new(seed, 65_536, Default::default());
            let land: Vec<usize> = (0..model.plates.len())
                .map(|i| 1000 + (unit(seed, i as i32, 5) * 9000.0) as usize)
                .collect();
            let rocks = model.plate_rocks(&land);
            assert_eq!(rocks, model.plate_rocks(&land), "not repeatable");
            let total: usize = land.iter().sum();
            for rock in [
                tectonics::ROCK_BASIN,
                tectonics::ROCK_SHIELD,
                tectonics::ROCK_VOLCANIC,
            ] {
                let share = land
                    .iter()
                    .zip(&rocks)
                    .filter(|(_, &r)| r == rock)
                    .map(|(l, _)| *l)
                    .sum::<usize>() as f64
                    / total as f64;
                assert!(
                    (0.05..=0.75).contains(&share),
                    "seed {seed}: rock {rock} has {share} of the land"
                );
            }
        }
    }

    #[test]
    fn a_range_of_high_ground_stands_on_one_rock() {
        for seed in [1u32, 42, 1337] {
            let config = WorldConfig {
                size: 16_384,
                ..Default::default()
            };
            let mut map = super::super::tectonics::generate(seed, config);
            generate(&mut map);
            let dim = map.dim();
            let high = |i: usize| map.height[i] >= RANGE_PROXY && map.continentalness[i] >= -0.2;
            let mut seen = vec![false; dim * dim];
            let mut big = 0;
            for start in 0..dim * dim {
                if seen[start] || !high(start) {
                    continue;
                }
                let (mut stack, mut cells) = (vec![start], Vec::new());
                seen[start] = true;
                while let Some(i) = stack.pop() {
                    cells.push(i);
                    let (x, z) = (i % dim, i / dim);
                    for (nx, nz) in [
                        (x + 1, z),
                        (x.wrapping_sub(1), z),
                        (x, z + 1),
                        (x, z.wrapping_sub(1)),
                    ] {
                        if nx < dim && nz < dim && !seen[nz * dim + nx] && high(nz * dim + nx) {
                            seen[nz * dim + nx] = true;
                            stack.push(nz * dim + nx);
                        }
                    }
                }
                if cells.len() < 300 {
                    continue;
                }
                big += 1;
                let mut votes = [0usize; 5];
                for &i in &cells {
                    // the granite core and marble margin of a metamorphic range are one province here
                    let p = map.geology[i];
                    votes[if p == INTRUSIVE { METAMORPHIC } else { p } as usize] += 1;
                }
                let most = *votes.iter().max().unwrap();
                assert!(
                    most as f64 >= 0.8 * cells.len() as f64,
                    "seed {seed}: a range of {} cells is split {votes:?}",
                    cells.len()
                );
            }
            assert!(big > 0, "seed {seed} has no range to check");
        }
    }

    #[test]
    fn generated_geology_repeats_for_the_same_seed() {
        let config = WorldConfig {
            size: 8192,
            ..Default::default()
        };
        let mut a = super::super::tectonics::generate(42, config);
        let mut b = a.clone();
        generate(&mut a);
        generate(&mut b);
        assert_eq!(a.geology, b.geology);
        assert_eq!(a.strata_offset, b.strata_offset);
        assert!(a
            .geology
            .iter()
            .all(|&p| (SEDIMENTARY..=INTRUSIVE).contains(&p)));
        assert!(a.strata_offset.iter().all(|p| p.is_finite()));
        assert!(a.facies.iter().all(|f| f.is_finite()));
        assert!(a.bedded.iter().all(|b| (0.0..=1.0).contains(b)));
        assert_eq!(a.facies, b.facies);
        assert_eq!(a.bedded, b.bedded);
        for i in [0, a.height.len() / 2, a.height.len() - 1] {
            let (x, z) = a.cell_centre(i % a.dim(), i / a.dim());
            assert_eq!(a.rock_at(x, 123.0, z), a.rock_at_cell(i, 123.0));
        }
    }

    #[test]
    fn softer_rock_erodes_faster_under_the_same_stream() {
        let config = WorldConfig {
            size: 1024,
            ..Default::default()
        };
        let mut hard = WorldMap::empty(1, config);
        for z in 0..hard.dim() {
            for x in 0..hard.dim() {
                let i = hard.index(x, z);
                hard.height[i] = if z == 0 { -1.0 } else { 320.0 + z as f32 * 4.0 };
                hard.geology[i] = VOLCANIC;
            }
        }
        let mut soft = hard.clone();
        soft.geology.fill(SEDIMENTARY);
        super::super::erosion::stream_power(&mut hard);
        super::super::erosion::stream_power(&mut soft);
        let hard_sum: f64 = hard.height.iter().map(|&h| h as f64).sum();
        let soft_sum: f64 = soft.height.iter().map(|&h| h as f64).sum();
        assert!(soft_sum < hard_sum, "soft {soft_sum}, hard {hard_sum}");
    }

    #[test]
    fn softer_rock_slumps_further_without_losing_mass() {
        let config = WorldConfig {
            size: 1024,
            ..Default::default()
        };
        let mut hard = WorldMap::empty(1, config);
        hard.height.fill(500.0);
        hard.geology.fill(VOLCANIC);
        let spike = hard.index(16, 16);
        hard.height[spike] = 900.0;
        let mut soft = hard.clone();
        soft.geology.fill(SEDIMENTARY);
        // The spike begins in a clay bed and exposes lower beds as it slumps.
        soft.strata_offset.fill(500.0);
        let before: f64 = hard.height.iter().map(|&h| h as f64).sum();
        super::super::erosion::thermal(&mut hard);
        super::super::erosion::thermal(&mut soft);
        assert!(
            soft.height[spike] < hard.height[spike],
            "soft {}, hard {}",
            soft.height[spike],
            hard.height[spike]
        );
        for map in [&hard, &soft] {
            let after: f64 = map.height.iter().map(|&h| h as f64).sum();
            assert!((after - before).abs() < 1.0, "mass {before} -> {after}");
        }
    }

    #[test]
    fn droplets_cut_soft_rock_more_than_hard_rock() {
        let config = WorldConfig {
            size: 4096,
            ..Default::default()
        };
        let mut hard = WorldMap::empty(7, config);
        let dim = hard.dim();
        for z in 0..dim {
            for x in 0..dim {
                let i = hard.index(x, z);
                hard.height[i] = 340.0 + z as f32;
                hard.geology[i] = VOLCANIC;
            }
        }
        let mut soft = hard.clone();
        soft.geology.fill(SEDIMENTARY);
        let before = hard.height.clone();
        super::super::erosion::hydraulic(&mut hard);
        super::super::erosion::hydraulic(&mut soft);
        let cut = |map: &WorldMap| -> f64 {
            before
                .iter()
                .zip(&map.height)
                .map(|(a, b)| (a - b).max(0.0) as f64)
                .sum()
        };
        assert!(
            cut(&soft) > cut(&hard),
            "soft {}, hard {}",
            cut(&soft),
            cut(&hard)
        );
    }
}
