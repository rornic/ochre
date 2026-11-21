//! Desert canyons: a trench stamped into the map's height down a descending line, so the water routed afterwards runs a river through its floor.

use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{hash, perlin, unit};
use ochre_base::WATER_LEVEL;

use super::super::climate::BiomeKind;
use super::super::hydrology::{flood_ties, priority_flood, Flood, NONE};
use super::super::shapes::sites_apart;
use super::super::WorldMap;
use super::landmarks::biomes;

const SALT: u32 = 0x1A4D_00C1;
const MAX_LEN: usize = 900;
// Cells each way the drainage line's bends are averaged over.
const SMOOTH: usize = 3;
// A canyon's head rises from the plateau over this many cells, and its floor falls at least this many blocks a cell.
const HEAD: f64 = 10.0;
const FALL: f64 = 0.15;
// Blocks over the sea the floor stays, and over the sea a canyon needs the plateau to start.
const SEA_MARGIN: f64 = 6.0;
// The most of the ground's height over the sea floor a canyon cuts.
const SHARE: f64 = 0.8;
const START_ABOVE: f64 = 80.0;
// Cells between canyon heads.
const APART: f64 = 150.0;
const EDGE: usize = 60;

// A canyon's stretch of floor: the middle of the trench, where the river runs, as (x, z, height, half width) in cells.
type Floor = Vec<(f64, f64, f64, f64)>;
// A fork's floor rises this many blocks a cell away from the canyon it joins, and its depth and width are these shares of that canyon's.
const RISE: f64 = 1.5;
const FORK_DEPTH: (f64, f64) = (0.5, 0.8);
const FORK_HALF: (f64, f64) = (0.6, 0.85);

// The floor of a canyon from `start` down the line the water will take over `flood`'s filled ground, to the sea or the map's edge, so it always drains.
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
        if let Some(&(_, _, below, _)) = floor.last() {
            level = level.min(below - FALL);
        }
        // never deeper than part of the ground over the sea, so the trench thins out toward the coast
        let level = level.max(height - SHARE * (height - sea - SEA_MARGIN));
        // the width swells and pinches along the way
        let wide = half * (0.65 + 0.7 * (0.5 + 0.5 * perlin(h ^ SALT, step as f64 * 0.04, 0.5)).clamp(0.0, 1.0));
        floor.push((x as f64, z as f64, level, wide));
        if flood.down[cell] == NONE {
            break;
        }
        cell = flood.down[cell] as usize;
    }
    // the drainage line steps cell to cell, so its bends are smoothed over a few cells
    let smooth: Vec<(f64, f64)> = (0..floor.len())
        .map(|i| {
            let window = &floor[i.saturating_sub(SMOOTH)..(i + SMOOTH + 1).min(floor.len())];
            let n = window.len() as f64;
            (window.iter().map(|f| f.0).sum::<f64>() / n, window.iter().map(|f| f.1).sum::<f64>() / n)
        })
        .collect();
    floor.iter().zip(smooth).map(|(f, (x, z))| (x, z, f.2, f.3)).collect()
}

// A fork off `main` at index `k`, heading upstream to one side and rising from the main floor until it meets the ground: ordered from the junction out.
fn fork(flood: &Flood, dim: usize, main: &Floor, k: usize, side: f64, (depth, half): (f64, f64), h: u32) -> Floor {
    let (x0, z0, level0, _) = main[k];
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
        out.push((x, z, level, half * (1.0 - 0.3 * j as f64 / len as f64)));
    }
    out
}

// Lowers `map.height` along `floor` to a trench of its half width, flat in its middle third and rising in a wall, from the ground as it stood.
fn stamp(map: &mut WorldMap, before: &[f32], floor: &Floor, keep: &[f32]) {
    let dim = map.dim();
    for &(px, pz, level, half) in floor {
        let reach = half.ceil() as i64 + 1;
        for z in (pz as i64 - reach).max(0)..=(pz as i64 + reach).min(dim as i64 - 1) {
            for x in (px as i64 - reach).max(0)..=(px as i64 + reach).min(dim as i64 - 1) {
                let t = libm::sqrt((x as f64 - px).powi(2) + (z as f64 - pz).powi(2)) / half;
                let i = z as usize * dim + x as usize;
                let wanted = level + (before[i] as f64 - level) * smoothstep(0.35, 1.0, t);
                let cut = (before[i] as f64 - wanted).max(0.0) * (1.0 - keep[i] as f64);
                map.height[i] = map.height[i].min((before[i] as f64 - cut) as f32);
            }
        }
    }
}

