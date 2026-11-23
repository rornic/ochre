//! Base relief for biomes the tectonics leave too flat: jungle's steep ridged hills, savanna's long low swells, glacier ice ridges, in proportion to each cell's surroundings.

use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{fbm, hash, ridged, unit};
use ochre_base::WATER_LEVEL;

use super::super::climate::{blur_box, closeness, BiomeKind};
use super::super::{WaterCell, WorldMap};
use super::landmarks::classes;

// One layer of a biome's relief: noise of `wavelength` blocks, ridged or smooth, adding up to about `amp` blocks either way.
struct Layer {
    biome: BiomeKind,
    wavelength: f64,
    amp: f64,
    ridged: bool,
    // a step instead of a wave: ground a plateau on one side of the noise's zero and a plain on the other, with a scarp between
    scarp: bool,
    // 0 for smooth ground, else the height of the ledges the relief is cut into, each a flat tread and a steep riser
    steps: f64,
    salt: u32,
}

const LAYERS: &[Layer] = &[
    // steep hills in ridges, cut into ledges and ridged again finer so they step round each other, over a broad swell
    Layer { biome: BiomeKind::Jungle, wavelength: 1100.0, amp: 42.0, ridged: true, scarp: false, steps: 9.0, salt: 0x4E11_0001 },
    Layer { biome: BiomeKind::Jungle, wavelength: 650.0, amp: 16.0, ridged: true, scarp: false, steps: 6.0, salt: 0x4E11_0008 },
    Layer { biome: BiomeKind::Jungle, wavelength: 3000.0, amp: 14.0, ridged: false, scarp: false, steps: 0.0, salt: 0x4E11_0002 },
    // long low swells with the odd low ridge between
    Layer { biome: BiomeKind::Savanna, wavelength: 2200.0, amp: 38.0, ridged: false, scarp: false, steps: 0.0, salt: 0x4E11_0003 },
    Layer { biome: BiomeKind::Savanna, wavelength: 1300.0, amp: 26.0, ridged: true, scarp: false, steps: 0.0, salt: 0x4E11_0004 },
    // and a plateau stepping down to a plain by an escarpment, as the old planation surfaces of Africa do
    Layer { biome: BiomeKind::Savanna, wavelength: 4200.0, amp: 36.0, ridged: false, scarp: true, steps: 0.0, salt: 0x4E11_0007 },
    // ice ridges over a rolling sheet
    Layer { biome: BiomeKind::Glacier, wavelength: 1800.0, amp: 45.0, ridged: true, scarp: false, steps: 0.0, salt: 0x4E11_0005 },
    Layer { biome: BiomeKind::Glacier, wavelength: 4500.0, amp: 30.0, ridged: false, scarp: false, steps: 0.0, salt: 0x4E11_0006 },
];
// Cells each way a biome's presence is softened over, so its relief fades out toward the next biome.
const SOFTEN: usize = 5;
// The ground this high over the sea takes the relief, and keeps the coast as it was below.
const INLAND: (f64, f64) = (5.0, 40.0);

// How much taller and longer than the table a seed makes a layer's relief, 0.6 to 1.5 and 0.75 to 1.4.
fn style(seed: u32, layer: &Layer) -> (f64, f64) {
    (0.6 + 0.9 * unit(seed ^ layer.salt, 1, 0), 0.75 + 0.65 * unit(seed ^ layer.salt, 2, 0))
}

// `rise` cut into ledges `step` blocks high: flat for the first of each, then a steep riser to the next.
fn stepped(rise: f64, step: f64) -> f64 {
    let along = rise / step;
    let base = along.floor();
    (base + smoothstep(0.65, 1.0, along - base)) * step
}

// Adds `layer`'s relief to `map.height`, over the cells `mask` (0..1) covers, except where `keep` holds a landmark.
fn apply(map: &mut WorldMap, mask: &[f32], layer: &Layer, (keep, wet): (&[f32], &[f32])) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f64;
    let (tall, long) = style(map.seed, layer);
    let seed = hash(map.seed, layer.salt as i32, 0);
    for i in (0..dim * dim).filter(|&i| mask[i] > 0.01) {
        let above = map.height[i] as f64 - sea;
        let weight = mask[i] as f64 * smoothstep(INLAND.0, INLAND.1, above) * (1.0 - keep[i] as f64) * (1.0 - smoothstep(0.04, 0.28, wet[i] as f64));
        if weight <= 0.0 {
            continue;
        }
        let (x, z) = map.cell_centre(i % dim, i / dim);
        let n = if layer.scarp {
            2.0 * smoothstep(-0.2, 0.2, fbm(seed, x, z, layer.wavelength * long, 2)) - 1.0
        } else if layer.ridged { (ridged(seed, x, z, layer.wavelength * long, 3) - 0.45) * 2.0 } else { fbm(seed, x, z, layer.wavelength * long, 3) };
        let rise = weight * layer.amp * tall * n;
        map.height[i] += (if layer.steps > 0.0 { stepped(rise, layer.steps) } else { rise }) as f32;
    }
}

