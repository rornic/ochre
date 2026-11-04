//! Climate from geography: warmth falls with latitude and height, a prevailing wind carries
//! moisture off the sea and drops it on rising ground, leaving rain shadows behind ranges, and a
//! Whittaker-style table turns warmth and moisture into biomes.

use super::{par_rows, WaterCell, WorldMap};
use ochre_base::biome::Biomes;
use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::fbm;
use ochre_base::WATER_LEVEL;

/// Warmth lost per block of height, the rate snow lines climb at, see `generator::terrain`.
pub const LAPSE_PER_BLOCK: f32 = 1.0 / 3500.0;

/// A regional climate: warmth is a sea-level baseline, not a miniature equator-to-pole map.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct ClimateConfig {
    pub warmth: f32,
    pub wind_x: f32,
    pub wind_z: f32,
    /// Scales the rain that biome moisture sees (rivers and lakes are unaffected).
    pub humidity: f32,
    /// How much warmer one side of the world is than the other, along `gradient_angle` (radians).
    pub gradient: f32,
    pub gradient_angle: f32,
    /// How much warmer (or colder, if negative) the far interior is than the coast.
    pub continentality: f32,
}

impl Default for ClimateConfig {
    fn default() -> Self {
        Self {
            warmth: 0.68,
            wind_x: 1.0,
            wind_z: 0.15,
            humidity: 1.0,
            gradient: 0.0,
            gradient_angle: 0.0,
            continentality: 0.0,
        }
    }
}

impl ClimateConfig {
    pub fn validate(self) -> Result<(), String> {
        if !self.warmth.is_finite()
            || !(0.0..=1.0).contains(&self.warmth)
            || !self.wind_x.is_finite()
            || !self.wind_z.is_finite()
            || !(0.01..=100.0)
                .contains(&(self.wind_x * self.wind_x + self.wind_z * self.wind_z).sqrt())
            || !(0.1..=3.0).contains(&self.humidity)
            || !self.gradient_angle.is_finite()
            || !(0.0..=0.5).contains(&self.gradient)
            || !(-0.3..=0.3).contains(&self.continentality)
        {
            return Err(
                "climate requires warmth in 0..=1, humidity in 0.1..=3 and a finite nonzero wind vector"
                    .into(),
            );
        }
        Ok(())
    }
}

// Offsets in warmth or moisture tried either side of a cell to find how close it is to a class boundary.
const MARGIN_PROBES: [f32; 4] = [0.01, 0.02, 0.03, 0.04];
// Cells from a class boundary past which a block never takes the biome across it.
const MARGIN_CELLS: i64 = 2;
// Warmth and moisture are blurred this many cells each way before biomes are picked from them.
pub(super) const FIELD_BLUR: usize = 4;
// Cells round a river or lake that it wets and cools, and by how much at the water's edge.
const RIPARIAN_REACH: u8 = 14;
// River widths, in blocks, that count for a little, a lot and everything of that reach, and the
// moisture between which ground is dry: rivers lose their water crossing it, see `hydrology`.
const RIPARIAN_WIDTHS: (f32, f32, f32) = (20.0, 38.0, 65.0);
const DRY_BELOW: (f64, f64) = (0.2, 0.45);
const RIPARIAN_MOISTURE: f32 = 1.0;
const RIPARIAN_COOLING: f32 = 0.14;
// Water cools its banks only where the ground is warmer than this: none at the first, in full at the second.
const RIPARIAN_COOLS_FROM: (f64, f64) = (0.30, 0.55);
// A cell takes the most common biome within this many cells, MODE_PASSES times, and a patch of
// fewer cells than MIN_BIOME_CELLS joins a neighbour.
const MODE_RADIUS: usize = 2;
const MODE_PASSES: usize = 2;
const MIN_BIOME_CELLS: usize = 400;
// Swamp: grassland, forest or jungle no higher than this over the sea, on ground flatter than this, at least this
// wet and within about 13 cells of a river or lake (`closeness` there).
const SWAMP_BELOW: f32 = 40.0;
const SWAMP_SLOPE: f32 = 0.2;
const SWAMP_MOISTURE: f32 = 0.65;
const SWAMP_NEAR: f32 = 0.02;
// Conifers on a mountain's lower slopes, this thick, thinning out between these heights over the
// sea to nothing: the treeline.
const ALPINE_FOREST: f64 = 0.3;
// The treeline over the sea at the default warmth: it rises and falls with a world's warmth.
const TREELINE: (f64, f64) = (1100.0, 1650.0);
const DEFAULT_WARMTH: f32 = 0.68;

/// Heights over the sea between which conifers thin to nothing, where the regional warmth is `warmth`.
pub fn treeline(warmth: f32) -> (f64, f64) {
    let shift = ((warmth - DEFAULT_WARMTH) / LAPSE_PER_BLOCK) as f64;
    let start = (TREELINE.0 + shift).max(50.0);
    (start, (TREELINE.1 + shift).max(start + 100.0))
}

// Cells between nearest-sea samples at which the warp and the gradient are worked out, then
// interpolated: both vary over kilometres.
const COARSE: usize = 8;
// Cells of distance from the sea at which the interior is fully inland (12 km at 32 m a cell).
const INLAND_CELLS: f32 = 375.0;

// Distance in cells from each cell to the nearest sea cell, by the 3-4 chamfer.
fn sea_distance(map: &WorldMap) -> Vec<f32> {
    let dim = map.dim();
    let far = i32::MAX / 2;
    let mut d: Vec<i32> = map
        .continentalness
        .iter()
        .map(|&c| if c < 0.0 { 0 } else { far })
        .collect();
    let at = |x: i64, z: i64| (0..dim as i64).contains(&x) && (0..dim as i64).contains(&z);
    for z in 0..dim as i64 {
        for x in 0..dim as i64 {
            let i = z as usize * dim + x as usize;
            for (dx, dz, w) in [(-1, -1, 4), (0, -1, 3), (1, -1, 4), (-1, 0, 3)] {
                if at(x + dx, z + dz) {
                    d[i] = d[i].min(d[(z + dz) as usize * dim + (x + dx) as usize] + w);
                }
            }
        }
    }
    for z in (0..dim as i64).rev() {
        for x in (0..dim as i64).rev() {
            let i = z as usize * dim + x as usize;
            for (dx, dz, w) in [(1, 1, 4), (0, 1, 3), (-1, 1, 4), (1, 0, 3)] {
                if at(x + dx, z + dz) {
                    d[i] = d[i].min(d[(z + dz) as usize * dim + (x + dx) as usize] + w);
                }
            }
        }
    }
    d.into_iter().map(|v| v as f32 / 3.0).collect()
}

