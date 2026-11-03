//! The detail laid over the map's ground at block scale. Everything here must give the same
//! answer on every machine, as separate machines build the same chunks independently: integer
//! hash noise, and arithmetic with no `sin`, `exp` or `powf`, at full detail. Wider cells, which
//! only LOD meshes use, may call `resolved`, which does.

use super::{par_rows, WorldMap};
use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{fbm_filtered, perlin, ridged};
use ochre_base::WATER_LEVEL;

const DETAIL_WAVELENGTH: f64 = 120.0;
const DETAIL_OCTAVES: u32 = 6;
/// Peak height of the detail, in blocks, over the gentlest ground and the roughest.
pub(crate) const GENTLE_DETAIL: f64 = 3.0;
pub(crate) const ROUGH_DETAIL: f64 = 38.0;

// Rows of dunes driven by the wind: a long gentle slope up to each crest, and a steep
// face falling away behind it.
const DUNE_SPACING: f64 = 90.0;
const DUNE_HEIGHT: f64 = 16.0;
// The share of each dune that's the gentle windward slope.
const WINDWARD: f64 = 0.6;
// How far, in dunes, the rows bend, and over what distance, so crests wander.
const CREST_BEND: f64 = 0.8;
const CREST_BEND_WAVELENGTH: f64 = 400.0;
// Where dunes stand tall and where they lie low.
const DUNE_FIELD_WAVELENGTH: f64 = 700.0;
// Dunes build up inland from the beach and give way to rock on high ground.
const DUNE_SHORE: (f64, f64) = (2.0, 14.0);
const DUNE_HIGH_GROUND: (f64, f64) = (70.0, 140.0);

// Patches of meadow on a mountain: noise this many blocks across, with the ground where it is over
// the second taking grass however steep.
// Cells either side that a mountain biome's presence is spread over, twice, to find the foothills.
const RANGE_REACH: usize = 18;
// Foothills: the ground round a range, within a few kilometres of it, is rough and cut into gullies and sharp ridges, as weather works at the edge of high ground.
const GULLY_WAVELENGTH: f64 = 85.0;
const GULLY_HEIGHT: f64 = 22.0;
// Keep foothill relief gentler than the mountain faces it surrounds.
pub(crate) const FOOTHILL_RELIEF: f64 = 0.6;

/// How much of the way, 0..1, ground `above_sea` blocks up and `range` (0..1) near a mountain range
/// is foothills: none far from a range or on the lowlands, all of it on the higher ground against one.
pub fn foothill_weight(range: f64, above_sea: f64) -> f64 {
    smoothstep(0.1, 0.55, range) * smoothstep(15.0, 90.0, above_sea)
}

/// Erosion's work on the ground: gullies and sharp ridges between them, in -1..1 where the ridges
/// are the highs, scaled by `GULLY_HEIGHT` and the weight by whoever calls it. Ridged noise
/// has sharp crests and rounded troughs, as slopes weather into ridge lines with gullies off them.
pub fn gullies(seed: u32, x: f64, z: f64) -> f64 {
    (ridged(seed ^ 0x6011, x, z, GULLY_WAVELENGTH, 3) - 0.45) * 2.0
}

/// How high gullies cut into ground `weight` of foothills or mountain.
pub fn gully_relief(seed: u32, x: f64, z: f64, weight: f64, cell: f64) -> f64 {
    // finer than wide cells can show, so they fade out in them, as the other detail does
    let seen = 1.0 - smoothstep(4.0, 16.0, cell);
    if weight <= 0.0 || seen <= 0.0 {
        return 0.0;
    }
    gullies(seed, x, z) * GULLY_HEIGHT * weight * seen
}

/// How high the detail rises over, or falls under, the map's ground at (`x`, `z`) on ground
/// `roughness` rough (0..1) and `above_sea` blocks up, in cells `cell` blocks wide.
pub fn detail(seed: u32, x: f64, z: f64, cell: f64, roughness: f64, above_sea: f64) -> f64 {
    let amplitude = GENTLE_DETAIL + (ROUGH_DETAIL - GENTLE_DETAIL) * roughness;
    // calm near the shoreline, where noise would break the coast up into specks of land
    let shore = 0.2 + 0.8 * smoothstep(0.0, 8.0, above_sea.abs());
    fbm_filtered(seed, x, z, DETAIL_WAVELENGTH, DETAIL_OCTAVES, cell) * amplitude * shore
}

