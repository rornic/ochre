//! The terms a place is scored on, worked out over a square grid of ground heights.

use super::super::climate::blur_box;

/// Half-widths of the three relief windows, in grid cells: with the default 64-block step, about 128, 512 and 1,536 blocks. The first is the scale of a
/// landmark (a few map cells), the second of a massif or a bowl, the third of a range or a basin.
pub(super) const WINDOWS: [usize; 3] = [2, 8, 24];

/// Half-width, in grid cells, the ground is smoothed over before anything is measured: a feature has to be about three cells (190 blocks at the default step)
/// across to count, so a thin spike a few blocks wide, which a landmark is not, does not score as a peak.
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
