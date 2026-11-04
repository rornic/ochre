//! Undercuts and arches: the mass of a cliff cut away near its face so the cap overhangs. The ground is smoothed over a
//! chunk column and its surroundings, and a block is cut when open air in that smooth ground is within a depth of it that
//! falls from the foot of the face to its rim. Smooth ground and a slowly changing depth leave no stray blocks, a rib
//! thinner than twice the depth is cut from both sides, which leaves an arch, and every decision reads world-space ground
//! and noise, so chunks agree.

use std::sync::Arc;

use super::region_store::HeightSampler;
use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::perlin;
use ochre_base::WATER_LEVEL;

/// How far from the face, in blocks, a cut can reach.
pub(crate) const REACH: i64 = 24;
// The smooth ground is a grid with a node every STEP blocks, blurred over BLUR nodes each way, and the window holds
// what a ray from the chunk column can reach.
const STEP: i64 = 2;
const SAMPLES: i64 = REACH / STEP;
const BLUR: i64 = 2;
const MARGIN: i64 = REACH + BLUR * STEP;
const SIZE: usize = ((16 + 2 * MARGIN) / STEP) as usize;
// A face must stand this tall, over ground at least FROM_SEA over the sea, and the cut start this far over the sea.
const MIN_FACE: f32 = 14.0;
const FROM_SEA: i64 = 40;
const CUT_FROM_SEA: i64 = 10;
// The smooth ground's gradient where a face starts to be cut and where it is cut in full.
const STEEP: (f64, f64) = (0.6, 1.2);
// A cap of fewer blocks than this under the surface is cut through.
const THIN_CAP: f32 = 4.0;
// A cut shallower than this is not made, since the ground's roughness would show through it as holes.
const MIN_CUT: f32 = 7.0;
// How far over the smooth ground a block is before it counts as open air, so the ground's own roughness is not cut.
const AIR: f32 = 2.0;
// The blocks of cap at least, over the cut.
const CAP: f32 = 6.0;
// The most a cliff builds the ground up, whose blocks the cut takes too.
const BUILT_UP: i64 = 18;
// Where the noise starts to make a blob of cut face, and how fast it grows to full depth.
const BLOB: f64 = -0.3;
const BLOB_RAMP: f64 = 0.4;
// Water within this many blocks of a column keeps it from being cut, or air beside the water would let it spill.
const WATER_REACH: i64 = REACH;
const WATER_STEP: i64 = 4;
const DIRECTIONS: [(i64, i64, f32); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, std::f32::consts::SQRT_2),
    (1, -1, std::f32::consts::SQRT_2),
    (-1, 1, std::f32::consts::SQRT_2),
    (-1, -1, std::f32::consts::SQRT_2),
];

/// The ground of a chunk column and what surrounds it, blurred.
pub(crate) struct Smooth {
    ground: Vec<f32>,
}

/// How one column is cut away.
pub(crate) struct Notch {
    smooth: Arc<Smooth>,
    // the column's place in the window
    at: (i64, i64),
    // the deepest the cut goes, at the foot
    depth: f32,
    foot: f32,
    rim: f32,
    lo: i64,
    top: i64,
    // how the depth varies along the ground, and the height over which a cap too thin to stand is cut through too
    rough: f32,
    thin_over: f32,
}

impl Notch {
    /// The lowest and highest block the cut can reach.
    #[cfg(test)]
    pub(crate) fn band(&self) -> (i64, i64) {
        (self.lo, self.top + BUILT_UP)
    }

    /// How much the cliff pass's roughness gives way to the smooth cut here, 0..1.
    pub(crate) fn smoothing(&self) -> f64 {
        smoothstep(MIN_CUT as f64, MIN_CUT as f64 + 6.0, self.depth as f64)
    }

    /// Whether the block at `y` in the column is cut away: open air in the smooth ground within the depth at that height,
    /// which falls from the foot of the face to its rim, so what is above overhangs.
    pub(crate) fn carves(&self, y: i64) -> bool {
        if y < self.lo || y > self.top + BUILT_UP {
            return false;
        }
        let yf = y as f32;
        let up = ((yf - self.foot) / (self.rim - self.foot)).clamp(0.0, 1.0);
        let mut depth = self.depth * self.rough * taper(self.foot, self.rim, up);
        if depth < MIN_CUT {
            // a cap thinner than a few blocks is just a shell on a slope, so it goes with what is under it
            if yf <= self.thin_over {
                return false;
            }
            depth = MIN_CUT;
        }
        let ground = &self.smooth.ground;
        let size = SIZE as i64;
        let (x, z) = self.at;
        // above the smooth ground is open air already
        if ground[(z * size + x) as usize] < yf - AIR {
            return true;
        }
        // the distance to open air along a ray is read between the samples either side of where the ground drops under
        // the block, since whole samples would step the cut's edge in twos and leave pits where it meets the surface
        DIRECTIONS.iter().any(|&(dx, dz, length)| {
            let mut before = ground[(z * size + x) as usize];
            for k in 1..=reach(depth) {
                let after = ground[((z + dz * k) * size + x + dx * k) as usize];
                if after < yf - AIR {
                    let fraction = (before - (yf - AIR)) / (before - after);
                    return ((k - 1) * STEP) as f32 + fraction * STEP as f32 <= depth / length;
                }
                before = after;
            }
            false
        })
    }
}

