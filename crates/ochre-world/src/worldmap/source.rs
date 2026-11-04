use std::sync::Arc;

use super::climate::{BiomeKind, LAPSE_PER_BLOCK};
use super::detail::{detail, dunes, foothill_weight, gully_relief, Dunes, FOOTHILL_RELIEF};
use super::water::{freeboard, RiverHit, RiverProfiles};
use super::FeatureKind;
use super::{WaterCell, WorldMap};
use ochre_base::biome::{BiomeId, Biomes, Climate, Modifier};
use ochre_base::features::{offset, ReliefCtx, ReliefKind, MAX_OFFSET};
use ochre_base::math::smoothstep_f64 as smoothstep;
use ochre_base::noise::{fbm_filtered, perlin};
use ochre_base::terrain_source::{RockColumn, SpringPart, TerrainSample, TerrainSource, WaterKind};
use ochre_base::WATER_LEVEL;

// How far a biome's cover is wobbled, so its edge wanders off the lines between the cells.
const BIOME_WOBBLE: f64 = 0.15;

// Land inland of the sea and out of any lake is at least this much over the sea, or it would flood.
const LAND_ABOVE_SEA: f64 = 0.3;

// Over the sea by up to this much the ground's detail is eased out, so a flat shore stays flat.
const SHORE_SMOOTH: f64 = 5.0;

// How far either side of still water's level, in blocks, the ground is held to the side of it its
// smooth ground is on, see `contain`, and over how far the ground's detail is calmed to nothing at
// the water's edge, so a shore and the bed out from it are smooth.
pub(super) const HOLD: f64 = 1.5;
const CALM: f64 = 8.0;
// Tableland weighs in between these heights above the sea (up from the first pair, out by the second), in patches this many blocks across.
// Beds step ground as a cell this wide sees it, see `landform_ground`.
const TERRACE_CELL: f64 = 32.0;
// Cells this wide or wider take their own biome, not a patchwork near a class boundary, whose patches are about `ZONE_PATCH` blocks.
const ZONE_CELL: f64 = 16.0;
const ZONE_PATCH: f64 = 12.0;
// Relief features need ground this high over the sea.
const RELIEF_ABOVE_SEA: f64 = 6.0;
// A kopje's heap shows bare rock where it stands this many blocks over the plain.
const KOPJE_ROCK_ABOVE: f64 = 4.0;
// Groves and clearings: trees keep this share of their density in a clearing, and grow in patches about `GROVE_PATCH` blocks across.
const CLEARING_KEEPS: f64 = 0.4;
const GROVE_PATCH: f64 = 160.0;
// How far, in blocks, and over what wavelength, the climate biome's cell is drifted at a point.
const DRIFT: f64 = 160.0;
const DRIFT_WAVELENGTH: f64 = 700.0;
// Land this many blocks over the sea on a volcanic province has the volcanic modifier.
const VOLCANIC_FROM: f64 = 120.0;

// A lake's hold on the ground round it, in blocks from its cells' centres: full out to `LAKE_NEAR`, then easing
// to nothing over `LAKE_FADE`. Measured, not counted in cells, so the ground's detail comes back gradually
// and a gully is never cut off at a cell's edge.
const LAKE_NEAR: f64 = 32.0;
const LAKE_FADE: f64 = 96.0;

// How far over still water's level, in blocks, ground surrounded by its cells is still under it: the water
// is drawn a whole block, under the level its cells were found by, so ground just over that would stand
// out of it as strips, parting it into pools.
const DROWN: f64 = 2.0;

// A river within this many blocks of still water's level is joined to it, as one running into or out of
// it is: their waters meet and the still water's is the one inside its shore.
const JOIN: f64 = 1.5;

// A river's valley floor is never lower than this over the sea or a lake near it, which would flood it in
// a wider inlet than the river.
const VALLEY_ABOVE_SEA: f64 = 0.6;

// How far past a river's banks, in blocks, its valley floor eases back out into the land's own ground.
const VALLEY_WIDTH: f64 = 40.0;

// Wider cells resolve only rivers at least a cell wide; finer cells retain every stream.
const RIVER_DETAIL_CELL: f64 = 16.0;
// A river rises in a round pool this many blocks across its middle, deepening from the first to the second
// under its level, in a rim of ground this wide held over it.
const SPRING_POOL: f32 = 2.6;
const SPRING_DEPTH: (f64, f64) = (0.8, 2.2);
const SPRING_RIM: f32 = 1.6;
// Past the rim the ground falls away from it a block for each block out, over this far, so it stands in a mound.
const SPRING_MOUND: f32 = 4.0;

// Tableland weight over which a river runs in a gorge: its banks widen up to 4 times and rise as a wall, not a slope.
const GORGE_FROM: f64 = 0.2;

// How much of the map's tree density a point keeps, `CLEARING_KEEPS` to 1: patches of grove and clearing, filtered for the
// sample's cell so wide cells see their average, never above 1 so `tree_bound` still bounds the density.
fn grove(seed: u32, x: f64, z: f64, cell: f64) -> f64 {
    let n = fbm_filtered(seed ^ 0x670E, x, z, GROVE_PATCH, 2, cell);
    CLEARING_KEEPS + (1.0 - CLEARING_KEEPS) * smoothstep(-0.2, 0.3, n)
}

// The ground over a river's channel: a parabolic bed up to the water's edge, then the bank
// easing out into the ground beside it. In a gorge (`gorge` is the tableland weight there) the bank is a wide
// floor and then a steep wall.
fn channel(ground: f32, river: RiverHit, gorge: f64) -> f32 {
    let half = river.width / 2.0;
    if river.distance < half {
        return ground.min(bed_height(river));
    }
    // the ground at the water's edge is a block over it, whatever the land is there, and eases to
    // the land's own height across the banks: cut down where it's higher, and built up a little
    // where a dip would leave the water standing over it, so the water is always held in by ground
    // and over the block the water's surface lies in by at least half of it, or the water stands over the bank
    let rim = (river.surface + freeboard(river.width)).max(river.surface.ceil() - 0.4);
    let t = ((river.distance - half) / bank_width(river, gorge)).clamp(0.0, 1.0);
    // The bank joins the surrounding terrain over its width. Block-level containment
    // is applied separately against actual neighbouring water, after all water is known.
    let rise = if gorge > GORGE_FROM {
        t * t * t
    } else {
        t * (2.0 - t)
    };
    rim + (ground - rim) * rise
}

// Where the bed of the river under it is, a parabola across it.
fn bed_height(river: RiverHit) -> f32 {
    let t = (river.distance / (river.width / 2.0)).min(1.0);
    river.bed + (river.surface - river.bed) * t * t
}

fn bank_width(river: RiverHit, gorge: f64) -> f32 {
    let width = 2.0 + river.width * 0.25;
    if gorge > GORGE_FROM {
        width * (1.0 + 3.0 * gorge as f32)
    } else {
        width
    }
}

// The sea or a lake, standing level in its basin.
#[derive(Debug, Copy, Clone)]
struct Still {
    level: f64,
    kind: WaterKind,
    // Influence of this water on nearby ground, 0..1, fading across map cells.
    near: f64,
    // how much of the four cells round a point are this water's, 0..1, smooth across cells
    cover: f64,
}

impl Still {
    // Whether a point whose smooth ground is `base` is under this water: under its level, or up to
    // `DROWN` over it where all four cells round it are the water's, which no rim can be among.
    fn under(&self, base: f64) -> bool {
        base < self.level + DROWN * smoothstep(0.5, 1.0, self.cover)
    }

    fn is(&self, other: &Still) -> bool {
        self.level == other.level && self.kind == other.kind
    }
}

// `ground` held over still water `w` where the map's smooth ground `base` is over its level, by as much
// as `near` the water the point is, see `contain`.
fn hold_over(ground: f64, base: f64, w: Still) -> f64 {
    if base < w.level {
        return ground;
    }
    let held = contain(ground, base, w.level, false);
    ground + (held - ground) * w.near
}

// `ground` held to still water at `level`, by which side of it the map's smooth ground `base` is on:
// `under` it, the ground stays under the water by as much as the smooth ground is, up to `HOLD`, and over
// it, over the water by as much. So the detail laid over the ground never crosses the water's edge, which
// is where the smooth ground meets the level: no island pokes through and no dry pit sinks under it.
fn contain(ground: f64, base: f64, level: f64, under: bool) -> f64 {
    let t = (base - level).abs().min(HOLD);
    if under {
        // at least half a block under, so it rounds to the block under the water's
        ground.min(level - t.max(0.55))
    } else {
        ground.max(level + t)
    }
}

/// How far from its line a river `width` blocks across shapes the ground: its water, banks and valley.
pub(super) fn river_reach(width: f32) -> f32 {
    width * 0.75 + 2.0 + VALLEY_WIDTH as f32
}

// The most `meander` moves a point, in blocks.
const MEANDER_REACH: f32 = 34.0 + 9.0;
// A one-block step, including the maximum local movement of both meander components.
// The Perlin gradient and fade bounds put it below 4.1 blocks; five is a conservative bound.
const NEIGHBOUR_MEANDER_REACH: f32 = 5.0;

// How far a river's line is moved off its straight run between cell centres at a point: slow
// bends a few hundred blocks long, with smaller wiggles on them.
fn meander(seed: u32, x: f64, z: f64) -> (f64, f64) {
    let bend = |salt: u32| {
        perlin(seed ^ salt, x / 260.0, z / 260.0) * 34.0
            + perlin(seed ^ salt ^ 0x55, x / 80.0, z / 80.0) * 9.0
    };
    (bend(0x3EA1), bend(0x3EA2))
}

