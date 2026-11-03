//! Shapes laid into the map's ground that erosion does not make: fjords cut into a mountainous coast and
//! volcanic cones raised on volcanic provinces. They are laid after erosion and before the mountain passes,
//! so the thermal slumping that follows cannot soften their walls.

use super::climate::blur_box;
use super::WorldMap;
use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{hash, perlin, unit};

// Cells of the map's edge no shape is laid within.
const EDGE: usize = 60;
// Candidates drawn for each site, the best being used.
const CANDIDATES: u32 = 32;

// A fjord starts on a coast cell with sea within COAST_REACH cells, where the uplift averaged over a window
// round it is at least FJORD_UPLIFT blocks.
const COAST_REACH: i64 = 2;
const FJORD_UPLIFT: f32 = 300.0;
const UPLIFT_WINDOW: usize = 5;
// It runs inland up the smoothed ground for this many cells (1.3 to 3.8 km at 32 m a cell).
const FJORD_LENGTH: (u32, u32) = (40, 120);
// A fjord is cut only where the ground rises this many cells, and mouths are this many cells apart.
const MIN_LENGTH: usize = 20;
const APART: f64 = 60.0;
// Its floor lies this far under the sea, 8 cells wide, with walls rising this many blocks a cell.
const FJORD_DEPTH: (f64, f64) = (40.0, 120.0);
const FLOOR_HALF_WIDTH: (f64, f64) = (2.5, 8.0);
// The centre line wanders this many cells either side over this many cells along it, and the walls are pushed in and out
// by up to RAGGED.0 cells over RAGGED.1 cells.
const MEANDER: (f64, f64) = (6.0, 14.0);
const RAGGED: (f64, f64) = (2.0, 12.0);
const WALL_PER_CELL: f64 = 40.0;
const ASCENT_SMOOTHING: usize = 2;

// A cone is this many cells across the radius (a crater of at most 300 cells, the smallest lake hydrology
// keeps) and this many blocks tall, with a crater a quarter as wide and a quarter of the height deep under its rim, its flanks and rim pushed in and out by up to CONE_RAGGED of its radius.
const CONE_RADIUS: (u32, u32) = (16, 36);
const CONE_HEIGHT: (f64, f64) = (150.0, 400.0);
const CRATER_WIDTH: f64 = 0.25;
const CRATER_DEPTH: f64 = 0.25;
const CONE_RAGGED: f64 = 0.18;
// A cone stands on volcanic ground over this height, and cones are packed this many cells apart for counting.
const CONE_FROM: f32 = -200.0;
const CONE_APART: f64 = 90.0;

fn in_range((lo, hi): (u32, u32), u: f64) -> u32 {
    lo + ((hi - lo + 1) as f64 * u).floor().min((hi - lo) as f64) as u32
}

// Where along its gradient direction a position lies, -1 (the cold side) to 1, from the config alone.
fn warm_side(map: &WorldMap, x: f64, z: f64) -> f64 {
    let c = map.config.climate;
    if c.gradient == 0.0 {
        return 0.0;
    }
    let half = map.config.half_size() as f64;
    let (sin, cos) = (
        libm::sin(c.gradient_angle as f64),
        libm::cos(c.gradient_angle as f64),
    );
    ((x * cos + z * sin) / half).clamp(-1.0, 1.0)
}

