//! Valleys: long troughs cut into inland ground before water is routed, a pure function of seed and place so block level reads the same field.

use super::climate::{blur_box, closeness, moisture_field, FIELD_BLUR, LAPSE_PER_BLOCK};
use super::hydrology::{basins, fill_where, flood_ties, priority_flood};
use super::landform::MOUNTAIN;
use super::{par_rows, WaterCell, WorldMap};
use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::perlin;
use ochre_base::WATER_LEVEL;

const SALT_FIELD: u32 = 0xA11E_0001;
const SALT_WARP: u32 = 0xA11E_0002;
const SALT_DEPTH: u32 = 0xA11E_0003;
const SALT_FADE: u32 = 0xA11E_0004;
const SALT_BEND: u32 = 0xA11E_0005;
const SALT_RAGGED: u32 = 0xA11E_0006;
const SALT_FINE: u32 = 0xA11E_0007;
const SALT_STEEP: u32 = 0xA11E_0008;
const SALT_CLIFF: u32 = 0xA11E_0009;
const SALT_LEDGE: u32 = 0xA11E_000A;
const SALT_MEANDER: u32 = 0xA11E_000B;
const SALT_ALPINE: u32 = 0xA11E_00A1;
// Cells each way the ranges' edge is softened over, so the valleys fade out toward the foothills.
const ALPINE_EDGE: usize = 5;
// A basin the ranges' valleys made this many blocks deep or less is filled; a deeper one is left a mountain lake.
const ALPINE_SHALLOW_BASIN: f32 = 45.0;

// Cells each way steppe is smoothed over, how far toward that it is pulled, the heights over the sea it is pulled between,
// the cells each way a mountain keeps it off, and the cells each way the zone's edge is softened over.
const PLAIN_EDGE: usize = 40;
const PLAIN_RADIUS: usize = 60;
const PLAIN_PULL: f64 = 0.9;
const PLAIN_ABOVE: (f64, f64) = (20.0, 80.0);
const PLAIN_MOUNTAIN: usize = 8;
// Slopes above this are mountain faces, and no valley is cut into them.
const FLAT_BELOW: (f64, f64) = (0.2, 0.4);
// A basin with this many cells, a third of it, that no basin held before the cut was made by the valleys.
const NEW_CELLS: usize = 30;
// A basin with this many cells, two thirds of them in a valley, is the valley's trough, taken whether or not it was a basin before.
const VALLEY_BASIN_CELLS: usize = 8;
// A valley basin this many blocks deep or less, or this many cells or fewer, is filled; a deeper one is left.
const SHALLOW_BASIN: f32 = 12.0;
const TINY_BASIN: usize = 12;
// Cells in a deep basin big enough to fill into a lake that floods the ground round it.
const BIG_BASIN: usize = 2000;
// Cells each way the footprint of a big basin is blurred over: the margin round it with no valley, fading out.
const FEED_BLUR: usize = 14;
// A basin this many blocks deep once filled is a lake to be, and no valley is cut through it.
const BASIN_DEPTH: f32 = 6.0;

/// How a valley is laid out. Nothing in it names a biome, so another preset can reuse the field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValleyProfile {
    /// Blocks between valley centres across their axis.
    pub period: f64,
    /// How many times longer a valley is than it is far from the next.
    pub stretch: f64,
    pub floor: f64,
    /// Blocks a wall takes to rise on the map.
    pub soft_edge: f64,
    /// Blocks a wall takes to rise at block level, from its steepest to its gentlest stretch.
    pub sharp_edge: (f64, f64),
    /// (wavelength, amplitude) wobbles of a wall's line: coarse ones the map sees too, fine ones only block level.
    pub coarse: [(f64, f64); 2],
    pub fine: [(f64, f64); 2],
    pub depth: (f64, f64),
    /// Blocks along an axis between the shallow and deep reaches of a valley, and the share of depth the shallow ones keep.
    pub fade_wavelength: f64,
    pub fade_min: f64,
    /// Blocks over the sea a valley never cuts down to, and the height over the sea above which none is cut.
    pub sea_margin: f64,
    pub upper: f64,
    /// The share of the full weight ground gets away from where the siting says to favour.
    pub base: f64,
}