/// Terrain read from a `WorldMap`, with detail laid over its ground.
pub struct WorldTerrain {
    map: Arc<WorldMap>,
    biomes: Biomes,
    // the biome each `BiomeKind` is, indexed by kind, and the same under the mountain and volcanic modifiers
    kinds: Vec<BiomeId>,
    mountain_kinds: Vec<BiomeId>,
    volcanic_kinds: Vec<BiomeId>,
    // the way each river cell's water leaves it, see `WorldMap::river_tangents`
    tangents: Vec<[f32; 2]>,
    // each river stretch's water level every few blocks along it, under the ground it runs over
    profiles: RiverProfiles,
    // by map cell, where a river rises in it: the spring's middle and level
    springs: Vec<Option<(f64, f64, f32)>>,
    // by map cell, whether any sea or lake cell is in the window `still_near` scans from it
    near_still: Vec<bool>,
    // by map cell, the distance in blocks to the nearest lake cell's centre (capped past its reach) and that lake's level
    lake_distance: Vec<f32>,
    lake_level: Vec<f32>,
    // what share of the sedimentary lowland this seed makes tableland, see `tableland`
    tableland_share: f64,
    // how this world's dunes are laid out, see `Dunes`
    dunes: Dunes,
    // how much longer than usual cliffs reach on mountain and tableland, see `cliff_boost`
    cliff_boost: f64,
}

impl WorldTerrain {
    pub fn new(map: Arc<WorldMap>) -> Self {
        let biomes = Biomes::new(map.seed);
        let kinds: Vec<BiomeId> = BiomeKind::ALL
            .iter()
            .map(|k| {
                k.name().map_or(BiomeId::BASE, |name| {
                    biomes.id(name).expect("biome in table")
                })
            })
            .collect();
        let under = |modifier| -> Vec<BiomeId> {
            kinds
                .iter()
                .map(|&kind| biomes.with_modifier(kind, modifier))
                .collect()
        };
        let (mountain_kinds, volcanic_kinds) =
            (under(Modifier::Mountain), under(Modifier::Volcanic));
        let tangents = map.river_tangents();
        let knobs = super::character::LandformKnobs::from_seed(map.seed);
        let (wx, wz) = (
            map.config.climate.wind_x as f64,
            map.config.climate.wind_z as f64,
        );
        let wind_length = (wx * wx + wz * wz).sqrt();
        let dunes = Dunes {
            scale: knobs.dune_scale,
            wind: (wx / wind_length, wz / wind_length),
        };
        let mut terrain = Self {
            map,
            biomes,
            kinds,
            mountain_kinds,
            volcanic_kinds,
            tangents,
            profiles: RiverProfiles::default(),
            springs: Vec::new(),
            near_still: Vec::new(),
            lake_distance: Vec::new(),
            lake_level: Vec::new(),
            tableland_share: knobs.tableland_share,
            dunes,
            cliff_boost: knobs.cliff_boost,
        };
        terrain.near_still = terrain.near_still();
        (terrain.lake_distance, terrain.lake_level) = terrain.lake_field();
        terrain.profiles = terrain.river_profiles();
        // looked up from the point before it's meandered, as well as after
        terrain.profiles.index(&terrain.map, MEANDER_REACH as f64);
        terrain.springs = terrain.springs();
        terrain
    }

    // Whether each cell has sea or lake in the 4x4 window `still_near` reaches from it (cells 1 back to 2 on),
    // so open land skips the scan. Separable max, clamped as the scan is.
    fn near_still(&self) -> Vec<bool> {
        let map = &self.map;
        let n = map.dim();
        let wet: Vec<bool> = map
            .water_kind
            .iter()
            .map(|&k| matches!(WaterCell::from_u8(k), WaterCell::Ocean | WaterCell::Lake))
            .collect();
        let spread =
            |src: &[bool], along_x: bool| -> Vec<bool> {
                let at = |x: usize, z: usize| src[map.index(x, z)];
                let mut out = vec![false; n * n];
                for z in 0..n {
                    for x in 0..n {
                        let c = if along_x { x } else { z };
                        out[map.index(x, z)] = (c.saturating_sub(1)..=(c + 2).min(n - 1))
                            .any(|k| if along_x { at(k, z) } else { at(x, k) });
                    }
                }
                out
            };
        spread(&spread(&wet, true), false)
    }

    // Where each river rises: a river cell no river drains into and no lake spills into.
    fn springs(&self) -> Vec<Option<(f64, f64, f32)>> {
        let map = &self.map;
        let dim = map.dim();
        (0..dim * dim)
            .map(|i| {
                if map.river_width[i] <= 0.0 {
                    return None;
                }
                let (cx, cz) = (i % dim, i / dim);
                let fed = map
                    .feeders(cx, cz)
                    .any(|(ux, uz)| map.river_width[map.index(ux, uz)] > 0.0);
                if fed || map.outlet_lake(cx, cz).is_some() {
                    return None;
                }
                let (x, z) = map.cell_centre(cx, cz);
                let surface = map.river_at(&self.profiles, x, z)?.surface;
                Some((x, z, surface))
            })
            .collect()
    }

