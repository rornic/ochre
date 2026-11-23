//! The terms a place is scored on, worked out over a square grid of ground heights.

use super::super::climate::blur_box;

/// Half-widths of the three relief windows in cells (128, 512, 1,536 blocks by default): landmark, massif/bowl, range
pub(super) const WINDOWS: [usize; 3] = [2, 8, 24];

/// Half-width in cells the ground is smoothed over before measuring: a feature needs ~3 cells across to score a peak
pub(super) const FOOTPRINT: usize = 1;

pub(super) struct Terms {
    /// The smoothed ground height, in blocks.
    pub ground: Vec<f32>,
    /// Height over (or under, as a negative) the land's mean within each window, in blocks.
    pub relief: [Vec<f32>; 3],
    /// The steepest rise over run across a cell, from the cells either side of it.
    pub steep: Vec<f32>,
}

pub(super) fn from_heights(raw: &[f32], dim: usize, step: f64) -> Terms {
    let heights = &blur_box(raw, dim, FOOTPRINT)[..];
    let relief = WINDOWS.map(|radius| {
        let mean = blur_box(heights, dim, radius);
        heights.iter().zip(&mean).map(|(h, m)| h - m).collect()
    });
    let at = |x: usize, z: usize| heights[z * dim + x];
    let mut steep = vec![0.0f32; heights.len()];
    for z in 0..dim {
        for x in 0..dim {
            let (x0, x1, z0, z1) = (x.saturating_sub(1), (x + 1).min(dim - 1), z.saturating_sub(1), (z + 1).min(dim - 1));
            let dx = (at(x1, z) - at(x0, z)) / ((x1 - x0).max(1) as f64 * step) as f32;
            let dz = (at(x, z1) - at(x, z0)) / ((z1 - z0).max(1) as f64 * step) as f32;
            steep[z * dim + x] = (dx * dx + dz * dz).sqrt();
        }
    }
    Terms { ground: heights.to_vec(), relief, steep }
}

/// Half-width, in cells, within which a fall counts for a cell (512 blocks by default), and for the lake share.
const FALL_REACH: i64 = 8;
const LAKE_WINDOW: usize = 4;
/// What a lake filling a whole window counts for, in blocks of fall.
const LAKE_WORTH: f32 = 40.0;
/// Falls under this many blocks are not worth a place.
const MIN_FALL: f32 = 10.0;

/// The water near each cell: the tallest fall within `FALL_REACH` cells, plus up to `LAKE_WORTH` for a lake.
pub(super) fn water(terrain: &super::super::WorldTerrain, dim: usize, step: f64, at: impl Fn(usize) -> (f64, f64)) -> Vec<f32> {
    use super::super::WaterCell;
    let map = terrain.map();
    let (origin_x, origin_z) = at(0);
    let mut falls = vec![0.0f32; dim * dim];
    for (x, z, drop, _) in terrain.falls().into_iter().filter(|f| f.2 >= MIN_FALL) {
        let (gx, gz) = (((x - origin_x) / step).round() as i64, ((z - origin_z) / step).round() as i64);
        for dz in -FALL_REACH..=FALL_REACH {
            for dx in -FALL_REACH..=FALL_REACH {
                let (cx, cz) = (gx + dx, gz + dz);
                if (0..dim as i64).contains(&cx) && (0..dim as i64).contains(&cz) {
                    let i = (cz as usize) * dim + cx as usize;
                    falls[i] = falls[i].max(drop);
                }
            }
        }
    }
    let lake: Vec<f32> = (0..dim * dim)
        .map(|i| {
            let (x, z) = at(i);
            let (u, v) = map.to_cell(x, z);
            let max = (map.dim() - 1) as f64;
            (map.water_kind[map.index(u.round().clamp(0.0, max) as usize, v.round().clamp(0.0, max) as usize)] == WaterCell::Lake as u8) as u8 as f32
        })
        .collect();
    let share = blur_box(&lake, dim, LAKE_WINDOW);
    falls.iter().zip(&share).map(|(f, l)| f + LAKE_WORTH * l).collect()
}
