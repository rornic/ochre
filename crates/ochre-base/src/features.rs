//! Relief features: small landforms a biome lays over its ground, each a pure function of the seed and
//! the position that returns blocks to add to the height, at most `MAX_OFFSET` either way.

use crate::math::smoothstep_f64 as smoothstep;
use crate::noise::{fbm, hash, perlin};
use std::f64::consts::FRAC_PI_2;

/// The most any feature raises or cuts the ground, in blocks.
pub const MAX_OFFSET: f64 = 18.0;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ReliefKind {
    Crevasses,
    Seracs,
    Pingos,
    PatternedGround,
    TundraHills,
    Drumlins,
    Yardangs,
    Kopjes,
    Gullies,
    Hummocks,
}

/// What a feature reads about the ground it lies on.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct ReliefCtx {
    /// How steep the smooth ground is, rise over run.
    pub gradient: f64,
    /// A unit vector down the slope, or zero on flat ground.
    pub downhill: (f64, f64),
    /// The direction the config's temperature gradient runs, in radians.
    pub ridge_angle: f64,
    /// A unit vector along the wind.
    pub wind: (f64, f64),
}

/// Blocks `kind` adds to the ground at (`x`, `z`), for a sample `cell` blocks wide: nothing once cells are 16 wide.
pub fn offset(kind: ReliefKind, seed: u32, x: f64, z: f64, cell: f64, ctx: &ReliefCtx) -> f64 {
    let detail = 1.0 - smoothstep(4.0, 16.0, cell);
    if detail <= 0.0 {
        return 0.0;
    }
    let raw = match kind {
        ReliefKind::Crevasses => crevasses(seed, x, z, ctx),
        ReliefKind::Seracs => seracs(seed, x, z, ctx),
        ReliefKind::Pingos => pingos(seed, x, z, ctx),
        ReliefKind::PatternedGround => patterned_ground(seed, x, z, ctx),
        ReliefKind::TundraHills => tundra_hills(seed, x, z, ctx),
        ReliefKind::Drumlins => drumlins(seed, x, z, ctx),
        ReliefKind::Yardangs => yardangs(seed, x, z, ctx),
        ReliefKind::Kopjes => kopjes(seed, x, z, ctx),
        ReliefKind::Gullies => gullies(seed, x, z, ctx),
        ReliefKind::Hummocks => hummocks(seed, x, z, ctx),
    };
    (raw * detail).clamp(-MAX_OFFSET, MAX_OFFSET)
}

// Occasional broad, asymmetric glacial hills rising above the open tundra, in provinces: some hold none and the rest are thick or thin, so
// they cluster and leave wide gaps instead of standing in a lattice.
fn tundra_hills(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let gentle = 1.0 - smoothstep(0.3, 0.6, ctx.gradient);
    if gentle <= 0.0 {
        return 0.0;
    }
    const CELL: f64 = 420.0;
    const PROVINCE: f64 = 1600.0;
    const CHANCE: f64 = 0.30;
    // the most a province scales the chance, for the cheap reject before the hill's place is worked out
    const MOST: f64 = 1.8;
    let (gx, gz) = ((x / CELL).floor() as i64, (z / CELL).floor() as i64);
    let mut best: f64 = 0.0;
    for cz in gz - 1..=gz + 1 {
        for cx in gx - 1..=gx + 1 {
            let h = cell_hash(seed ^ 0x741D_A111, cx, cz);
            if unit_of(h, 1) >= CHANCE * MOST { continue; }
            let px = (cx as f64 + 0.1 + 0.8 * unit_of(h, 2)) * CELL;
            let pz = (cz as f64 + 0.1 + 0.8 * unit_of(h, 3)) * CELL;
            let province = cell_hash(seed ^ 0x9407_C0DE, (px / PROVINCE).floor() as i64, (pz / PROVINCE).floor() as i64);
            let scale = if unit_of(province, 1) < 0.4 { 0.0 } else { 0.6 + 1.2 * unit_of(province, 2) };
            if unit_of(h, 1) >= CHANCE * scale { continue; }
            let angle = unit_of(h, 4) * std::f64::consts::TAU;
            let (sin, cos) = (libm::sin(angle), libm::cos(angle));
            let (dx, dz) = (x - px, z - pz);
            let (u, v) = (dx * cos + dz * sin, -dx * sin + dz * cos);
            let (a, b) = (35.0 + 30.0 * unit_of(h, 5), 22.0 + 18.0 * unit_of(h, 6));
            let d = (u / a).powi(2) + (v / b).powi(2);
            if d < 1.0 {
                let t = 1.0 - d;
                best = best.max((10.0 + 8.0 * unit_of(h, 7)) * t * t);
            }
        }
    }
    best * gentle
}