    // How far the nearest spring is from (`x`, `z`), in the meandered space rivers are laid in, and its level.
    fn spring_near(&self, x: f64, z: f64) -> Option<(f32, f32)> {
        let map = &self.map;
        let (u, v) = map.to_cell(x, z);
        let max = map.dim() as i64 - 1;
        let (u, v) = (u.round() as i64, v.round() as i64);
        (-1..=1)
            .flat_map(|dz| (-1..=1).map(move |dx| (u + dx, v + dz)))
            .filter(|&(cx, cz)| (0..=max).contains(&cx) && (0..=max).contains(&cz))
            .filter_map(|(cx, cz)| self.springs[map.index(cx as usize, cz as usize)])
            .map(|(sx, sz, surface)| {
                (
                    (((x - sx).powi(2) + (z - sz).powi(2)).sqrt()) as f32,
                    surface,
                )
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }

    // Each river's water level every few blocks along it, under the map's smooth ground, which is
    // the floor of its valley and graded as rivers are: with the river drawn off its map line by the
    // meander.
    fn river_profiles(&self) -> RiverProfiles {
        let seed = self.map.seed;
        self.map.build_river_profiles(
            &self.tangents,
            &|x, z| self.valley_floor(x, z, self.map.height_at(x, z) as f64),
            &|x, z| {
                let (mx, mz) = meander(seed, x, z);
                let (mx, mz) = meander(seed, x - mx, z - mz);
                (x - mx, z - mz)
            },
            &|x, z| {
                let base = self.map.height_at(x, z) as f64;
                self.still_round(x, z)
                    .into_iter()
                    .flatten()
                    .any(|w| w.under(base))
            },
        )
    }

    /// Where the rivers' water falls, as (x, z, drop, level above the fall).
    pub fn falls(&self) -> Vec<(f64, f64, f32, f32)> {
        let seed = self.map.seed;
        self.profiles
            .falls()
            .into_iter()
            .map(|(x, z, drop, level)| {
                let (mx, mz) = meander(seed, x, z);
                let (mx, mz) = meander(seed, x - mx, z - mz);
                (x - mx, z - mz, drop, level)
            })
            .collect()
    }

    // The still water with cells within `reach` cells of a point, `reach` 1 being the four round it: the
    // sea first, then up to three lakes, each at its level, a whole block.
    fn still_near(&self, x: f64, z: f64, reach: i64) -> [Option<Still>; 4] {
        let map = &self.map;
        let (u, v) = map.to_cell(x, z);
        let (cx, cz) = (u.floor() as i64, v.floor() as i64);
        let (fx, fz) = (u - u.floor(), v - v.floor());
        let max = map.dim() as i64 - 1;
        if !self.near_still[map.index(cx.clamp(0, max) as usize, cz.clamp(0, max) as usize)] {
            return [None; 4];
        }
        let mut still: [Option<Still>; 4] = [None; 4];
        // which of the four cells round the point are within a cell of each water's cells
        let mut corners = [[false; 4]; 4];
        // and which are each water's own cells
        let mut owns = [[false; 4]; 4];
        for dz in 1 - reach..=reach {
            for dx in 1 - reach..=reach {
                let i = map.index(
                    (cx + dx).clamp(0, max) as usize,
                    (cz + dz).clamp(0, max) as usize,
                );
                let (slot, level, kind) = match WaterCell::from_u8(map.water_kind[i]) {
                    WaterCell::Ocean => (0, WATER_LEVEL as f64, WaterKind::Ocean),
                    WaterCell::Lake => {
                        let level = map.water_surface[i].floor() as f64;
                        let Some(slot) =
                            (1..4).find(|&k| still[k].is_none_or(|w: Still| w.level == level))
                        else {
                            continue;
                        };
                        (slot, level, WaterKind::Lake)
                    }
                    _ => continue,
                };
                still[slot] = Some(Still {
                    level,
                    kind,
                    near: 0.0,
                    cover: 0.0,
                });
                for (c, (a, b)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                    corners[slot][c] |= (dx - a).abs() <= 1 && (dz - b).abs() <= 1;
                    owns[slot][c] |= dx == a && dz == b;
                }
            }
        }
        let blend = |bits: [bool; 4]| {
            let at = |c: usize| if bits[c] { 1.0 } else { 0.0 };
            (at(0) * (1.0 - fx) + at(1) * fx) * (1.0 - fz) + (at(2) * (1.0 - fx) + at(3) * fx) * fz
        };
        for ((w, near), own) in still.iter_mut().zip(corners).zip(owns) {
            if let Some(w) = w {
                w.near = blend(near);
                w.cover = blend(own);
            }
        }
        still
    }

    // For each map cell, how far in blocks it is from the nearest lake cell's centre, and that lake's level:
    // a two-pass chamfer distance carrying the level along. Distances are capped past the lake's reach.
    fn lake_field(&self) -> (Vec<f32>, Vec<f32>) {
        let map = &self.map;
        let n = map.dim();
        let cell = map.config.map_cell as f32;
        let cap = (LAKE_NEAR + LAKE_FADE) as f32 + cell;
        let mut distance = vec![cap; n * n];
        let mut level = vec![0.0f32; n * n];
        for i in 0..n * n {
            if map.water_kind[i] == WaterCell::Lake as u8 {
                (distance[i], level[i]) = (0.0, map.water_surface[i].floor());
            }
        }
        let mut relax = |x: usize, z: usize, dx: i64, dz: i64| {
            let (nx, nz) = (x as i64 + dx, z as i64 + dz);
            if nx < 0 || nz < 0 || nx >= n as i64 || nz >= n as i64 {
                return;
            }
            let (i, j) = (map.index(x, z), map.index(nx as usize, nz as usize));
            let step = if dx != 0 && dz != 0 {
                cell * std::f32::consts::SQRT_2
            } else {
                cell
            };
            if distance[j] + step < distance[i] {
                (distance[i], level[i]) = (distance[j] + step, level[j]);
            }
        };
        for z in 0..n {
            for x in 0..n {
                for (dx, dz) in [(-1, 0), (0, -1), (-1, -1), (1, -1)] {
                    relax(x, z, dx, dz);
                }
            }
        }
        for z in (0..n).rev() {
            for x in (0..n).rev() {
                for (dx, dz) in [(1, 0), (0, 1), (1, 1), (-1, 1)] {
                    relax(x, z, dx, dz);
                }
            }
        }
        (distance, level)
    }

    // The nearest lake's hold on a point, if it is within reach: `near` falls smoothly with the distance in
    // blocks, from the field's four cells round the point, and the lake is the one nearest of them. Where
    // two lakes are within reach only the nearer holds the ground, as in the field.
    fn lake_near(&self, x: f64, z: f64) -> Option<Still> {
        let map = &self.map;
        let (u, v) = map.to_cell(x, z);
        let (cx, cz) = (u.floor() as i64, v.floor() as i64);
        let (fx, fz) = (u - u.floor(), v - v.floor());
        let max = map.dim() as i64 - 1;
        let cap = (LAKE_NEAR + LAKE_FADE) as f32 + map.config.map_cell as f32;
        let (mut distance, mut nearest) = (0.0, (f32::MAX, 0.0f32));
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let i = map.index(
                (cx + dx).clamp(0, max) as usize,
                (cz + dz).clamp(0, max) as usize,
            );
            let weight =
                (if dx == 0 { 1.0 - fx } else { fx }) * (if dz == 0 { 1.0 - fz } else { fz });
            distance += self.lake_distance[i] as f64 * weight;
            if self.lake_distance[i] < nearest.0 {
                nearest = (self.lake_distance[i], self.lake_level[i]);
            }
        }
        (nearest.0 < cap).then(|| Still {
            level: nearest.1 as f64,
            kind: WaterKind::Lake,
            near: 1.0 - smoothstep(LAKE_NEAR, LAKE_NEAR + LAKE_FADE, distance),
            cover: 0.0,
        })
    }

    // The still water a point can be in: that of the four cells round it. Two lakes' cells are never side
    // by side, as one basin's are joined, nor a lake's and the sea's. Every other cell beside a lake's or the
    // sea's is at least as high as its brim, or the water would have spilled there, so the water's edge
    // always falls between the four: where the smooth ground meets its level, never at a cell's side, and
    // never past a rim on ground falling away beyond it.
    fn still_round(&self, x: f64, z: f64) -> [Option<Still>; 4] {
        self.still_near(x, z, 1)
    }

    // The still water whose shore a point may be near: the sea within a cell more each way than `still_round`,
    // and the nearest lake by its distance, so the ground is held and calmed on past the farthest the water reaches.
    fn still_by(&self, x: f64, z: f64) -> [Option<Still>; 4] {
        let mut by = self.still_near(x, z, 2);
        by[1..].fill(None);
        by[1] = self.lake_near(x, z);
        by
    }

    // The ground at a point with no river or lake water on it: the landform ground, and dunes over it
    // where the climate is desert.
    fn ground_dry(&self, x: f64, z: f64, cell: f64, base: f64, calm: f64) -> f64 {
        let ground = self.landform_ground(x, z, cell, base, calm);
        ground + self.dune_height(x, z, cell, base, calm)
    }

    // How high the dunes rise over the ground at a point: only on desert, calmed at the shore.
    fn dune_height(&self, x: f64, z: f64, cell: f64, base: f64, calm: f64) -> f64 {
        let desert = self.kind_cover(BiomeKind::Desert, x, z);
        if desert <= 0.0 {
            return 0.0;
        }
        let sea = WATER_LEVEL as f64;
        let shore = smoothstep(0.0, SHORE_SMOOTH, base - sea) * calm;
        dunes(self.map.seed ^ 0xD05E, x, z, cell, base - sea, self.dunes) * desert * shore
    }

    // The map's smooth height with the detail laid over it, and terraced, calmed by `calm`, 0..1. It
    // reads the map's height, range, roughness and landform alone, never climate or biome: this is the
    // ground water is routed over, see `landform`.
    fn landform_ground(&self, x: f64, z: f64, cell: f64, base: f64, calm: f64) -> f64 {
        let map = &self.map;
        let sea = WATER_LEVEL as f64;
        let shore = smoothstep(0.0, SHORE_SMOOTH, base - sea) * calm;
        let fine = self.detail_at(x, z, cell, base) * shore;
        let smooth = base + fine;
        let mountain = self.landform_cover(x, z);
        let weight = mountain.max(self.tableland(x, z, base - sea, mountain)) * calm;
        if weight <= 0.0 {
            return smooth;
        }
        // the beds step the ground by its broad shape: stepping the finest detail too made a dip over a
        // ledge's edge fall a whole bed, a narrow pit tens of blocks deep. The fine detail rides on top.
        let broad = self.detail_at(x, z, cell.max(TERRACE_CELL), base) * shore;
        let stepped = map.terraced(x, z, base + broad, base - sea) + (fine - broad);
        smooth + (stepped - smooth) * weight
    }

    // How much of the ground at a point is tableland, 0..1, see `landform::tableland`.
    fn tableland(&self, x: f64, z: f64, above_sea: f64, mountain: f64) -> f64 {
        super::landform::tableland(
            self.map.seed,
            self.tableland_share,
            x,
            z,
            above_sea,
            mountain,
        )
    }

    // `ground` held to the still water round a point, `base` being the map's smooth ground there, and
    // whether the point is in still water, with the water. `round` and `by` are `still_round` and `still_by` there.
    fn held(
        &self,
        base: f64,
        mut ground: f64,
        round: &[Option<Still>; 4],
        by: &[Option<Still>; 4],
    ) -> (f64, Option<Still>) {
        let mut wet = None;
        for &w in round.iter().flatten() {
            if w.under(base) {
                ground = contain(ground, base.min(w.level), w.level, true);
                wet = Some(w);
            }
        }
        for &w in by.iter().flatten() {
            if !wet.is_some_and(|v: Still| v.is(&w)) {
                ground = hold_over(ground, base, w);
            }
        }
        // inland of the sea the ground never sinks under it, or it would flood as a pond cut off from it
        if wet.is_none() {
            ground = ground.max(WATER_LEVEL as f64 + LAND_ABOVE_SEA);
        }
        (ground, wet)
    }

    // How much of the ground's detail is kept at a point, 0..1: none at still water's edge, `CALM`
    // blocks of smooth ground either side of its level.
    fn calm(base: f64, by: &[Option<Still>; 4]) -> f64 {
        by.iter()
            .flatten()
            .map(|w| 1.0 - w.near * (1.0 - smoothstep(0.0, CALM, (base - w.level).abs())))
            .fold(1.0, f64::min)
    }

    /// The ground at a point with no river or lake water on it.
    pub fn dry_ground(&self, x: f64, z: f64, cell: f64) -> f64 {
        let base = self.map.height_at(x, z) as f64;
        let (round, by) = (self.still_round(x, z), self.still_by(x, z));
        let ground = self.ground_dry(x, z, cell, base, Self::calm(base, &by));
        self.held(base, ground, &round, &by).0
    }

    // How much of the ground round a point is `kind`, 0..1: smooth across the map's cells, and
    // wobbled so a biome's edge wanders off the lines between them.
    fn kind_cover(&self, kind: BiomeKind, x: f64, z: f64) -> f64 {
        self.cover(|i| self.map.biome[i] == kind as u8, kind as u32, x, z)
    }

    // The same for mountain landform, wobbled as the mountain biome was before it.
    fn landform_cover(&self, x: f64, z: f64) -> f64 {
        let mountain = |i: usize| self.map.landform[i] == super::landform::MOUNTAIN;
        self.cover(mountain, BiomeKind::Mountain as u32, x, z)
    }

    fn cover(&self, is: impl Fn(usize) -> bool, salt: u32, x: f64, z: f64) -> f64 {
        let map = &self.map;
        let (u, v) = map.to_cell(x, z);
        let (cx, cz) = (u.floor() as i64, v.floor() as i64);
        let (fx, fz) = (u - u.floor(), v - v.floor());
        let max = map.dim() as i64 - 1;
        let mut cover = 0.0;
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let i = map.index(
                (cx + dx).clamp(0, max) as usize,
                (cz + dz).clamp(0, max) as usize,
            );
            if is(i) {
                cover +=
                    (if dx == 0 { 1.0 - fx } else { fx }) * (if dz == 0 { 1.0 - fz } else { fz });
            }
        }
        if cover <= 0.0 || cover >= 1.0 {
            return cover;
        }
        let wobble = perlin(map.seed ^ 0xB107 ^ salt, x / 60.0, z / 60.0) * BIOME_WOBBLE;
        smoothstep(0.25, 0.75, cover + wobble)
    }