// Position along the gradient direction, -1 to 1, with the coordinates warped so a line of equal
// warmth wanders instead of running straight, worked out on a coarse grid and interpolated.
fn gradient_position(map: &WorldMap) -> Vec<f32> {
    let dim = map.dim();
    let cfg = map.config.climate;
    let (sin, cos) = (
        libm::sin(cfg.gradient_angle as f64),
        libm::cos(cfg.gradient_angle as f64),
    );
    let half = map.config.half_size() as f64;
    let nodes = dim.div_ceil(COARSE) + 1;
    let mut coarse = vec![0.0f32; nodes * nodes];
    par_rows(nodes, &mut coarse, |row, out| {
        for (col, u) in out.iter_mut().enumerate() {
            let (x, z) = map.cell_centre((col * COARSE).min(dim - 1), (row * COARSE).min(dim - 1));
            let wx = x + 1500.0 * fbm(map.seed ^ 0x7E01, x, z, 8000.0, 2);
            let wz = z + 1500.0 * fbm(map.seed ^ 0x7E02, x, z, 8000.0, 2);
            *u = ((wx * cos + wz * sin) / half).clamp(-1.0, 1.0) as f32;
        }
    });
    let mut out = vec![0.0f32; dim * dim];
    par_rows(dim, &mut out, |row, cells| {
        let (fz, z0) = (row as f32 / COARSE as f32, row / COARSE);
        let tz = fz - z0 as f32;
        for (col, u) in cells.iter_mut().enumerate() {
            let (fx, x0) = (col as f32 / COARSE as f32, col / COARSE);
            let tx = fx - x0 as f32;
            let at = |x: usize, z: usize| coarse[z * nodes + x];
            let top = at(x0, z0) * (1.0 - tx) + at(x0 + 1, z0) * tx;
            let bottom = at(x0, z0 + 1) * (1.0 - tx) + at(x0 + 1, z0 + 1) * tx;
            *u = top * (1.0 - tz) + bottom * tz;
        }
    });
    out
}

/// Each cell's sea-level warmth from the config alone: the baseline, the gradient and how far
/// inland it is, before local variation, height or water. The treeline follows it.
pub(super) fn regional_warmth(map: &WorldMap) -> Vec<f32> {
    let cfg = map.config.climate;
    let mut warmth = vec![cfg.warmth; map.height.len()];
    if cfg.gradient != 0.0 {
        for (w, u) in warmth.iter_mut().zip(gradient_position(map)) {
            *w += cfg.gradient * u;
        }
    }
    if cfg.continentality != 0.0 {
        for (w, d) in warmth.iter_mut().zip(sea_distance(map)) {
            *w += cfg.continentality * (d / INLAND_CELLS).min(1.0);
        }
    }
    warmth
}

/// What grows on a stretch of land, stored in `WorldMap::biome`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum BiomeKind {
    Grassland = 0,
    Glacier = 1,
    Desert = 2,
    Forest = 3,
    Tundra = 4,
    Taiga = 5,
    Steppe = 6,
    Savanna = 7,
    Jungle = 8,
    /// The sea floor, which has no land biome.
    Sea = 9,
    /// Only a scratch value while landform is worked out: the biome layer never holds it, since a mountain is
    /// the climate biome under it with the mountain modifier on, see `Modifier`.
    Mountain = 10,
    /// Flat, low, wet ground by fresh water.
    Swamp = 11,
}

impl BiomeKind {
    pub const ALL: [BiomeKind; 12] = [
        Self::Grassland,
        Self::Glacier,
        Self::Desert,
        Self::Forest,
        Self::Tundra,
        Self::Taiga,
        Self::Steppe,
        Self::Savanna,
        Self::Jungle,
        Self::Sea,
        Self::Mountain,
        Self::Swamp,
    ];

    pub fn from_u8(v: u8) -> Self {
        Self::ALL
            .get(v as usize)
            .copied()
            .unwrap_or(Self::Grassland)
    }

    /// The name in `Biomes`, or None for the base terrain.
    pub fn name(self) -> Option<&'static str> {
        match self {
            Self::Grassland | Self::Sea | Self::Mountain => None,
            Self::Glacier => Some("glacier"),
            Self::Desert => Some("desert"),
            Self::Forest => Some("forest"),
            Self::Tundra => Some("tundra"),
            Self::Taiga => Some("taiga"),
            Self::Steppe => Some("steppe"),
            Self::Savanna => Some("savanna"),
            Self::Jungle => Some("jungle"),
            Self::Swamp => Some("swamp"),
        }
    }
}

/// The biome where it is `temperature` warm and `moisture` wet, both 0..1, on land.
pub fn classify(temperature: f32, moisture: f32) -> BiomeKind {
    if temperature < 0.1 {
        BiomeKind::Glacier
    } else if temperature < 0.26 {
        if moisture < 0.35 {
            BiomeKind::Tundra
        } else {
            BiomeKind::Taiga
        }
    } else if temperature < 0.58 {
        match moisture {
            m if m < 0.3 => BiomeKind::Steppe,
            m if m < 0.58 => BiomeKind::Grassland,
            _ => BiomeKind::Forest,
        }
    } else {
        match moisture {
            m if m < 0.14 => BiomeKind::Desert,
            m if m < 0.5 => BiomeKind::Savanna,
            m if m < 0.7 => BiomeKind::Forest,
            _ => BiomeKind::Jungle,
        }
    }
}

// Tree density 0..1 for a biome, before the cold thins it out.
fn tree_density(kind: BiomeKind, biomes: &Biomes) -> f64 {
    kind.name().and_then(|name| biomes.id(name)).map_or(
        biomes.tree_density(ochre_base::biome::BiomeId::BASE),
        |id| biomes.tree_density(id),
    )
}

