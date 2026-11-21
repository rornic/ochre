//! Landmarks (meso slice 2): big raised forms and bowls, sited by a rule table over the map's own layers, the form drawn from the rock
//! under the site, laid with the other stamped shapes before the passes are carved. A biome has landmarks if the table has rows for it.
//! See `docs/meso-slice1-decision.md`.

use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{fbm, hash, unit};
use ochre_base::WATER_LEVEL;

use super::super::climate::{blur_box, classify, moisture_field, BiomeKind, FIELD_BLUR, LAPSE_PER_BLOCK};
use super::super::geology::{METAMORPHIC, SEDIMENTARY, VOLCANIC};
use super::super::shapes::{ragged_t, sites_apart};
use super::super::WorldMap;

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
    biome: BiomeKind,
    primitive: Primitive,
    // the geology classes it forms on, none listed for any: a butte needs a hard bed over soft ones
    rock: &'static [u8],
    weight: f64,
    // radius in cells (a ridge's half length), and height or a bowl's depth in blocks
    radius: (f64, f64),
    height: (f64, f64),
}

// The share of sited places that get a landmark, from thin patches to full ones, and a patch's width in landmark spacings.
const SPAWN: (f64, f64) = (0.35, 1.0);
const PATCH: f64 = 2.5;

const CAPPED: &[u8] = &[SEDIMENTARY, VOLCANIC];
const BEDDED: &[u8] = &[SEDIMENTARY, METAMORPHIC];
const ANY: &[u8] = &[];

// Sized from the slice 1 baseline: 130 blocks of relief over about 190 blocks reads from 4 km, and 250 over 750 as a mass. Every biome keeps
// a massif and a bowl that form on any rock, so a site never lacks a row.
const TABLE: &[Row] = &[
    Row { biome: BiomeKind::Steppe, primitive: Primitive::Butte, rock: CAPPED, weight: 0.35, radius: (2.5, 4.0), height: (150.0, 260.0) },
    Row { biome: BiomeKind::Steppe, primitive: Primitive::Massif, rock: ANY, weight: 0.25, radius: (7.0, 11.0), height: (250.0, 380.0) },
    Row { biome: BiomeKind::Steppe, primitive: Primitive::Ridge, rock: BEDDED, weight: 0.25, radius: (8.0, 13.0), height: (160.0, 260.0) },
    Row { biome: BiomeKind::Steppe, primitive: Primitive::Bowl, rock: ANY, weight: 0.15, radius: (4.0, 7.0), height: (60.0, 120.0) },
    Row { biome: BiomeKind::Tundra, primitive: Primitive::Massif, rock: ANY, weight: 0.35, radius: (6.0, 10.0), height: (220.0, 340.0) },
    Row { biome: BiomeKind::Tundra, primitive: Primitive::Butte, rock: CAPPED, weight: 0.25, radius: (2.5, 3.5), height: (140.0, 220.0) },
    Row { biome: BiomeKind::Tundra, primitive: Primitive::Ridge, rock: BEDDED, weight: 0.25, radius: (7.0, 12.0), height: (150.0, 240.0) },
    Row { biome: BiomeKind::Tundra, primitive: Primitive::Bowl, rock: ANY, weight: 0.15, radius: (4.0, 6.0), height: (50.0, 100.0) },
    // desert: mesas and buttes over a hard cap, the domes of bare rock left standing, hogbacks, and dry basins
    Row { biome: BiomeKind::Desert, primitive: Primitive::Butte, rock: CAPPED, weight: 0.40, radius: (2.5, 4.0), height: (130.0, 240.0) },
    Row { biome: BiomeKind::Desert, primitive: Primitive::Massif, rock: ANY, weight: 0.25, radius: (6.0, 9.0), height: (200.0, 320.0) },
    Row { biome: BiomeKind::Desert, primitive: Primitive::Ridge, rock: BEDDED, weight: 0.20, radius: (8.0, 12.0), height: (140.0, 230.0) },
    Row { biome: BiomeKind::Desert, primitive: Primitive::Bowl, rock: ANY, weight: 0.15, radius: (4.0, 6.0), height: (50.0, 100.0) },
    // grassland: big rolling hills and long escarpments, kept lower so it stays pastoral
    Row { biome: BiomeKind::Grassland, primitive: Primitive::Massif, rock: ANY, weight: 0.45, radius: (7.0, 11.0), height: (180.0, 300.0) },
    Row { biome: BiomeKind::Grassland, primitive: Primitive::Ridge, rock: BEDDED, weight: 0.30, radius: (8.0, 12.0), height: (120.0, 200.0) },
    Row { biome: BiomeKind::Grassland, primitive: Primitive::Butte, rock: CAPPED, weight: 0.10, radius: (2.5, 3.5), height: (100.0, 170.0) },
    Row { biome: BiomeKind::Grassland, primitive: Primitive::Bowl, rock: ANY, weight: 0.15, radius: (4.0, 7.0), height: (40.0, 80.0) },
    // forest: wooded hills and ridges, and basins that fill
    Row { biome: BiomeKind::Forest, primitive: Primitive::Massif, rock: ANY, weight: 0.40, radius: (7.0, 11.0), height: (200.0, 320.0) },
    Row { biome: BiomeKind::Forest, primitive: Primitive::Ridge, rock: BEDDED, weight: 0.35, radius: (8.0, 13.0), height: (130.0, 220.0) },
    Row { biome: BiomeKind::Forest, primitive: Primitive::Bowl, rock: ANY, weight: 0.25, radius: (4.0, 7.0), height: (40.0, 90.0) },
    // taiga: glaciated country, long moraine ridges and kettle basins between the hills
    Row { biome: BiomeKind::Taiga, primitive: Primitive::Massif, rock: ANY, weight: 0.35, radius: (6.0, 10.0), height: (200.0, 320.0) },
    Row { biome: BiomeKind::Taiga, primitive: Primitive::Ridge, rock: ANY, weight: 0.30, radius: (7.0, 12.0), height: (110.0, 200.0) },
    Row { biome: BiomeKind::Taiga, primitive: Primitive::Bowl, rock: ANY, weight: 0.35, radius: (4.0, 6.0), height: (40.0, 90.0) },
];