    // How far the ground's own detail lifts or sinks it from the map's smooth `base` at a point, before
    // rivers and lakes calm it.
    fn detail_at(&self, x: f64, z: f64, cell: f64, base: f64) -> f64 {
        let map = &self.map;
        let seed = map.seed;
        let sea = WATER_LEVEL as f64;
        let roughness = map.bilinear_bytes(&map.roughness, x, z) as f64 / 255.0;
        let mut detail = detail(seed, x, z, cell, roughness, base - sea);
        // the ground round a range is weathered: gullies and sharp ridges cut into it, as the range's own faces are
        let mountain = self.landform_cover(x, z);
        let near_range = map.bilinear_bytes(&map.range, x, z) as f64 / 255.0;
        let foothills =
            (1.0 - mountain) * FOOTHILL_RELIEF * foothill_weight(near_range, base - sea);
        let rough = mountain + foothills;
        detail += gully_relief(
            seed,
            x,
            z,
            rough * smoothstep(40.0, 110.0, base - sea),
            cell,
        );
        detail
    }

    // The floor of a river's valley at a point, the map's smooth ground `base` but never under the sea
    // or a lake near it, which would otherwise flood the valley in a wider inlet than the river and
    // leave the river running in under its level.
    fn valley_floor(&self, x: f64, z: f64, base: f64) -> f64 {
        let floor = base.max(WATER_LEVEL as f64 + VALLEY_ABOVE_SEA);
        // a lake within a few dozen blocks counts, as a river's last cells run in toward it
        [
            (0.0, 0.0),
            (40.0, 0.0),
            (-40.0, 0.0),
            (0.0, 40.0),
            (0.0, -40.0),
        ]
        .into_iter()
        .filter_map(|(dx, dz)| self.still_round(x + dx, z + dz)[1])
        .fold(floor, |floor, lake| {
            floor.max(lake.level + VALLEY_ABOVE_SEA)
        })
    }

    // A point moved a little to one side by a wandering offset, to look up what the map holds by cell
    // without following the cells' squares.
    fn wandered(&self, x: f64, z: f64) -> (f64, f64) {
        let seed = self.map.seed;
        let wander = self.map.config.map_cell as f64 * 1.5;
        (
            x + perlin(seed ^ 0x51A7, x / 40.0, z / 40.0) * wander,
            z + perlin(seed ^ 0xB10E, x / 40.0, z / 40.0) * wander,
        )
    }

    /// What each of the relief features of the biome at a point adds to the ground there, ignoring water.
    pub fn relief_at(&self, x: f64, z: f64) -> Vec<(ReliefKind, f64)> {
        self.relief_offsets(self.biome_at(x, z, 1.0), x, z, 1.0)
    }

    fn relief_offsets(&self, biome: BiomeId, x: f64, z: f64, cell: f64) -> Vec<(ReliefKind, f64)> {
        let kinds = self.biomes.relief(biome);
        if kinds.is_empty() || cell >= 16.0 {
            return Vec::new();
        }
        let map = &self.map;
        let at = |dx: f64, dz: f64| map.height_at(x + dx, z + dz) as f64;
        let (gx, gz) = (
            (at(8.0, 0.0) - at(-8.0, 0.0)) / 16.0,
            (at(0.0, 8.0) - at(0.0, -8.0)) / 16.0,
        );
        let gradient = (gx * gx + gz * gz).sqrt();
        let downhill = if gradient > 1e-6 {
            (-gx / gradient, -gz / gradient)
        } else {
            (0.0, 0.0)
        };
        let ctx = ReliefCtx {
            gradient,
            downhill,
            ridge_angle: map.config.climate.gradient_angle as f64,
            wind: {
                let (wx, wz) = (
                    map.config.climate.wind_x as f64,
                    map.config.climate.wind_z as f64,
                );
                let length = (wx * wx + wz * wz).sqrt();
                (wx / length, wz / length)
            },
        };
        kinds
            .iter()
            .map(|&kind| (kind, offset(kind, map.seed, x, z, cell, &ctx)))
            .collect()
    }

    // The cell whose climate biome a point takes: the point drifted by a slow warp, so edges between climates
    // meander over hundreds of blocks without closing into islands, or `at` where that would leave the land.
    fn climate_cell(&self, x: f64, z: f64, at: usize) -> usize {
        let seed = self.map.seed;
        let (dx, dz) = (
            perlin(seed ^ 0x3E41, x / DRIFT_WAVELENGTH, z / DRIFT_WAVELENGTH),
            perlin(seed ^ 0x3E42, x / DRIFT_WAVELENGTH, z / DRIFT_WAVELENGTH),
        );
        let (u, v) = self.map.to_cell(x + dx * DRIFT, z + dz * DRIFT);
        let max = (self.map.dim() - 1) as f64;
        let j = self.map.index(
            u.round().clamp(0.0, max) as usize,
            v.round().clamp(0.0, max) as usize,
        );
        if self.map.height[j] < WATER_LEVEL as f32 || self.map.height[at] < WATER_LEVEL as f32 {
            at
        } else {
            j
        }
    }

    // The biome of the cell nearest a point, found a little to one side by a wandering offset so
    // the edges between biomes wander too, rather than following the cells' squares.
    fn biome_at(&self, x: f64, z: f64, cell: f64) -> BiomeId {
        let (x, z) = self.wandered(x, z);
        let (u, v) = self.map.to_cell(x, z);
        let max = (self.map.dim() - 1) as f64;
        let i = self.map.index(
            u.round().clamp(0.0, max) as usize,
            v.round().clamp(0.0, max) as usize,
        );
        let j = self.climate_cell(x, z, i);
        // the climate biome, with the ground's modifier on it: high ground on a volcanic province is volcanic
        // whatever the climate, bare basalt on its slopes, and mountain landform is mountain
        let mut kind = self.map.biome[j] as usize;
        // near a class boundary a block may take the biome across it, in patches that thin out with the margin
        let margin = self.map.biome_margin[j];
        if margin < 255 && cell < ZONE_CELL {
            let n = (fbm_filtered(self.map.seed ^ 0xB10D, x, z, ZONE_PATCH, 1, cell) + 1.0) / 2.0;
            if n < 0.5 * (1.0 - margin as f64 / 255.0) {
                kind = self.map.biome_alt[j] as usize;
            }
        }
        if self.map.geology[i] == super::geology::VOLCANIC
            && self.map.height_at(x, z) as f64 - WATER_LEVEL as f64 >= VOLCANIC_FROM
        {
            return self.volcanic_kinds[kind];
        }
        if self.map.landform[i] == super::landform::MOUNTAIN {
            return self.mountain_kinds[kind];
        }
        self.kinds[kind]
    }

    #[cfg(test)]
    pub(super) fn rivers(&self) -> (&[[f32; 2]], &RiverProfiles) {
        (&self.tangents, &self.profiles)
    }

    pub fn map(&self) -> &Arc<WorldMap> {
        &self.map
    }