/// Background climate shared by runoff and vegetation. Transport sweeps in upwind
/// dependency order, mixing the two upstream neighbours for oblique winds. Rates are
/// per metre and lift is in metres, rather than per map cell.
fn climate_fields(map: &WorldMap) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let dim = map.dim();
    let cfg = map.config.climate;
    let regional = regional_warmth(map);
    let mut temperature = vec![0.0; dim * dim];
    par_rows(dim, &mut temperature, |row, out| {
        for (col, t) in out.iter_mut().enumerate() {
            let (x, z) = map.cell_centre(col, row);
            *t = (regional[row * dim + col]
                + fbm(map.seed ^ 0xC11A, x, z, 18000.0, 2) as f32 * 0.06)
                .clamp(0.0, 1.0);
        }
    });
    let norm = (cfg.wind_x * cfg.wind_x + cfg.wind_z * cfg.wind_z).sqrt();
    let ax = cfg.wind_x.abs() / norm;
    let az = cfg.wind_z.abs() / norm;
    let step = map.config.map_cell as f32 / (ax + az);
    let mut vapour = vec![0.0f32; dim * dim];
    let mut rain = vec![0.0f32; dim * dim];
    for rz in 0..dim {
        let z = if cfg.wind_z >= 0.0 { rz } else { dim - 1 - rz };
        for rx in 0..dim {
            let x = if cfg.wind_x >= 0.0 { rx } else { dim - 1 - rx };
            let i = map.index(x, z);
            let upstream = |horizontal: bool| {
                let available = if horizontal { rx > 0 } else { rz > 0 };
                if !available {
                    return (0.65, map.height[i]);
                }
                let j = if horizontal {
                    map.index(if cfg.wind_x >= 0.0 { x - 1 } else { x + 1 }, z)
                } else {
                    map.index(x, if cfg.wind_z >= 0.0 { z - 1 } else { z + 1 })
                };
                (vapour[j], map.height[j])
            };
            let (qx, hx) = upstream(true);
            let (qz, hz) = upstream(false);
            let carried = (ax * qx + az * qz) / (ax + az);
            if map.height[i] < WATER_LEVEL as f32 {
                vapour[i] =
                    1.0 - (1.0 - carried) * libm::expf(-step * 0.003 * (0.3 + temperature[i]));
            } else {
                let lift = (map.height[i] - (ax * hx + az * hz) / (ax + az)).max(0.0);
                let fraction = 1.0 - libm::expf(-0.00045 * step - lift / 700.0);
                let falls = carried * fraction;
                vapour[i] = (carried - falls * 0.30).max(0.0);
                rain[i] = falls * 1000.0 / step;
            }
        }
    }
    let rain = blur_box(&rain, dim, 2);
    let mut moisture = vec![1.0; dim * dim];
    for i in 0..moisture.len() {
        if map.height[i] < WATER_LEVEL as f32 {
            continue;
        }
        let surface = (temperature[i] - map.height[i].max(0.0) * LAPSE_PER_BLOCK).max(0.0);
        let demand = 0.20 + 0.50 * surface;
        moisture[i] = (rain[i] * cfg.humidity / demand).clamp(0.0, 1.0);
    }
    (temperature, moisture, rain)
}

pub(super) fn moisture_field(map: &WorldMap) -> (Vec<f32>, Vec<f32>) {
    let (temperature, moisture, _) = climate_fields(map);
    (temperature, moisture)
}

/// How dry a cell of this moisture is, 0..1.
pub(super) fn dryness(moisture: f32) -> f32 {
    smoothstep(DRY_BELOW.1, DRY_BELOW.0, moisture as f64) as f32
}

