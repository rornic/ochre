//! Desert canyons: a trench in the map's height along the water's line, and a narrow slot cut deeper at block level

use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{hash, perlin, unit};
use ochre_base::WATER_LEVEL;

use super::super::climate::BiomeKind;
use super::super::hydrology::{flood_ties, priority_flood, Flood, NONE};
use super::super::shapes::sites_apart;
use super::super::{WorldMap, NEIGHBOURS};
use super::landmarks::biomes;

const SALT: u32 = 0x1A4D_00C1;
const MAX_LEN: usize = 4000;
// Cells each way the drainage line's bends are averaged over.
const SMOOTH: usize = 3;
// A canyon's head rises from the plateau over this many cells, and its floor falls at least this many blocks a cell.
const HEAD: f64 = 10.0;
const FALL: f64 = 0.15;
// Blocks over the sea the floor stays, and over the sea a canyon needs the plateau to start.
const SEA_MARGIN: f64 = 6.0;
const START_ABOVE: f64 = 80.0;
// The share of a canyon's depth the map's trench holds; the block-level slot is the rest, so this much deeper again.
const SOFT: f64 = 0.6;
const SLOT_RATIO: f64 = (1.0 - SOFT) / SOFT;
// The most of the ground's height over the sea the map's trench cuts.
const SHARE: f64 = 0.5;
// Cells between canyon heads.
const APART: f64 = 150.0;
const EDGE: usize = 60;
// Wadis, the dry beds of desert, savanna and steppe: ground this high over the sea, and cells between heads.
const WADI_ABOVE: f64 = 30.0;
const WADI_APART: f64 = 60.0;

// A canyon's stretch of floor: (x, z, height, half width in cells, slot depth in blocks); the slot is cut below it.
type Floor = Vec<(f64, f64, f64, f64, f64)>;
// A fork's floor rises this many blocks a cell away from its canyon; its depth and width are shares of the canyon's.
const RISE: f64 = 1.5;
const FORK_DEPTH: (f64, f64) = (0.5, 0.8);
const FORK_HALF: (f64, f64) = (0.6, 0.85);

// The slot's depth under a trench floor `depth` under the ground, in the tens of blocks the map holds.
fn slot(depth: f64) -> f64 {
    ((depth * SLOT_RATIO / 10.0).round() * 10.0).clamp(0.0, 150.0)
}

// The floor of a canyon from `start` down the line water will take over `flood`'s filled ground, so it always drains.
fn trace(flood: &Flood, dim: usize, start: (usize, usize), depth: f64, half: f64, h: u32) -> Floor {
    let sea = WATER_LEVEL as f64;
    let mut cell = start.1 * dim + start.0;
    let mut floor: Floor = Vec::new();
    for step in 0..MAX_LEN {
        let (x, z) = (cell % dim, cell / dim);
        let height = flood.filled[cell] as f64;
        if x < EDGE || z < EDGE || x >= dim - EDGE || z >= dim - EDGE || height <= sea + 1.0 {
            break;
        }
        let mut level = height - depth * smoothstep(0.0, HEAD, step as f64);
        if let Some(&(_, _, below, _, _)) = floor.last() {
            level = level.min(below - FALL);
        }
        // never deeper than part of the ground over the sea, so the trench thins out toward the coast
        let level = level.max(height - SHARE * (height - sea - SEA_MARGIN));
        // the width swells and pinches along the way
        let wide = half * (0.65 + 0.7 * (0.5 + 0.5 * perlin(h ^ SALT, step as f64 * 0.04, 0.5)).clamp(0.0, 1.0));
        floor.push((x as f64, z as f64, level, wide, slot(height - level)));
        if flood.down[cell] == NONE {
            break;
        }
        cell = flood.down[cell] as usize;
    }
    floor
}

fn smoothed(floor: &Floor) -> Floor {
    (0..floor.len())
        .map(|i| {
            let window = &floor[i.saturating_sub(SMOOTH)..(i + SMOOTH + 1).min(floor.len())];
            let n = window.len() as f64;
            (window.iter().map(|f| f.0).sum::<f64>() / n, window.iter().map(|f| f.1).sum::<f64>() / n, floor[i].2, floor[i].3, floor[i].4)
        })
        .collect()
}