// Samples along a ray that reach a depth, at most those of the window.
fn reach(depth: f32) -> i64 {
    ((depth / STEP as f32).ceil() as i64 + 1).min(SAMPLES)
}

// How much of the full depth is cut `up` the face, 0..1: all of it up the lower face, then falling to nothing under a cap
// of a few blocks or a tenth of the face.
fn taper(foot: f32, rim: f32, up: f32) -> f32 {
    let cap = 1.0 - (CAP / (rim - foot)).max(0.1);
    smoothstep(cap as f64, (cap - 0.2) as f64, up as f64) as f32
}

// The height under which the depth reaches `MIN_CUT`, so something is cut, found by halving.
fn uncut_over(foot: f32, rim: f32, full: f32) -> f32 {
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..16 {
        let mid = (low + high) / 2.0;
        if full * taper(foot, rim, mid) >= MIN_CUT {
            low = mid;
        } else {
            high = mid;
        }
    }
    foot + (rim - foot) * low
}

// The depth the face is cut at a point: patches of undercut ground, narrowed to blobs, deeper where the seed's cliffs reach far.
fn depth_at(seed: u32, boost: f64, x: i64, z: i64) -> f64 {
    let (fx, fz) = (x as f64, z as f64);
    let patch = smoothstep(-0.3, 0.2, perlin(seed ^ 0x0DC1, fx / 120.0, fz / 120.0));
    // wandered, so a blob's edge is not a circle
    let (wx, wz) = (
        fx + 50.0 * perlin(seed ^ 0x0DC4, fx / 70.0, fz / 70.0),
        fz + 50.0 * perlin(seed ^ 0x0DC5, fx / 70.0, fz / 70.0),
    );
    let blob = smoothstep(
        BLOB,
        BLOB + BLOB_RAMP,
        perlin(seed ^ 0x0DC2, wx / 160.0, wz / 160.0),
    );
    (10.0 + 14.0 * boost) * patch * blob
}

// The ground blurred by a box of `BLUR` blocks each way, edges held.
fn blur(ground: &[f32]) -> Vec<f32> {
    let size = SIZE as i64;
    let pass = |from: &[f32], across: bool| {
        let mut out = vec![0.0; from.len()];
        for a in 0..size {
            for b in 0..size {
                let mut sum = 0.0;
                for d in -BLUR..=BLUR {
                    let c = (b + d).clamp(0, size - 1);
                    sum += if across {
                        from[(a * size + c) as usize]
                    } else {
                        from[(c * size + a) as usize]
                    };
                }
                let at = if across { a * size + b } else { b * size + a };
                out[at as usize] = sum / (2 * BLUR + 1) as f32;
            }
        }
        out
    };
    pass(&pass(ground, true), false)
}