pub fn run(map: &mut WorldMap) {
    let dim = map.dim();
    let sea = WATER_LEVEL as f32;
    let seed = map.seed;

    let (temperature, moisture, precipitation) = climate_fields(map);
    map.precipitation = precipitation;
    let biomes = Biomes::new(seed);
    // smooth over a few cells each way, so a biome doesn't flip back and forth along the edge
    // where its warmth or moisture hovers at a threshold
    let mut moisture = blur_box(&moisture, dim, FIELD_BLUR);
    let mut temperature = blur_box(&temperature, dim, FIELD_BLUR);
    // only big water changes the ground round it: a wide river or a lake cools and wets its
    // banks, so a green belt follows one through a desert, and a stream does nothing of the kind
    let near_water = fresh_water_closeness(map);
    for i in 0..dim * dim {
        if map.height[i] >= sea {
            moisture[i] = (moisture[i] + RIPARIAN_MOISTURE * near_water[i]).min(1.0);
            // not in cold country, where it would cut strips of snow along every bank
            let warm = smoothstep(
                RIPARIAN_COOLS_FROM.0,
                RIPARIAN_COOLS_FROM.1,
                temperature[i] as f64,
            );
            temperature[i] =
                (temperature[i] - RIPARIAN_COOLING * near_water[i] * warm as f32).max(0.0);
        }
    }

    let mut kinds = vec![BiomeKind::Sea as u8; dim * dim];
    for i in 0..dim * dim {
        let h = map.height[i];
        if h < sea {
            continue;
        }
        // what a cell grows depends on how warm it is at its height
        let warmth = (temperature[i] - (h - sea) * LAPSE_PER_BLOCK).max(0.0);
        kinds[i] = classify(warmth, moisture[i]) as u8;
    }
    // flat, low, wet ground by fresh water is swamp, before the tidying removes the specks of it
    let climate_kinds = kinds.clone();
    let slope = super::gameplay::slope_layer(map);
    let fresh = closeness(map, |i| {
        map.water_kind[i] == WaterCell::Lake as u8 || map.water_kind[i] == WaterCell::River as u8
    });
    for i in 0..dim * dim {
        let above = map.height[i] - sea;
        let eligible = matches!(
            BiomeKind::from_u8(kinds[i]),
            BiomeKind::Grassland | BiomeKind::Forest | BiomeKind::Jungle
        );
        if eligible
            && map.height[i] >= sea
            && map.landform[i] != super::landform::MOUNTAIN
            && above <= SWAMP_BELOW
            && slope[i] < SWAMP_SLOPE
            && moisture[i] >= SWAMP_MOISTURE
            && fresh[i] >= SWAMP_NEAR
        {
            kinds[i] = BiomeKind::Swamp as u8;
        }
    }
    let mut kinds = tidy_biomes(&kinds, dim);
    // the tidying can move a swamp's edge onto ground that is too high, or mountain: that ground is its climate's again
    for i in 0..dim * dim {
        if kinds[i] == BiomeKind::Swamp as u8
            && (map.height[i] - sea > SWAMP_BELOW || map.landform[i] == super::landform::MOUNTAIN)
        {
            kinds[i] = climate_kinds[i];
        }
    }

    let (alt, margin) = boundary_margins(map, &temperature, &moisture, &kinds);
    map.biome_alt = alt;
    map.biome_margin = margin;

    let regional = regional_warmth(map);
    let mut trees = vec![0u8; dim * dim];
    for i in 0..dim * dim {
        if map.height[i] < sea {
            continue;
        }
        // lakes and rivers, and nothing grows in them
        let wet = map.water_kind[i] == WaterCell::Lake as u8
            || map.water_kind[i] == WaterCell::River as u8;
        let kind = BiomeKind::from_u8(kinds[i]);
        let above = (map.height[i] - sea) as f64;
        // a mountain's slopes are forested with conifers below the treeline where it is wet enough, thinning
        // out into bare rock above it, instead of the few scattered trees the table gives the biome
        let density = if map.landform[i] == super::landform::MOUNTAIN {
            let line = treeline(regional[i].clamp(0.0, 1.0));
            let belt = ALPINE_FOREST
                * smoothstep(15.0, 60.0, above)
                * (1.0 - smoothstep(line.0, line.1, above))
                * smoothstep(0.15, 0.5, moisture[i] as f64);
            (belt + 0.02).max(tree_density(kind, &biomes).min(0.04))
        } else {
            tree_density(kind, &biomes)
        } * if wet { 0.0 } else { 1.0 };
        trees[i] = (density.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    map.temperature = temperature;
    map.moisture = moisture;
    map.biome = kinds;
    map.trees = trees;
}

// For each land cell, the climate biome just across its nearest class boundary and how far off it the cell is: the
// first of `MARGIN_PROBES` warmth or moisture offsets that changes the class sets the margin (offset over the last, as 0..255; 255 when
// none does) and the biome there. A cell the swamp rule or the tidying made something its climate is not stays 255.
fn boundary_margins(
    map: &WorldMap,
    temperature: &[f32],
    moisture: &[f32],
    kinds: &[u8],
) -> (Vec<u8>, Vec<u8>) {
    let sea = WATER_LEVEL as f32;
    let mut alt = kinds.to_vec();
    let mut margin = vec![255u8; kinds.len()];
    for i in 0..kinds.len() {
        let h = map.height[i];
        if h < sea {
            continue;
        }
        let warmth = (temperature[i] - (h - sea) * LAPSE_PER_BLOCK).max(0.0);
        if classify(warmth, moisture[i]) as u8 != kinds[i] {
            continue;
        }
        'probes: for (step, offset) in MARGIN_PROBES.into_iter().enumerate() {
            for (dw, dm) in [(offset, 0.0), (-offset, 0.0), (0.0, offset), (0.0, -offset)] {
                let other = classify((warmth + dw).max(0.0), (moisture[i] + dm).clamp(0.0, 1.0));
                if other as u8 != kinds[i] {
                    alt[i] = other as u8;
                    margin[i] = ((step + 1) * 255 / MARGIN_PROBES.len()) as u8;
                    break 'probes;
                }
            }
        }
    }
    // the climate margin alone spreads patches for hundreds of blocks where the climate changes slowly, so patches stop a
    // few cells from a boundary
    let dim = map.dim();
    for i in 0..kinds.len() {
        if margin[i] == 255 {
            continue;
        }
        let (cx, cz) = ((i % dim) as i64, (i / dim) as i64);
        let near = (1..=MARGIN_CELLS).find(|&d| {
            (-d..=d).any(|dz| {
                (-d..=d).any(|dx| {
                    let (x, z) = (cx + dx, cz + dz);
                    (dx.abs() == d || dz.abs() == d)
                        && (0..dim as i64).contains(&x)
                        && (0..dim as i64).contains(&z)
                        && kinds[z as usize * dim + x as usize] != kinds[i]
                })
            })
        });
        let by_distance = near.map_or(255, |d| ((d - 1) * 255 / MARGIN_CELLS) as u8);
        margin[i] = margin[i].max(by_distance);
    }
    (alt, margin)
}

// How strongly water near each cell wets and cools it, 0..1: a lake or a very wide river counts
// in full, a wide river by less, a narrow stream not at all, falling to 0 at RIPARIAN_REACH cells
// away and squared so the green hugs the bank.
fn fresh_water_closeness(map: &WorldMap) -> Vec<f32> {
    let tiers: [(f32, f32); 3] = [
        (RIPARIAN_WIDTHS.0, 0.45),
        (RIPARIAN_WIDTHS.1, 0.8),
        (RIPARIAN_WIDTHS.2, 1.0),
    ];
    let mut out = vec![0.0f32; map.height.len()];
    for (index, (width, strength)) in tiers.into_iter().enumerate() {
        let near = closeness(map, |i| {
            let river = map.water_kind[i] == WaterCell::River as u8 && map.river_width[i] >= width;
            // lakes are the top tier
            river || (index == tiers.len() - 1 && map.water_kind[i] == WaterCell::Lake as u8)
        });
        for (o, n) in out.iter_mut().zip(near) {
            *o = o.max(n * strength);
        }
    }
    out
}

// 1 on the cells `source` picks, falling to 0 at RIPARIAN_REACH cells from them, squared.
fn closeness(map: &WorldMap, source: impl Fn(usize) -> bool) -> Vec<f32> {
    let dim = map.dim();
    let reach = RIPARIAN_REACH;
    let mut dist = vec![u8::MAX; dim * dim];
    let mut frontier: Vec<usize> = Vec::new();
    for (i, d) in dist.iter_mut().enumerate() {
        if source(i) {
            *d = 0;
            frontier.push(i);
        }
    }
    let mut next = Vec::new();
    for d in 1..=reach {
        for &i in &frontier {
            let (x, z) = ((i % dim) as i64, (i / dim) as i64);
            for (dx, dz) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                let (nx, nz) = (x + dx, z + dz);
                if nx < 0 || nz < 0 || nx >= dim as i64 || nz >= dim as i64 {
                    continue;
                }
                let n = nz as usize * dim + nx as usize;
                if dist[n] == u8::MAX {
                    dist[n] = d;
                    next.push(n);
                }
            }
        }
        std::mem::swap(&mut frontier, &mut next);
        next.clear();
    }
    dist.iter()
        .map(|&d| {
            if d == u8::MAX {
                0.0
            } else {
                let t = 1.0 - d as f32 / (reach as f32 + 1.0);
                t * t
            }
        })
        .collect()
}