fn cell_hash(seed: u32, cx: i64, cz: i64) -> u32 {
    hash(seed, cx as i32, cz as i32)
}

fn unit_of(h: u32, salt: u32) -> f64 {
    hash(h, salt as i32, 0) as f64 / 4_294_967_296.0
}

// Slots cut across a gently convex slope in belts, 2 to 5 blocks wide, 8 to 18 deep, 40 to 150 long, with a bridge
// of uncut ground every 15 to 30 blocks along each.
fn crevasses(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let g = ctx.gradient;
    let gentle = smoothstep(0.1, 0.15, g) * (1.0 - smoothstep(0.6, 0.7, g));
    if gentle <= 0.0 || ctx.downhill == (0.0, 0.0) {
        return 0.0;
    }
    let belt = smoothstep(0.0, 0.35, fbm(seed ^ 0xC4E1, x, z, 700.0, 1));
    if belt <= 0.0 {
        return 0.0;
    }
    // a fixed frame: one turning with the slope shifts `across` by the distance from the origin, a block at a time
    let (dz, dx) = (
        libm::sin(ctx.ridge_angle + FRAC_PI_2),
        libm::cos(ctx.ridge_angle + FRAC_PI_2),
    );
    let period = 30.0;
    let down = x * dx + z * dz + 8.0 * perlin(seed ^ 0xC4E2, x / 50.0, z / 50.0);
    let across = -x * dz + z * dx;
    let row = (down / period).floor();
    let h = cell_hash(seed ^ 0xC4E3, row as i64, 0);
    let centre = 0.25 + 0.5 * unit_of(h, 1);
    let half = 1.0 + 1.5 * unit_of(h, 2);
    let from = ((down / period - row) - centre).abs() * period;
    if from >= half {
        return 0.0;
    }
    let reach = smoothstep(
        0.1,
        0.3,
        perlin(seed ^ 0xC4E5, across / 70.0 + row * 17.3, row * 3.1),
    );
    let gap = 15.0 + 15.0 * unit_of(h, 3);
    let bridge = if (across / gap + unit_of(h, 4)).fract().abs() < 0.18 {
        1.0
    } else {
        0.0
    };
    let depth = 8.0 + 10.0 * unit_of(h, 5);
    let t = from / half;
    -depth * (1.0 - t * t) * reach * belt * gentle * (1.0 - bridge)
}

// Jagged ice pillars on steep ground, built up: 6 to 18 high, 3 to 8 across.
fn seracs(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let steep = smoothstep(1.2, 1.5, ctx.gradient);
    if steep <= 0.0 {
        return 0.0;
    }
    const SIZE: f64 = 10.0;
    let (gx, gz) = ((x / SIZE).floor() as i64, (z / SIZE).floor() as i64);
    let mut best: f64 = 0.0;
    for cz in gz - 1..=gz + 1 {
        for cx in gx - 1..=gx + 1 {
            let h = cell_hash(seed ^ 0x5E4A, cx, cz);
            if unit_of(h, 1) >= 0.35 {
                continue;
            }
            let (px, pz) = (
                (cx as f64 + 0.2 + 0.6 * unit_of(h, 2)) * SIZE,
                (cz as f64 + 0.2 + 0.6 * unit_of(h, 3)) * SIZE,
            );
            let radius = 1.5 + 2.5 * unit_of(h, 4);
            let d = ((x - px).powi(2) + (z - pz).powi(2)).sqrt();
            if d < radius {
                let height = 6.0 + 12.0 * unit_of(h, 5);
                best = best.max(height * (1.0 - d / radius));
            }
        }
    }
    best * steep
}