// A fork off `main` at index `k`, heading upstream to one side and rising from the main floor until it meets ground.
fn fork(flood: &Flood, dim: usize, main: &Floor, k: usize, side: f64, (depth, half): (f64, f64), h: u32) -> Floor {
    let (x0, z0, level0, ..) = main[k];
    let (up, down) = (main[k.saturating_sub(4)], main[(k + 4).min(main.len() - 1)]);
    let mut heading = libm::atan2(up.1 - down.1, up.0 - down.0) + side * (0.6 + 0.5 * unit(h, 5, 0));
    let len = 25 + (45.0 * unit(h, 6, 0)) as usize;
    let (mut x, mut z, mut level) = (x0, z0, level0);
    let mut out: Floor = Vec::new();
    for j in 1..=len {
        heading += perlin(h ^ SALT, j as f64 * 0.12, 0.5) * 0.12;
        (x, z) = (x + libm::cos(heading), z + libm::sin(heading));
        let (cx, cz) = (x.round().clamp(0.0, (dim - 1) as f64) as usize, z.round().clamp(0.0, (dim - 1) as f64) as usize);
        let ground = flood.filled[cz * dim + cx] as f64;
        let wanted = (ground - depth * (1.0 - smoothstep((len - 10) as f64, len as f64, j as f64))).min(level0 + RISE * j as f64);
        level = wanted.max(level + FALL);
        if cx < EDGE || cz < EDGE || cx >= dim - EDGE || cz >= dim - EDGE || level >= ground - 1.0 {
            break;
        }
        out.push((x, z, level, half * (1.0 - 0.3 * j as f64 / len as f64), slot(ground - level)));
    }
    out
}

// Lowers `map.height` along `floor` to a trench of its half width, flat in the middle with steep walls.
fn stamp(map: &mut WorldMap, before: &[f32], floor: &Floor, shield: &mut [f32]) {
    let dim = map.dim();
    for &(px, pz, level, half, _) in floor {
        let reach = half.ceil() as i64 + 1;
        for z in (pz as i64 - reach).max(0)..=(pz as i64 + reach).min(dim as i64 - 1) {
            for x in (px as i64 - reach).max(0)..=(px as i64 + reach).min(dim as i64 - 1) {
                let t = libm::sqrt((x as f64 - px).powi(2) + (z as f64 - pz).powi(2)) / half;
                let i = z as usize * dim + x as usize;
                let wanted = level + (before[i] as f64 - level) * smoothstep(0.5, 1.0, t);
                let cut = (before[i] as f64 - wanted).max(0.0);
                map.height[i] = map.height[i].min((before[i] as f64 - cut) as f32);
                shield[i] = shield[i].max(1.0 - smoothstep(1.0, 1.4, t) as f32);
            }
        }
    }
}

// Marks `cells` (a chain of neighbours, each with its slot) in `canyon`, each pointing to the next, where unmarked.
fn mark(canyon: &mut [u8], dim: usize, cells: &[((usize, usize), f64)]) {
    for (n, &((x, z), slot)) in cells.iter().enumerate() {
        let towards = cells.get(n + 1).and_then(|&((nx, nz), _)| NEIGHBOURS.iter().position(|&d| d == (nx as i64 - x as i64, nz as i64 - z as i64)));
        let byte = towards.map_or(0, |d| d as u8 + 1) | ((slot / 10.0) as u8) << 4;
        if canyon[z * dim + x] == 0 {
            canyon[z * dim + x] = byte;
        }
    }
}

// The cells a floor runs over, one for each run of its points in a cell.
fn cells_of(floor: &Floor, dim: usize) -> Vec<((usize, usize), f64)> {
    let mut cells: Vec<((usize, usize), f64)> = Vec::new();
    for &(x, z, _, _, slot) in floor {
        let at = (x.round().clamp(0.0, (dim - 1) as f64) as usize, z.round().clamp(0.0, (dim - 1) as f64) as usize);
        if cells.last().map(|c| c.0) != Some(at) {
            cells.push((at, slot));
        }
    }
    cells
}