/// How each column of the chunk column at (`kx`, `kz`) is undercut, if it is: only over ground a seed's cliff `boosts` reach,
/// which is mountain and tableland, that stands as a face and is not by water. `tops` are the columns' surfaces.
pub(crate) fn notches(
    heights: &mut HeightSampler,
    seed: u32,
    (kx, kz): (i64, i64),
    tops: &[i64],
    boosts: &[f32],
) -> Vec<Option<Box<Notch>>> {
    let none = || (0..tops.len()).map(|_| None).collect();
    if boosts.iter().all(|&b| b <= 0.0) {
        return none();
    }
    let (ox, oz) = (kx * 16 - MARGIN, kz * 16 - MARGIN);
    let size = SIZE as i64;
    // most ground is not a face anywhere near, which a coarse look shows
    let (mut low, mut high) = (i64::MAX, i64::MIN);
    for iz in (0..size).step_by(4) {
        for ix in (0..size).step_by(4) {
            let g = heights.ground_top(ox + ix * STEP, oz + iz * STEP);
            low = low.min(g);
            high = high.max(g);
        }
    }
    if ((high - low) as f32) < MIN_FACE || high - WATER_LEVEL < FROM_SEA {
        return none();
    }
    let mut raw = Vec::with_capacity(SIZE * SIZE);
    let (mut low, mut high) = (i64::MAX, i64::MIN);
    for iz in 0..size {
        for ix in 0..size {
            let g = heights.ground_top(ox + ix * STEP, oz + iz * STEP);
            low = low.min(g);
            high = high.max(g);
            raw.push(g as f32);
        }
    }
    if ((high - low) as f32) < MIN_FACE || high - WATER_LEVEL < FROM_SEA {
        return none();
    }
    let smooth = Arc::new(Smooth { ground: blur(&raw) });
    // the foot and rim of the face round the corners and middle of the chunk column, read between for each column
    let corner = |lx: i64, lz: i64| {
        let (mut foot, mut rim) = (f32::MAX, f32::MIN);
        let (cx, cz) = ((MARGIN + lx) / STEP, (MARGIN + lz) / STEP);
        for dz in (-SAMPLES..=SAMPLES).step_by(2) {
            for dx in (-SAMPLES..=SAMPLES).step_by(2) {
                let g = smooth.ground[((cz + dz) * size + cx + dx) as usize];
                foot = foot.min(g);
                rim = rim.max(g);
            }
        }
        (foot, rim)
    };
    let nodes = [0, 8, 16].map(|lz| [0, 8, 16].map(|lx| corner(lx, lz)));
    // water near a column, from a coarse look round the window
    let wet_size = (size * STEP / WATER_STEP + 1) as usize;
    let mut wet = vec![false; wet_size * wet_size];
    for nz in 0..wet_size {
        for nx in 0..wet_size {
            let (x, z) = (ox + nx as i64 * WATER_STEP, oz + nz as i64 * WATER_STEP);
            wet[nz * wet_size + nx] = heights.water_top(x, z) > WATER_LEVEL;
        }
    }
    let near = (WATER_REACH / WATER_STEP) as usize;
    let wet_near = |lx: i64, lz: i64| {
        let (cx, cz) = (
            ((MARGIN + lx) / WATER_STEP) as usize,
            ((MARGIN + lz) / WATER_STEP) as usize,
        );
        (cz.saturating_sub(near)..=(cz + near).min(wet_size - 1)).any(|z| {
            (cx.saturating_sub(near)..=(cx + near).min(wet_size - 1)).any(|x| wet[z * wet_size + x])
        })
    };
    let mut out: Vec<Option<Box<Notch>>> = none();
    for lx in 0..16i64 {
        for lz in 0..16i64 {
            let index = (lx * 16 + lz) as usize;
            let (top, boost) = (tops[index], boosts[index]);
            if boost <= 0.0 || top - WATER_LEVEL < CUT_FROM_SEA {
                continue;
            }
            let (world_x, world_z) = (kx * 16 + lx, kz * 16 + lz);
            let mut depth = depth_at(seed, boost as f64, world_x, world_z) as f32;
            if depth < MIN_CUT || wet_near(lx, lz) {
                continue;
            }
            // only steep ground is a cliff to overhang, the rest would just recede
            let (nx, nz) = ((MARGIN + lx) / STEP, (MARGIN + lz) / STEP);
            let at = |dx: i64, dz: i64| smooth.ground[((nz + dz) * size + nx + dx) as usize];
            let (gx, gz) = (
                (at(2, 0) - at(-2, 0)) / (4 * STEP) as f32,
                (at(0, 2) - at(0, -2)) / (4 * STEP) as f32,
            );
            let steep = (gx * gx + gz * gz).sqrt();
            depth *= smoothstep(STEEP.0, STEEP.1, steep as f64) as f32;
            if depth < MIN_CUT {
                continue;
            }
            let (fx, fz) = (lx as f32 / 8.0, lz as f32 / 8.0);
            let (i, j) = (fx.min(1.999) as usize, fz.min(1.999) as usize);
            let (tx, tz) = (fx - i as f32, fz - j as f32);
            let read = |pick: fn((f32, f32)) -> f32| {
                let at = |a: usize, b: usize| pick(nodes[b][a]);
                let low = at(i, j) * (1.0 - tx) + at(i + 1, j) * tx;
                let high = at(i, j + 1) * (1.0 - tx) + at(i + 1, j + 1) * tx;
                low * (1.0 - tz) + high * tz
            };
            let (foot, rim) = (read(|n| n.0), read(|n| n.1));
            if rim - foot < MIN_FACE || (rim as i64) - WATER_LEVEL < FROM_SEA {
                continue;
            }
            // varying with height too would cut pockets that open through the top, so only along the ground
            let rough = 0.8
                + 0.2 * perlin(seed ^ 0x0DC3, world_x as f64 / 30.0, world_z as f64 / 30.0) as f32;
            let over = uncut_over(foot, rim, depth * rough);
            out[index] = Some(Box::new(Notch {
                smooth: smooth.clone(),
                at: ((MARGIN + lx) / STEP, (MARGIN + lz) / STEP),
                depth,
                foot,
                rim,
                lo: (foot as i64).max(WATER_LEVEL + CUT_FROM_SEA),
                top,
                rough,
                thin_over: if (top as f32) - over < THIN_CAP {
                    over
                } else {
                    f32::MAX
                },
            }));
        }
    }
    out
}