// Round ice-cored mounds on flat ground, 5 to 14 high and 7 to 18 across a radius, in drifts with open tundra between. Two lattices of
// different cell sizes, each turned to its own angle, are laid over each other, and a smooth field thickens and thins them, so the mounds
// fall in no grid or bands.
fn pingos(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let flat = 1.0 - smoothstep(0.08, 0.15, ctx.gradient);
    if flat <= 0.0 {
        return 0.0;
    }
    // each lattice's cell size, the cosine and sine of its angle, its chance of a mound in a cell, and its hash salt
    const LATTICES: [(f64, f64, f64, f64, u32); 2] = [
        (190.0, 0.900447, 0.434966, 0.65, 0x91A6),
        (150.0, 0.813878, -0.581035, 0.65, 0x4C7D),
    ];
    // the drifts are this many blocks across, and the field scales the chance by up to this much
    const DRIFT: f64 = 800.0;
    const MOST: f64 = 1.8;
    let mut best: f64 = 0.0;
    for (size, cos, sin, chance, salt) in LATTICES {
        let (u, v) = (x * cos + z * sin, -x * sin + z * cos);
        let (gx, gz) = ((u / size).floor() as i64, (v / size).floor() as i64);
        for cz in gz - 1..=gz + 1 {
            for cx in gx - 1..=gx + 1 {
                let h = cell_hash(seed ^ salt, cx, cz);
                // the cheap reject, before the mound's place is worked out
                if unit_of(h, 1) >= chance * MOST {
                    continue;
                }
                let (pu, pv) = (
                    (cx as f64 + 0.1 + 0.8 * unit_of(h, 2)) * size,
                    (cz as f64 + 0.1 + 0.8 * unit_of(h, 3)) * size,
                );
                // the field where the mound is in the world, so both lattices thicken in the same places
                let (wx, wz) = (pu * cos - pv * sin, pu * sin + pv * cos);
                let thick = smoothstep(-0.25, 0.4, perlin(seed ^ 0x51F0, wx / DRIFT, wz / DRIFT)) * MOST;
                if unit_of(h, 1) >= chance * thick {
                    continue;
                }
                let size_roll = unit_of(h, 4);
                let radius = 7.0 + 11.0 * size_roll * size_roll;
                let d = ((u - pu).powi(2) + (v - pv).powi(2)).sqrt();
                if d < radius {
                    let t = 1.0 - d / radius;
                    best = best.max((5.0 + 9.0 * unit_of(h, 5)) * t * t * (3.0 - 2.0 * t));
                }
            }
        }
    }
    best * flat
}

// A net of low ridges, 12 to 20 blocks across, one block high: frost cracks.
fn patterned_ground(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let flat = 1.0 - smoothstep(0.1, 0.2, ctx.gradient);
    if flat <= 0.0 {
        return 0.0;
    }
    const SIZE: f64 = 16.0;
    let (gx, gz) = ((x / SIZE).floor() as i64, (z / SIZE).floor() as i64);
    let (mut nearest, mut second) = (f64::MAX, f64::MAX);
    for cz in gz - 1..=gz + 1 {
        for cx in gx - 1..=gx + 1 {
            let h = cell_hash(seed ^ 0x9A77, cx, cz);
            let (px, pz) = (
                (cx as f64 + unit_of(h, 1)) * SIZE,
                (cz as f64 + unit_of(h, 2)) * SIZE,
            );
            let d = ((x - px).powi(2) + (z - pz).powi(2)).sqrt();
            if d < nearest {
                (nearest, second) = (d, nearest);
            } else if d < second {
                second = d;
            }
        }
    }
    (1.0 - smoothstep(0.0, 2.0, second - nearest)) * flat
}

