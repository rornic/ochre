//! Ponds: shallow round pools thawed out of frozen ground on flat tundra and taiga, marked so hydrology keeps them.

use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{hash, unit};
use ochre_base::WATER_LEVEL;

use super::super::climate::BiomeKind;
use super::super::shapes::sites_apart;
use super::super::WorldMap;
use super::landmarks::biomes;

const SALT: u32 = 0x1A4D_00B0;
// Cells between ponds, and kept off the map's edge; the ground they stand on, over the sea.
const APART: f64 = 4.0;
const EDGE: usize = 60;
const ABOVE: f64 = 8.0;

// Sinks a pond at (`sx`, `sz`): `radius` cells across, `depth` blocks deep at its middle, a bowl with a flat floor.
fn sink(map: &mut WorldMap, (sx, sz): (usize, usize), radius: f64, depth: f64) {
    let dim = map.dim();
    let reach = radius.ceil() as i64 + 1;
    for z in (sz as i64 - reach).max(0)..=(sz as i64 + reach).min(dim as i64 - 1) {
        for x in (sx as i64 - reach).max(0)..=(sx as i64 + reach).min(dim as i64 - 1) {
            let t = libm::sqrt((x as f64 - sx as f64).powi(2) + (z as f64 - sz as f64).powi(2)) / radius;
            map.height[z as usize * dim + x as usize] -= (depth * (1.0 - smoothstep(0.35, 1.0, t))) as f32;
        }
    }
}

/// Sinks up to `count` ponds, 1 to 2.2 cells across and 4 to 10 blocks deep; returns their middle cells.
pub(crate) fn run(map: &mut WorldMap, count: u32) -> Vec<bool> {
    let dim = map.dim();
    let mut kept = vec![false; dim * dim];
    if count == 0 || dim <= 2 * EDGE {
        return kept;
    }
    let sea = WATER_LEVEL as f64;
    let eligible: Vec<usize> = biomes(map)
        .iter()
        .enumerate()
        .filter(|&(i, b)| matches!(b, Some(BiomeKind::Tundra | BiomeKind::Taiga)) && map.height[i] as f64 - sea >= ABOVE)
        .map(|(i, _)| i)
        .collect();
    for (sx, sz) in sites_apart(map, eligible, count as usize, SALT, APART) {
        let h = hash(map.seed ^ SALT, sx as i32, sz as i32);
        let (radius, depth) = (1.0 + 1.2 * unit(h, 2, 0), 4.0 + 6.0 * unit(h, 3, 0));
        sink(map, (sx, sz), radius, depth);
        let reach = (radius * 0.5).ceil() as i64;
        for z in (sz as i64 - reach).max(0)..=(sz as i64 + reach).min(dim as i64 - 1) {
            for x in (sx as i64 - reach).max(0)..=(sx as i64 + reach).min(dim as i64 - 1) {
                kept[z as usize * dim + x as usize] = true;
            }
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    #[test]
    fn a_pond_is_a_shallow_round_dip_with_a_flat_floor() {
        let config = WorldConfig { size: 400 * 32, map_cell: 32, ..Default::default() };
        let mut map = WorldMap::empty(11, config);
        map.height.fill(WATER_LEVEL as f32 + 50.0);
        let dim = map.dim();
        sink(&mut map, (200, 200), 4.0, 8.0);
        let at = |dx: i64, dz: i64| map.height[((200 + dz) as usize) * dim + (200 + dx) as usize];
        assert!((at(0, 0) - (WATER_LEVEL as f32 + 42.0)).abs() < 0.01, "{}", at(0, 0));
        assert_eq!(at(0, 0), at(0, 1), "the floor is not flat");
        assert!(at(0, 0) < at(2, 0) && at(2, 0) < at(4, 0) + 0.01 && at(4, 0) > at(2, 0));
        assert_eq!(WATER_LEVEL as f32 + 50.0, at(6, 6), "dug beyond its width");
    }

    #[test]
    fn no_ponds_when_asked_for_none() {
        let config = WorldConfig { size: 400 * 32, map_cell: 32, ..Default::default() };
        let mut map = WorldMap::empty(11, config);
        let before = map.height.clone();
        assert!(run(&mut map, 0).iter().all(|&k| !k));
        assert_eq!(before, map.height);
    }
}