// A box blur of `radius` cells each way along both axes.
// The highest of `values` within `reach` cells of each cell, along each row and then each column.
pub(super) fn window_max(values: &[f32], dim: usize, reach: usize) -> Vec<f32> {
    let mut along_x = vec![f32::MIN; values.len()];
    par_rows(dim, &mut along_x, |row, cells| {
        for (col, cell) in cells.iter_mut().enumerate() {
            let (lo, hi) = (col.saturating_sub(reach), (col + reach).min(dim - 1));
            *cell = (lo..=hi)
                .map(|x| values[row * dim + x])
                .fold(f32::MIN, f32::max);
        }
    });
    // down each column a row at a time, so the reads run along rows
    let mut out = vec![f32::MIN; values.len()];
    par_rows(dim, &mut out, |row, cells| {
        for z in row.saturating_sub(reach)..=(row + reach).min(dim - 1) {
            for (cell, &v) in cells.iter_mut().zip(&along_x[z * dim..(z + 1) * dim]) {
                *cell = cell.max(v);
            }
        }
    });
    out
}

pub(super) fn blur_box(layer: &[f32], dim: usize, radius: usize) -> Vec<f32> {
    let along_x = {
        let mut out = vec![0.0f32; layer.len()];
        par_rows(dim, &mut out, |row, cells| {
            for (col, cell) in cells.iter_mut().enumerate() {
                let (lo, hi) = (col.saturating_sub(radius), (col + radius).min(dim - 1));
                let sum: f32 = (lo..=hi).map(|x| layer[row * dim + x]).sum();
                *cell = sum / (hi - lo + 1) as f32;
            }
        });
        out
    };
    let mut out = vec![0.0f32; layer.len()];
    par_rows(dim, &mut out, |row, cells| {
        let (lo, hi) = (row.saturating_sub(radius), (row + radius).min(dim - 1));
        for (col, cell) in cells.iter_mut().enumerate() {
            let sum: f32 = (lo..=hi).map(|z| along_x[z * dim + col]).sum();
            *cell = sum / (hi - lo + 1) as f32;
        }
    });
    out
}

// Biomes with ragged edges and specks tidied: each land cell takes the most common biome in the
// cells round it, then any patch left smaller than MIN_BIOME_CELLS is given to the biome it
// borders most. The sea is left alone.
pub(super) fn tidy_biomes(kinds: &[u8], dim: usize) -> Vec<u8> {
    let sea = BiomeKind::Sea as u8;
    let mut kinds = kinds.to_vec();
    for _ in 0..MODE_PASSES {
        let mut out = vec![0u8; kinds.len()];
        par_rows(dim, &mut out, |row, cells| {
            for (col, cell) in cells.iter_mut().enumerate() {
                let here = kinds[row * dim + col];
                if here == sea {
                    *cell = sea;
                    continue;
                }
                let mut counts = [0u16; BiomeKind::ALL.len()];
                for z in row.saturating_sub(MODE_RADIUS)..=(row + MODE_RADIUS).min(dim - 1) {
                    for x in col.saturating_sub(MODE_RADIUS)..=(col + MODE_RADIUS).min(dim - 1) {
                        counts[kinds[z * dim + x] as usize] += 1;
                    }
                }
                counts[sea as usize] = 0;
                // ties keep the cell's own biome
                let best = (0..counts.len())
                    .max_by_key(|&k| (counts[k], k as u8 == here))
                    .unwrap();
                *cell = best as u8;
            }
        });
        kinds = out;
    }
    // a patch of one biome too small to be worth its own: merged into the neighbour it shares
    // the longest edge with
    let mut small_passes = 0;
    loop {
        let mut changed = false;
        let mut comp = vec![u32::MAX; kinds.len()];
        let mut stack = Vec::new();
        for start in 0..kinds.len() {
            if comp[start] != u32::MAX || kinds[start] == sea {
                continue;
            }
            let id = start as u32;
            let mut cells = Vec::new();
            let mut border = [0u32; BiomeKind::ALL.len()];
            comp[start] = id;
            stack.push(start);
            while let Some(i) = stack.pop() {
                cells.push(i);
                let (x, z) = ((i % dim) as i64, (i / dim) as i64);
                for (dx, dz) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, nz) = (x + dx, z + dz);
                    if nx < 0 || nz < 0 || nx >= dim as i64 || nz >= dim as i64 {
                        continue;
                    }
                    let n = nz as usize * dim + nx as usize;
                    if kinds[n] == kinds[start] {
                        if comp[n] == u32::MAX {
                            comp[n] = id;
                            stack.push(n);
                        }
                    } else if kinds[n] != sea {
                        border[kinds[n] as usize] += 1;
                    }
                }
            }
            if cells.len() < MIN_BIOME_CELLS {
                if let Some(best) = (0..border.len())
                    .filter(|&k| border[k] > 0)
                    .max_by_key(|&k| border[k])
                {
                    for &i in &cells {
                        kinds[i] = best as u8;
                    }
                    changed = true;
                }
            }
        }
        small_passes += 1;
        if !changed || small_passes >= 3 {
            break;
        }
    }
    kinds
}

