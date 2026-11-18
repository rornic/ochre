//! Landmarks on steppe and tundra (meso slice 2): big raised forms and bowls, sited by a rule table over the map's own layers,
//! laid with the other stamped shapes before the passes are carved. See `docs/meso-slice1-decision.md`.

use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{hash, unit};
use ochre_base::WATER_LEVEL;

use super::super::climate::{blur_box, moisture_field, FIELD_BLUR, LAPSE_PER_BLOCK};
use super::super::shapes::{ragged_t, sites_apart};
use super::super::valleys::steppe_likeness;
use super::super::WorldMap;

/// The country a row is for, read from the climate fields before the climate pass decides biomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Country {
    Steppe,
    Tundra,
}

/// A height profile, in blocks, round a site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    /// A flat top on steep sides.
    Butte,
    /// A broad dome, the mass seen from far off.
    Massif,
    /// A long crest, three times as long as it is wide.
    Ridge,
    /// A basin: sited and scored, but not counted among the landmarks seen from a ridge.
    Bowl,
}

struct Row {
    country: Country,
    primitive: Primitive,
    weight: f64,
    // radius in cells (a ridge's half length), and height or a bowl's depth in blocks
    radius: (f64, f64),
    height: (f64, f64),
}

// Sized from the slice 1 baseline: 130 blocks of relief over about 190 blocks reads from 4 km, and 250 over 750 as a mass.
const TABLE: &[Row] = &[
    Row { country: Country::Steppe, primitive: Primitive::Butte, weight: 0.35, radius: (2.5, 4.0), height: (150.0, 260.0) },
    Row { country: Country::Steppe, primitive: Primitive::Massif, weight: 0.25, radius: (7.0, 11.0), height: (250.0, 380.0) },
    Row { country: Country::Steppe, primitive: Primitive::Ridge, weight: 0.25, radius: (8.0, 13.0), height: (160.0, 260.0) },
    Row { country: Country::Steppe, primitive: Primitive::Bowl, weight: 0.15, radius: (4.0, 7.0), height: (60.0, 120.0) },
    Row { country: Country::Tundra, primitive: Primitive::Massif, weight: 0.35, radius: (6.0, 10.0), height: (220.0, 340.0) },
    Row { country: Country::Tundra, primitive: Primitive::Butte, weight: 0.25, radius: (2.5, 3.5), height: (140.0, 220.0) },
    Row { country: Country::Tundra, primitive: Primitive::Ridge, weight: 0.25, radius: (7.0, 12.0), height: (150.0, 240.0) },
    Row { country: Country::Tundra, primitive: Primitive::Bowl, weight: 0.15, radius: (4.0, 6.0), height: (50.0, 100.0) },
];

// Cells kept clear of the map's edge, as the other shapes keep.
const EDGE: usize = 60;
// The ground a landmark stands on: above the shore and below the high ground.
const ABOVE: (f32, f32) = (20.0, 700.0);
// Flat enough to stand out of.
const FLAT: f32 = 0.15;
const STEPPE_FROM: f32 = 0.6;
// Tundra's warmth at height, the band `cold_uplands` reads.
const TUNDRA: (f64, f64) = (0.05, 0.22);
const SALT: u32 = 0x1A4D_0001;

/// A landmark laid down: its site in cells, kind, radius in cells and height (a bowl's depth) in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landmark {
    pub x: usize,
    pub z: usize,
    pub country: Country,
    pub primitive: Primitive,
    pub radius: f64,
    pub height: f64,
}

// The country of each cell, if it is steppe or tundra ground a landmark could stand on.
fn countries(map: &WorldMap) -> Vec<Option<Country>> {
    let dim = map.dim();
    let (temperature, moisture) = moisture_field(map);
    let moisture = blur_box(&moisture, dim, FIELD_BLUR);
    let slope = super::super::gameplay::slope_layer(map);
    let sea = WATER_LEVEL as f32;
    (0..dim * dim)
        .map(|i| {
            let (x, z) = (i % dim, i / dim);
            let above = map.height[i] - sea;
            if x < EDGE || z < EDGE || x >= dim - EDGE || z >= dim - EDGE || !(ABOVE.0..ABOVE.1).contains(&above) || slope[i] >= FLAT {
                return None;
            }
            let warmth = (temperature[i] as f64 - above as f64 * LAPSE_PER_BLOCK as f64).max(0.0);
            if steppe_likeness(warmth, moisture[i] as f64) as f32 >= STEPPE_FROM {
                Some(Country::Steppe)
            } else if (TUNDRA.0..TUNDRA.1).contains(&warmth) {
                Some(Country::Tundra)
            } else {
                None
            }
        })
        .collect()
}

// The height a primitive adds at a ragged distance `t` (0 at the site, 1 at its radius), as a share of its height.
fn profile(primitive: Primitive, t: f64) -> f64 {
    match primitive {
        Primitive::Butte => 1.0 - smoothstep(0.7, 1.0, t),
        Primitive::Massif => libm::pow((1.0 - t * t).max(0.0), 1.5),
        Primitive::Ridge => 1.0 - smoothstep(0.15, 1.0, t),
        Primitive::Bowl => -(1.0 - smoothstep(0.3, 1.0, t)),
    }
}