pub const STEPPE: ValleyProfile = ValleyProfile {
    period: 2400.0,
    stretch: 2.0,
    floor: 200.0,
    soft_edge: 200.0,
    sharp_edge: (14.0, 64.0),
    coarse: [(700.0, 90.0), (300.0, 40.0)],
    fine: [(120.0, 16.0), (50.0, 7.0)],
    depth: (40.0, 70.0),
    fade_wavelength: 3500.0,
    fade_min: 0.4,
    sea_margin: 6.0,
    upper: 220.0,
    base: 0.25,
};

/// The valleys cut through ranges: long, deep, wide-floored troughs a few kilometres apart, a meadow and a river on the floor to rest and cross by, walls that
/// rise over several hundred blocks with cliff bands, and the ridges between left standing.
pub const ALPINE: ValleyProfile = ValleyProfile {
    period: 5200.0,
    stretch: 2.5,
    floor: 220.0,
    soft_edge: 700.0,
    sharp_edge: (20.0, 90.0),
    coarse: [(1400.0, 160.0), (600.0, 70.0)],
    fine: [(160.0, 24.0), (60.0, 10.0)],
    depth: (140.0, 260.0),
    fade_wavelength: 6000.0,
    fade_min: 0.5,
    sea_margin: 20.0,
    upper: 1400.0,
    base: 0.0,
};

/// A valley's depth in blocks and its wall's rise, 0..1, as the map (`soft`) and a block (`sharp`) see it; `cliff` is how much of the slope is a face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cut {
    pub depth: f64,
    pub soft: f64,
    pub sharp: f64,
    pub cliff: f64,
}

/// A world's valleys: pure in the seed, the axis and the profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Valley {
    seed: u32,
    cos: f64,
    sin: f64,
    profile: ValleyProfile,
}

// Soft and sharp masks: the face stays inside the slope, so both are 1 on the floor and 0 on the plateau.
fn masks(profile: &ValleyProfile, dist: f64, sharp_dist: f64, edge: f64, at: f64) -> (f64, f64) {
    let half = profile.floor / 2.0;
    let at = at.clamp(
        0.5 * edge / profile.soft_edge,
        1.0 - 0.5 * edge / profile.soft_edge,
    );
    let mid = half + profile.soft_edge * at;
    let soft = 1.0 - smoothstep(half, half + profile.soft_edge, dist);
    let sharp = 1.0 - smoothstep(mid - edge / 2.0, mid + edge / 2.0, sharp_dist);
    (soft, sharp)
}

// Blocks to add to a distance at (x, z), from noise of each (wavelength, amplitude).
fn ragged(seed: u32, x: f64, z: f64, waves: &[(f64, f64)]) -> f64 {
    waves
        .iter()
        .enumerate()
        .map(|(i, &(wavelength, amplitude))| {
            amplitude * perlin(seed.wrapping_add(i as u32), x / wavelength, z / wavelength)
        })
        .sum()
}

impl Valley {
    pub fn new(seed: u32, angle: f64, profile: ValleyProfile) -> Self {
        Self {
            seed,
            cos: libm::cos(angle),
            sin: libm::sin(angle),
            profile,
        }
    }

    /// The steppe valleys of `map`, running along the world's ridge angle.
    pub fn of(map: &WorldMap) -> Self {
        Self::new(map.seed, map.config.climate.gradient_angle as f64, STEPPE)
    }

    /// The valleys of `map`'s ranges, across the world's ridge angle and from a field of their own.
    pub fn alpine(map: &WorldMap) -> Self {
        Self::new(map.seed ^ SALT_ALPINE, map.config.climate.gradient_angle as f64 + 1.1, ALPINE)
    }