    fn sample_uncontained(
        &self,
        x: f64,
        z: f64,
        cell: f64,
        check_neighbors: bool,
    ) -> (TerrainSample, bool) {
        let map = &self.map;
        let seed = map.seed;
        let biome = self.biome_at(x, z, cell);
        let base = map.height_at(x, z) as f64;
        // where rivers are laid, drawn off the map's lines by the meander, if any reach this cell
        let (u, v) = map.to_cell(x, z);
        let max = (map.dim() - 1) as f64;
        let here = map.index(
            u.round().clamp(0.0, max) as usize,
            v.round().clamp(0.0, max) as usize,
        );
        let meandered = (!self.profiles.near(here).is_empty()).then(|| {
            let (mx, mz) = meander(seed, x, z);
            (x + mx, z + mz)
        });
        // the nearest river and, where one joins another, the other too
        let rivers = meandered.map_or([None, None], |(mx, mz)| {
            let mut resolved = [None, None];
            let mut n = 0;
            for hit in map.river_hits(&self.profiles, mx, mz).into_iter().flatten() {
                if cell <= RIVER_DETAIL_CELL || hit.width as f64 >= cell {
                    resolved[n] = Some(hit);
                    n += 1;
                }
            }
            resolved
        });
        let spring = meandered
            .filter(|_| cell <= RIVER_DETAIL_CELL)
            .and_then(|(mx, mz)| self.spring_near(mx, mz))
            .filter(|&(distance, _)| distance < SPRING_POOL + SPRING_RIM + SPRING_MOUND);
        let spring_part = match spring {
            Some((distance, _)) if distance < SPRING_POOL => SpringPart::Pool,
            Some((distance, _)) if distance < SPRING_POOL + SPRING_RIM => SpringPart::Rim,
            _ => SpringPart::None,
        };
        let (still, by) = (self.still_round(x, z), self.still_by(x, z));
        let mut ground = self.ground_dry(x, z, cell, base, Self::calm(base, &by));
        // a river over tableland runs in a gorge
        let gorge = if rivers[0].is_some() {
            let mountain = self.landform_cover(x, z);
            self.tableland(x, z, base - WATER_LEVEL as f64, mountain)
        } else {
            0.0
        };
        // a river runs in a valley with a graded floor, the map's smooth ground, which it eases out
        // of into the land's detail and terraces across the valley's sides: so its water, held under
        // that floor, never needs a bank built up, whatever the ground's steps and hollows. Not under
        // still water, whose bed it would raise.
        let valley = rivers
            .iter()
            .flatten()
            .map(|r| {
                let edge = r.width as f64 / 2.0 + bank_width(*r, gorge) as f64;
                1.0 - smoothstep(edge, edge + VALLEY_WIDTH, r.distance as f64)
            })
            .fold(0.0, f64::max);
        let under_still = still.into_iter().flatten().any(|w| w.under(base));
        if !under_still && valley > 0.0 {
            ground += (self.valley_floor(x, z, base) - ground) * valley;
        }
        let (held, wet) = self.held(base, ground, &still, &by);
        ground = held;
        // a river is left out where it's in still water it runs into or out of, or that stands over it:
        // the still water's shore is the water's edge there. One over the still water it passes is cut
        // and banked as ever, its banks keeping the two apart.
        let shaped = |r: &RiverHit| wet.is_none_or(|w| r.surface as f64 > w.level + JOIN);
        // near still water it's joined to, a river builds no banks: the two meet at one level, and the
        // still water's shore holds both, where banks would stand between them as a strip of land
        let joined_by = |r: &RiverHit| {
            by.iter()
                .flatten()
                .any(|w| (r.surface as f64 - w.level).abs() <= JOIN)
        };
        if let Some(river) = rivers[0].filter(shaped) {
            ground = if joined_by(&river) {
                if river.distance < river.width / 2.0 {
                    ground.min(bed_height(river) as f64)
                } else {
                    ground
                }
            } else {
                channel(ground as f32, river, gorge) as f64
            };
        }
        // a river the nearest one meets still has its bed cut where its water is, so a stream is
        // not cut off short of the bank of the river it runs into, but not where it lies well under the
        // nearest's water, which its banks hold there, as another stretch of a river curving back by it does
        if let Some(joining) = rivers[1]
            .filter(|r| r.distance < r.width / 2.0)
            .filter(shaped)
        {
            let held_off = rivers[0].filter(shaped).is_some_and(|main| {
                (joining.surface + JOIN as f32) < main.surface
                    && main.distance - main.width / 2.0 < HOLD as f32
            });
            if !held_off {
                ground = ground.min(bed_height(joining) as f64);
            }
        }
        // and the other river's bank holds too, unless the two are joined: where one stretch of a river
        // bends past another, the nearer's banks would cut the farther's from under its water
        if let Some(other) = rivers[1].filter(shaped) {
            let out = other.distance - other.width / 2.0;
            // and only on dry ground: inside the nearest's water, as where a stream runs into a river,
            // the other's banks would stand up through it as a strip of land
            let joined_or_under = rivers[0].is_some_and(|main| {
                (main.surface - other.surface).abs() <= JOIN as f32
                    || main.distance < main.width / 2.0
            });
            if !joined_or_under && (0.0..HOLD as f32).contains(&out) {
                ground = ground.max((other.surface + out) as f64);
            }
        }
        // and no river's banks sink still water's shore under its level beside it, but in the river's own water
        if !rivers
            .iter()
            .flatten()
            .filter(|r| shaped(r))
            .any(|r| r.distance < r.width / 2.0)
        {
            for &w in by.iter().flatten() {
                if !wet.is_some_and(|v: Still| v.is(&w)) {
                    ground = hold_over(ground, base, w);
                }
            }
        }
        // a river rises in a bowl, held in a rim of ground over its level, but where the river runs out
        if let Some((distance, surface)) = spring {
            if spring_part == SpringPart::Pool {
                let t = (distance / SPRING_POOL) as f64;
                ground = ground.min(
                    surface as f64
                        - (SPRING_DEPTH.0 + (SPRING_DEPTH.1 - SPRING_DEPTH.0) * (1.0 - t * t)),
                );
            } else if !rivers[0].is_some_and(|r| r.distance < r.width / 2.0 + 1.0) {
                let out = (distance - SPRING_POOL - SPRING_RIM).max(0.0) as f64;
                ground = ground.max(surface.ceil() as f64 + 0.2 - out);
            }
        }
        let ground = ground.clamp(
            (map.config.bounds.min_y + 16) as f64,
            (map.config.bounds.max_y - 64) as f64,
        );
        // a biome's relief features, nowhere near water or the sea, so the water rule is untouched
        let mut rocky = false;
        let ground = if rivers[0].is_none()
            && spring.is_none()
            && by.iter().all(Option::is_none)
            && ground >= WATER_LEVEL as f64 + RELIEF_ABOVE_SEA
        {
            let offsets = self.relief_offsets(biome, x, z, cell);
            let sum = offsets.iter().map(|&(_, offset)| offset).sum::<f64>();
            rocky = offsets
                .iter()
                .any(|&(kind, offset)| kind == ReliefKind::Kopjes && offset >= KOPJE_ROCK_ABOVE);
            (ground + sum.clamp(-MAX_OFFSET, MAX_OFFSET)).max(WATER_LEVEL as f64 + LAND_ABOVE_SEA)
        } else {
            ground
        };

        // where two meet, the wider river's level is the water's
        let river_water = rivers
            .iter()
            .flatten()
            .filter(|r| shaped(r))
            .filter(|r| r.distance < r.width / 2.0 && ground < r.surface as f64)
            .max_by(|a, b| a.width.total_cmp(&b.width));
        // a river by still water it runs into never stands over the still water's whole block by much,
        // or it floods the shore beside it over that level
        let by_still = |surface: f32| {
            by.iter()
                .flatten()
                .find(|w| (w.level..w.level + 2.0).contains(&(surface as f64)))
                .map_or(surface, |w| w.level as f32)
        };
        let (water_surface, water_kind) =
            if let Some((_, surface)) = spring.filter(|_| spring_part == SpringPart::Pool) {
                (surface, WaterKind::River)
            } else if let Some(river) = river_water {
                (by_still(river.surface), WaterKind::River)
            } else if let Some(w) = wet.filter(|w| ground < w.level) {
                (w.level as f32, w.kind)
            } else {
                (0.0, WaterKind::None)
            };
        let wet = water_kind != WaterKind::None;
        // Skip the four neighbour samples if even the widest possible stretch cannot reach
        // an adjacent block. The profile index includes a cell and the full meander reach.
        let near_river = check_neighbors
            && !wet
            && cell <= 1.0
            && (spring.is_some()
                || meandered.is_some_and(|(mx, mz)| {
                    let margin = rivers[0].map(|r| r.distance - r.width / 2.0);
                    if margin.is_some_and(|d| d <= NEIGHBOUR_MEANDER_REACH) {
                        return true;
                    }
                    let (mu, mv) = map.to_cell(mx, mz);
                    let cell = map.index(
                        mu.round().clamp(0.0, max) as usize,
                        mv.round().clamp(0.0, max) as usize,
                    );
                    let (width, mouth) = self.profiles.neighbour_bounds(cell);
                    mouth || margin.is_some_and(|d| d <= NEIGHBOUR_MEANDER_REACH + width)
                }));
        let sample = TerrainSample {
            continentalness: map.bilinear(&map.continentalness, x, z) as f64,
            height: ground,
            biome,
            // nothing grows in water
            trees: if wet || spring_part != SpringPart::None {
                0.0
            } else {
                map.bilinear_bytes(&map.trees, x, z) as f64 / 255.0 * grove(seed, x, z, cell)
            },
            water_surface,
            water_kind,
            spring: spring_part,
            rocky,
        };
        (sample, near_river)
    }
}

impl TerrainSource for WorldTerrain {
    fn bounds(&self) -> ochre_base::WorldBounds {
        self.map.config.bounds
    }

    fn seed(&self) -> u32 {
        self.map.seed
    }

    fn biomes(&self) -> &Biomes {
        &self.biomes
    }

    fn sample_for_cell(&self, x: f64, z: f64, cell: f64) -> TerrainSample {
        let (mut sample, near_river) = self.sample_uncontained(x, z, cell, true);
        // A dry block beside water must reach that water's top block. Use uncontained
        // neighbour samples so this is one local operation, independent of sample order.
        if cell <= 1.0 && near_river && sample.water_kind == WaterKind::None {
            for (dx, dz) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                let (neighbour, _) = self.sample_uncontained(x + dx, z + dz, cell, false);
                if neighbour.water_kind != WaterKind::None {
                    let water_top = ochre_terrain::generate::region_store::water_block_top(
                        neighbour.water_surface,
                    );
                    sample.height = sample.height.max(water_top as f64 - 0.4);
                }
            }
        }
        sample
    }

    // How warm it is at the ground here, which falls with height at the rate snow lines climb at,
    // so trees and grass higher up are cooler, and pines grow on the slopes of a range.
    fn rock_column(&self, x: i64, z: i64) -> Box<dyn RockColumn + '_> {
        let (x, z) = self.wandered(x as f64, z as f64);
        Box::new(self.map.rock_column(x, z))
    }

    fn climate(&self, x: f64, z: f64) -> Climate {
        let above_sea = (self.map.height_at(x, z) - WATER_LEVEL as f32).max(0.0);
        // a biome reads lusher or drier than its climate, in grass and flora tint
        let (warmer, wetter) = self.biomes.climate_shift(self.biome_at(x, z, 1.0));
        Climate {
            temperature: (self.temperature(x, z) - above_sea * LAPSE_PER_BLOCK + warmer)
                .clamp(0.0, 1.0),
            humidity: (self.map.bilinear(&self.map.moisture, x, z) + wetter).clamp(0.0, 1.0),
        }
    }

    fn temperature(&self, x: f64, z: f64) -> f32 {
        self.map
            .bilinear(&self.map.temperature, x, z)
            .clamp(0.0, 1.0)
    }

    fn tree_bound(&self, x: f64, z: f64) -> f64 {
        self.map.bilinear_bytes(&self.map.trees, x, z) as f64 / 255.0
    }

    // The seed's cliff boost, where there is mountain or tableland to carry it: 0 on other ground.
    fn cliff_boost(&self, x: f64, z: f64) -> f32 {
        let mountain = self.landform_cover(x, z);
        let above_sea = self.map.height_at(x, z) as f64 - WATER_LEVEL as f64;
        (self.cliff_boost * mountain.max(self.tableland(x, z, above_sea, mountain))) as f32
    }

    // The spawn region nearest the world's middle.
    // The spawn region nearest the world's middle.
    fn find_spawn(&self) -> (i64, i64, f64) {
        let nearest = self
            .map
            .features
            .iter()
            .filter(|f| f.kind == FeatureKind::Spawn)
            .min_by_key(|f| (f.x as i64).abs() + (f.z as i64).abs());
        let (x, z) = nearest.map_or((0, 0), |f| (f.x as i64, f.z as i64));
        (x, z, self.height(x as f64, z as f64))
    }
}