/// Cuts fjords into the ground of mountainous coasts: `share` of the mouths its coast has room for, so a world with
/// a long mountainous coast has more of them than one with a short coast.
pub(super) fn fjords(map: &mut WorldMap, share: f64) -> Vec<Vec<(usize, usize)>> {
    let mut lines = Vec::new();
    if share <= 0.0 {
        return lines;
    }
    let dim = map.dim();
    let floor = (map.config.bounds.min_y + 16) as f32;
    let uplift = blur_box(&map.uplift, dim, UPLIFT_WINDOW);
    let near_sea = |cx: usize, cz: usize| {
        (-COAST_REACH..=COAST_REACH).any(|dz| {
            (-COAST_REACH..=COAST_REACH).any(|dx| {
                let (x, z) = ((cx as i64 + dx) as usize, (cz as i64 + dz) as usize);
                map.height[z * dim + x] < 0.0
            })
        })
    };
    let coast: Vec<usize> = (EDGE..dim - EDGE)
        .flat_map(|cz| (EDGE..dim - EDGE).map(move |cx| (cx, cz)))
        .filter(|&(cx, cz)| {
            let i = cz * dim + cx;
            map.height[i] >= 0.0 && uplift[i] >= FJORD_UPLIFT && near_sea(cx, cz)
        })
        .map(|(cx, cz)| cz * dim + cx)
        .collect();
    // the mouths the coast has room for, packed as far apart as `APART` in a seeded order
    let mut order = coast.clone();
    order.sort_by_key(|&i| hash(map.seed ^ 0xF107_D003, i as i32, 0));
    let mut slots: Vec<(f64, f64)> = Vec::new();
    for i in order {
        let (x, z) = ((i % dim) as f64, (i / dim) as f64);
        if slots
            .iter()
            .all(|&(sx, sz)| ((x - sx).powi(2) + (z - sz).powi(2)).sqrt() >= APART)
        {
            slots.push((x, z));
        }
    }
    let count = (slots.len() as f64 * share).round() as usize;
    if count == 0 {
        return lines;
    }
    let ascent = blur_box(&map.height, dim, ASCENT_SMOOTHING);
    // up the smoothed ground from `site`, one cell at a time, for `length` cells or until it stops rising
    let climb = |site: usize, length: usize| {
        let mut line = vec![(site % dim, site / dim)];
        let mut came = site;
        for _ in 0..length {
            let (cx, cz) = *line.last().unwrap();
            let here = ascent[cz * dim + cx];
            let next = (-1i64..=1)
                .flat_map(|dz| (-1i64..=1).map(move |dx| (dx, dz)))
                .filter(|&d| d != (0, 0))
                .map(|(dx, dz)| ((cx as i64 + dx) as usize, (cz as i64 + dz) as usize))
                .filter(|&(x, z)| x >= EDGE && z >= EDGE && x < dim - EDGE && z < dim - EDGE)
                .filter(|&(x, z)| z * dim + x != came)
                .max_by(|a, b| ascent[a.1 * dim + a.0].total_cmp(&ascent[b.1 * dim + b.0]));
            match next {
                Some((x, z)) if ascent[z * dim + x] > here => {
                    came = cz * dim + cx;
                    line.push((x, z));
                }
                _ => break,
            }
        }
        line
    };
    let mut mouths: Vec<(usize, usize)> = Vec::new();
    for n in 0..count as i32 {
        let length = in_range(FJORD_LENGTH, unit(map.seed ^ 0xF107_D001, n, 0)) as usize;
        let depth =
            FJORD_DEPTH.0 + (FJORD_DEPTH.1 - FJORD_DEPTH.0) * unit(map.seed ^ 0xF107_D002, n, 0);
        // the candidates best first: the most uplift, the cold side of the world favoured, clear of earlier mouths
        let score = |i: usize| {
            let (x, z) = map.cell_centre(i % dim, i / dim);
            uplift[i] as f64 * (1.0 - 0.5 * warm_side(map, x, z))
        };
        let mut candidates: Vec<usize> = (0..CANDIDATES)
            .map(|k| coast[hash(map.seed ^ 0xF107_D000, n, k as i32) as usize % coast.len()])
            .filter(|&i| {
                mouths.iter().all(|&(x, z)| {
                    let (dx, dz) = ((i % dim) as f64 - x as f64, (i / dim) as f64 - z as f64);
                    (dx * dx + dz * dz).sqrt() >= APART
                })
            })
            .collect();
        candidates.sort_by(|&a, &b| score(b).total_cmp(&score(a)).then(a.cmp(&b)));
        // the first whose ground rises far enough to cut a fjord of some length
        let Some(line) = candidates
            .into_iter()
            .map(|site| climb(site, length))
            .find(|line| line.len() >= MIN_LENGTH)
        else {
            continue;
        };
        mouths.push(line[0]);
        // the centre line wanders from side to side, the floor's width and depth drift along it, and the
        // walls are ragged, so a fjord is not a straight trench
        let centre: Vec<(f64, f64)> = line
            .iter()
            .enumerate()
            .map(|(k, &(x, z))| {
                let (px, pz) = line[k.saturating_sub(2)];
                let (nx, nz) = line[(k + 2).min(line.len() - 1)];
                let (dx, dz) = ((nx as f64 - px as f64), (nz as f64 - pz as f64));
                let len = (dx * dx + dz * dz).sqrt().max(1.0);
                let t = k as f64;
                let side = MEANDER.0 * perlin(map.seed ^ 0xF107_D004, t / MEANDER.1, n as f64)
                    + 0.35
                        * MEANDER.0
                        * perlin(map.seed ^ 0xF107_D005, t / (MEANDER.1 / 3.0), n as f64);
                (x as f64 - dz / len * side, z as f64 + dx / len * side)
            })
            .collect();
        let along = |k: usize| {
            let t = k as f64;
            let drift = |salt: u32, wavelength: f64| {
                0.5 + 0.5 * perlin(map.seed ^ salt, t / wavelength, n as f64)
            };
            (
                FLOOR_HALF_WIDTH.0
                    + (FLOOR_HALF_WIDTH.1 - FLOOR_HALF_WIDTH.0) * drift(0xF107_D006, 10.0),
                depth * (0.8 + 0.4 * drift(0xF107_D007, 18.0)),
            )
        };
        let top = line
            .iter()
            .map(|&(x, z)| map.height[z * dim + x])
            .fold(0.0f32, f32::max) as f64;
        let reach = (FLOOR_HALF_WIDTH.1
            + MEANDER.0
            + RAGGED.0
            + (top + depth * 1.2) / WALL_PER_CELL)
            .ceil() as i64
            + 3;
        let (lo_x, hi_x) = centre
            .iter()
            .fold((f64::MAX, 0.0f64), |(l, h), &(x, _)| (l.min(x), h.max(x)));
        let (lo_z, hi_z) = centre
            .iter()
            .fold((f64::MAX, 0.0f64), |(l, h), &(_, z)| (l.min(z), h.max(z)));
        for cz in (lo_z as i64 - reach).max(0)..=(hi_z as i64 + reach).min(dim as i64 - 1) {
            for cx in (lo_x as i64 - reach).max(0)..=(hi_x as i64 + reach).min(dim as i64 - 1) {
                let (d, k) = centre
                    .iter()
                    .enumerate()
                    .map(|(k, &(x, z))| {
                        (
                            ((x - cx as f64).powi(2) + (z - cz as f64).powi(2)).sqrt(),
                            k,
                        )
                    })
                    .fold(
                        (f64::INFINITY, 0),
                        |best, c| if c.0 < best.0 { c } else { best },
                    );
                let (half, floor_depth) = along(k);
                // ragged walls: the distance is pushed in and out by noise a few cells long
                let rough = |wavelength: f64| {
                    perlin(
                        map.seed ^ 0xF107_D008,
                        cx as f64 / wavelength,
                        cz as f64 / wavelength,
                    )
                };
                let ragged = RAGGED.0
                    * (rough(RAGGED.1)
                        + 0.6 * rough(RAGGED.1 / 2.5)
                        + 0.35 * rough(RAGGED.1 / 6.0));
                let cut = -floor_depth + WALL_PER_CELL * (d + ragged - half).max(0.0);
                let i = cz as usize * dim + cx as usize;
                map.height[i] = (map.height[i] as f64).min(cut).max(floor as f64) as f32;
            }
        }
        lines.push(line);
    }
    lines
}