// Cells kept clear of the map's edge, as the other shapes keep.
const EDGE: usize = 60;
// The ground a landmark stands on: above the shore and below the high ground.
const ABOVE: (f32, f32) = (20.0, 700.0);
// Flat enough to stand out of.
const FLAT: f32 = 0.15;
const SALT: u32 = 0x1A4D_0001;

/// A landmark laid down: its site in cells, kind, radius in cells and height (a bowl's depth) in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landmark {
    pub x: usize,
    pub z: usize,
    pub biome: BiomeKind,
    pub primitive: Primitive,
    pub radius: f64,
    pub height: f64,
}

// The biome of each cell, if the table has rows for it and a landmark could stand on the ground: the climate's own classification of its warmth
// at height and its moisture, before the climate pass decides.
pub(super) fn biomes(map: &WorldMap) -> Vec<Option<BiomeKind>> {
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
            let warmth = (temperature[i] - above * LAPSE_PER_BLOCK).max(0.0);
            let biome = classify(warmth, moisture[i]);
            TABLE.iter().any(|r| r.biome == biome).then_some(biome)
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

// The row a site draws, by the weights of the rows for its biome that form on the rock under it.
fn row_for(biome: BiomeKind, rock: u8, h: u32) -> &'static Row {
    let rows: Vec<&Row> = TABLE.iter().filter(|r| r.biome == biome && (r.rock.is_empty() || r.rock.contains(&rock))).collect();
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
    // sited closer so the count holds, then each kept by a chance that swells and thins over patches: dart throwing alone packs a near-regular lattice
    let wavelength = apart * PATCH;
    let chance = |x: usize, z: usize| SPAWN.0 + (SPAWN.1 - SPAWN.0) * smoothstep(-0.4, 0.4, fbm(map.seed ^ SALT ^ 6, x as f64, z as f64, wavelength, 2));
    let apart = apart * libm::sqrt((SPAWN.0 + SPAWN.1) / 2.0);
    let dim = map.dim();
    let mut keep = vec![0.0f32; dim * dim];
    if dim <= 2 * EDGE {
        return (Vec::new(), keep);
    }
    let biome = biomes(map);
    let eligible: Vec<usize> = (0..dim * dim).filter(|&i| biome[i].is_some()).collect();
    let mut placed = Vec::new();
    for (sx, sz) in sites_apart(map, eligible, usize::MAX, SALT, apart) {
        let Some(at) = biome[sz * dim + sx] else { continue };
        let h = hash(map.seed ^ SALT ^ 1, sx as i32, sz as i32);
        if unit(h, 5, 0) >= chance(sx, sz) {
            continue;
        }
        let row = row_for(at, map.geology[sz * dim + sx], h);
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
        placed.push(Landmark { x: sx, z: sz, biome: at, primitive: row.primitive, radius, height });
    }
    (placed, keep)
}