    pub fn at(&self, x: f64, z: f64) -> Cut {
        let p = &self.profile;
        let (along, across) = (x * self.cos + z * self.sin, z * self.cos - x * self.sin);
        let bend =
            0.9 * perlin(
                self.seed ^ SALT_WARP,
                x / (2.5 * p.period),
                z / (2.5 * p.period),
            ) + 0.45
                * perlin(
                    self.seed ^ SALT_BEND,
                    x / (0.8 * p.period),
                    z / (0.8 * p.period),
                )
                + 0.2
                    * perlin(
                        self.seed ^ SALT_MEANDER,
                        x / (0.3 * p.period),
                        z / (0.3 * p.period),
                    );
        let warped = across + bend * p.period;
        let field = |u: f64, v: f64| {
            perlin(
                self.seed ^ SALT_FIELD,
                u / p.period,
                v / (p.period * p.stretch),
            )
        };
        let n = field(warped, along);
        // Blocks to the line: field over slope, so a floor is as wide where the field is flat as where it is steep.
        let step = 30.0;
        let (gu, gv) = (
            (field(warped + step, along) - field(warped - step, along)) / (2.0 * step),
            (field(warped, along + step) - field(warped, along - step)) / (2.0 * step),
        );
        let line = n.abs() / libm::hypot(gu, gv).max(0.35 / p.period);
        // a point this far from the line is on the plateau whatever the wobbles do
        let reach = p.floor / 2.0
            + p.soft_edge.max(p.sharp_edge.1)
            + p.coarse.iter().chain(&p.fine).map(|w| w.1).sum::<f64>();
        if line > reach {
            return Cut {
                depth: 0.0,
                soft: 0.0,
                sharp: 0.0,
                cliff: 0.0,
            };
        }
        let dist = line + ragged(self.seed ^ SALT_RAGGED, x, z, &p.coarse);
        let sharp_dist = dist + ragged(self.seed ^ SALT_FINE, x, z, &p.fine);
        let steep = smoothstep(
            0.25,
            0.75,
            perlin(self.seed ^ SALT_STEEP, x / 450.0, z / 450.0) * 0.5 + 0.5,
        );
        let edge = p.sharp_edge.0 + (p.sharp_edge.1 - p.sharp_edge.0) * steep;
        let at = 0.5 + 0.18 * perlin(self.seed ^ SALT_LEDGE, x / 800.0, z / 800.0);
        let (soft, sharp) = masks(p, dist, sharp_dist, edge, at);
        if soft <= 0.0 && sharp <= 0.0 {
            return Cut {
                depth: 0.0,
                soft: 0.0,
                sharp: 0.0,
                cliff: 0.0,
            };
        }
        // about a third of a wall is cliff, in stretches a few hundred blocks long
        let cliff = smoothstep(
            0.4,
            0.6,
            perlin(self.seed ^ SALT_CLIFF, x / 550.0, z / 550.0) * 0.5 + 0.5,
        );
        let varies = (perlin(
            self.seed ^ SALT_DEPTH,
            x / (3.0 * p.period),
            z / (3.0 * p.period),
        ) * 0.5
            + 0.5)
            .clamp(0.0, 1.0);
        let reach = (perlin(
            self.seed ^ SALT_FADE,
            along / p.fade_wavelength,
            warped / p.period,
        ) * 0.5
            + 0.5)
            .clamp(0.0, 1.0);
        let fade = p.fade_min + (1.0 - p.fade_min) * smoothstep(0.25, 0.75, reach);
        Cut {
            depth: (p.depth.0 + (p.depth.1 - p.depth.0) * varies) * fade,
            soft,
            sharp,
            cliff,
        }
    }
}

/// Block-level ground minus the map's: the map is cut by `soft`, a block by `sharp`, so floor and plateau stay put.
pub fn sharpening(cut: &Cut, weight: f64) -> f64 {
    cut.depth * weight * cut.cliff * (cut.soft - cut.sharp)
}

/// How much of a wall a point is, 0..1: the weight where the wall's rise is half over, and none on the floor or plateau.
pub fn wall(cut: &Cut, weight: f64) -> f64 {
    weight * cut.cliff * 4.0 * cut.sharp * (1.0 - cut.sharp)
}

// How steppe-like ground this warm (at its height) and this wet is: 1 inside classify's steppe band, easing off outside it.
pub(super) fn steppe_likeness(warmth: f64, moisture: f64) -> f64 {
    smoothstep(0.22, 0.30, warmth)
        * (1.0 - smoothstep(0.54, 0.62, warmth))
        * (1.0 - smoothstep(0.25, 0.35, moisture))
}