/// Lays the relief of jungle, savanna and glacier country into `map.height`, leaving `keep` (landmarks) as they stand.
pub(crate) fn run(map: &mut WorldMap, keep: &[f32]) {
    let dim = map.dim();
    let class = classes(map);
    // the rivers and lakes of the first water run: steep ground beside them left a river's bank lower than its water, so the relief keeps about 7 cells off
    let wet = closeness(map, |i| map.water_kind[i] == WaterCell::Lake as u8 || map.water_kind[i] == WaterCell::River as u8);
    for biome in [BiomeKind::Jungle, BiomeKind::Savanna, BiomeKind::Glacier] {
        let present: Vec<f32> = class.iter().map(|&c| (c == Some(biome)) as u8 as f32).collect();
        if present.iter().all(|&p| p == 0.0) {
            continue;
        }
        let mask = blur_box(&present, dim, SOFTEN);
        for layer in LAYERS.iter().filter(|l| l.biome == biome) {
            apply(map, &mask, layer, (keep, &wet));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    fn plain() -> WorldMap {
        let config = WorldConfig { size: 400 * 32, map_cell: 32, ..Default::default() };
        let mut map = WorldMap::empty(11, config);
        map.height.fill(WATER_LEVEL as f32 + 60.0);
        map
    }

    #[test]
    fn a_layer_raises_and_lowers_ground_within_its_amplitude_and_only_where_the_mask_covers() {
        let mut map = plain();
        let dim = map.dim();
        let mut mask = vec![0.0f32; dim * dim];
        for z in 100..300 {
            for x in 100..300 {
                mask[z * dim + x] = 1.0;
            }
        }
        let keep = vec![0.0f32; dim * dim];
        let before = map.height.clone();
        let jungle = &LAYERS[0];
        apply(&mut map, &mask, jungle, (&keep, &vec![0.0f32; dim * dim]));
        let (mut up, mut down) = (0.0f32, 0.0f32);
        for i in 0..dim * dim {
            let d = map.height[i] - before[i];
            if mask[i] == 0.0 {
                assert_eq!(0.0, d, "relief outside the mask");
            }
            up = up.max(d);
            down = down.min(d);
        }
        let most = (jungle.amp * 1.5 * 1.1) as f32;
        assert!(up > 20.0 && down < -20.0 && up <= most && down >= -most, "{down} to {up}");
    }

    #[test]
    fn ledges_are_flat_treads_with_steep_risers_and_never_run_backwards() {
        let heights: Vec<f64> = (0..600).map(|i| stepped(i as f64 * 0.1 - 20.0, 16.0)).collect();
        assert!(heights.windows(2).all(|w| w[1] >= w[0] - 1e-9), "a ledge runs backwards");
        let flat = heights.windows(2).filter(|w| (w[1] - w[0]).abs() < 1e-6).count();
        let riser = heights.windows(2).map(|w| w[1] - w[0]).fold(0.0, f64::max);
        assert!(flat > 150, "{flat} flat steps of 600");
        // 16 blocks over a third of a step of 16 along 0.1 steps: a riser climbs well over a block a tenth
        assert!(riser > 0.4, "{riser}");
        assert!((stepped(0.0, 16.0)).abs() < 1e-9 && (stepped(16.0, 16.0) - 16.0).abs() < 1e-9);
    }

    #[test]
    fn landmarks_and_the_coast_keep_their_shape() {
        let mut map = plain();
        let dim = map.dim();
        let mask = vec![1.0f32; dim * dim];
        let mut keep = vec![0.0f32; dim * dim];
        keep.iter_mut().take(dim * 100).for_each(|k| *k = 1.0);
        for h in map.height.iter_mut().skip(dim * 300) {
            *h = WATER_LEVEL as f32 + 1.0;
        }
        let before = map.height.clone();
        apply(&mut map, &mask, &LAYERS[0], (&keep, &vec![0.0f32; dim * dim]));
        assert_eq!(before[..dim * 100], map.height[..dim * 100], "a landmark's ground moved");
        for i in dim * 300..dim * dim {
            assert!((map.height[i] - before[i]).abs() < 4.0, "the coast moved {}", map.height[i] - before[i]);
        }
    }

    #[test]
    fn seeds_make_the_same_relief_taller_or_shorter() {
        let sizes: Vec<f64> = (0..100).map(|seed| style(seed, &LAYERS[0]).0).collect();
        let (low, high) = sizes.iter().fold((f64::MAX, 0.0f64), |(l, h), &s| (l.min(s), h.max(s)));
        assert!(low < 0.7 && high > 1.4 && low >= 0.6 && high <= 1.5, "{low} to {high}");
    }
}
