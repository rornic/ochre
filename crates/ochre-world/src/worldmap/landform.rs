//! Landform classes, decided from the ground alone, never from climate or water: block-scale detail is
//! laid by landform, so the map's water can be worked out over the ground it will really stand on.

use super::climate::{tidy_biomes, window_max, BiomeKind};
use super::WorldMap;
use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::fbm;
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

// Tableland: heights above the sea it is full between, and the noise patches it comes in.
const TABLE_WINDOW: (f64, f64, f64, f64) = (40.0, 90.0, 300.0, 360.0);
const TABLE_WAVELENGTH: f64 = 20000.0;

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

/// How much of the ground at a point is tableland, 0..1: the terraced beds of sedimentary country that
/// isn't mountain, between `TABLE_WINDOW` heights, in patches that cover about `share` of it. Like
/// mountain it reads height, rock province and noise alone, never climate or biome.
pub(super) fn tableland(
    seed: u32,
    share: f64,
    x: f64,
    z: f64,
    above_sea: f64,
    mountain: f64,
) -> f64 {
    let window = smoothstep(TABLE_WINDOW.0, TABLE_WINDOW.1, above_sea)
        * (1.0 - smoothstep(TABLE_WINDOW.2, TABLE_WINDOW.3, above_sea));
    if window <= 0.0 || mountain >= 1.0 {
        return 0.0;
    }
    let n = (fbm(seed ^ 0x7AB1, x, z, TABLE_WAVELENGTH, 2) + 1.0) / 2.0;
    // the noise sits between about 0.3 and 0.65, so the cut that leaves `share` of it above runs from 0.59 down to 0.41
    let cut = 0.5 - 0.3 * (share - 0.5);
    (1.0 - mountain) * window * smoothstep(cut - 0.04, cut + 0.04, n)
}

/// Steps the tableland's ground into its beds, so water is routed over the treads it will stand on.
pub fn terrace(map: &mut WorldMap, share: f64) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f64;
    for i in 0..dim * dim {
        let above = map.height[i] as f64 - sea;
        let mountain = (map.landform[i] == MOUNTAIN) as u8 as f64;
        let (x, z) = map.cell_centre(i % dim, i / dim);
        let weight = tableland(map.seed, share, x, z, above, mountain);
        if weight > 0.0 {
            let height = map.height[i] as f64;
            let stepped = map.terraced(x, z, height, above);
            map.height[i] = (height + (stepped - height) * weight) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::{character::LandformKnobs, generate, WorldConfig};

    #[test]
    fn tableland_heights_sit_on_their_beds_before_water_is_routed() {
        let config = WorldConfig {
            size: 65536,
            map_cell: 128,
            ..Default::default()
        };
        let map = generate(1, config);
        let share = LandformKnobs::from_seed(1).tableland_share;
        let sea = WATER_LEVEL as f64;
        let dim = map.dim();
        let mut off = Vec::new();
        for i in 0..dim * dim {
            let (x, z) = map.cell_centre(i % dim, i / dim);
            let (height, mountain) = (
                map.height[i] as f64,
                (map.landform[i] == MOUNTAIN) as u8 as f64,
            );
            if tableland(1, share, x, z, height - sea, mountain) < 1.0 {
                continue;
            }
            off.push((map.terraced(x, z, height, height - sea) - height).abs());
        }
        assert!(
            off.len() > 500,
            "only {} cells of full tableland",
            off.len()
        );
        off.sort_by(f64::total_cmp);
        // 17 blocks off without the pass, and rivers cut some cells further after it
        assert!(
            off[off.len() / 2] < 8.0,
            "a tableland cell is typically {} blocks off its bed",
            off[off.len() / 2]
        );
    }
}