#[cfg(test)]
mod sea_level_tests {
    use super::*;

    #[test]
    fn ocean_at_zero_is_wet_and_has_a_negative_bed() {
        let mut map = WorldMap::empty(
            42,
            super::super::WorldConfig {
                size: 1024,
                ..Default::default()
            },
        );
        map.height.fill(-100.0);
        map.continentalness.fill(-0.5);
        map.water_kind.fill(WaterCell::Ocean as u8);
        map.temperature.fill(0.68);
        map.trees.fill(255);
        let terrain = WorldTerrain::new(std::sync::Arc::new(map));
        let s = terrain.sample(0.0, 0.0);
        assert_eq!(s.water_kind, WaterKind::Ocean);
        assert_eq!(s.water_surface, 0.0);
        assert!(s.height < -50.0);
        assert_eq!(s.trees, 0.0);
        let water = terrain.water(0.0, 0.0);
        assert_eq!(water.kind, WaterKind::Ocean);
        assert!(water.depth > 50.0);
    }

    #[test]
    fn broad_rivers_remain_water_in_coarse_cells() {
        let mut map = WorldMap::empty(
            42,
            super::super::WorldConfig {
                size: 2048,
                ..Default::default()
            },
        );
        map.height.fill(100.0);
        map.temperature.fill(0.5);
        let dim = map.dim();
        for x in 0..dim {
            let i = map.index(x, dim / 2);
            map.height[i] = 96.0;
            map.water_kind[i] = WaterCell::River as u8;
            map.water_surface[i] = 100.0;
            map.river_width[i] = 300.0;
            map.flow[i] = 1000.0;
            map.down[i] = if x + 1 < dim {
                0
            } else {
                super::super::NO_FLOW
            };
        }
        let terrain = WorldTerrain::new(std::sync::Arc::new(map));
        for cell in [1.0, 32.0, 64.0, 128.0] {
            let s = terrain.sample_for_cell(0.0, 16.0, cell);
            assert_eq!(
                s.water_kind,
                WaterKind::River,
                "river disappeared at cell {cell}"
            );
            assert!(s.height < s.water_surface as f64);
        }
        assert_eq!(
            terrain.sample_for_cell(0.0, 16.0, 512.0).water_kind,
            WaterKind::None
        );
    }
}

#[cfg(test)]
mod river_bank_tests {
    use super::*;

    #[test]
    fn lake_hold_eases_with_distance_and_the_nearer_lake_wins() {
        let config = super::super::WorldConfig {
            size: 1024,
            ..Default::default()
        };
        let mut map = super::super::WorldMap::empty(1, config);
        for (x, level) in [(8, 10.0), (24, 40.0)] {
            let i = map.index(x, 16);
            map.water_kind[i] = WaterCell::Lake as u8;
            map.water_surface[i] = level;
        }
        let (x0, z) = map.cell_centre(8, 16);
        let terrain = WorldTerrain::new(Arc::new(map));
        let hold = |dx: f64| terrain.still_by(x0 + dx, z)[1];
        assert_eq!(hold(0.0).unwrap().near, 1.0);
        // full within `LAKE_NEAR`, then easing out without a step
        let mut last = 1.0;
        for d in (0..=128).step_by(4) {
            let near = hold(d as f64).map_or(0.0, |w| w.near);
            assert!(near <= last + 1e-9, "near rises at {d}: {last} to {near}");
            assert!(last - near < 0.12, "near steps at {d}: {last} to {near}");
            last = near;
        }
        assert!(last < 1e-6, "still held {last} at {} blocks", 128);
        // the second lake is sixteen cells on: past the midpoint it, not the first, is the one that holds
        assert_eq!(hold(100.0).map(|w| w.level), Some(10.0));
        assert_eq!(hold(16.0 * 32.0 - 100.0).map(|w| w.level), Some(40.0));
    }