/// The cells in the middle of each bowl, whose basin holds a lake however small, see `hydrology::run_keeping`.
pub(crate) fn bowl_cells(dim: usize, placed: &[Landmark]) -> Vec<bool> {
    let mut bowls = vec![false; dim * dim];
    for l in placed.iter().filter(|l| l.primitive == Primitive::Bowl) {
        let reach = (l.radius * 0.5).ceil() as i64;
        for z in (l.z as i64 - reach).max(0)..=(l.z as i64 + reach).min(dim as i64 - 1) {
            for x in (l.x as i64 - reach).max(0)..=(l.x as i64 + reach).min(dim as i64 - 1) {
                bowls[z as usize * dim + x as usize] = true;
            }
        }
    }
    bowls
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
            assert!(matches!(a.biome, BiomeKind::Tundra | BiomeKind::Taiga), "{:?} on cold ground", a.biome);
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
                assert!(d >= 40.0 * libm::sqrt((SPAWN.0 + SPAWN.1) / 2.0) - 0.01, "two landmarks {d} cells apart");
            }
        }
    }

    #[test]
    fn landmarks_come_in_patches_not_on_a_lattice() {
        let mut map = flat(0.45, 0.2);
        let (placed, _) = run(&mut map, 40.0);
        let mut blocks = [0f64; 25];
        for l in &placed {
            blocks[(l.z / 80).min(4) * 5 + (l.x / 80).min(4)] += 1.0;
        }
        let mean = blocks.iter().sum::<f64>() / 25.0;
        let spread = (blocks.iter().map(|b| (b - mean).powi(2)).sum::<f64>() / 25.0).sqrt() / mean;
        // dart throwing alone gives about 0.15 here
        assert!(placed.len() > 20 && spread > 0.4, "{} landmarks, spread {spread}", placed.len());
    }

    #[test]
    fn biomes_without_rows_get_no_landmarks() {
        // the owner's call: jungle, savanna and glacier need their base terrain worked out first
        for biome in [BiomeKind::Jungle, BiomeKind::Savanna, BiomeKind::Glacier, BiomeKind::Swamp] {
            assert!(TABLE.iter().all(|r| r.biome != biome), "{biome:?} has rows");
        }
        let mut map = flat(0.75, 1.6);
        let (placed, _) = run(&mut map, 40.0);
        assert!(placed.iter().all(|l| TABLE.iter().any(|r| r.biome == l.biome)));
    }

    #[test]
    fn every_biome_keeps_a_massif_and_a_bowl_that_form_on_any_rock() {
        let biomes: std::collections::BTreeSet<u8> = TABLE.iter().map(|r| r.biome as u8).collect();
        assert!(biomes.len() >= 6, "{biomes:?}");
        for r in TABLE {
            let rows: Vec<&Row> = TABLE.iter().filter(|o| o.biome == r.biome).collect();
            for primitive in [Primitive::Massif, Primitive::Bowl] {
                assert!(rows.iter().any(|o| o.primitive == primitive && o.rock.is_empty()), "{:?} lacks an any-rock {primitive:?}", r.biome);
            }
            assert!(rows.iter().filter(|o| o.primitive != Primitive::Bowl).all(|o| o.height.0 >= 100.0), "{:?} has a raised row under 100 blocks", r.biome);
        }
    }

    #[test]
    fn rock_picks_the_form() {
        let drawn = |rock: u8| -> std::collections::BTreeSet<&'static str> {
            (0..400u32)
                .map(|h| match row_for(BiomeKind::Desert, rock, hash(7, h as i32, 0)).primitive {
                    Primitive::Butte => "butte",
                    Primitive::Massif => "massif",
                    Primitive::Ridge => "ridge",
                    Primitive::Bowl => "bowl",
                })
                .collect()
        };
        // 0 is generic rock, which holds no cap and has no dipping beds
        assert_eq!(["bowl", "massif"].into_iter().collect::<std::collections::BTreeSet<_>>(), drawn(0));
        assert!(drawn(SEDIMENTARY).contains("butte") && drawn(SEDIMENTARY).contains("ridge"));
        assert!(drawn(VOLCANIC).contains("butte") && !drawn(VOLCANIC).contains("ridge"));
        assert!(drawn(METAMORPHIC).contains("ridge") && !drawn(METAMORPHIC).contains("butte"));
    }
}