// How steppe-like each cell's ground is, 0..1, from its warmth at its height and its moisture.
fn favoured_layer(map: &WorldMap) -> Vec<f32> {
    let dim = map.dim();
    let (temperature, moisture) = moisture_field(map);
    let moisture = blur_box(&moisture, dim, FIELD_BLUR);
    (0..dim * dim)
        .map(|i| {
            let above = map.height[i] as f64 - WATER_LEVEL as f64;
            let warmth = (temperature[i] as f64 - above * LAPSE_PER_BLOCK as f64).max(0.0);
            steppe_likeness(warmth, moisture[i] as f64) as f32
        })
        .collect()
}

// Pulls steppe ground toward a smoothed copy of itself, averaged over land only, so it is open plain and not hills.
fn flatten(map: &mut WorldMap, favoured: &[f32], keep: &[f32]) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f32;
    let land: Vec<f32> = map.height.iter().map(|&h| (h >= sea + 1.0) as u8 as f32).collect();
    let weighted: Vec<f32> = map.height.iter().zip(&land).map(|(h, l)| h * l).collect();
    let (sum, share) = (blur_box(&weighted, dim, PLAIN_RADIUS), blur_box(&land, dim, PLAIN_RADIUS));
    let mountain: Vec<f32> = map.landform.iter().map(|&l| (l == MOUNTAIN) as u8 as f32).collect();
    let near_mountain = blur_box(&mountain, dim, PLAIN_MOUNTAIN);
    let favoured = blur_box(favoured, dim, PLAIN_EDGE);
    for i in 0..dim * dim {
        let above = (map.height[i] - sea) as f64;
        let weight = PLAIN_PULL
            * land[i] as f64
            * favoured[i] as f64
            * smoothstep(PLAIN_ABOVE.0, PLAIN_ABOVE.1, above)
            * (1.0 - smoothstep(0.0, 0.25, near_mountain[i] as f64))
            // after the blur, so a landmark's middle takes none of the flattening round it
            * (1.0 - keep[i] as f64);
        if weight > 0.0 {
            let smooth = sum[i] / share[i].max(1e-3);
            map.height[i] += ((smooth - map.height[i]) as f64 * weight) as f32;
        }
    }
}