// Streamlined swells in parallel along the config's gradient angle: 80 to 200 long, 40 to 70 wide, 4 to 8 high.
fn drumlins(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let gentle = 1.0 - smoothstep(0.25, 0.4, ctx.gradient);
    if gentle <= 0.0 {
        return 0.0;
    }
    let (sin, cos) = (libm::sin(ctx.ridge_angle), libm::cos(ctx.ridge_angle));
    let (u, v) = (x * cos + z * sin, -x * sin + z * cos);
    const LENGTH: f64 = 240.0;
    const WIDTH: f64 = 90.0;
    let (gu, gv) = ((u / LENGTH).floor() as i64, (v / WIDTH).floor() as i64);
    let mut best: f64 = 0.0;
    for cv in gv - 1..=gv + 1 {
        for cu in gu - 1..=gu + 1 {
            let h = cell_hash(seed ^ 0xD27E, cu, cv);
            if unit_of(h, 1) >= 0.5 {
                continue;
            }
            let (pu, pv) = (
                (cu as f64 + 0.3 + 0.4 * unit_of(h, 2)) * LENGTH,
                (cv as f64 + 0.3 + 0.4 * unit_of(h, 3)) * WIDTH,
            );
            let (a, b) = (40.0 + 60.0 * unit_of(h, 4), 20.0 + 15.0 * unit_of(h, 5));
            let e = ((u - pu) / a).powi(2) + ((v - pv) / b).powi(2);
            if e < 1.0 {
                best = best.max((4.0 + 4.0 * unit_of(h, 6)) * (1.0 - e) * (1.0 - e));
            }
        }
    }
    best * gentle
}

// Parallel wind-carved ridges and troughs along the wind, in patches: ridges up to 5 high, troughs up to 2.5 deep,
// each 60 to 200 long, over ground gentler than 0.35.
fn yardangs(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let gentle = 1.0 - smoothstep(0.2, 0.35, ctx.gradient);
    let patch = smoothstep(-0.05, 0.25, fbm(seed ^ 0x7A9D, x, z, 900.0, 1));
    if gentle <= 0.0 || patch <= 0.0 {
        return 0.0;
    }
    let (wx, wz) = ctx.wind;
    let (along, across) = (x * wx + z * wz, -x * wz + z * wx);
    let n = perlin(seed ^ 0x7A9E, across / 16.0, along / 130.0);
    (5.0 * smoothstep(0.15, 0.6, n) - 2.5 * smoothstep(0.15, 0.6, -n)) * patch * gentle
}

// Rounded rock heaps on the plain in clusters of 2 to 6: 6 to 14 high, 10 to 25 across.
fn kopjes(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let gentle = 1.0 - smoothstep(0.2, 0.4, ctx.gradient);
    if gentle <= 0.0 {
        return 0.0;
    }
    const SIZE: f64 = 400.0;
    let (gx, gz) = ((x / SIZE).floor() as i64, (z / SIZE).floor() as i64);
    let mut best: f64 = 0.0;
    for cz in gz - 1..=gz + 1 {
        for cx in gx - 1..=gx + 1 {
            let h = cell_hash(seed ^ 0x40B1, cx, cz);
            if unit_of(h, 1) >= 0.35 {
                continue;
            }
            let (px, pz) = (
                (cx as f64 + 0.25 + 0.5 * unit_of(h, 2)) * SIZE,
                (cz as f64 + 0.25 + 0.5 * unit_of(h, 3)) * SIZE,
            );
            let heaps = 2 + (unit_of(h, 4) * 5.0) as u32;
            for k in 0..heaps {
                let m = hash(h, k as i32, 7);
                let (hx, hz) = (
                    px + (unit_of(m, 1) - 0.5) * 80.0,
                    pz + (unit_of(m, 2) - 0.5) * 80.0,
                );
                let radius = 5.0 + 7.5 * unit_of(m, 3);
                let d2 = ((x - hx).powi(2) + (z - hz).powi(2)) / (radius * radius);
                if d2 < 1.0 {
                    best = best.max((6.0 + 8.0 * unit_of(m, 4)) * (1.0 - d2).sqrt());
                }
            }
        }
    }
    best * gentle
}