#[cfg(test)]
mod tests {
    use super::*;

    // climate reads the landform, which is laid first
    fn run(map: &mut WorldMap) {
        super::super::landform::run(map);
        super::run(map);
    }
    use crate::worldmap::WorldConfig;

    #[test]
    fn boundary_margins_are_untouched_on_sea_and_every_zone_cell_has_another_biome_across_it() {
        let config = WorldConfig {
            size: 65536,
            map_cell: 128,
            ..Default::default()
        };
        let map = super::super::generate(1, config);
        let sea = WATER_LEVEL as f32;
        let (mut zones, mut inside) = (0, 0);
        for i in 0..map.height.len() {
            if map.height[i] < sea {
                assert_eq!(255, map.biome_margin[i], "sea cell {i} has a margin");
            } else if map.biome_margin[i] < 255 {
                zones += 1;
                assert_ne!(
                    map.biome_alt[i], map.biome[i],
                    "zone cell {i} has no other biome"
                );
            } else {
                inside += 1;
            }
        }
        assert!(zones > 100, "only {zones} zone cells");
        assert!(
            inside > zones,
            "{inside} cells inside against {zones} in zones"
        );
    }

    // Sea in the west, then land with a ridge across it north to south, then flat land beyond.
    fn ridge_world() -> WorldMap {
        let config = WorldConfig {
            size: 256 * 32,
            map_cell: 32,
            ..Default::default()
        };
        let dim = config.dim();
        let mut map = WorldMap::empty(1, config);
        for z in 0..dim {
            for x in 0..dim {
                map.height[z * dim + x] = match x {
                    0..=39 => -68.0,
                    40..=79 => 4.0,
                    80..=119 => 4.0 + (x - 79) as f32 * 8.0,
                    120..=159 => 324.0 - (x - 119) as f32 * 8.0,
                    _ => 4.0,
                };
            }
        }
        map
    }

    #[test]
    fn test_rain_falls_on_the_windward_side_and_not_behind_the_range() {
        let mut map = ridge_world();
        run(&mut map);
        let dim = map.dim();
        let at = |x: usize| map.moisture[(dim / 2) * dim + x];
        let windward = (85..115).map(at).sum::<f32>() / 30.0;
        let shadow = (165..200).map(at).sum::<f32>() / 35.0;
        assert!(
            windward > shadow + 0.3,
            "windward {windward} against rain shadow {shadow}"
        );
    }

    #[test]
    fn regional_temperature_is_not_an_equator_to_pole_gradient() {
        let mut map = ridge_world();
        run(&mut map);
        let dim = map.dim();
        let north = map.temperature[4 * dim + 60];
        let centre = map.temperature[(dim / 2) * dim + 60];
        assert!((north - centre).abs() < 0.12);
        let sea_level = map.temperature[(dim / 2) * dim + 119];
        let surface = sea_level - map.height[(dim / 2) * dim + 119] * LAPSE_PER_BLOCK;
        assert!(surface < sea_level - 0.08);
    }

    #[test]
    fn reversing_the_wind_reverses_the_rain_shadow() {
        let mut west = ridge_world();
        west.config.climate.wind_z = 0.0;
        let mut east = west.clone();
        east.config.climate.wind_x = -1.0;
        let (_, _, wr) = climate_fields(&west);
        let (_, _, er) = climate_fields(&east);
        let row = west.dim() / 2;
        let average = |rain: &[f32], range: std::ops::Range<usize>| {
            let count = range.len() as f32;
            range.map(|x| rain[west.index(x, row)]).sum::<f32>() / count
        };
        assert!(average(&wr, 90..115) > average(&wr, 125..150));
        assert!(average(&er, 125..150) > average(&er, 90..115));
    }

    #[test]
    fn a_dry_region_does_not_receive_a_wet_biome_quota() {
        let mut map = WorldMap::empty(
            42,
            WorldConfig {
                size: 16384,
                ..Default::default()
            },
        );
        map.height.fill(50.0);
        let (_, moisture) = moisture_field(&map);
        assert!(moisture.iter().all(|&m| m < 0.6));
        let centre = map.index(map.dim() / 2, map.dim() / 2);
        assert!(moisture[centre] < 0.25);
    }

    #[test]
    fn wind_direction_not_vector_magnitude_controls_transport() {
        let a = ridge_world();
        let mut b = a.clone();
        b.config.climate.wind_x *= 2.0;
        b.config.climate.wind_z *= 2.0;
        let (_, ma) = moisture_field(&a);
        let (_, mb) = moisture_field(&b);
        assert!(ma.iter().zip(mb).all(|(a, b)| (a - b).abs() < 1e-5));
    }

    // A river along `row` over `x` cells, `width` blocks wide.
    fn with_river(width: f32, x: std::ops::Range<usize>) -> WorldMap {
        let mut map = ridge_world();
        let row = map.dim() / 2;
        for x in x {
            let i = map.index(x, row);
            map.water_kind[i] = WaterCell::River as u8;
            map.river_width[i] = width;
            map.water_surface[i] = 12.0;
        }
        map
    }

    #[test]
    fn test_a_big_river_wets_and_cools_the_dry_ground_round_it() {
        let mut map = with_river(70.0, 150..252);
        let (dim, row) = (map.dim(), map.dim() / 2);
        run(&mut map);
        let at = |x: usize, z: usize| map.moisture[z * dim + x];
        let t = |x: usize, z: usize| map.temperature[z * dim + x];
        assert_eq!(
            WaterCell::River as u8,
            map.water_kind[row * dim + 190],
            "a big river crosses the desert"
        );
        // beside the river, against the same dry plain well away from it
        assert!(
            at(190, row + 2) > at(190, row + 40) + 0.3,
            "{} vs {}",
            at(190, row + 2),
            at(190, row + 40)
        );
        // (warmth falls with latitude, so the dry ground to compare with is along the same row)
        assert!(t(190, row + 2) < t(125, row + 2) - 0.05);
        // and wet enough for trees where it would otherwise be steppe or desert
        let dry = BiomeKind::from_u8(map.biome[(row + 40) * dim + 190]);
        let bank = BiomeKind::from_u8(map.biome[(row + 2) * dim + 190]);
        assert_ne!(dry, bank);
        assert!(map.trees[(row + 2) * dim + 190] >= map.trees[(row + 40) * dim + 190]);
    }