/// How a world's dunes are laid out: `scale` multiplies their spacing and height, and the rows run
/// across `wind`, a unit vector pointing the way the air travels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dunes {
    pub scale: f64,
    pub wind: (f64, f64),
}

impl Default for Dunes {
    fn default() -> Self {
        Self {
            scale: 1.0,
            wind: (1.0, 0.0),
        }
    }
}

// 0..1 along the rows of dunes, 1 on each crest.
fn dune(seed: u32, x: f64, z: f64, style: Dunes) -> f64 {
    let bend = perlin(seed, x / CREST_BEND_WAVELENGTH, z / CREST_BEND_WAVELENGTH) * CREST_BEND;
    let along = x * style.wind.0 + z * style.wind.1;
    let t = (along / (DUNE_SPACING * style.scale) + bend).rem_euclid(1.0);
    if t < WINDWARD {
        smoothstep(0.0, WINDWARD, t)
    } else {
        smoothstep(1.0, WINDWARD, t)
    }
}

/// Height the dunes add on desert `above_sea` blocks up. Cells too wide to see the rows of dunes
/// see their average height, half a crest.
pub fn dunes(seed: u32, x: f64, z: f64, cell: f64, above_sea: f64, style: Dunes) -> f64 {
    let room = smoothstep(DUNE_SHORE.0, DUNE_SHORE.1, above_sea)
        * smoothstep(DUNE_HIGH_GROUND.1, DUNE_HIGH_GROUND.0, above_sea);
    if room <= 0.0 {
        return 0.0;
    }
    let field = smoothstep(
        -0.5,
        0.3,
        perlin(
            seed.wrapping_add(1),
            x / DUNE_FIELD_WAVELENGTH,
            z / DUNE_FIELD_WAVELENGTH,
        ),
    );
    let resolved = if cell <= 1.0 {
        1.0
    } else {
        ochre_base::noise::resolved(DUNE_SPACING * style.scale, cell)
    };
    let shape = 0.5 + (dune(seed, x, z, style) - 0.5) * resolved;
    shape * DUNE_HEIGHT * style.scale * (0.4 + 0.6 * field) * room
}