// The row a site draws, by the rows' weights for its country.
fn row_for(country: Country, h: u32) -> &'static Row {
    let rows: Vec<&Row> = TABLE.iter().filter(|r| r.country == country).collect();
    let total: f64 = rows.iter().map(|r| r.weight).sum();
    let mut pick = unit(h, 1, 0) * total;
    for row in &rows {
        pick -= row.weight;
        if pick < 0.0 {
            return row;
        }
    }
    rows[rows.len() - 1]
}

/// Lays landmarks on steppe and tundra, each `apart` cells from the others, into `map.height`. Returns them, and a mask of how
/// much each cell belongs to one (1 inside, easing off past its radius), which the steppe's flattening leaves alone.
pub(crate) fn run(map: &mut WorldMap, apart: f64) -> (Vec<Landmark>, Vec<f32>) {
    let dim = map.dim();
    let mut keep = vec![0.0f32; dim * dim];
    if dim <= 2 * EDGE {
        return (Vec::new(), keep);
    }
    let country = countries(map);
    let eligible: Vec<usize> = (0..dim * dim).filter(|&i| country[i].is_some()).collect();
    let mut placed = Vec::new();
    for (sx, sz) in sites_apart(map, eligible, usize::MAX, SALT, apart) {
        let Some(at) = country[sz * dim + sx] else { continue };
        let h = hash(map.seed ^ SALT ^ 1, sx as i32, sz as i32);
        let row = row_for(at, h);
        let radius = row.radius.0 + (row.radius.1 - row.radius.0) * unit(h, 2, 0);
        let height = row.height.0 + (row.height.1 - row.height.0) * unit(h, 3, 0);
        // a ridge's crest runs at a seeded angle, a third as wide as it is long
        let angle = unit(h, 4, 0) * std::f64::consts::PI;
        let (along, across) = (libm::cos(angle), libm::sin(angle));
        let reach = (radius * 1.3).ceil() as i64;
        for z in (sz as i64 - reach).max(0)..=(sz as i64 + reach).min(dim as i64 - 1) {
            for x in (sx as i64 - reach).max(0)..=(sx as i64 + reach).min(dim as i64 - 1) {
                let t = if row.primitive == Primitive::Ridge {
                    let (dx, dz) = ((x - sx as i64) as f64, (z - sz as i64) as f64);
                    let (a, c) = (dx * along + dz * across, -dx * across + dz * along);
                    libm::sqrt((a / radius).powi(2) + (c / (radius / 3.0)).powi(2))
                } else {
                    ragged_t(map, SALT ^ 2, (x, z), (sx, sz), radius)
                };
                let i = z as usize * dim + x as usize;
                map.height[i] += (height * profile(row.primitive, t)) as f32;
                keep[i] = keep[i].max(1.0 - smoothstep(1.0, 1.3, t) as f32);
            }
        }
        placed.push(Landmark { x: sx, z: sz, country: at, primitive: row.primitive, radius, height });
    }
    (placed, keep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::WorldConfig;

    fn flat(warmth: f32, humidity: f32) -> WorldMap {
        let config = WorldConfig {
            size: 400 * 32,
            map_cell: 32,
            climate: crate::worldmap::climate::ClimateConfig { warmth, humidity, gradient: 0.0, continentality: 0.0, ..Default::default() },
            ..Default::default()
        };
        let mut map = WorldMap::empty(11, config);
        map.height.fill(WATER_LEVEL as f32 + 120.0);
        map
    }

    #[test]
    fn landmarks_rise_on_cold_ground_and_stay_apart() {
        let mut map = flat(0.2, 1.0);
        let before = map.height.clone();
        let (placed, keep) = run(&mut map, 40.0);
        assert!(!placed.is_empty(), "no landmark on cold flat ground");
        for (n, a) in placed.iter().enumerate() {
            assert_eq!(Country::Tundra, a.country);
            let i = a.z * map.dim() + a.x;
            let rise = map.height[i] - before[i];
            if a.primitive == Primitive::Bowl {
                assert!(rise < -40.0, "a bowl {rise} deep");
            } else {
                assert!(rise as f64 >= 0.9 * a.height, "a {:?} rose {rise} of {}", a.primitive, a.height);
            }
            assert_eq!(1.0, keep[i]);
            for b in &placed[n + 1..] {
                let d = ((a.x as f64 - b.x as f64).powi(2) + (a.z as f64 - b.z as f64).powi(2)).sqrt();
                assert!(d >= 40.0, "two landmarks {d} cells apart");
            }
        }
    }

    #[test]
    fn no_landmark_on_warm_wet_ground() {
        let mut map = flat(0.75, 1.6);
        let (placed, keep) = run(&mut map, 40.0);
        assert!(placed.is_empty());
        assert!(keep.iter().all(|&k| k == 0.0));
    }

    #[test]
    fn every_country_has_raised_rows_and_a_bowl() {
        for country in [Country::Steppe, Country::Tundra] {
            let rows: Vec<&Row> = TABLE.iter().filter(|r| r.country == country).collect();
            assert!(rows.iter().any(|r| r.primitive == Primitive::Bowl));
            assert!(rows.iter().filter(|r| r.primitive != Primitive::Bowl).all(|r| r.height.0 >= 130.0), "a raised row under the readable 130 blocks");
        }
    }
}