    #[test]
    fn test_a_big_river_does_not_cool_cold_ground_into_snow() {
        let mut map = with_river(70.0, 150..252);
        map.config.climate.warmth = 0.34;
        let (dim, row) = (map.dim(), map.dim() / 2);
        run(&mut map);
        let t = |x: usize, z: usize| map.temperature[z * dim + x];
        // beside the river against the same row well away from it: no more than a hair colder
        assert!(
            t(190, row + 2) > t(190, row + 40) - 0.02,
            "{} vs {}",
            t(190, row + 2),
            t(190, row + 40)
        );
    }

    #[test]
    fn test_flat_low_wet_ground_by_a_big_river_is_swamp_and_nowhere_else_is() {
        let mut map = with_river(70.0, 150..252);
        let (dim, row) = (map.dim(), map.dim() / 2);
        run(&mut map);
        let kind = |x: usize, z: usize| BiomeKind::from_u8(map.biome[z * dim + x]);
        // the flat plain beside the river, low and wet
        let swamp_by_river = (170..230)
            .flat_map(|x| (row - 6..row + 6).map(move |z| (x, z)))
            .filter(|&(x, z)| kind(x, z) == BiomeKind::Swamp)
            .count();
        assert!(
            swamp_by_river > 100,
            "only {swamp_by_river} swamp cells beside the river"
        );
        // and every swamp cell is low, flat and near the water, never mountain
        let sea = WATER_LEVEL as f32;
        for i in 0..map.biome.len() {
            if BiomeKind::from_u8(map.biome[i]) == BiomeKind::Swamp {
                assert!(
                    map.height[i] - sea <= SWAMP_BELOW,
                    "swamp at {} up",
                    map.height[i]
                );
                assert_ne!(super::super::landform::MOUNTAIN, map.landform[i]);
                let (x, z) = (i % dim, i / dim);
                assert!(
                    (row as i64 - z as i64).abs() <= 16,
                    "swamp {} cells from the river",
                    row as i64 - z as i64
                );
                assert!(x >= 120, "swamp at column {x}, before the plain");
            }
        }
    }

    #[test]
    fn test_the_biome_layer_never_holds_the_mountain_scratch_kind() {
        let mut map = ridge_world();
        run(&mut map);
        assert!(map.landform.contains(&super::super::landform::MOUNTAIN));
        assert!(map.biome.iter().all(|&k| k != BiomeKind::Mountain as u8));
    }

    #[test]
    fn test_a_narrow_stream_leaves_the_biome_round_it_alone() {
        let (mut with, mut without) = (with_river(12.0, 90..120), ridge_world());
        let (dim, row) = (with.dim(), with.dim() / 2);
        run(&mut with);
        run(&mut without);
        for x in 95..115 {
            let i = (row + 3) * dim + x;
            assert_eq!(
                without.biome[i], with.biome[i],
                "biome changed by a stream at {x}"
            );
        }
    }

    #[test]
    fn test_mountain_slopes_have_a_conifer_belt_that_ends_at_the_treeline_and_the_air_cools_with_height(
    ) {
        use ochre_base::terrain_source::TerrainSource;
        let mut fixture = ridge_world();
        let dim = fixture.dim();
        for z in 0..dim {
            for x in 40..dim {
                let distance = (x as f32 - 125.0).abs();
                fixture.height[z * dim + x] = (1900.0 - distance * 28.0).max(4.0);
            }
        }
        run(&mut fixture);
        let map = std::sync::Arc::new(fixture);
        let sea = WATER_LEVEL as f32;
        let (mut lower, mut thick, mut high, mut bare) = (0, 0, 0, 0);
        for i in 0..map.biome.len() {
            if map.landform[i] != super::super::landform::MOUNTAIN || map.water_kind[i] != 0 {
                continue;
            }
            let above = map.height[i] - sea;
            if (150.0..500.0).contains(&above) {
                lower += 1;
                thick += (map.trees[i] >= 40) as usize;
            }
            if above > 1650.0 {
                high += 1;
                bare += (map.trees[i] <= 10) as usize;
            }
        }
        assert!(
            lower > 100 && thick * 2 > lower,
            "{thick} of {lower} lower slope cells forested"
        );
        assert!(
            high > 50 && bare == high,
            "{bare} of {high} cells above the treeline are bare"
        );
        // warmth falls with height at the rate snow lines climb, at one place and then a higher one
        let terrain = crate::worldmap::WorldTerrain::new(map.clone());
        let low = (0..map.height.len())
            .find(|&i| map.height[i] > sea && map.height[i] < sea + 20.0 && map.water_kind[i] == 0)
            .unwrap();
        let peak = (0..map.height.len())
            .find(|&i| map.height[i] > sea + 1000.0)
            .unwrap();
        let warmth = |i: usize| {
            let (x, z) = map.cell_centre(i % map.dim(), i / map.dim());
            (terrain.climate(x, z).temperature, terrain.temperature(x, z))
        };
        let (high_air, high_sea_level) = warmth(peak);
        let (low_air, low_sea_level) = warmth(low);
        assert!(
            (low_sea_level - low_air) < 0.03,
            "lowland cooled by {}",
            low_sea_level - low_air
        );
        assert!(
            high_sea_level - high_air > 0.25,
            "a peak cooled by only {}",
            high_sea_level - high_air
        );
    }