/// Fills the roughness layer: steep and high ground is rough, plains and flat biomes are not.
pub fn roughness_layer(map: &mut WorldMap) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f32;
    let cell = map.config.map_cell as f32;
    // how near each cell is to a range: the share of mountain in a wide window round it, widened
    let mountain: Vec<f32> = map.landform.iter().map(|&k| k as f32).collect();
    let near = super::climate::blur_box(
        &super::climate::blur_box(&mountain, dim, RANGE_REACH),
        dim,
        RANGE_REACH,
    );
    map.range = near
        .iter()
        .map(|&v| ((v * 2.5).clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    let mut out = vec![0u8; dim * dim];
    par_rows(dim, &mut out, |row, cells| {
        let h = |x: usize, z: usize| map.height[z * dim + x];
        for (col, out) in cells.iter_mut().enumerate() {
            let (xl, xr) = (col.saturating_sub(1), (col + 1).min(dim - 1));
            let (zl, zr) = (row.saturating_sub(1), (row + 1).min(dim - 1));
            let dx = (h(xr, row) - h(xl, row)) / ((xr - xl).max(1) as f32 * cell);
            let dz = (h(col, zr) - h(col, zl)) / ((zr - zl).max(1) as f32 * cell);
            let slope = (dx * dx + dz * dz).sqrt() as f64;
            let above_sea = (h(col, row) - sea) as f64;
            let mut r = (0.75 * smoothstep(0.1, 0.6, slope)
                + 0.35 * smoothstep(60.0, 300.0, above_sea))
            .clamp(0.08, 1.0);
            if map.landform[row * dim + col] == super::landform::MOUNTAIN {
                r = r.max(0.75);
            }
            // the foothills are rough too, the more the nearer a range
            let range = map.range[row * dim + col] as f64 / 255.0;
            r = r.max(0.8 * FOOTHILL_RELIEF * foothill_weight(range, above_sea));
            if above_sea < 0.0 {
                r *= 0.15;
            }
            *out = (r * 255.0).round() as u8;
        }
    });
    map.roughness = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dunes_rise_inland_but_not_on_beaches_or_mountains() {
        let rise = |above_sea: f64| {
            (0..200)
                .map(|x| dunes(1, x as f64 * 3.0, 0.0, 1.0, above_sea, Dunes::default()))
                .fold(0.0, f64::max)
        };
        assert_eq!(0.0, rise(1.0));
        assert_eq!(0.0, rise(200.0));
        assert!(
            rise(30.0) > DUNE_HEIGHT * 0.5,
            "dunes only rose {}",
            rise(30.0)
        );
        assert!(rise(30.0) <= DUNE_HEIGHT);
    }

    #[test]
    fn test_dunes_scale_in_spacing_and_height_and_run_across_the_wind() {
        let crests = |style: Dunes, along: (f64, f64)| {
            (0..6000)
                .filter(|&i| {
                    let (x, z) = (i as f64 * along.0, i as f64 * along.1);
                    dune(1, x, z, style) > 0.999 && dune(1, x + along.0, z + along.1, style) < 0.999
                })
                .count()
        };
        let plain = Dunes::default();
        let wide = Dunes {
            scale: 1.5,
            ..plain
        };
        assert!(crests(wide, (1.0, 0.0)) < crests(plain, (1.0, 0.0)) * 3 / 4);
        let rise = |style: Dunes| {
            (0..400)
                .map(|x| dunes(1, x as f64 * 3.0, 0.0, 1.0, 30.0, style))
                .fold(0.0, f64::max)
        };
        assert!(
            rise(wide) > rise(plain) * 1.2,
            "{} {}",
            rise(wide),
            rise(plain)
        );
        // rows across a wind along +z change along z and hardly at all along x
        let north = Dunes {
            wind: (0.0, 1.0),
            ..plain
        };
        let change = |dx: f64, dz: f64| {
            (0..400)
                .map(|i| {
                    let (x, z) = (i as f64 * 7.0, i as f64 * 3.0);
                    (dune(1, x + dx, z + dz, north) - dune(1, x, z, north)).abs()
                })
                .sum::<f64>()
        };
        assert!(
            change(0.0, 20.0) > change(20.0, 0.0) * 3.0,
            "{} {}",
            change(0.0, 20.0),
            change(20.0, 0.0)
        );
    }

    #[test]
    fn test_dunes_rise_gently_and_fall_steeply() {
        let (mut up, mut down) = (0.0f64, 0.0f64);
        for x in 0..2000 {
            let step = dune(1, x as f64 + 1.0, 0.0, Dunes::default())
                - dune(1, x as f64, 0.0, Dunes::default());
            (up, down) = (up.max(step), down.min(step));
        }
        assert!(-down > up * 1.3, "steepest rise {up}, steepest fall {down}");
    }

    #[test]
    fn test_foothills_are_near_a_range_and_up_off_the_lowland() {
        assert_eq!(0.0, foothill_weight(0.0, 200.0));
        assert_eq!(0.0, foothill_weight(0.9, 5.0));
        assert!(foothill_weight(0.9, 120.0) > 0.99);
        assert!(foothill_weight(0.3, 60.0) > 0.0 && foothill_weight(0.3, 60.0) < 0.9);
    }

    #[test]
    fn test_gullies_cut_sharp_ridges_and_fade_out_in_wide_cells_and_off_the_weight() {
        let relief: Vec<f64> = (0..4000)
            .map(|i| gully_relief(2, i as f64 * 2.0, 17.0, 1.0, 1.0))
            .collect();
        let (lo, hi) = relief
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(
            hi - lo > 15.0 && hi <= GULLY_HEIGHT * 1.1 && lo >= -GULLY_HEIGHT,
            "{lo}..{hi}"
        );
        assert_eq!(0.0, gully_relief(2, 5.0, 5.0, 0.0, 1.0));
        assert_eq!(0.0, gully_relief(2, 5.0, 5.0, 1.0, 32.0));
    }

    #[test]
    fn test_rougher_ground_has_taller_detail() {
        let spread = |roughness: f64| {
            (0..400)
                .map(|i| detail(3, i as f64 * 5.3, i as f64 * 2.1, 1.0, roughness, 100.0).abs())
                .fold(0.0, f64::max)
        };
        assert!(spread(1.0) > spread(0.0) * 4.0);
    }
}