    #[test]
    fn lake_banks_stay_over_the_water_for_eight_blocks_from_the_shore() {
        for seed in [1, 42, 300059728] {
            let terrain = super::super::test_terrain(seed);
            let map = &terrain.map;
            let lakes = (0..map.water_kind.len())
                .filter(|&i| map.water_kind[i] == WaterCell::Lake as u8)
                .step_by(7)
                .take(80);
            let mut checked = 0;
            for i in lakes {
                let (x0, z0) = map.cell_centre(i % map.dim(), i / map.dim());
                for (dx, dz) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    let at = |k: f64| terrain.sample(x0 + dx * k, z0 + dz * k);
                    let shore = (0..100)
                        .map(|k| k as f64 * 2.0)
                        .find(|&k| at(k).water_kind != WaterKind::Lake);
                    let Some(shore) = shore else { continue };
                    let level = at(shore - 2.0).water_surface as f64;
                    for k in 0..4 {
                        let bank = at(shore + k as f64 * 2.0);
                        if bank.water_kind != WaterKind::None {
                            break;
                        }
                        assert!(
                            bank.height >= level - 0.5,
                            "seed {seed}: bank {} under the lake's {level} at ({}, {})",
                            bank.height,
                            x0 + dx * (shore + k as f64 * 2.0),
                            z0 + dz * (shore + k as f64 * 2.0),
                        );
                        checked += 1;
                    }
                }
            }
            assert!(checked > 200, "seed {seed}: only {checked} bank samples");
        }
    }

    #[test]
    fn landform_ground_ignores_climate_and_biome() {
        let config = super::super::WorldConfig {
            size: 8192,
            ..Default::default()
        };
        let map = super::super::generate(5, config);
        let mut scrambled = map.clone();
        scrambled.biome.fill(BiomeKind::Desert as u8);
        scrambled.moisture.fill(0.0);
        scrambled.temperature.fill(0.0);
        scrambled.trees.fill(0);
        let (a, b) = (
            WorldTerrain::new(Arc::new(map)),
            WorldTerrain::new(Arc::new(scrambled)),
        );
        let mut moved = 0;
        for i in 0..400 {
            let (x, z) = (
                (i % 20) as f64 * 280.0 - 2800.0,
                (i / 20) as f64 * 280.0 - 2800.0,
            );
            let base = a.map.height_at(x, z) as f64;
            let ground = a.landform_ground(x, z, 1.0, base, 1.0);
            assert_eq!(
                ground,
                b.landform_ground(x, z, 1.0, base, 1.0),
                "at {x},{z}"
            );
            moved += ((ground - base).abs() > 0.5) as usize;
        }
        assert!(moved > 50, "only {moved} of 400 points carry any detail");
    }

    #[test]
    fn relief_features_come_only_from_their_biome_and_cover_some_ground() {
        let biomes = ochre_base::biome::Biomes::new(0);
        let config = super::super::WorldConfig {
            size: 65536,
            map_cell: 128,
            ..Default::default()
        };
        // seed 2 has cold country
        let terrain = WorldTerrain::new(Arc::new(super::super::generate(2, config)));
        let mut covered = 0;
        for i in 0..640 {
            for j in 0..640 {
                let (x, z) = (i as f64 * 100.0 - 32000.0, j as f64 * 100.0 - 32000.0);
                let allowed = biomes.relief(terrain.biome_at(x, z, 1.0));
                for (kind, offset) in terrain.relief_at(x, z) {
                    assert!(
                        allowed.contains(&kind),
                        "{kind:?} outside its biome at {x},{z}"
                    );
                    assert!(offset.abs() <= MAX_OFFSET);
                    covered += (offset != 0.0) as usize;
                }
            }
        }
        assert!(covered > 20, "only {covered} points carry a relief feature");
    }

    #[test]
    fn a_kopje_is_bare_rock_where_its_heap_stands_over_the_plain_and_nowhere_else() {
        // seed 1337 has savanna with kopjes round -700 -928, on the map a game builds
        let config =
            super::super::character::world_config(1337, super::super::WorldConfig::default());
        let terrain = WorldTerrain::new(Arc::new(super::super::generate(1337, config)));
        let (mut rocky, mut heaped) = (0, 0);
        for i in 0..400 {
            for j in 0..400 {
                let (x, z) = (-1000.0 + i as f64 * 2.0, -1300.0 + j as f64 * 2.0);
                let high = terrain.relief_at(x, z).iter().any(|&(kind, offset)| {
                    kind == ReliefKind::Kopjes && offset >= KOPJE_ROCK_ABOVE
                });
                let sample = terrain.sample(x, z);
                // the heap is not raised near water or the sea, so no rock there either
                assert!(!sample.rocky || high, "rock with no heap at {x},{z}");
                rocky += sample.rocky as usize;
                heaped += high as usize;
            }
        }
        assert!(heaped > 0, "no kopje heap round -700 -928");
        assert!(rocky > 0, "no bare rock on {heaped} heap columns");
    }

    #[test]
    fn groves_keep_between_the_clearing_share_and_all_of_the_trees_and_never_exceed_the_bound() {
        let mut map = super::super::WorldMap::empty(
            5,
            super::super::WorldConfig {
                size: 4096,
                ..Default::default()
            },
        );
        map.trees.fill(200);
        map.height.fill(60.0);
        let terrain = WorldTerrain::new(Arc::new(map));
        for cell in [1.0, 4.0, 16.0, 64.0, 512.0] {
            let (mut lowest, mut highest) = (f64::MAX, f64::MIN);
            for i in 0..100 {
                for j in 0..100 {
                    let (x, z) = (i as f64 * 37.0, j as f64 * 37.0);
                    let g = grove(5, x, z, cell);
                    assert!((CLEARING_KEEPS..=1.0).contains(&g), "grove {g} at {x},{z}");
                    assert!(terrain.sample_for_cell(x, z, cell).trees <= terrain.tree_bound(x, z));
                    (lowest, highest) = (lowest.min(g), highest.max(g));
                }
            }
            // wide cells see an average, so they vary less
            if cell == 1.0 {
                assert!(lowest < 0.5 && highest > 0.95, "{lowest} {highest}");
            }
            if cell == 512.0 {
                assert!(highest - lowest < 0.5, "{lowest} {highest}");
            }
        }
    }

    #[test]
    fn blocks_near_a_class_boundary_take_the_biome_across_it_and_wide_cells_do_not() {
        let config = super::super::WorldConfig {
            size: 65536,
            map_cell: 128,
            ..Default::default()
        };
        let terrain = WorldTerrain::new(Arc::new(super::super::generate(1, config)));
        let map = &terrain.map;
        let max = (map.dim() - 1) as f64;
        let (mut zones, mut mixed) = (0, 0);
        for k in 0..40_000u32 {
            let (x, z) = (
                (k % 200) as f64 * 320.0 - 32000.0,
                (k / 200) as f64 * 320.0 - 32000.0,
            );
            let (wx, wz) = terrain.wandered(x, z);
            let (u, v) = map.to_cell(wx, wz);
            let i = map.index(
                u.round().clamp(0.0, max) as usize,
                v.round().clamp(0.0, max) as usize,
            );
            let i = terrain.climate_cell(wx, wz, i);
            let (fine, wide) = (
                terrain.biome_at(x, z, 1.0),
                terrain.biome_at(x, z, ZONE_CELL),
            );
            assert_eq!(
                wide,
                terrain.biome_at(x, z, 64.0),
                "wide cells differ at {x},{z}"
            );
            assert_eq!(
                fine,
                terrain.biome_at(x, z, 1.0),
                "not deterministic at {x},{z}"
            );
            if map.biome_margin[i] == 255 {
                assert_eq!(
                    fine, wide,
                    "a cell well inside its biome takes another at {x},{z}"
                );
            } else {
                zones += 1;
                mixed += (fine != wide) as usize;
            }
        }
        assert!(zones > 200, "only {zones} points in a boundary zone");
        assert!(
            mixed > 20,
            "only {mixed} of {zones} zone points take the biome across"
        );
    }

    #[test]
    fn tableland_covers_about_its_share_of_the_eligible_lowland_and_only_there() {
        let config = super::super::WorldConfig {
            size: 65536,
            map_cell: 128,
            ..Default::default()
        };
        let mut terrain = WorldTerrain::new(Arc::new(super::super::generate(1, config)));
        let sea = WATER_LEVEL as f64;
        let points: Vec<(f64, f64, f64)> = (0..90)
            .flat_map(|i| {
                (0..90).map(move |j| (i as f64 * 700.0 - 31500.0, j as f64 * 700.0 - 31500.0))
            })
            .map(|(x, z)| (x, z, terrain.map.height_at(x, z) as f64 - sea))
            .filter(|&(x, z, above)| {
                (100.0..250.0).contains(&above) && terrain.landform_cover(x, z) == 0.0
            })
            .collect();
        assert!(points.len() > 200, "only {} eligible points", points.len());
        let mut coverage = |share: f64| {
            terrain.tableland_share = share;
            let on = points
                .iter()
                .filter(|&&(x, z, above)| terrain.tableland(x, z, above, 0.0) > 0.5)
                .count();
            on as f64 / points.len() as f64
        };
        let (few, many) = (coverage(0.2), coverage(0.8));
        assert!(few > 0.05 && few < 0.4, "share 0.2 covers {few}");
        assert!(many > 0.6 && many < 0.95, "share 0.8 covers {many}");
        // none under the window, over it, or where mountain already is
        assert_eq!(0.0, terrain.tableland(0.0, 0.0, 20.0, 0.0));
        assert_eq!(0.0, terrain.tableland(0.0, 0.0, 500.0, 0.0));
        assert_eq!(0.0, terrain.tableland(0.0, 0.0, 150.0, 1.0));
    }

    // PR 4a check, as measured not as hoped: fewer than 1 in 100 dune points over 2 blocks tall are within 30
    // blocks of river or lake water. The rest follow the biome's own wetting round wide water, see `climate`.
    #[test]
    fn almost_no_tall_dunes_are_within_30_blocks_of_river_or_lake_water() {
        for seed in [1u32, 42, 1337, 300059728] {
            let config = super::super::character::world_config(seed, Default::default());
            let terrain = WorldTerrain::new(Arc::new(super::super::generate(seed, config)));
            let (mut tall, mut near) = (0, 0);
            for i in 0..1400 {
                for j in 0..1400 {
                    let (x, z) = (i as f64 * 47.0 - 32800.0, j as f64 * 47.0 - 32800.0);
                    let base = terrain.map.height_at(x, z) as f64;
                    if terrain.dune_height(x, z, 1.0, base, 1.0) <= 2.0 {
                        continue;
                    }
                    tall += 1;
                    let wet = (0..12).any(|k| {
                        let a = k as f64 * std::f64::consts::TAU / 12.0;
                        [10.0, 20.0, 30.0].iter().any(|d| {
                            let s = terrain.sample(x + d * libm::cos(a), z + d * libm::sin(a));
                            matches!(s.water_kind, WaterKind::River | WaterKind::Lake)
                        })
                    });
                    near += wet as usize;
                }
            }
            eprintln!(
                "DUNECHECK seed {seed}: {tall} tall dune points, {near} within 30 blocks of water"
            );
            assert!(near * 100 <= tall, "seed {seed}: {near} of {tall}");
        }
    }

    #[test]
    fn the_cliff_boost_is_the_seeds_on_mountain_and_tableland_and_nothing_elsewhere() {
        use ochre_base::terrain_source::TerrainSource;
        let config = super::super::WorldConfig {
            size: 65536,
            map_cell: 128,
            ..Default::default()
        };
        let mut terrain = WorldTerrain::new(Arc::new(super::super::generate(1, config)));
        terrain.cliff_boost = 1.0;
        let (mut on, mut off, mut over) = (0, 0, 0);
        for i in 0..90 {
            for j in 0..90 {
                let (x, z) = (i as f64 * 700.0 - 31500.0, j as f64 * 700.0 - 31500.0);
                let boost = terrain.cliff_boost(x, z);
                let above = terrain.map.height_at(x, z) as f64 - WATER_LEVEL as f64;
                let mountain = terrain.landform_cover(x, z);
                let carries = mountain > 0.5 || terrain.tableland(x, z, above, mountain) > 0.5;
                over += (!(0.0..=1.0).contains(&boost)) as usize;
                if carries {
                    on += (boost > 0.5) as usize;
                } else if mountain == 0.0 && terrain.tableland(x, z, above, mountain) == 0.0 {
                    off += (boost != 0.0) as usize;
                }
            }
        }
        assert!(on > 100, "only {on} boosted points");
        assert_eq!((0, 0), (off, over));
        terrain.cliff_boost = 0.0;
        assert_eq!(0.0, terrain.cliff_boost(0.0, 0.0));
    }

    #[test]
    fn modifiers_sit_on_the_climate_biome_by_landform_and_province() {
        let config = super::super::WorldConfig {
            size: 65536,
            map_cell: 128,
            ..Default::default()
        };
        let terrain = WorldTerrain::new(Arc::new(super::super::generate(1337, config)));
        let biomes = terrain.biomes();
        let (mut volcanic, mut mountain, mut plain, mut wrong) = (0, 0, 0, 0);
        for i in 0..120 {
            for j in 0..120 {
                let (x, z) = (i as f64 * 540.0 - 32400.0, j as f64 * 540.0 - 32400.0);
                let (u, v) = terrain.map.to_cell(x, z);
                let cell = terrain.map.index(
                    u.round().clamp(0.0, 511.0) as usize,
                    v.round().clamp(0.0, 511.0) as usize,
                );
                let above = terrain.map.height_at(x, z) as f64 - WATER_LEVEL as f64;
                let modifier = biomes.modifier(terrain.biome_at(x, z, 1.0));
                let on_volcanic = terrain.map.geology[cell] == super::super::geology::VOLCANIC;
                if on_volcanic && above >= VOLCANIC_FROM + 20.0 {
                    volcanic += 1;
                    wrong += (modifier != Modifier::Volcanic) as usize;
                } else if above < VOLCANIC_FROM - 20.0 || !on_volcanic {
                    if terrain.map.landform[cell] == super::super::landform::MOUNTAIN {
                        mountain += 1;
                        wrong += (modifier != Modifier::Mountain) as usize;
                    } else {
                        plain += 1;
                        wrong += (modifier != Modifier::None) as usize;
                    }
                }
            }
        }
        assert!(
            volcanic > 30 && mountain > 100 && plain > 1000,
            "{volcanic} {mountain} {plain}"
        );
        // the wander moves the cell a little, so a point at the edge of a landform can fall either side
        assert!(
            wrong * 20 <= volcanic + mountain + plain,
            "{wrong} points have the wrong modifier"
        );
    }

    fn river_edge(terrain: &WorldTerrain, centre: (i64, i64)) -> (i64, i64, f32, f64) {
        for z in centre.1 - 96..=centre.1 + 96 {
            for x in centre.0 - 96..=centre.0 + 96 {
                let water = terrain.sample(x as f64, z as f64);
                if water.water_kind != WaterKind::River {
                    continue;
                }
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let bank = terrain.sample((x + dx) as f64, (z + dz) as f64);
                    if bank.water_kind == WaterKind::None {
                        return (x, z, water.water_surface, bank.height);
                    }
                }
            }
        }
        panic!("no river edge near {centre:?}");
    }

    #[test]
    fn neighbour_meander_search_bound_covers_block_steps() {
        for seed in [1, 42, 3942325500] {
            for x in (-32000..=32000).step_by(1799) {
                for z in (-32000..=32000).step_by(1811) {
                    let (x, z) = (x as f64, z as f64);
                    let (mx, mz) = meander(seed, x, z);
                    for (dx, dz) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                        let (nx, nz) = meander(seed, x + dx, z + dz);
                        let distance = (dx + nx - mx).hypot(dz + nz - mz);
                        assert!(
                            distance <= NEIGHBOUR_MEANDER_REACH as f64,
                            "seed {seed} at {x},{z}: step moved {distance}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn river_bank_joins_ground_beyond_its_width() {
        let mut river = RiverHit {
            distance: 1.0,
            width: 2.0,
            surface: 100.0,
            bed: 99.0,
        };
        assert!(channel(80.0, river, 0.0) >= 100.2);
        river.distance = 100.0;
        assert_eq!(channel(80.0, river, 0.0), 80.0);
    }

    #[test]
    fn a_gorge_widens_the_bank_to_a_floor_and_a_wall_and_only_cuts() {
        let mut river = RiverHit {
            distance: 0.0,
            width: 8.0,
            surface: 100.0,
            bed: 98.0,
        };
        // no tableland, or too little: today's bank, width and profile
        assert_eq!(bank_width(river, 0.0), bank_width(river, GORGE_FROM));
        assert_eq!(4.0, bank_width(river, 0.0));
        // full tableland: four times as wide
        assert_eq!(16.0, bank_width(river, 1.0));
        let ground = 160.0;
        let mut last = 0.0;
        for step in 0..=40 {
            river.distance = river.width / 2.0 + step as f32 * 0.5;
            let plain = channel(ground, river, 0.0);
            let gorge = channel(ground, river, 1.0);
            // the bank is cut down to the land, never above it, and over the water's rim
            assert!(
                gorge <= ground && gorge >= river.surface,
                "{gorge} at {step}"
            );
            // and it climbs, with a flat floor by the water and the steepest rise out at the wall
            assert!(gorge >= last);
            last = gorge;
            if step == 8 {
                assert!(
                    gorge < plain,
                    "the floor is lower than the plain bank: {gorge} {plain}"
                );
            }
        }
        river.distance = river.width / 2.0 + 15.0;
        let wall = channel(ground, river, 1.0)
            - channel(
                ground,
                RiverHit {
                    distance: river.distance - 1.0,
                    ..river
                },
                1.0,
            );
        river.distance = river.width / 2.0 + 2.0;
        let floor = channel(ground, river, 1.0)
            - channel(
                ground,
                RiverHit {
                    distance: river.distance - 1.0,
                    ..river
                },
                1.0,
            );
        assert!(wall > floor * 5.0, "wall {wall} against floor {floor}");
    }

    #[test]
    fn mountain_ground_far_from_a_river_keeps_its_own_height() {
        let map = Arc::new(super::super::generate(3942325500, Default::default()));
        let terrain = WorldTerrain::new(map);
        let (x, z) = (20700.0, -1920.0);
        let ground = terrain.sample(x, z);
        let dry = terrain.dry_ground(x, z, 1.0);
        assert_eq!(ground.water_kind, WaterKind::None);
        assert!(
            (ground.height - dry).abs() < 0.01,
            "distant ground {} was raised above dry terrain {dry}",
            ground.height
        );
        let (_, _, water, bank) = river_edge(&terrain, (18000, -6776));
        assert!(
            ochre_terrain::generate::region_store::bed_top(bank as f32, 0.0)
                >= ochre_terrain::generate::region_store::water_block_top(water),
            "bank {} does not hold neighboring water {}",
            bank,
            water
        );
        let mut wet_columns = 0;
        for z in (-6900..-6650).step_by(8) {
            for x in (17900..18100).step_by(8) {
                let water = terrain.sample(x as f64, z as f64);
                if water.water_kind != WaterKind::River {
                    continue;
                }
                wet_columns += 1;
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let bank = terrain.sample((x + dx) as f64, (z + dz) as f64);
                    if bank.water_kind == WaterKind::None {
                        assert!(
                            ochre_terrain::generate::region_store::bed_top(
                                bank.height as f32,
                                bank.water_surface
                            ) >= ochre_terrain::generate::region_store::water_block_top(
                                water.water_surface
                            ),
                            "water at {x},{z} is open toward {},{}",
                            x + dx,
                            z + dz
                        );
                    }
                }
            }
        }
        assert!(wet_columns > 0, "the sampled valley needs river coverage");
    }

    #[test]
    fn adjacent_river_water_and_bank_remain_contained() {
        let map = Arc::new(super::super::generate(1, Default::default()));
        let terrain = WorldTerrain::new(map);
        let (x, z, water, bank) = river_edge(&terrain, (8220, -5324));
        let up = terrain.sample(x as f64, z as f64);
        let down = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .map(|(dx, dz)| terrain.sample((x + dx) as f64, (z + dz) as f64))
            .find(|sample| sample.water_kind == WaterKind::River)
            .expect("river edge should have a wet neighbor");
        assert_eq!(up.water_kind, WaterKind::River);
        assert_eq!(down.water_kind, WaterKind::River);
        assert!(
            (up.water_surface - down.water_surface).abs() < 2.0,
            "adjacent river levels jump from {} to {}",
            up.water_surface,
            down.water_surface
        );
        for (dx, dz) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let neighbour = terrain.sample(x as f64 + dx, z as f64 + dz);
            if neighbour.water_kind == WaterKind::None {
                assert!(
                    up.water_surface as f64 - neighbour.height <= 1.5,
                    "water at {x},{z} stands over dry ground at {},{}",
                    x as f64 + dx,
                    z as f64 + dz
                );
            }
        }

        // The dry neighbour must reach the water's block, even when both rounded
        // heights are close enough that their continuous surfaces look contained.
        assert!(
            ochre_terrain::generate::region_store::bed_top(bank as f32, 0.0)
                >= ochre_terrain::generate::region_store::water_block_top(water),
            "bank {} does not hold water {}",
            bank,
            water
        );
    }

    #[test]
    fn river_edge_holds_water_on_seed_42() {
        let map = Arc::new(super::super::generate(42, Default::default()));
        let terrain = WorldTerrain::new(map);
        let (_, _, water, bank) = river_edge(&terrain, (12040, -24720));
        assert!(
            ochre_terrain::generate::region_store::bed_top(bank as f32, 0.0)
                >= ochre_terrain::generate::region_store::water_block_top(water),
            "bank {} does not hold water {}",
            bank,
            water
        );
    }

    #[test]
    fn lake_outlet_holds_its_river_water() {
        let map = Arc::new(super::super::generate(42, Default::default()));
        let terrain = WorldTerrain::new(map.clone());
        for i in 0..map.height.len() {
            if map.water_kind[i] != WaterCell::River as u8 {
                continue;
            }
            let (cx, cz) = (i % map.dim(), i / map.dim());
            if map.outlet_lake(cx, cz).is_none() {
                continue;
            }
            let (x, z) = map.cell_centre(cx, cz);
            for dz in -32..=32 {
                for dx in -32..=32 {
                    let (x, z) = (x + dx as f64, z + dz as f64);
                    let water = terrain.sample(x, z);
                    if water.water_kind != WaterKind::River {
                        continue;
                    }
                    for (nx, nz) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                        let bank = terrain.sample(x + nx, z + nz);
                        if bank.water_kind == WaterKind::None {
                            assert!(
                                ochre_terrain::generate::region_store::bed_top(
                                    bank.height as f32,
                                    bank.water_surface
                                ) >= ochre_terrain::generate::region_store::water_block_top(
                                    water.water_surface
                                )
                            );
                            return;
                        }
                    }
                }
            }
        }
        panic!("no river bank found at a lake outlet");
    }

    #[test]
    fn steep_bend_holds_water_on_its_outside_bank() {
        // Recreate the earlier seed-42 terrain; rock-dependent erosion moves this bend.
        let config = Default::default();
        let mut map = super::super::tectonics::generate(42, config);
        super::super::hydrology::run(&mut map, false);
        super::super::erosion::run(&mut map);
        let floor = (config.bounds.min_y + 16) as f32;
        map.height.iter_mut().for_each(|h| *h = h.max(floor));
        super::super::gameplay::carve_passes(&mut map);
        super::super::hydrology::scatter(&mut map);
        super::super::hydrology::run(&mut map, true);
        map.height.iter_mut().for_each(|h| *h = h.max(floor));
        super::super::landform::run(&mut map);
        super::super::climate::run(&mut map);
        super::super::detail::roughness_layer(&mut map);
        super::super::gameplay::label(&mut map);
        let map = Arc::new(map);
        let terrain = WorldTerrain::new(map);
        let wet = terrain.sample(-21752.0, 15523.0);
        let bank = terrain.sample(-21752.0, 15524.0);
        assert_eq!(wet.water_kind, WaterKind::River);
        assert_eq!(bank.water_kind, WaterKind::None);
        assert!(
            ochre_terrain::generate::region_store::bed_top(bank.height as f32, 0.0)
                >= ochre_terrain::generate::region_store::water_block_top(wet.water_surface),
            "dry bank {} does not hold river surface {}",
            bank.height,
            wet.water_surface
        );
    }
}