/// Raises volcanic cones with craters on the map's volcanic provinces: `share` of the sites the provinces have room
/// for, so a world with a lot of volcanic ground has more of them.
pub(super) fn cones(map: &mut WorldMap, share: f64) -> Vec<(usize, usize, f64)> {
    let mut sites: Vec<(usize, usize, f64)> = Vec::new();
    if share <= 0.0 {
        return sites;
    }
    let dim = map.dim();
    let volcanic: Vec<usize> = (EDGE..dim - EDGE)
        .flat_map(|cz| (EDGE..dim - EDGE).map(move |cx| cz * dim + cx))
        .filter(|&i| map.geology[i] == super::geology::VOLCANIC && map.height[i] > CONE_FROM)
        .collect();
    // the sites the volcanic ground has room for, packed CONE_APART apart in a seeded order
    let mut order = volcanic.clone();
    order.sort_by_key(|&i| hash(map.seed ^ 0xC04E_0003, i as i32, 0));
    let mut room: Vec<(f64, f64)> = Vec::new();
    for i in order {
        let (x, z) = ((i % dim) as f64, (i / dim) as f64);
        if room
            .iter()
            .all(|&(sx, sz)| ((x - sx).powi(2) + (z - sz).powi(2)).sqrt() >= CONE_APART)
        {
            room.push((x, z));
        }
    }
    let count = (room.len() as f64 * share).round() as usize;
    for n in 0..count as i32 {
        let radius = in_range(CONE_RADIUS, unit(map.seed ^ 0xC04E_0000, n, 0)) as f64;
        let height =
            CONE_HEIGHT.0 + (CONE_HEIGHT.1 - CONE_HEIGHT.0) * unit(map.seed ^ 0xC04E_0001, n, 0);
        // the highest of the candidates, clear of the cones already raised
        let site = (0..CANDIDATES)
            .map(|k| volcanic[hash(map.seed ^ 0xC04E_0002, n, k as i32) as usize % volcanic.len()])
            .filter(|&i| {
                sites.iter().all(|&(x, z, r)| {
                    let (dx, dz) = ((i % dim) as f64 - x as f64, (i / dim) as f64 - z as f64);
                    (dx * dx + dz * dz).sqrt() > 1.5 * (r + radius)
                })
            })
            .max_by(|&a, &b| map.height[a].total_cmp(&map.height[b]).then(b.cmp(&a)));
        let Some(site) = site else { continue };
        let (sx, sz) = (site % dim, site / dim);
        sites.push((sx, sz, radius));
        let reach = (radius * (1.0 + CONE_RAGGED)).ceil() as i64;
        for cz in (sz as i64 - reach).max(0)..=(sz as i64 + reach).min(dim as i64 - 1) {
            for cx in (sx as i64 - reach).max(0)..=(sx as i64 + reach).min(dim as i64 - 1) {
                let (dx, dz) = ((cx - sx as i64) as f64, (cz - sz as i64) as f64);
                // flanks and rim pushed in and out, so a cone is not a round heap
                let wavelength = (radius * 0.6).max(4.0);
                let ragged = CONE_RAGGED
                    * radius
                    * (perlin(
                        map.seed ^ 0xC04E_0004,
                        cx as f64 / wavelength,
                        cz as f64 / wavelength,
                    ) + 0.5
                        * perlin(
                            map.seed ^ 0xC04E_0004,
                            cx as f64 / (wavelength / 3.0),
                            cz as f64 / (wavelength / 3.0),
                        ));
                let d = (dx * dx + dz * dz).sqrt() + ragged;
                let t = d / radius;
                if !(0.0..1.0).contains(&t) {
                    continue;
                }
                // inside the crater the cone is cut back to a bowl under its rim: the design's cone minus a quarter of its
                // height leaves the middle higher than the rim, which is a bump and not a crater
                let rim = CRATER_WIDTH;
                let flank = |t: f64| height * libm::pow(1.0 - t, 1.5);
                let shape = if t < rim {
                    flank(rim) - CRATER_DEPTH * height * (1.0 - smoothstep(0.5, 1.0, t / rim))
                } else {
                    flank(t)
                };
                let i = cz as usize * dim + cx as usize;
                map.height[i] += shape as f32;
            }
        }
    }
    sites
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    // Sea in the west to column 150, then land climbing east in steep ground, with plenty of uplift all over.
    fn mountainous_coast() -> WorldMap {
        let config = WorldConfig {
            size: 400 * 32,
            map_cell: 32,
            ..Default::default()
        };
        let dim = config.dim();
        let mut map = WorldMap::empty(7, config);
        for cz in 0..dim {
            for cx in 0..dim {
                let i = cz * dim + cx;
                map.height[i] = if cx < 150 {
                    -300.0
                } else {
                    (cx - 150) as f32 * 20.0
                };
                map.uplift[i] = 500.0;
            }
        }
        map
    }

    #[test]
    fn a_fjord_is_cut_from_the_coast_into_the_mountains_and_only_cuts() {
        let before = mountainous_coast();
        let mut map = before.clone();
        let lines = fjords(&mut map, 0.6);
        assert!(lines.len() >= 2, "{} fjords", lines.len());
        let dim = map.dim();
        let mut cut = 0;
        for i in 0..map.height.len() {
            assert!(map.height[i] <= before.height[i], "raised ground at {i}");
            cut += (map.height[i] < before.height[i]) as usize;
        }
        assert!(cut > 1000, "only {cut} cells cut");
        for line in &lines {
            // starts at the sea's edge and runs inland up the ground
            let (cx, _) = line[0];
            assert!((148..=152).contains(&cx), "starts at column {cx}");
            assert!(line.len() > 3, "a line of {}", line.len());
            let (last_x, _) = *line.last().unwrap();
            assert!(last_x > cx, "does not run inland");
            // its floor is under the sea, so the sea fills it
            for &(x, z) in line {
                assert!(
                    map.height[z * dim + x] as f64 <= -0.8 * FJORD_DEPTH.0 + 1.0,
                    "floor at {x},{z} is {}",
                    map.height[z * dim + x]
                );
            }
            // and its wall climbs from the floor no faster than WALL_PER_CELL a cell
            let (x, z) = line[line.len() / 2];
            let mut last = map.height[z * dim + x];
            for step in 1..30 {
                let h = map.height[(z + step).min(dim - 1) * dim + x];
                assert!(
                    (h - last) as f64 <= 3.5 * WALL_PER_CELL,
                    "wall too steep: {last} to {h}"
                );
                last = h;
            }
        }
    }

    #[test]
    fn fjords_are_long_enough_and_their_mouths_are_apart() {
        let mut map = mountainous_coast();
        let lines = fjords(&mut map, 0.6);
        assert!(lines.len() >= 2, "{} fjords", lines.len());
        for (a, la) in lines.iter().enumerate() {
            assert!(la.len() >= MIN_LENGTH, "a fjord of {} cells", la.len());
            for lb in &lines[a + 1..] {
                let (dx, dz) = (
                    la[0].0 as f64 - lb[0].0 as f64,
                    la[0].1 as f64 - lb[0].1 as f64,
                );
                assert!(
                    (dx * dx + dz * dz).sqrt() >= APART,
                    "mouths {:?} and {:?}",
                    la[0],
                    lb[0]
                );
            }
        }
    }

    #[test]
    fn a_fjord_wanders_and_is_not_a_straight_trench() {
        // on this ground the climb runs straight inland, so any sideways drift is the fjord's own
        let mut map = mountainous_coast();
        let lines = fjords(&mut map, 0.6);
        let dim = map.dim();
        let line = lines.iter().max_by_key(|l| l.len()).unwrap();
        let mut drift = Vec::new();
        let (first, last) = (line[0].0, line.last().unwrap().0);
        for x in first + 2..last.saturating_sub(2) {
            let (z0, z1) = (line[0].1.saturating_sub(40), (line[0].1 + 40).min(dim - 1));
            let floor: Vec<usize> = (z0..=z1)
                .filter(|&z| map.height[z * dim + x] as f64 <= -0.8 * FJORD_DEPTH.0 + 1.0)
                .collect();
            if !floor.is_empty() {
                drift.push(floor.iter().sum::<usize>() as f64 / floor.len() as f64);
            }
        }
        assert!(drift.len() > 10, "a floor over {} columns", drift.len());
        let (lo, hi) = drift
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(
            hi - lo >= 3.0,
            "the floor's centre only moves {} cells",
            hi - lo
        );
    }

    #[test]
    fn no_fjords_leave_the_map_alone_and_the_same_seed_cuts_the_same() {
        let mut map = mountainous_coast();
        assert!(fjords(&mut map, 0.0).is_empty());
        assert_eq!(mountainous_coast().height, map.height);
        let (mut a, mut b) = (mountainous_coast(), mountainous_coast());
        fjords(&mut a, 0.3);
        fjords(&mut b, 0.3);
        assert_eq!(a.height, b.height);
    }

    // Flat land 50 blocks up, volcanic in its east half and plain in the west.
    fn half_volcanic_plain() -> WorldMap {
        let config = WorldConfig {
            size: 500 * 32,
            map_cell: 32,
            ..Default::default()
        };
        let dim = config.dim();
        let mut map = WorldMap::empty(3, config);
        for cz in 0..dim {
            for cx in 0..dim {
                let i = cz * dim + cx;
                map.height[i] = 50.0;
                map.geology[i] = if cx >= dim / 2 {
                    super::super::geology::VOLCANIC
                } else {
                    1
                };
            }
        }
        map
    }

    #[test]
    fn cones_stand_only_on_volcanic_ground_with_a_crater_under_the_rim_and_only_raise() {
        let before = half_volcanic_plain();
        let mut map = before.clone();
        let sites = cones(&mut map, 0.5);
        assert!(sites.len() >= 3, "{} cones", sites.len());
        let dim = map.dim();
        let mut moved = 0;
        for i in 0..map.height.len() {
            assert!(map.height[i] >= before.height[i], "lowered ground at {i}");
            moved += (map.height[i] > before.height[i]) as usize;
            if map.height[i] != before.height[i] {
                assert!(
                    i % dim >= dim / 2 - 8,
                    "raised plain ground at column {}",
                    i % dim
                );
            }
        }
        assert!(moved > 3000, "only {moved} cells raised");
        for &(sx, sz, r) in &sites {
            assert!((CONE_RADIUS.0 as f64..=CONE_RADIUS.1 as f64).contains(&r));
            assert!(sx >= dim / 2, "a cone on the plain at {sx}");
            // a bowl: the middle is lower than the highest of the rim
            let centre = map.height[sz * dim + sx];
            let rim = (0..24)
                .map(|k| {
                    let a = k as f64 * std::f64::consts::TAU / 24.0;
                    let (x, z) = (
                        sx as f64 + 0.27 * r * libm::cos(a),
                        sz as f64 + 0.27 * r * libm::sin(a),
                    );
                    map.height[z as usize * dim + x as usize]
                })
                .fold(0.0f32, f32::max);
            assert!(
                centre + 20.0 < rim,
                "cone at ({sx},{sz}) r {r}: centre {centre}, rim {rim}"
            );
        }
    }

    #[test]
    fn no_cone_share_leaves_the_map_alone_and_the_same_seed_raises_the_same() {
        let mut map = half_volcanic_plain();
        assert!(cones(&mut map, 0.0).is_empty());
        assert_eq!(half_volcanic_plain().height, map.height);
        let (mut a, mut b) = (half_volcanic_plain(), half_volcanic_plain());
        cones(&mut a, 0.4);
        cones(&mut b, 0.4);
        assert_eq!(a.height, b.height);
        // and with no volcanic ground there is nothing to raise one on
        let mut plain = half_volcanic_plain();
        plain.geology.fill(1);
        assert!(cones(&mut plain, 0.5).is_empty());
    }

    #[test]
    fn a_world_with_no_mountainous_coast_gets_no_fjord() {
        let mut map = mountainous_coast();
        map.uplift.fill(0.0);
        let before = map.height.clone();
        assert!(fjords(&mut map, 0.3).is_empty());
        assert_eq!(before, map.height);
    }
}