/// Cuts up to `count` canyons into desert plateau, each a drop of 90 to 200 blocks (less where the plateau is low) and 2 to 3.5 cells to a side, swelling and pinching, with forks off its deeper reaches, leaving `keep` (landmarks) alone.
pub(crate) fn run(map: &mut WorldMap, count: u32, keep: &[f32]) {
    let dim = map.dim();
    if count == 0 || dim <= 2 * EDGE {
        return;
    }
    let sea = WATER_LEVEL as f32;
    let eligible: Vec<usize> = biomes(map)
        .iter()
        .enumerate()
        .filter(|&(i, b)| *b == Some(BiomeKind::Desert) && (map.height[i] - sea) as f64 >= START_ABOVE)
        .map(|(i, _)| i)
        .collect();
    if eligible.is_empty() {
        return;
    }
    let flood = priority_flood(&map.height, dim, &flood_ties(map));
    let before = map.height.clone();
    for (sx, sz) in sites_apart(map, eligible, count as usize, SALT, APART) {
        let h = hash(map.seed ^ SALT, sx as i32, sz as i32);
        let above = before[sz * dim + sx] as f64 - sea as f64;
        // never deeper than the plateau leaves room for over the sea
        let (half, depth) = (2.0 + 1.5 * unit(h, 2, 0), (90.0 + 110.0 * unit(h, 3, 0)).min((above - SEA_MARGIN) * 0.7));
        let floor = trace(&flood, dim, (sx, sz), depth, half, h);
        stamp(map, &before, &floor, keep);
        // forks off the deeper reaches, upstream to either side
        let mut k = 30;
        while k + 5 < floor.len() {
            let hk = hash(h, k as i32, 7);
            if unit(hk, 1, 0) < 0.7 && flood.filled[floor[k].1 as usize * dim + floor[k].0 as usize] as f64 - floor[k].2 > 40.0 {
                let side = if unit(hk, 2, 0) < 0.5 { -1.0 } else { 1.0 };
                let ratio = |(lo, hi): (f64, f64), n: i32| lo + (hi - lo) * unit(hk, n, 0);
                stamp(map, &before, &fork(&flood, dim, &floor, k, side, (depth * ratio(FORK_DEPTH, 3), floor[k].3 * ratio(FORK_HALF, 4)), hk), keep);
            }
            k += 35 + (40.0 * unit(hk, 8, 0)) as usize;
        }
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
        let (mut map, keep) = slope();
        let (dim, before) = (map.dim(), map.height.clone());
        let floor = trace(&priority_flood(&map.height, dim, &flood_ties(&map)), dim, (100, 200), 150.0, 3.0, 5);
        assert!(floor.len() > 40, "{} cells long", floor.len());
        assert!(floor.windows(2).all(|w| w[1].2 < w[0].2), "the floor rises");
        stamp(&mut map, &before, &floor, &keep);
        let (mx, mz, level, _) = floor[floor.len() / 2];
        let mid = map.height[mz as usize * dim + mx as usize] as f64;
        assert!((mid - level).abs() < 1.0, "the middle is {mid}, the floor {level}");
        assert!(before[mz as usize * dim + mx as usize] as f64 - mid > 100.0, "no wall");
        for i in (0..dim * dim).filter(|&i| floor.iter().all(|&(x, z, _, _)| ((i % dim) as f64 - x).hypot((i / dim) as f64 - z) > 5.0)) {
            assert_eq!(before[i], map.height[i], "cut beyond its width at {i}");
        }
    }

    #[test]
    fn a_canyon_swells_and_pinches_and_forks_climb_away_from_it() {
        let (map, _) = slope();
        let dim = map.dim();
        let flood = priority_flood(&map.height, dim, &flood_ties(&map));
        let main = trace(&flood, dim, (100, 200), 150.0, 3.0, 5);
        let (narrow, wide) = main.iter().fold((f64::MAX, 0.0f64), |(a, b), f| (a.min(f.3), b.max(f.3)));
        assert!(wide > 1.4 * narrow, "{narrow} to {wide} wide");
        let branch = fork(&flood, dim, &main, 60, 1.0, (100.0, 2.0), 9);
        assert!(branch.len() > 10, "a fork {} long", branch.len());
        assert!(branch[0].2 > main[60].2, "a fork starts under the canyon's floor");
        assert!(branch.windows(2).all(|w| w[1].2 > w[0].2), "a fork's floor falls toward its head");
    }

    #[test]
    fn landmarks_are_left_standing() {
        let (mut map, mut keep) = slope();
        let (dim, before) = (map.dim(), map.height.clone());
        keep.fill(1.0);
        let floor = trace(&priority_flood(&map.height, dim, &flood_ties(&map)), dim, (100, 200), 150.0, 3.0, 5);
        stamp(&mut map, &before, &floor, &keep);
        assert_eq!(before, map.height);
    }
}