/// Cuts up to `count` desert and `gorges` wetter-plateau canyons, 90 to 200 blocks deep, with forks; map holds 60%
pub(crate) fn run(map: &mut WorldMap, count: u32, gorges: u32, wadis: u32, keep: &mut [f32]) {
    let dim = map.dim();
    if (count == 0 && gorges == 0 && wadis == 0) || dim <= 2 * EDGE {
        return;
    }
    let sea = WATER_LEVEL as f32;
    let biome = biomes(map);
    let high = |i: usize| (map.height[i] - sea) as f64 >= START_ABOVE;
    let wet = |b: &Option<BiomeKind>| matches!(b, Some(BiomeKind::Savanna | BiomeKind::Grassland | BiomeKind::Forest | BiomeKind::Jungle | BiomeKind::Taiga | BiomeKind::Steppe));
    let desert: Vec<usize> = (0..dim * dim).filter(|&i| biome[i] == Some(BiomeKind::Desert) && high(i)).collect();
    let wetter: Vec<usize> = (0..dim * dim).filter(|&i| wet(&biome[i]) && high(i)).collect();
    let dry: Vec<usize> = (0..dim * dim)
        .filter(|&i| matches!(biome[i], Some(BiomeKind::Desert | BiomeKind::Savanna | BiomeKind::Steppe)) && (map.height[i] - sea) as f64 >= WADI_ABOVE)
        .collect();
    if desert.is_empty() && wetter.is_empty() && dry.is_empty() {
        return;
    }
    let flood = priority_flood(&map.height, dim, &flood_ties(map));
    let before = map.height.clone();
    // where a trench is cut, so the steppe's flattening and the valleys leave it alone, as they do a landmark
    let mut shield = vec![0.0f32; dim * dim];
    for (sites, number, salt, wadi) in [(desert, count, SALT, false), (wetter, gorges, SALT ^ 0x6, false), (dry, wadis, SALT ^ 0x7, true)] {
        for (sx, sz) in sites_apart(map, sites, number as usize, salt, if wadi { WADI_APART } else { APART }) {
            cut(map, &flood, &before, (sx, sz), hash(map.seed ^ salt, sx as i32, sz as i32), &mut shield, wadi);
        }
    }
    for (k, s) in keep.iter_mut().zip(&shield) {
        *k = k.max(*s);
    }
}

