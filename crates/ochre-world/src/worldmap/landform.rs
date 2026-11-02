//! Landform classes, decided from the ground alone, never from climate or water: block-scale detail is
//! laid by landform, so the map's water can be worked out over the ground it will really stand on.

use super::climate::{tidy_biomes, window_max, BiomeKind};
use super::WorldMap;
use ochre_base::WATER_LEVEL;

pub const MOUNTAIN: u8 = 1;

// Land this many blocks over the sea is mountain, or this many with ground at least this steep.
const ABOVE: f32 = 150.0;
const STEEP_ABOVE: f32 = 70.0;
const STEEP: f32 = 0.4;
// High or steep ground is only mountain if a real peak, this far above the sea, stands within `PEAK_REACH`
// cells; rougher ground short of one is hills.
const PEAK: f32 = 300.0;
const PEAK_REACH: usize = 10;

/// Fills `map.landform` from the ground in `map.height`.
pub fn run(map: &mut WorldMap) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f32;
    let slope = super::gameplay::slope_layer(map);
    let peaks = window_max(&map.height, dim, PEAK_REACH);
    // tidied with the same filter the biomes use, over three classes: sea, mountain and the rest
    let kinds: Vec<u8> = (0..dim * dim)
        .map(|i| {
            let above = map.height[i] - sea;
            if map.height[i] < sea {
                BiomeKind::Sea as u8
            } else if peaks[i] - sea >= PEAK
                && (above >= ABOVE || (above >= STEEP_ABOVE && slope[i] >= STEEP))
            {
                BiomeKind::Mountain as u8
            } else {
                BiomeKind::Grassland as u8
            }
        })
        .collect();
    map.landform = tidy_biomes(&kinds, dim)
        .into_iter()
        .map(|k| (k == BiomeKind::Mountain as u8) as u8 * MOUNTAIN)
        .collect();
}
