//! Eskers: sinuous gravel ridges from a glacier's meltwater, raised along the line water takes in taiga and tundra.

use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{hash, perlin, unit};
use ochre_base::WATER_LEVEL;

use super::super::climate::BiomeKind;
use super::super::hydrology::{flood_ties, priority_flood, Flood, NONE};
use super::super::shapes::sites_apart;
use super::super::WorldMap;
use super::landmarks::biomes;

const SALT: u32 = 0x1A4D_00E5;
// Cells between heads, and kept off the map's edge.
const APART: f64 = 120.0;
const EDGE: usize = 60;
// Cells each way the path's bends are averaged over, and the cells at each end over which the ridge rises and falls.
const SMOOTH: usize = 3;
const TAPER: f64 = 10.0;
// The ground an esker stands on, over the sea.
const ABOVE: f64 = 15.0;

// The cells `len` steps down the line the water will take from `start`, as positions with their bends averaged.
fn path(flood: &Flood, dim: usize, start: (usize, usize), len: usize) -> Vec<(f64, f64)> {
    let sea = WATER_LEVEL as f32;
    let mut cell = start.1 * dim + start.0;
    let mut raw: Vec<(f64, f64)> = Vec::new();
    for _ in 0..len {
        let (x, z) = (cell % dim, cell / dim);
        if x < EDGE || z < EDGE || x >= dim - EDGE || z >= dim - EDGE || flood.filled[cell] <= sea + 1.0 {
            break;
        }
        raw.push((x as f64, z as f64));
        if flood.down[cell] == NONE {
            break;
        }
        cell = flood.down[cell] as usize;
    }
    (0..raw.len())
        .map(|i| {
            let window = &raw[i.saturating_sub(SMOOTH)..(i + SMOOTH + 1).min(raw.len())];
            let n = window.len() as f64;
            (window.iter().map(|p| p.0).sum::<f64>() / n, window.iter().map(|p| p.1).sum::<f64>() / n)
        })
        .collect()
}

// Raises `map.height` along `line` by up to `height`, a ridge `half` cells wide easing at its ends, leaving `keep`.
fn raise(map: &mut WorldMap, before: &[f32], line: &[(f64, f64)], half: f64, height: f64, h: u32, keep: &[f32]) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f64;
    let mut added = vec![0.0f64; dim * dim];
    let reach = half.ceil() as i64 + 1;
    for (n, &(px, pz)) in line.iter().enumerate() {
        let ends = smoothstep(0.0, TAPER, n as f64) * smoothstep(0.0, TAPER, (line.len() - 1 - n) as f64);
        let top = height * ends * (0.75 + 0.25 * perlin(h ^ SALT, n as f64 * 0.07, 0.5));
        for z in (pz as i64 - reach).max(0)..=(pz as i64 + reach).min(dim as i64 - 1) {
            for x in (px as i64 - reach).max(0)..=(px as i64 + reach).min(dim as i64 - 1) {
                let t = libm::sqrt((x as f64 - px).powi(2) + (z as f64 - pz).powi(2)) / half;
                let i = z as usize * dim + x as usize;
                added[i] = added[i].max(top * (1.0 - smoothstep(0.3, 1.0, t)));
            }
        }
    }
    for i in (0..dim * dim).filter(|&i| added[i] > 0.0 && before[i] as f64 - sea > 3.0) {
        map.height[i] = map.height[i].max(before[i] + (added[i] * (1.0 - keep[i] as f64)) as f32);
    }
}

/// Raises up to `count` eskers on taiga and tundra 15+ blocks over the sea, 30 to 130 cells long, 14 to 30 high.
pub(crate) fn run(map: &mut WorldMap, count: u32, keep: &[f32]) {
    let dim = map.dim();
    if count == 0 || dim <= 2 * EDGE {
        return;
    }
    let sea = WATER_LEVEL as f32;
    let eligible: Vec<usize> = biomes(map)
        .iter()
        .enumerate()
        .filter(|&(i, b)| matches!(b, Some(BiomeKind::Taiga | BiomeKind::Tundra)) && (map.height[i] - sea) as f64 >= ABOVE)
        .map(|(i, _)| i)
        .collect();
    if eligible.is_empty() {
        return;
    }
    let flood = priority_flood(&map.height, dim, &flood_ties(map));
    let before = map.height.clone();
    for (sx, sz) in sites_apart(map, eligible, count as usize, SALT, APART) {
        let h = hash(map.seed ^ SALT, sx as i32, sz as i32);
        let line = path(&flood, dim, (sx, sz), 50 + (80.0 * unit(h, 2, 0)) as usize);
        if line.len() >= 30 {
            raise(map, &before, &line, 0.8 + 0.6 * unit(h, 3, 0), 14.0 + 16.0 * unit(h, 4, 0), h, keep);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    // A plain 60 blocks over the sea that falls a quarter block a cell toward +x.
    fn plain() -> (WorldMap, Vec<f32>) {
        let config = WorldConfig { size: 400 * 32, map_cell: 32, ..Default::default() };
        let mut map = WorldMap::empty(11, config);
        let dim = map.dim();
        for (i, h) in map.height.iter_mut().enumerate() {
            *h = WATER_LEVEL as f32 + 60.0 - 0.25 * (i % dim) as f32;
        }
        let keep = vec![0.0; dim * dim];
        (map, keep)
    }

    #[test]
    fn an_esker_is_a_narrow_ridge_along_the_water_that_rises_and_falls_at_its_ends() {
        let (mut map, keep) = plain();
        let (dim, before) = (map.dim(), map.height.clone());
        let line = path(&priority_flood(&map.height, dim, &flood_ties(&map)), dim, (100, 200), 100);
        assert!(line.len() > 60, "{} cells long", line.len());
        raise(&mut map, &before, &line, 1.0, 25.0, 3, &keep);
        let at = |p: (f64, f64)| map.height[p.1.round() as usize * dim + p.0.round() as usize] - before[p.1.round() as usize * dim + p.0.round() as usize];
        assert!(at(line[line.len() / 2]) > 12.0, "{} high in the middle", at(line[line.len() / 2]));
        assert!(at(line[0]) < 2.0 && at(line[line.len() - 1]) < 2.0, "the ends do not taper");
        assert!((0..dim * dim).all(|i| map.height[i] >= before[i]), "an esker cut ground");
        let far = (0..dim * dim).filter(|&i| line.iter().all(|&(x, z)| ((i % dim) as f64 - x).hypot((i / dim) as f64 - z) > 4.0));
        assert!(far.into_iter().all(|i| map.height[i] == before[i]), "raised beyond its width");
    }

    #[test]
    fn landmarks_are_left_standing() {
        let (mut map, mut keep) = plain();
        let (dim, before) = (map.dim(), map.height.clone());
        keep.fill(1.0);
        let line = path(&priority_flood(&map.height, dim, &flood_ties(&map)), dim, (100, 200), 100);
        raise(&mut map, &before, &line, 1.0, 25.0, 3, &keep);
        assert_eq!(before, map.height);
    }
}
