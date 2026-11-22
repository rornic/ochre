//! How far each cell stands over the ground round it, so crests and hollows can be told apart.

use super::climate::blur_box;
use super::WorldMap;
use ochre_base::WATER_LEVEL;

// Cells each way the surrounding ground is averaged over, and the blocks over or under it that read as the extremes.
const RADIUS: usize = 8;
const SCALE: f32 = 25.0;

/// Fills `map.exposure`: 0..1 as a byte, 0.5 on level ground, up on crests and down in hollows, averaged over land only.
pub fn run(map: &mut WorldMap) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f32;
    let land: Vec<f32> = map.height.iter().map(|&h| (h >= sea) as u8 as f32).collect();
    let weighted: Vec<f32> = map.height.iter().zip(&land).map(|(h, l)| h * l).collect();
    let (sum, share) = (blur_box(&weighted, dim, RADIUS), blur_box(&land, dim, RADIUS));
    map.exposure = (0..dim * dim)
        .map(|i| {
            if land[i] == 0.0 {
                return 128;
            }
            let around = sum[i] / share[i].max(1e-3);
            let exposure = 0.5 + (map.height[i] - around) / (2.0 * SCALE);
            (exposure.clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    fn tiny() -> WorldMap {
        let config = WorldConfig { size: 8192, map_cell: 32, ..Default::default() };
        let mut map = WorldMap::empty(1, config);
        map.height = vec![WATER_LEVEL as f32 + 100.0; map.dim() * map.dim()];
        map
    }

    #[test]
    fn level_ground_reads_half_and_crests_and_hollows_read_either_side() {
        let mut map = tiny();
        let dim = map.dim();
        run(&mut map);
        assert!(map.exposure.iter().all(|&e| e == 128), "level ground");
        let (crest, hollow) = (100 * dim + 100, 100 * dim + 180);
        map.height[crest] += 60.0;
        map.height[hollow] -= 60.0;
        run(&mut map);
        assert_eq!(255, map.exposure[crest]);
        assert_eq!(0, map.exposure[hollow]);
        assert_eq!(128, map.exposure[100 * dim + 20], "ground far from either");
    }

    #[test]
    fn the_sea_reads_half_and_does_not_pull_the_coast_up() {
        let mut map = tiny();
        let dim = map.dim();
        map.height[..dim * dim / 2].fill(WATER_LEVEL as f32 - 50.0);
        run(&mut map);
        assert_eq!(128, map.exposure[10 * dim + 10], "sea");
        assert_eq!(128, map.exposure[(dim / 2 + 1) * dim + 100], "land by the sea");
    }
}