// One canyon from `site`: the trench, the slot's marks and the forks.
fn cut(map: &mut WorldMap, flood: &Flood, before: &[f32], (sx, sz): (usize, usize), h: u32, shield: &mut [f32], wadi: bool) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f64;
    let above = before[sz * dim + sx] as f64 - sea;
    // never deeper than the plateau leaves room for over the sea
    let knobs = super::super::character::LandformKnobs::from_seed(map.seed);
    let (half, total) = if wadi {
        (1.4 + 1.0 * unit(h, 2, 0), 18.0 + 22.0 * unit(h, 3, 0))
    } else {
        (1.2 + 1.0 * unit(h, 2, 0), ((90.0 + 110.0 * unit(h, 3, 0)) * knobs.canyon_depth).min((above - SEA_MARGIN) * 0.85))
    };
    let half = half * knobs.canyon_width;
    // a wadi is all in the map's trench: shallow, wide and open, with no slot and no forks
    let floor = trace(flood, dim, (sx, sz), if wadi { total } else { total * SOFT }, half, h);
    stamp(map, before, &smoothed(&floor), &mut *shield);
    if wadi {
        return;
    }
    // forks off the deeper reaches, upstream to either side
    let mut forks = Vec::new();
    let mut k = 30;
    while k + 5 < floor.len() {
        let hk = hash(h, k as i32, 7);
        if unit(hk, 1, 0) < 0.7 && flood.filled[floor[k].1 as usize * dim + floor[k].0 as usize] as f64 - floor[k].2 > 25.0 {
            let side = if unit(hk, 2, 0) < 0.5 { -1.0 } else { 1.0 };
            let ratio = |(lo, hi): (f64, f64), n: i32| lo + (hi - lo) * unit(hk, n, 0);
            let branch = fork(flood, dim, &floor, k, side, (total * SOFT * ratio(FORK_DEPTH, 3), floor[k].3 * ratio(FORK_HALF, 4)), hk);
            stamp(map, before, &branch, &mut *shield);
            forks.push((k, branch));
        }
        k += 35 + (40.0 * unit(hk, 8, 0)) as usize;
    }
    // the main line first, so a fork never takes one of its cells
    mark(&mut map.canyon, dim, &cells_of(&floor, dim));
    for (k, branch) in &forks {
        // pointing downstream, to the cell of the canyon it joins
        let mut cells = cells_of(branch, dim);
        cells.reverse();
        cells.push(((floor[*k].0 as usize, floor[*k].1 as usize), 0.0));
        mark(&mut map.canyon, dim, &cells);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    // A plateau 500 blocks over the sea that falls half a block a cell toward +x.
    fn slope() -> (WorldMap, Vec<f32>) {
        let config = WorldConfig { size: 400 * 32, map_cell: 32, ..Default::default() };
        let mut map = WorldMap::empty(11, config);
        let dim = map.dim();
        for (i, h) in map.height.iter_mut().enumerate() {
            *h = WATER_LEVEL as f32 + 500.0 - 0.5 * (i % dim) as f32;
        }
        let keep = vec![0.0; dim * dim];
        (map, keep)
    }

    #[test]
    fn a_canyon_is_a_trench_whose_floor_only_falls_and_whose_walls_stand() {
        let (mut map, _) = slope();
        let (dim, before) = (map.dim(), map.height.clone());
        let floor = trace(&priority_flood(&map.height, dim, &flood_ties(&map)), dim, (100, 200), 90.0, 1.5, 5);
        assert!(floor.len() > 40, "{} cells long", floor.len());
        assert!(floor.windows(2).all(|w| w[1].2 < w[0].2), "the floor rises");
        let line = smoothed(&floor);
        stamp(&mut map, &before, &line, &mut vec![0.0; dim * dim]);
        let (mx, mz, level, ..) = line[line.len() / 2];
        let mid = map.height[mz as usize * dim + mx as usize] as f64;
        assert!((mid - level).abs() < 1.0, "the middle is {mid}, the floor {level}");
        assert!(before[mz as usize * dim + mx as usize] as f64 - mid > 60.0, "no wall");
        for i in (0..dim * dim).filter(|&i| line.iter().all(|&(x, z, ..)| ((i % dim) as f64 - x).hypot((i / dim) as f64 - z) > 5.0)) {
            assert_eq!(before[i], map.height[i], "cut beyond its width at {i}");
        }
    }

    #[test]
    fn a_canyon_swells_and_pinches_and_forks_climb_away_from_it() {
        let (map, _) = slope();
        let dim = map.dim();
        let flood = priority_flood(&map.height, dim, &flood_ties(&map));
        let main = trace(&flood, dim, (100, 200), 90.0, 1.5, 5);
        let (narrow, wide) = main.iter().fold((f64::MAX, 0.0f64), |(a, b), f| (a.min(f.3), b.max(f.3)));
        assert!(wide > 1.4 * narrow, "{narrow} to {wide} wide");
        let branch = fork(&flood, dim, &main, 60, 1.0, (60.0, 1.0), 9);
        assert!(branch.len() > 10, "a fork {} long", branch.len());
        assert!(branch[0].2 > main[60].2, "a fork starts under the canyon's floor");
        assert!(branch.windows(2).all(|w| w[1].2 > w[0].2), "a fork's floor falls toward its head");
    }

    #[test]
    fn the_line_marks_each_cell_pointing_to_the_next_with_its_slot() {
        let mut canyon = vec![0u8; 100];
        mark(&mut canyon, 10, &[((1, 1), 30.0), ((2, 1), 50.0), ((3, 2), 40.0)]);
        // east is index 0, and south east 1
        assert_eq!(1 | 3 << 4, canyon[11]);
        assert_eq!(2 | 5 << 4, canyon[12]);
        assert_eq!(4 << 4, canyon[23], "the last cell points nowhere");
        mark(&mut canyon, 10, &[((1, 1), 90.0), ((9, 9), 90.0)]);
        assert_eq!(1 | 3 << 4, canyon[11], "a marked cell was taken");
    }

    #[test]
    fn a_wadi_is_a_shallow_open_bed_with_no_slot_and_a_canyon_is_deep_with_one() {
        let (mut wadi, _) = slope();
        let (dim, before) = (wadi.dim(), wadi.height.clone());
        let flood = priority_flood(&wadi.height, dim, &flood_ties(&wadi));
        cut(&mut wadi, &flood, &before, (100, 200), 5, &mut vec![0.0; dim * dim], true);
        let deepest = (0..dim * dim).map(|i| before[i] - wadi.height[i]).fold(0.0, f32::max);
        assert!((8.0..=45.0).contains(&deepest), "a wadi {deepest} deep");
        assert!(wadi.canyon.iter().all(|&b| b == 0), "a wadi marked a slot");
        let (mut canyon, _) = slope();
        cut(&mut canyon, &flood, &before, (100, 200), 5, &mut vec![0.0; dim * dim], false);
        let deepest = (0..dim * dim).map(|i| before[i] - canyon.height[i]).fold(0.0, f32::max);
        assert!(deepest > 45.0 && canyon.canyon.iter().any(|&b| b != 0), "a canyon {deepest} deep");
    }

    #[test]
    fn a_trench_is_shielded_and_cuts_through_whatever_stands_in_its_way() {
        let (mut map, _) = slope();
        let (dim, before) = (map.dim(), map.height.clone());
        let floor = trace(&priority_flood(&map.height, dim, &flood_ties(&map)), dim, (100, 200), 90.0, 1.5, 5);
        let mut shield = vec![0.0f32; dim * dim];
        stamp(&mut map, &before, &smoothed(&floor), &mut shield);
        let (x, z, level, ..) = floor[floor.len() / 2];
        let at = z as usize * dim + x as usize;
        assert_eq!(1.0, shield[at], "the middle of the trench is not shielded");
        assert!(map.height[at] as f64 - level < 1.0 && shield.contains(&0.0));
    }
}
