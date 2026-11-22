//! Sea stacks: pillars of rock left standing in shallow water a little off a cliffed coast, as the waves wear the headland back.

use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{hash, unit};
use ochre_base::WATER_LEVEL;

use super::super::climate::blur_box;
use super::super::shapes::sites_apart;
use super::super::WorldMap;

const SALT: u32 = 0x1A4D_005A;
// Land this many blocks over the sea is a cliff, and a stack stands in water between these depths, 2 to 4 cells off it.
const CLIFF: f32 = 25.0;
const DEPTH: (f32, f32) = (3.0, 28.0);
const OFF: (usize, usize) = (1, 4);
// Cells between stacks, and kept off the map's edge.
const APART: f64 = 3.0;
const EDGE: usize = 40;

// The sea cells a stack may stand in.
fn eligible(map: &WorldMap) -> Vec<usize> {
    let dim = map.dim();
    let sea = WATER_LEVEL as f32;
    let cliffs: Vec<f32> = map.height.iter().map(|&h| (h >= sea + CLIFF) as u8 as f32).collect();
    let (near, close) = (blur_box(&cliffs, dim, OFF.1), blur_box(&cliffs, dim, OFF.0));
    (0..dim * dim)
        .filter(|&i| {
            let (x, z) = (i % dim, i / dim);
            let under = sea - map.height[i];
            x >= EDGE && z >= EDGE && x < dim - EDGE && z < dim - EDGE && (DEPTH.0..=DEPTH.1).contains(&under) && near[i] > 0.0 && close[i] == 0.0
        })
        .collect()
}

// Raises a stack at (`sx`, `sz`): a pillar `radius` cells to a side and `height` blocks over the sea, steep sided.
fn raise(map: &mut WorldMap, (sx, sz): (usize, usize), radius: f64, height: f64) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f64;
    let reach = radius.ceil() as i64 + 1;
    for z in (sz as i64 - reach).max(0)..=(sz as i64 + reach).min(dim as i64 - 1) {
        for x in (sx as i64 - reach).max(0)..=(sx as i64 + reach).min(dim as i64 - 1) {
            let t = libm::sqrt((x as f64 - sx as f64).powi(2) + (z as f64 - sz as f64).powi(2)) / radius;
            let i = z as usize * dim + x as usize;
            let top = sea + height * (1.0 - smoothstep(0.45, 1.0, t));
            map.height[i] = map.height[i].max(top as f32);
        }
    }
}

/// Raises up to `count` sea stacks, each 20 to 55 blocks over the sea and a cell or so across, in the shallows 2 to 4 cells off a cliffed coast.
pub(crate) fn run(map: &mut WorldMap, count: u32) {
    if count == 0 || map.dim() <= 2 * EDGE {
        return;
    }
    let sites = eligible(map);
    for (sx, sz) in sites_apart(map, sites, count as usize, SALT, APART) {
        let h = hash(map.seed ^ SALT, sx as i32, sz as i32);
        raise(map, (sx, sz), 0.7 + 0.6 * unit(h, 2, 0), 20.0 + 35.0 * unit(h, 3, 0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    // Cliffed land 60 blocks over the sea in the west half, and shallows 10 blocks deep in the east.
    fn coast() -> WorldMap {
        let config = WorldConfig { size: 400 * 32, map_cell: 32, ..Default::default() };
        let mut map = WorldMap::empty(11, config);
        let dim = map.dim();
        for (i, h) in map.height.iter_mut().enumerate() {
            *h = if i % dim < dim / 2 { WATER_LEVEL as f32 + 60.0 } else { WATER_LEVEL as f32 - 10.0 };
        }
        map
    }

    #[test]
    fn stacks_stand_in_the_shallows_a_little_off_a_cliff_and_nowhere_else() {
        let mut map = coast();
        let (dim, before) = (map.dim(), map.height.clone());
        run(&mut map, 40);
        let raised: Vec<usize> = (0..dim * dim).filter(|&i| map.height[i] != before[i]).collect();
        assert!(raised.len() > 20, "{} cells raised", raised.len());
        let shore = dim / 2;
        for &i in &raised {
            let x = i % dim;
            assert!(x >= shore && x <= shore + OFF.1 + 3, "a stack {} cells off", x as i64 - shore as i64);
            assert!(map.height[i] > before[i] && before[i] < WATER_LEVEL as f32);
        }
        let top = raised.iter().map(|&i| map.height[i]).fold(f32::MIN, f32::max);
        assert!(top > WATER_LEVEL as f32 + 15.0 && top <= WATER_LEVEL as f32 + 56.0, "{top}");
    }

    #[test]
    fn open_sea_and_low_coasts_get_none() {
        let mut map = coast();
        map.height.iter_mut().for_each(|h| *h = h.min(WATER_LEVEL as f32 + 10.0));
        let before = map.height.clone();
        run(&mut map, 40);
        assert_eq!(before, map.height, "a stack off a low coast");
    }
}