// Wide swales across slopes steeper than 0.2: 2 to 6 deep and about 10 across, wandering.
fn gullies(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let sloped = smoothstep(0.2, 0.3, ctx.gradient);
    if sloped <= 0.0 || ctx.downhill == (0.0, 0.0) {
        return 0.0;
    }
    // a fixed frame, as for crevasses
    let (dz, dx) = (
        libm::sin(ctx.ridge_angle + FRAC_PI_2),
        libm::cos(ctx.ridge_angle + FRAC_PI_2),
    );
    let (down, across) = (x * dx + z * dz, -x * dz + z * dx);
    let wander = 40.0 * perlin(seed ^ 0x6011, down / 150.0, across / 150.0);
    let n = perlin(seed ^ 0x6012, (across + wander) / 50.0, down / 200.0);
    let depth = 2.0 + 4.0 * smoothstep(0.2, 0.5, ctx.gradient);
    // a wide swale, since a narrow one is a row of one-block slits
    -depth * (1.0 - smoothstep(0.0, 0.3, n.abs())) * sloped
}

// Raised tussocks on flat wet ground, 1 to 3 high, 4 to 8 across.
fn hummocks(seed: u32, x: f64, z: f64, ctx: &ReliefCtx) -> f64 {
    let flat = 1.0 - smoothstep(0.1, 0.2, ctx.gradient);
    if flat <= 0.0 {
        return 0.0;
    }
    const SIZE: f64 = 8.0;
    let (gx, gz) = ((x / SIZE).floor() as i64, (z / SIZE).floor() as i64);
    let mut best: f64 = 0.0;
    for cz in gz - 1..=gz + 1 {
        for cx in gx - 1..=gx + 1 {
            let h = cell_hash(seed ^ 0x40A3, cx, cz);
            if unit_of(h, 1) >= 0.6 {
                continue;
            }
            let (px, pz) = (
                (cx as f64 + 0.2 + 0.6 * unit_of(h, 2)) * SIZE,
                (cz as f64 + 0.2 + 0.6 * unit_of(h, 3)) * SIZE,
            );
            let radius = 2.0 + 2.0 * unit_of(h, 4);
            let d = ((x - px).powi(2) + (z - pz).powi(2)).sqrt();
            if d < radius {
                let t = 1.0 - d / radius;
                best = best.max((1.0 + 2.0 * unit_of(h, 5)) * t * t * (3.0 - 2.0 * t));
            }
        }
    }
    best * flat
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::noise::unit;

    const KINDS: [ReliefKind; 10] = [
        ReliefKind::Crevasses,
        ReliefKind::Seracs,
        ReliefKind::Pingos,
        ReliefKind::PatternedGround,
        ReliefKind::TundraHills,
        ReliefKind::Drumlins,
        ReliefKind::Yardangs,
        ReliefKind::Kopjes,
        ReliefKind::Gullies,
        ReliefKind::Hummocks,
    ];

    fn ctx(gradient: f64) -> ReliefCtx {
        ReliefCtx {
            gradient,
            downhill: (0.6, 0.8),
            ridge_angle: 0.7,
            wind: (0.8, 0.6),
        }
    }

    // a gradient each kind works on
    fn fitting(kind: ReliefKind) -> f64 {
        match kind {
            ReliefKind::Crevasses => 0.35,
            ReliefKind::Seracs => 1.8,
            ReliefKind::Gullies => 0.4,
            _ => 0.02,
        }
    }

    fn grid() -> impl Iterator<Item = (f64, f64)> {
        (0..100)
            .flat_map(|i| (0..100).map(move |j| (i as f64 * 7.3 - 350.0, j as f64 * 7.9 - 400.0)))
    }

    #[test]
    fn features_stay_within_the_most_and_are_pure_in_the_seed() {
        for kind in KINDS {
            let ctx = ctx(fitting(kind));
            let mut moved = 0;
            for (x, z) in grid() {
                let a = offset(kind, 5, x, z, 1.0, &ctx);
                assert!(a.abs() <= MAX_OFFSET, "{kind:?} {a} at {x},{z}");
                assert_eq!(a, offset(kind, 5, x, z, 1.0, &ctx));
                moved += (a != 0.0) as usize;
            }
            assert!(moved > 5, "{kind:?} moves only {moved} of 10,000 points");
        }
    }

    #[test]
    fn tundra_hills_are_raised_rare_and_come_in_provinces_with_gaps_between() {
        let ctx = ctx(0.05);
        // a 5 km square, since the hills are hundreds of blocks apart
        let (mut open, mut high, mut samples) = (0usize, 0usize, 0usize);
        let mut empty_blocks = std::collections::HashSet::new();
        let mut hill_blocks = std::collections::HashSet::new();
        for i in 0..200 {
            for j in 0..200 {
                let (x, z) = (i as f64 * 25.0 - 2500.0, j as f64 * 25.0 - 2500.0);
                let height = offset(ReliefKind::TundraHills, 5, x, z, 1.0, &ctx);
                assert!(height >= 0.0);
                samples += 1;
                open += usize::from(height < 0.1);
                high += usize::from(height > 5.0);
                // 800-block squares, with or without any hill in them
                let block = ((x / 800.0).floor() as i64, (z / 800.0).floor() as i64);
                if height > 0.1 {
                    hill_blocks.insert(block);
                } else {
                    empty_blocks.insert(block);
                }
            }
        }
        assert!(open * 100 > samples * 95, "{open} of {samples} open");
        assert!(high > 20, "{high} raised samples");
        assert!(empty_blocks.iter().any(|b| !hill_blocks.contains(b)), "every 800 block square holds a hill");
    }

    #[test]
    fn features_vanish_in_wide_cells_and_on_ground_they_do_not_suit() {
        for kind in KINDS {
            let suited = ctx(fitting(kind));
            let unsuited = ctx(match kind {
                ReliefKind::Crevasses => 0.05,
                ReliefKind::Seracs => 0.5,
                ReliefKind::Gullies => 0.05,
                _ => 1.5,
            });
            for (x, z) in grid() {
                assert_eq!(
                    0.0,
                    offset(kind, 5, x, z, 16.0, &suited),
                    "{kind:?} at cell 16"
                );
                assert_eq!(
                    0.0,
                    offset(kind, 5, x, z, 64.0, &suited),
                    "{kind:?} at cell 64"
                );
                assert_eq!(
                    0.0,
                    offset(kind, 5, x, z, 1.0, &unsuited),
                    "{kind:?} on bad ground"
                );
            }
        }
    }

    #[test]
    fn a_crevasse_is_never_wider_than_five_blocks_and_has_bridges() {
        let ctx = ctx(0.35);
        let (dz, dx) = (
            libm::sin(ctx.ridge_angle + FRAC_PI_2),
            libm::cos(ctx.ridge_angle + FRAC_PI_2),
        );
        let (mut widest, mut run, mut cuts) = (0, 0, 0);
        // walk down the slope across the slots
        for k in 0..60_000 {
            let s = k as f64 * 0.25;
            let (x, z) = (-4000.0 + s * dx + 300.0, 900.0 + s * dz);
            if offset(ReliefKind::Crevasses, 9, x, z, 1.0, &ctx) < -0.5 {
                run += 1;
                cuts += 1;
                widest = widest.max(run);
            } else {
                run = 0;
            }
        }
        // a quarter block a step
        assert!(cuts > 100, "no crevasses along the line");
        assert!(widest as f64 * 0.25 <= 5.0 + 0.5, "{widest} steps wide");
        // along a slot's length there are gaps in the cut
        let mut slot = None;
        for (x, z) in grid() {
            if offset(ReliefKind::Crevasses, 9, x, z, 1.0, &ctx) < -3.0 {
                slot = Some((x, z));
                break;
            }
        }
        let (x, z) = slot.expect("a slot");
        let along = (-dz, dx);
        let cut = |t: f64| {
            offset(
                ReliefKind::Crevasses,
                9,
                x + along.0 * t,
                z + along.1 * t,
                1.0,
                &ctx,
            ) < -0.5
        };
        assert!((-80..=80).any(|t| !cut(t as f64)), "a slot with no bridge");
    }

    #[test]
    fn unit_hashes_are_in_range() {
        for k in 0..1000 {
            let v = unit(3, k, -k);
            assert!((0.0..1.0).contains(&v));
        }
    }
}