/// The siting weight of every cell, 0..255 as 0..1, taken from the ground as it stands before any valley is cut.
fn siting(map: &WorldMap, share: f64, ties: &[u32], favoured: &[f32]) -> (Vec<u8>, Vec<bool>) {
    let dim = map.dim();
    let slope = super::gameplay::slope_layer(map);
    // No valley within about 7 cells of a lake or a basin hydrology will fill, full weight from about 12.
    let flood = priority_flood(&map.height, dim, ties);
    let deep = |i: usize| flood.filled[i] - map.height[i] >= BASIN_DEPTH;
    let lakes = closeness(map, |i| {
        map.water_kind[i] == WaterCell::Lake as u8 || deep(i)
    });
    // No valley near a big basin's footprint: a lower sump at its flat rim fills the whole basin into a lake.
    let mut big = vec![0.0f32; dim * dim];
    for basin in basins(&map.height, &flood, dim)
        .0
        .iter()
        .filter(|b| b.depth >= BASIN_DEPTH && b.cells.len() >= BIG_BASIN)
    {
        for &i in &basin.cells {
            big[i] = 1.0;
        }
    }
    let feeds = blur_box(&big, dim, FEED_BLUR);
    let p = STEPPE;
    let sea = WATER_LEVEL as f64;
    let mut weights = vec![0u8; dim * dim];
    par_rows(dim, &mut weights, |row, out| {
        for (col, byte) in out.iter_mut().enumerate() {
            let i = row * dim + col;
            let above = map.height[i] as f64 - sea;
            // the weight rises with the ground so the deepest cut still leaves `sea_margin` over the sea
            let window = ((above - p.sea_margin) / p.depth.1).clamp(0.0, 1.0)
                * (1.0 - smoothstep(p.upper, p.upper + 40.0, above));
            if window <= 0.0 {
                continue;
            }
            let favoured = favoured[i] as f64;
            let flat = 1.0 - smoothstep(FLAT_BELOW.0, FLAT_BELOW.1, slope[i] as f64);
            let dry = (1.0 - smoothstep(0.04, 0.28, lakes[i] as f64))
                * (1.0 - smoothstep(0.02, 0.2, feeds[i] as f64));
            let weight = share * (p.base + (1.0 - p.base) * favoured) * window * flat * dry;
            *byte = (weight.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    });
    // the cells a basin holds before the valleys are cut
    let held = (0..dim * dim)
        .map(|i| flood.filled[i] - map.height[i] > 0.25)
        .collect();
    (weights, held)
}

/// Flattens steppe ground but where `keep` holds a landmark, then cuts the valleys into `map.height` and stores their siting weight in
/// `map.valley`. A `share` of 0 changes nothing.
pub fn run(map: &mut WorldMap, share: f64, keep: &[f32]) {
    if share <= 0.0 {
        return;
    }
    let dim = map.dim();
    let favoured = favoured_layer(map);
    flatten(map, &favoured, keep);
    let ties = flood_ties(map);
    let (mut weights, held) = siting(map, share, &ties, &favoured);
    // no valley through a landmark
    for (w, k) in weights.iter_mut().zip(keep) {
        *w = (*w as f32 * (1.0 - k)) as u8;
    }
    map.valley = weights;
    let valley = Valley::of(map);
    let mut heights = std::mem::take(&mut map.height);
    par_rows(dim, &mut heights, |row, cells| {
        for (col, cell) in cells.iter_mut().enumerate() {
            let weight = map.valley[row * dim + col] as f64 / 255.0;
            if weight <= 0.0 {
                continue;
            }
            let (x, z) = map.cell_centre(col, row);
            let cut = valley.at(x, z);
            *cell -= (cut.depth * cut.soft * weight) as f32;
        }
    });
    // Fill each shallow basin the valleys made up to its dam, or it becomes a lake that floods the flat ground round it. A deep one is left alone: filling it would
    // plug the valley, and a channel cut out of it leaves a dry slot across the plain.
    fill_where(&mut heights, dim, &ties, |basin| {
        let new = basin.cells.iter().filter(|&&i| !held[i]).count();
        let inside = basin.cells.iter().filter(|&&i| map.valley[i] > 0).count();
        let valley_basin = (new >= NEW_CELLS && new * 3 >= basin.cells.len()) || (inside >= VALLEY_BASIN_CELLS && inside * 3 >= basin.cells.len() * 2);
        valley_basin && (basin.depth <= SHALLOW_BASIN || basin.cells.len() <= TINY_BASIN)
    });
    map.height = heights;
}

/// Cuts the ranges' valleys into `map.height` and stores their siting weight in `map.alpine`: mountain ground only, fading toward the foothills, never
/// deep enough to leave less than the profile's margin over the sea.
pub fn run_alpine(map: &mut WorldMap) {
    let dim = map.dim();
    let p = ALPINE;
    let sea = WATER_LEVEL as f64;
    let mountain: Vec<f32> = map.landform.iter().map(|&l| (l == MOUNTAIN) as u8 as f32).collect();
    if mountain.len() != dim * dim {
        return;
    }
    let soft = blur_box(&mountain, dim, ALPINE_EDGE);
    let mut weights = vec![0u8; dim * dim];
    par_rows(dim, &mut weights, |row, out| {
        for (col, byte) in out.iter_mut().enumerate() {
            let i = row * dim + col;
            let above = map.height[i] as f64 - sea;
            let window = ((above - p.sea_margin) / p.depth.1).clamp(0.0, 1.0) * (1.0 - smoothstep(p.upper, p.upper + 40.0, above));
            let weight = soft[i] as f64 * window;
            *byte = (weight.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    });
    map.alpine = weights;
    let valley = Valley::alpine(map);
    let ties = flood_ties(map);
    // the cells a basin holds before the valleys are cut
    let flood = priority_flood(&map.height, dim, &ties);
    let held: Vec<bool> = (0..dim * dim).map(|i| flood.filled[i] - map.height[i] > 0.25).collect();
    let mut heights = std::mem::take(&mut map.height);
    par_rows(dim, &mut heights, |row, cells| {
        for (col, cell) in cells.iter_mut().enumerate() {
            let weight = map.alpine[row * dim + col] as f64 / 255.0;
            if weight <= 0.0 {
                continue;
            }
            let (x, z) = map.cell_centre(col, row);
            let cut = valley.at(x, z);
            *cell -= (cut.depth * cut.soft * weight) as f32;
        }
    });
    // Fill each shallow basin the cut made up to its dam, so the water runs down the floor and does not stand in a chain of lakes; a deep one is left as a tarn.
    fill_where(&mut heights, dim, &ties, |basin| {
        let new = basin.cells.iter().filter(|&&i| !held[i]).count();
        let inside = basin.cells.iter().filter(|&&i| map.alpine[i] > 0).count();
        let valley_basin = (new >= NEW_CELLS && new * 3 >= basin.cells.len()) || (inside >= VALLEY_BASIN_CELLS && inside * 3 >= basin.cells.len() * 2);
        valley_basin && (basin.depth <= ALPINE_SHALLOW_BASIN || basin.cells.len() <= TINY_BASIN)
    });
    map.height = heights;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    fn tiny(above: f32) -> WorldMap {
        let config = WorldConfig {
            size: 8192,
            map_cell: 32,
            ..Default::default()
        };
        let mut map = WorldMap::empty(7, config);
        map.height = vec![WATER_LEVEL as f32 + above; map.dim() * map.dim()];
        map
    }

    #[test]
    fn the_soft_and_sharp_walls_share_their_floor_plateau_and_midpoint() {
        let p = STEPPE;
        let edge = p.sharp_edge.0;
        assert_eq!((1.0, 1.0), masks(&p, 0.0, 0.0, edge, 0.5));
        assert_eq!((0.0, 0.0), masks(&p, 10_000.0, 10_000.0, edge, 0.5));
        let mid = p.floor / 2.0 + p.soft_edge / 2.0;
        let (soft, sharp) = masks(&p, mid, mid, edge, 0.5);
        assert!((soft - 0.5).abs() < 1e-9 && (sharp - 0.5).abs() < 1e-9);
        // a wall of the deepest cut over the steepest edge is a cliff, gradient 1 or more, and over the gentlest a slope
        assert!(p.depth.1 / p.sharp_edge.0 >= 1.0 && p.depth.1 / p.sharp_edge.1 < 1.5);
        // the whole slope is gentle: 70 blocks over 200
        assert!(p.depth.1 / p.soft_edge < 0.5);
        let (low, high) = (
            p.floor / 2.0 + p.soft_edge * 0.25,
            p.floor / 2.0 + p.soft_edge * 0.75,
        );
        let (soft, sharp) = masks(&p, low, low, edge, 0.5);
        assert!(soft < sharp, "the face holds the floor side up to its edge");
        let (soft, sharp) = masks(&p, high, high, edge, 0.5);
        assert!(soft > sharp, "and the plateau side down to it");
        // a face placed anywhere up the slope stays inside it
        for at in [0.0, 0.3, 1.0] {
            assert_eq!((1.0, 1.0), masks(&p, 0.0, 0.0, edge, at));
            assert_eq!((0.0, 0.0), masks(&p, 10_000.0, 10_000.0, edge, at));
        }
    }

    #[test]
    fn sharpening_leaves_floor_and_plateau_on_the_maps_ground() {
        let valley = Valley::new(3, 0.7, STEPPE);
        let mut seen = (false, false, false);
        for i in 0..4000 {
            let cut = valley.at(i as f64 * 53.0 - 90_000.0, (i % 97) as f64 * 311.0);
            let moved = sharpening(&cut, 1.0).abs();
            if cut.soft == 1.0 && cut.sharp == 1.0 || cut.soft == 0.0 && cut.sharp == 0.0 {
                assert_eq!(0.0, moved);
            }
            seen = (
                seen.0 || cut.soft == 1.0,
                seen.1 || cut.soft == 0.0,
                seen.2 || moved > 1.0,
            );
            assert!(cut.depth <= STEPPE.depth.1 + 1e-9 && wall(&cut, 1.0) <= 1.0);
        }
        assert!(
            seen.0 && seen.1 && seen.2,
            "floor, plateau and wall all found: {seen:?}"
        );
    }

    #[test]
    fn valleys_are_the_same_every_time_and_differ_by_seed() {
        let (a, b) = (Valley::new(1, 0.5, STEPPE), Valley::new(2, 0.5, STEPPE));
        let points: Vec<(f64, f64)> = (0..200)
            .map(|i| (i as f64 * 411.0, i as f64 * -173.0))
            .collect();
        assert!(points.iter().all(|&(x, z)| a.at(x, z) == a.at(x, z)));
        assert!(points.iter().any(|&(x, z)| a.at(x, z) != b.at(x, z)));
    }

    #[test]
    fn a_second_preset_gives_valid_cuts_with_no_biome_named() {
        let wadi = ValleyProfile {
            period: 900.0,
            stretch: 3.0,
            floor: 60.0,
            sharp_edge: (8.0, 20.0),
            soft_edge: 60.0,
            depth: (10.0, 20.0),
            ..STEPPE
        };
        let valley = Valley::new(5, 1.1, wadi);
        for i in 0..500 {
            let cut = valley.at(i as f64 * 97.0, i as f64 * 61.0);
            assert!(
                cut.depth <= 20.0 + 1e-9
                    && (0.0..=1.0).contains(&cut.soft)
                    && (0.0..=1.0).contains(&cut.sharp)
            );
        }
    }

    #[test]
    fn steppe_likeness_is_full_in_the_steppe_band_and_gone_outside_it() {
        assert_eq!(1.0, steppe_likeness(0.4, 0.1));
        assert_eq!(0.0, steppe_likeness(0.1, 0.1), "too cold");
        assert_eq!(0.0, steppe_likeness(0.4, 0.5), "too wet");
        assert_eq!(0.0, steppe_likeness(0.8, 0.1), "too warm");
    }

    #[test]
    fn a_share_of_zero_changes_nothing_and_a_cut_never_nears_the_sea() {
        let mut map = tiny(120.0);
        let before = map.height.clone();
        let none = vec![0.0; map.height.len()];
        run(&mut map, 0.0, &none);
        assert_eq!(before, map.height);
        assert!(map.valley.iter().all(|&w| w == 0));
        let none = vec![0.0; map.height.len()];
        run(&mut map, 1.0, &none);
        assert!(
            map.height.iter().zip(&before).any(|(a, b)| a < b),
            "some ground was cut"
        );
        let floor = WATER_LEVEL as f32 + STEPPE.sea_margin as f32;
        assert!(
            map.height.iter().all(|&h| h >= floor),
            "min {}",
            map.height.iter().cloned().fold(f32::MAX, f32::min)
        );
        assert!(map
            .height
            .iter()
            .zip(&before)
            .all(|(a, b)| b - a <= STEPPE.depth.1 as f32 + 1e-3));
    }

    #[test]
    fn flattening_smooths_steppe_and_leaves_low_coast_and_mountains_alone() {
        let mut map = tiny(100.0);
        let dim = map.dim();
        for i in 0..dim * dim {
            let noise = ochre_base::noise::unit(3, (i % dim) as i32, (i / dim) as i32) as f32;
            map.height[i] = WATER_LEVEL as f32 + 100.0 + 25.0 * noise;
            map.landform[i] = ((i % dim) < 40) as u8 * MOUNTAIN;
            if i / dim < 10 {
                map.height[i] = WATER_LEVEL as f32 + 5.0;
            }
        }
        let before = map.height.clone();
        flatten(&mut map, &vec![1.0; dim * dim], &vec![0.0; dim * dim]);
        let spread = |h: &[f32]| {
            let cells: Vec<f32> = (100..200).flat_map(|z| (100..200).map(move |x| h[z * dim + x])).collect();
            let mean = cells.iter().sum::<f32>() / cells.len() as f32;
            (cells.iter().map(|c| (c - mean).powi(2)).sum::<f32>() / cells.len() as f32).sqrt()
        };
        assert!(spread(&map.height) < 0.4 * spread(&before), "{} of {}", spread(&map.height), spread(&before));
        assert_eq!(before[100 * dim + 10], map.height[100 * dim + 10], "mountain");
        assert_eq!(before[3 * dim + 100], map.height[3 * dim + 100], "low coast");
    }

    #[test]
    fn no_valley_is_cut_into_ground_within_the_sea_margin() {
        let mut map = tiny(4.0);
        let before = map.height.clone();
        let none = vec![0.0; map.height.len()];
        run(&mut map, 1.0, &none);
        assert_eq!(before, map.height);
    }
}