    #[test]
    fn test_biomes_have_no_specks() {
        let map = crate::worldmap::generate(
            3,
            WorldConfig {
                size: 16_384,
                map_cell: 32,
                ..Default::default()
            },
        );
        let dim = map.dim();
        let sea = BiomeKind::Sea as u8;
        let mut seen = vec![false; map.biome.len()];
        let mut small = 0;
        for start in 0..map.biome.len() {
            if seen[start] || map.biome[start] == sea {
                continue;
            }
            let (mut stack, mut cells, mut touches_other_land) = (vec![start], 0, false);
            seen[start] = true;
            while let Some(i) = stack.pop() {
                cells += 1;
                let (x, z) = (i % dim, i / dim);
                for (nx, nz) in [
                    (x.wrapping_sub(1), z),
                    (x + 1, z),
                    (x, z.wrapping_sub(1)),
                    (x, z + 1),
                ] {
                    if nx >= dim || nz >= dim {
                        continue;
                    }
                    let n = nz * dim + nx;
                    if map.biome[n] == map.biome[start] && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    } else if map.biome[n] != map.biome[start] && map.biome[n] != sea {
                        touches_other_land = true;
                    }
                }
            }
            // a patch with land beside it that is small enough to be a speck
            if touches_other_land && cells < MIN_BIOME_CELLS / 2 {
                small += 1;
            }
        }
        // one is let by: river belts through dry country can leave a sliver the tidy-up just misses
        assert!(small <= 1, "{small} specks of biome");
    }

    #[test]
    fn test_whittaker_table_covers_the_extremes() {
        assert_eq!(BiomeKind::Glacier, classify(0.0, 0.5));
        assert_eq!(BiomeKind::Tundra, classify(0.2, 0.1));
        assert_eq!(BiomeKind::Taiga, classify(0.2, 0.9));
        assert_eq!(BiomeKind::Steppe, classify(0.45, 0.1));
        assert_eq!(BiomeKind::Forest, classify(0.45, 0.9));
        assert_eq!(BiomeKind::Desert, classify(0.9, 0.05));
        assert_eq!(BiomeKind::Savanna, classify(0.9, 0.4));
        assert_eq!(BiomeKind::Jungle, classify(0.9, 0.95));
    }

    #[test]
    fn test_every_kind_has_a_biome_in_the_table() {
        let biomes = Biomes::new(1);
        for kind in BiomeKind::ALL {
            if let Some(name) = kind.name() {
                assert!(biomes.id(name).is_some(), "{name} missing");
            }
        }
    }

    // Sea in the west to column 40, land beyond, over a flat 256-cell world.
    fn coast_world(config: ClimateConfig) -> WorldMap {
        let world = WorldConfig {
            size: 256 * 32,
            map_cell: 32,
            climate: config,
            ..Default::default()
        };
        let dim = world.dim();
        let mut map = WorldMap::empty(1, world);
        for z in 0..dim {
            for x in 0..dim {
                let sea = x < 40;
                map.height[z * dim + x] = if sea { -68.0 } else { 4.0 };
                map.continentalness[z * dim + x] = if sea { -1.0 } else { 1.0 };
            }
        }
        map
    }

    fn row_mean(field: &[f32], dim: usize, xs: std::ops::Range<usize>) -> f32 {
        let n = xs.len() * dim;
        (0..dim)
            .flat_map(|z| xs.clone().map(move |x| (z, x)))
            .map(|(z, x)| field[z * dim + x])
            .sum::<f32>()
            / n as f32
    }

    #[test]
    fn test_the_default_config_has_the_old_temperature_and_treeline() {
        let map = coast_world(ClimateConfig::default());
        let dim = map.dim();
        let (temperature, _) = moisture_field(&map);
        for (x, z) in [(50, 3), (120, 90), (200, 200)] {
            let (wx, wz) = map.cell_centre(x, z);
            let old =
                (0.68 + fbm(map.seed ^ 0xC11A, wx, wz, 18000.0, 2) as f32 * 0.06).clamp(0.0, 1.0);
            assert_eq!(old, temperature[z * dim + x]);
        }
        assert_eq!((1100.0, 1650.0), treeline(ClimateConfig::default().warmth));
    }

    #[test]
    fn test_a_gradient_makes_one_side_warmer_along_its_angle_and_wanders() {
        let map = coast_world(ClimateConfig {
            gradient: 0.2,
            gradient_angle: 0.0,
            ..Default::default()
        });
        let dim = map.dim();
        let regional = regional_warmth(&map);
        let west = row_mean(&regional, dim, 0..64);
        let east = row_mean(&regional, dim, dim - 64..dim);
        assert!(east - west > 0.12, "east {east} west {west}");
        // the line where it crosses the baseline is not a straight north-south run
        let crossing: Vec<usize> = (0..dim)
            .map(|z| {
                (0..dim)
                    .find(|&x| regional[z * dim + x] >= 0.68)
                    .unwrap_or(dim)
            })
            .collect();
        let (lo, hi) = (
            crossing.iter().min().unwrap(),
            crossing.iter().max().unwrap(),
        );
        assert!(hi - lo >= 4, "crossing columns {lo} to {hi}");
    }

    #[test]
    fn test_continentality_warms_the_interior_against_the_coast() {
        let map = coast_world(ClimateConfig {
            continentality: 0.1,
            ..Default::default()
        });
        let dim = map.dim();
        let regional = regional_warmth(&map);
        let coast = row_mean(&regional, dim, 41..46);
        let inland = row_mean(&regional, dim, dim - 5..dim);
        assert!(inland - coast > 0.04, "coast {coast} inland {inland}");
        let cold = coast_world(ClimateConfig {
            continentality: -0.1,
            ..Default::default()
        });
        let regional = regional_warmth(&cold);
        assert!(row_mean(&regional, dim, dim - 5..dim) < row_mean(&regional, dim, 41..46));
    }

    #[test]
    fn test_treeline_follows_warmth_within_its_clamps() {
        let mut last = (0.0, 0.0);
        for step in 0..=100 {
            let line = treeline(step as f32 / 100.0);
            assert!(line.0 >= 50.0 && line.1 >= line.0 + 100.0, "{line:?}");
            assert!(line.0 >= last.0 && line.1 >= last.1);
            last = line;
        }
        assert_eq!((50.0, 150.0), treeline(0.1));
    }

    #[test]
    fn test_every_run_of_the_temperature_field_agrees() {
        let map = coast_world(ClimateConfig {
            gradient: 0.15,
            gradient_angle: 2.0,
            continentality: -0.05,
            ..Default::default()
        });
        assert_eq!(moisture_field(&map).0, moisture_field(&map).0);
    }
}
