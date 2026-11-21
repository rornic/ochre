//! Desert canyons: a trench stamped into the map's height down a descending line, so the water routed afterwards runs a river through its floor.

use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{hash, unit};
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
const SHARE: f64 = 0.6;
const START_ABOVE: f64 = 80.0;
// Cells between canyon heads.
const APART: f64 = 150.0;
const EDGE: usize = 60;

// A canyon's stretch of floor: the middle of the trench, where the river runs, as (x, z, height) in cells.
type Floor = Vec<(f64, f64, f64)>;

// The floor of a canyon from `start` down the line the water will take over `flood`'s filled ground, to the sea or the map's edge, so it always drains.
fn trace(flood: &Flood, dim: usize, start: (usize, usize), depth: f64) -> Floor {
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
        if let Some(&(_, _, below)) = floor.last() {
            level = level.min(below - FALL);
        }
        // never deeper than part of the ground over the sea, so the trench thins out toward the coast
        let level = level.max(height - SHARE * (height - sea - SEA_MARGIN));
        floor.push((x as f64, z as f64, level));
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
    floor.iter().zip(smooth).map(|(f, (x, z))| (x, z, f.2)).collect()
}

// Lowers `map.height` along `floor` to a trench `half` cells to a side, flat in its middle third and rising in a wall, from the ground as it stood.
fn stamp(map: &mut WorldMap, before: &[f32], floor: &Floor, half: f64, keep: &[f32]) {
    let dim = map.dim();
    let reach = half.ceil() as i64 + 1;
    for &(px, pz, level) in floor {
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

/// Cuts up to `count` canyons into desert plateau, each a drop of 90 to 200 blocks (less where the plateau is low) and 2 to 3.5 cells to a side, leaving `keep` (landmarks) alone.
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
        let floor = trace(&flood, dim, (sx, sz), depth);
        stamp(map, &before, &floor, half, keep);
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
        let floor = trace(&priority_flood(&map.height, dim, &flood_ties(&map)), dim, (100, 200), 150.0);
        assert!(floor.len() > 40, "{} cells long", floor.len());
        assert!(floor.windows(2).all(|w| w[1].2 < w[0].2), "the floor rises");
        stamp(&mut map, &before, &floor, 3.0, &keep);
        let (mx, mz, level) = floor[floor.len() / 2];
        let mid = map.height[mz as usize * dim + mx as usize] as f64;
        assert!((mid - level).abs() < 1.0, "the middle is {mid}, the floor {level}");
        assert!(before[mz as usize * dim + mx as usize] as f64 - mid > 100.0, "no wall");
        for i in (0..dim * dim).filter(|&i| floor.iter().all(|&(x, z, _)| ((i % dim) as f64 - x).hypot((i / dim) as f64 - z) > 5.0)) {
            assert_eq!(before[i], map.height[i], "cut beyond its width at {i}");
        }
    }

    #[test]
    fn landmarks_are_left_standing() {
        let (mut map, mut keep) = slope();
        let (dim, before) = (map.dim(), map.height.clone());
        keep.fill(1.0);
        let floor = trace(&priority_flood(&map.height, dim, &flood_ties(&map)), dim, (100, 200), 150.0);
        stamp(&mut map, &before, &floor, 3.0, &keep);
        assert_eq!(before, map.height);
    }
}
