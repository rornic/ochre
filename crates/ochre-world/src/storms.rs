//! The weather as a pure function of the world's seed, where and when: no state is stored or
//! stepped, so any moment anywhere can be asked for, by a client drawing the sky, a server
//! catching a chunk up, or a test, and they all agree.
//!
//! It's a map of precipitation, cloud and thunder. Storms drift with the seed's prevailing wind
//! (the same wind the world map carries its rain on). They're laid on a lattice that moves with
//! that wind: each lattice cell may hold a storm for each slot of time, decided by an integer hash
//! of the seed, the cell and the slot. A storm grows, matures and dies over a fraction of a day or
//! two, and its cloud shield reaches farther ahead of it than behind, so the sky clouds over
//! before the rain arrives.
//!
//! The climate has its say in one place, `air_mass`: slow air masses that drift with the storms
//! and last a day or two. Storms are only born inside one, and their rain only lands inside one
//! strong enough for the ground there, which almost everywhere is rarely, in jungle often, and in
//! desert never. So rain comes in short runs of showery days with dry ones between. Cloud follows
//! the same air masses a little more loosely: it builds before the rain and thins out after it,
//! and between spells the sky is completely clear. Where the ground's cold enough for frost,
//! precipitation falls as snow, and never thunders.
//!
//! Every value comes from `+ - * / sqrt floor` and integer hashing, which give the same answer
//! on every machine, so a server and its clients see the same weather.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use bevy::{
    math::{DVec2, DVec3},
    prelude::Resource,
};
use ochre_base::{
    biome::Climate,
    cold::{FROST_BAND, SNOW_TEMPERATURE},
    math::smoothstep_f64,
    noise::{hash, perlin},
    terrain_source::TerrainSource,
};
use ochre_terrain::world::World;

use crate::sky::{DAY_SECONDS, TICKS_PER_SECOND};

/// How the weather is at one place and moment.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Sky {
    /// How hard rain or snow falls, 0..1.
    pub precipitation: f32,
    /// The share of it that falls as snow, 0 for rain to 1: snow where the ground is cold enough
    /// for frost, as `ochre_base::cold::frost_cold` has it.
    pub snow: f32,
    /// The share of the sky with cloud over it, 0..1: the clear-sky clouds of the climate, and
    /// up to full cover under a storm's shield.
    pub cloud: f32,
    /// How much of a storm's thick, dark cloud is overhead, 0..1.
    pub storm: f32,
    /// How much of a thunderstorm's core is overhead, 0..1: where lightning strikes, never in snow.
    pub thunder: f32,
}

/// One storm: where it was in the wind's frame, when it was born and for how long it lives, in
/// game seconds, how wide its core is in blocks, and how strong it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Storm {
    /// Where it sits in the frame drifting with its wind: its centre is `origin + wind * t`.
    origin: DVec2,
    /// How it drifts, in blocks a game second.
    wind: DVec2,
    born: f64,
    life: f64,
    radius: f64,
    strength: f64,
    thundery: bool,
    /// How much longer than wide it is, 1 for round, and the way its long side lies, a unit x, z.
    aspect: f64,
    long_side: DVec2,
    /// Seeds its ragged edge.
    id: u32,
}

// The lattice storms are born on: cells this many blocks across, drifting with the wind, and
// slots of this many game seconds. At most one storm per cell and slot.
const CELL: f64 = 12_000.0;
const SLOT: f64 = 3.0 * 3600.0;
// How long a storm lives, in game seconds, at the least and most (a game day is six hours).
const LIFE: (f64, f64) = (4.0 * 3600.0, 10.0 * 3600.0);
// How wide its rain core is, in blocks, most of them nearer the smaller end.
const RADIUS: (f64, f64) = (2_000.0, 6_000.0);
// Its cloud shield reaches this many radii out, stretched this much more ahead of it than behind.
const SHIELD: f64 = 1.9;
const AHEAD: f64 = 1.6;
// How much longer than wide a storm can be: a squall line, lying across the wind.
const MOST_ASPECT: f64 = 2.0;
// Its edge wanders in and out by this share of its radius, over lumps this many blocks across.
const RAGGED: f64 = 0.25;
const RAGGED_LUMP: f64 = 3_500.0;
// How likely a cell is to hold a storm in a slot deep in a wet spell.
const MOST_LIKELY: f64 = 0.9;
// How fast storms drift, in blocks a game second, at the least and most for a seed.
const DRIFT: (f64, f64) = (2.5, 5.0);
// Climate is looked up on a lattice this many blocks across, and blended between.
const CLIMATE_CELL: f64 = 1_000.0;

// The farthest the shield of a storm `radius` wide and `aspect` times longer than wide reaches
// from its centre, ahead of it along its long side at its most ragged, with room to spare.
fn shield_reach(radius: f64, aspect: f64) -> f64 {
    radius * SHIELD * AHEAD * aspect.sqrt() * (1.0 + RAGGED) * 1.02
}

// Wet spells: air masses this many blocks across, drifting with the storms, which change over
// this many game seconds (a game day and a half). Storms are only born inside one.
const SPELL_CELL: f64 = 100_000.0;
const SPELL_SLOT: f64 = 1.5 * DAY_SECONDS as f64;

// Salts for the hashes, so the draws don't line up.
const SALT_WIND: u32 = 0x57A1_0001;
const SALT_STORM: u32 = 0x57A1_0002;
const SALT_SPELL: u32 = 0x57A1_0003;

/// The weather over one world: a pure function of its seed, its climate and the time.
#[derive(Resource)]
pub struct Forecast {
    seed: u32,
    /// How the storms drift, in blocks a game second.
    wind: DVec2,
    climate: Arc<dyn Fn(f64, f64) -> Climate + Send + Sync>,
    // the climate at lattice points, as moisture and warmth, filled as it's asked for
    lattice: Mutex<HashMap<(i32, i32), (f32, f32)>>,
    // the moisture and warmth under the last map's cells, and which cells they were
    kept_map_climate: Mutex<Option<(MapCells, CellClimates)>>,
}

/// The moisture and warmth at each cell of a map.
type CellClimates = Arc<Vec<(f64, f64)>>;

/// The cells a map covers, see `Forecast::map`.
#[derive(Clone, Copy, PartialEq)]
struct MapCells {
    corner: DVec2,
    size: usize,
    cell: f64,
}

impl std::fmt::Debug for Forecast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Forecast")
            .field("seed", &self.seed)
            .field("wind", &self.wind)
            .finish()
    }
}

/// A game tick as game seconds.
pub fn seconds(tick: u64) -> f64 {
    tick as f64 / TICKS_PER_SECOND as f64
}

// The `n`th number, 0..1, drawn for whatever storm lattice cell `cell` may hold in slot `slot`.
fn draw(seed: u32, cell: (i32, i32), slot: i64, n: u32) -> f64 {
    let h = hash(seed ^ SALT_STORM, cell.0, cell.1);
    hash(
        h ^ n.wrapping_mul(0x9E37_79B9),
        slot as i32,
        (slot >> 32) as i32,
    ) as f64
        / 4_294_967_296.0
}

fn lerp(range: (f64, f64), t: f64) -> f64 {
    range.0 + (range.1 - range.0) * t
}

/// What the air over a place is doing, each 0..1.
struct Air {
    /// In a wet spell: storms are born and rain lands.
    wet: f64,
    /// In a cloudy spell, which starts before a wet one and ends after it; between them the sky is clear.
    cloudy: f64,
}

// Slow noise over air masses drifting with the storms, past bars the climate sets: it's all the climate does to the weather.
fn air_mass(seed: u32, carried: DVec2, t: f64, moisture: f64, warmth: f64) -> Air {
    let p = DVec3::new(
        carried.x / SPELL_CELL,
        carried.y / SPELL_CELL,
        t / SPELL_SLOT,
    );
    let cell = p.floor();
    let f = p - cell;
    let fade = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let corner = |dx: i32, dy: i32, dz: i32| {
        let h = hash(seed ^ SALT_SPELL, cell.x as i32 + dx, cell.y as i32 + dy);
        let slot = cell.z as i64 + dz as i64;
        hash(h, slot as i32, (slot >> 32) as i32) as f64 / 4_294_967_296.0
    };
    let mix = |a: f64, b: f64, t: f64| a + (b - a) * t;
    let plane = |dz| {
        mix(
            mix(corner(0, 0, dz), corner(1, 0, dz), fade.x),
            mix(corner(0, 1, dz), corner(1, 1, dz), fade.x),
            fade.y,
        )
    };
    let noise = mix(plane(0), plane(1), fade.z);
    let tropical = smoothstep_f64(SPELL_WET.0, SPELL_WET.1, moisture)
        * smoothstep_f64(SPELL_WARM.0, SPELL_WARM.1, warmth);
    let dry = 1.0 - smoothstep_f64(SPELL_DRY_GROUND.0, SPELL_DRY_GROUND.1, moisture);
    let bar = lerp(SPELL_BAR, tropical);
    let wet_bar = bar + SPELL_DRY_BAR * dry;
    let cloudy_bar = bar + CLOUDY_DRY_BAR * dry;
    Air {
        wet: smoothstep_f64(wet_bar - 0.04, wet_bar + 0.04, noise),
        cloudy: smoothstep_f64(
            cloudy_bar - CLOUDY_BELOW_BAR.0,
            cloudy_bar - CLOUDY_BELOW_BAR.1,
            noise,
        ),
    }
}

// The noise a wet spell needs to clear, over the driest climates and the wettest.
const SPELL_BAR: (f64, f64) = (0.76, 0.45);
// The moisture and warmth over which the bar falls from the first to the second: only where
// it's both wet and warm, so everywhere stays mostly dry but jungle (and warm seas), which is wet.
const SPELL_WET: (f64, f64) = (0.55, 0.95);
const SPELL_WARM: (f64, f64) = (0.45, 0.7);
// Below the first moisture the bar's raised by this much, out of the noise's reach, so deserts stay
// dry, easing back by the second, over steppe.
const SPELL_DRY_GROUND: (f64, f64) = (0.16, 0.35);
const SPELL_DRY_BAR: f64 = 0.4;
// How far under the wet bar the noise is where cloud starts to build, and where it's all there.
const CLOUDY_BELOW_BAR: (f64, f64) = (0.25, 0.05);
// Dry ground raises the cloud bar by this much, so steppe and desert are clearer still.
const CLOUDY_DRY_BAR: f64 = 0.25;

// The share of what falls on ground of `warmth` that's snow, 0..1: all of it where the ground's
// cold enough for frost to settle, easing to none across the frost band, as on the ground.
fn snow_share(warmth: f64) -> f64 {
    let (snow, band) = (SNOW_TEMPERATURE as f64, FROST_BAND as f64);
    1.0 - smoothstep_f64(snow, snow + band, warmth)
}

// The share of the sky with fair-weather cloud over it, from clear desert skies to humid ones.
fn fair_cloud(moisture: f64) -> f64 {
    0.12 + 0.48 * smoothstep_f64(0.05, 0.85, moisture)
}

impl Storm {
    /// Where its centre is at `t`.
    pub fn centre(&self, t: f64) -> DVec2 {
        self.origin + self.wind * t
    }

    /// How grown it is at `t`, 0 before it's born and after it dies, 1 when mature, and how wide
    /// its core is then.
    fn grown(&self, t: f64) -> (f64, f64) {
        let s = (t - self.born) / self.life;
        if !(0.0..=1.0).contains(&s) {
            return (0.0, 0.0);
        }
        let strength = smoothstep_f64(0.0, 0.25, s) * (1.0 - smoothstep_f64(0.7, 1.0, s));
        let radius = self.radius * (0.55 + 0.45 * smoothstep_f64(0.0, 0.35, s));
        (strength, radius)
    }

    /// The farthest its shield reaches from its centre, in blocks.
    fn reach(&self) -> f64 {
        shield_reach(self.radius, self.aspect)
    }

    /// Whether it's alive at `t`.
    pub fn alive(&self, t: f64) -> bool {
        (self.born..=self.born + self.life).contains(&t)
    }

    // Its precipitation, cloud shield, dark cloud and thunder at `p` at `t`, before the climate
    // there has its say, see `Forecast::finish`.
    fn at(&self, p: DVec2, t: f64) -> [f64; 4] {
        let (grown, radius) = self.grown(t);
        if grown <= 0.0 {
            return [0.0; 4];
        }
        let delta = p - self.centre(t);
        if delta.length_squared() > self.reach() * self.reach() {
            return [0.0; 4];
        }
        let ahead = self.wind.normalize_or_zero();
        let along = delta.dot(ahead);
        // a lumpy edge, which drifts with it
        let ragged = 1.0
            + RAGGED
                * perlin(
                    self.id ^ 0x5A61_7ED0,
                    delta.x / RAGGED_LUMP,
                    delta.y / RAGGED_LUMP,
                );
        let root = self.aspect.sqrt();
        // distance, stretched ahead of it where the anvil runs before the storm, and along its
        // long side, and pushed in and out by its ragged edge
        let reach = |stretch: f64| {
            let delta = if along > 0.0 {
                delta - ahead * (along - along / stretch)
            } else {
                delta
            };
            let long = delta.dot(self.long_side) / root;
            let short = delta.perp_dot(self.long_side) * root;
            (long * long + short * short).sqrt() / ragged
        };
        let core = reach(1.15);
        let shield = reach(AHEAD);
        let rain = self.strength * grown * (1.0 - smoothstep_f64(0.55 * radius, radius, core));
        let cloud = grown * (1.0 - smoothstep_f64(radius, SHIELD * radius, shield));
        let dark =
            self.strength * grown * (1.0 - smoothstep_f64(0.3 * radius, 1.3 * radius, shield));
        let thunder = if self.thundery {
            self.strength * grown * (1.0 - smoothstep_f64(0.3 * radius, 0.8 * radius, core))
        } else {
            0.0
        };
        [rain, cloud, dark, thunder]
    }
}

impl Forecast {
    /// The weather over a world of `seed`, its storms drifting along `wind` (any length), whose
    /// climate is `climate`.
    pub fn new(
        seed: u32,
        wind: [f32; 2],
        climate: impl Fn(f64, f64) -> Climate + Send + Sync + 'static,
    ) -> Self {
        let direction = DVec2::new(wind[0] as f64, wind[1] as f64)
            .try_normalize()
            .unwrap_or(DVec2::X);
        let speed = lerp(DRIFT, hash(seed ^ SALT_WIND, 0, 0) as f64 / 4_294_967_296.0);
        Self {
            seed,
            wind: direction * speed,
            climate: Arc::new(climate),
            lattice: Mutex::new(HashMap::new()),
            kept_map_climate: Mutex::new(None),
        }
    }

    /// The weather over the world `terrain` draws, its storms on the map's prevailing wind.
    pub fn from_terrain(terrain: Arc<dyn TerrainSource>) -> Self {
        let (seed, wind) = (terrain.seed(), terrain.prevailing_wind());
        Self::new(seed, wind, move |x, z| terrain.climate(x, z))
    }

    /// The weather over `world`, as `from_terrain`, reading its climate through its region store.
    pub fn from_world(world: &World) -> Self {
        let store = world.region_store.clone();
        let terrain = store.terrain();
        let (seed, wind) = (terrain.seed(), terrain.prevailing_wind());
        Self::new(seed, wind, move |x, z| store.terrain().climate(x, z))
    }

    /// How the storms drift, in blocks a game second.
    pub fn wind(&self) -> DVec2 {
        self.wind
    }

    /// The climate at (x, z), as moisture and warmth, blended from the lattice.
    pub fn climate_at(&self, x: f64, z: f64) -> (f64, f64) {
        let (u, v) = (x / CLIMATE_CELL, z / CLIMATE_CELL);
        let (i, j) = (u.floor(), v.floor());
        let (fu, fv) = (u - i, v - j);
        let (i, j) = (i as i32, j as i32);
        let mut lattice = self.lattice.lock().unwrap_or_else(|e| e.into_inner());
        let mut at = |di: i32, dj: i32| -> (f64, f64) {
            let key = (i + di, j + dj);
            let (m, w) = *lattice.entry(key).or_insert_with(|| {
                let c = (self.climate)(key.0 as f64 * CLIMATE_CELL, key.1 as f64 * CLIMATE_CELL);
                (c.humidity, c.temperature)
            });
            (m as f64, w as f64)
        };
        let (a, b, c, d) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1));
        let mix = |a: f64, b: f64, t: f64| a + (b - a) * t;
        (
            mix(mix(a.0, b.0, fu), mix(c.0, d.0, fu), fv),
            mix(mix(a.1, b.1, fu), mix(c.1, d.1, fu), fv),
        )
    }

    // The storm lattice cell `cell` holds in slot `slot`, if any.
    fn lattice_storm(&self, cell: (i32, i32), slot: i64) -> Option<Storm> {
        let d = |n| draw(self.seed, cell, slot, n);
        // none where even the deepest spell would hold none, without asking the climate
        if d(0) >= MOST_LIKELY {
            return None;
        }
        let born = (slot as f64 + d(1)) * SLOT;
        let origin = DVec2::new(cell.0 as f64 + d(2), cell.1 as f64 + d(3)) * CELL;
        // only born in a wet spell, more often the deeper in it
        let at_birth = origin + self.wind * born;
        let (moisture, warmth) = self.climate_at(at_birth.x, at_birth.y);
        if d(0) >= MOST_LIKELY * air_mass(self.seed, origin, born, moisture, warmth).wet {
            return None;
        }
        let strength = lerp((0.55, 1.0), d(5));
        // squall lines lie across the wind, turned up to 0.7 radians (40 degrees) either way: a
        // small angle's cos and sin, so no platform maths
        let turn = (d(9) - 0.5) * 1.4;
        let small_turn = DVec2::new(1.0 - turn * turn / 2.0, turn);
        let long_side = small_turn
            .rotate(self.wind.normalize_or_zero().perp())
            .normalize_or(DVec2::Y);
        Some(Storm {
            origin,
            wind: self.wind,
            born,
            life: lerp(LIFE, d(4)),
            // more small storms than large
            radius: lerp(RADIUS, d(6) * d(6)),
            strength,
            thundery: strength > 0.78,
            aspect: lerp((1.0, MOST_ASPECT), d(8) * d(8)),
            long_side,
            id: (d(7) * 4_294_967_296.0) as u32,
        })
    }

    /// Every storm alive at `t` (game seconds) that could reach anywhere within `reach` blocks of `centre`.
    pub fn storms_near(&self, centre: DVec2, reach: f64, t: f64) -> Vec<Storm> {
        // the lattice drifts with the wind, so it's looked up where the point was carried from
        let carried = centre - self.wind * t;
        let reach = reach + shield_reach(RADIUS.1, MOST_ASPECT);
        let span = (reach / CELL).ceil() as i32;
        let (ci, cj) = (
            (carried.x / CELL).floor() as i32,
            (carried.y / CELL).floor() as i32,
        );
        let first = ((t - LIFE.1) / SLOT).floor() as i64;
        let last = (t / SLOT).floor() as i64;
        let mut storms = Vec::new();
        for slot in first..=last {
            for j in cj - span..=cj + span {
                for i in ci - span..=ci + span {
                    if let Some(storm) = self.lattice_storm((i, j), slot) {
                        if storm.alive(t) && storm.centre(t).distance(centre) <= reach {
                            storms.push(storm);
                        }
                    }
                }
            }
        }
        storms
    }

    /// Where the thunderstorms within `reach` blocks of `centre` have their cores at `t` (game
    /// seconds): where lightning strikes, seen from afar.
    pub fn thunderstorms_near(&self, centre: DVec2, reach: f64, t: f64) -> Vec<DVec2> {
        self.storms_near(centre, reach, t)
            .iter()
            .filter(|storm| {
                let core = storm.centre(t);
                core.distance(centre) < reach
                    && self.sky_among(std::slice::from_ref(storm), core, t).thunder > 0.5
            })
            .map(|storm| storm.centre(t))
            .collect()
    }

    /// The weather at (x, z) at game tick `tick`.
    pub fn sky(&self, x: f64, z: f64, tick: u64) -> Sky {
        let t = seconds(tick);
        let p = DVec2::new(x, z);
        let storms = self.storms_near(p, 0.0, t);
        self.sky_among(&storms, p, t)
    }

    /// The weather at `p` at `t` (game seconds), from `storms`, which must hold every storm that
    /// reaches `p` then, see `storms_near`.
    pub fn sky_among(&self, storms: &[Storm], p: DVec2, t: f64) -> Sky {
        let (moisture, warmth) = self.climate_at(p.x, p.y);
        let mut most = [0.0f64; 4];
        for storm in storms {
            most = strongest(most, storm.at(p, t));
        }
        self.finish(most, p, t, (moisture, warmth))
    }

    /// The weather over a square of `size` by `size` cells, each `cell` blocks across, its
    /// north-west corner at `corner`, sampled at their centres at `tick`: row-major `[z][x]`.
    /// The same as asking `sky` at each centre, for far less.
    pub fn map(&self, corner: DVec2, size: usize, cell: f64, tick: u64) -> Vec<Sky> {
        let t = seconds(tick);
        let half = size as f64 * cell / 2.0;
        let middle = corner + DVec2::splat(half);
        let storms = self.storms_near(middle, half * std::f64::consts::SQRT_2, t);
        let centre = |i: usize, j: usize| corner + (DVec2::new(i as f64, j as f64) + 0.5) * cell;
        let climate = self.map_climate(corner, size, cell, &centre);
        let mut most = vec![[0.0f64; 4]; size * size];
        // each storm reaches only so far, so only the cells within that are worked out for it
        for storm in &storms {
            let at = storm.centre(t);
            let reach = storm.reach();
            let range = |c: f64, from: f64| {
                let low = ((c - reach - from) / cell - 0.5).floor().max(0.0) as usize;
                let high = ((c + reach - from) / cell + 0.5).ceil().max(0.0) as usize;
                low.min(size)..high.min(size)
            };
            for j in range(at.y, corner.y) {
                for i in range(at.x, corner.x) {
                    let k = j * size + i;
                    most[k] = strongest(most[k], storm.at(centre(i, j), t));
                }
            }
        }
        most.into_iter()
            .enumerate()
            .map(|(k, most)| self.finish(most, centre(k % size, k / size), t, climate[k]))
            .collect()
    }

    // The weather at `p` at `t` from the strongest storm values there, over ground of `moisture`
    // and `warmth`: the rain only falls inside a wet spell strong enough for the climate there, so
    // only warm, wet ground gets many wet days, and the fair-weather clouds fill in under a
    // storm's shield.
    fn finish(&self, most: [f64; 4], p: DVec2, t: f64, (moisture, warmth): (f64, f64)) -> Sky {
        let [mut rain, shield, dark, mut thunder] = most;
        let air = air_mass(self.seed, p - self.wind * t, t, moisture, warmth);
        rain *= air.wet;
        thunder *= air.wet;
        // storm cloud thins out over ground too dry for cloudy spells
        let (shield, dark) = (shield * air.cloudy, dark * air.cloudy);
        let fair = fair_cloud(moisture) * air.cloudy;
        let snow = snow_share(warmth);
        Sky {
            precipitation: rain as f32,
            snow: snow as f32,
            cloud: (fair + (0.97 - fair).max(0.0) * shield) as f32,
            storm: dark as f32,
            thunder: (thunder * (1.0 - snow)) as f32,
        }
    }

    // The moisture and warmth at the centre of each cell of a map, kept from the last map over the
    // same cells.
    fn map_climate(
        &self,
        corner: DVec2,
        size: usize,
        cell: f64,
        centre: &impl Fn(usize, usize) -> DVec2,
    ) -> CellClimates {
        let cells = MapCells { corner, size, cell };
        let mut kept = self
            .kept_map_climate
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some((kept_cells, climate)) = kept.as_ref() {
            if *kept_cells == cells {
                return climate.clone();
            }
        }
        let climate: CellClimates = Arc::new(
            (0..size * size)
                .map(|k| {
                    let p = centre(k % size, k / size);
                    self.climate_at(p.x, p.y)
                })
                .collect(),
        );
        *kept = Some((cells, climate.clone()));
        climate
    }
}

// The larger of each of storm values `a` and `b`.
fn strongest(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sky::{DAY_SECONDS, DAY_TICKS};

    fn uniform(moisture: f32, warmth: f32) -> impl Fn(f64, f64) -> Climate + Send + Sync {
        move |_, _| Climate {
            temperature: warmth,
            humidity: moisture,
        }
    }

    // The share of samples over a wide spread of places and two months where it rains at all.
    fn rainy_share(forecast: &Forecast) -> f64 {
        let mut wet = 0;
        let mut all = 0;
        for step in 0..60 * 12u64 {
            let sky = forecast.map(DVec2::splat(-40_000.0), 16, 5_000.0, step * DAY_TICKS / 12);
            wet += sky.iter().filter(|s| s.precipitation > 0.1).count();
            all += sky.len();
        }
        wet as f64 / all as f64
    }

    // Whether it rains at (x, z) at all on each of `days` days, looked at every quarter hour.
    fn wet_days(forecast: &Forecast, x: f64, z: f64, days: u64) -> Vec<bool> {
        (0..days)
            .map(|day| {
                (0..24).any(|q| {
                    forecast
                        .sky(x, z, day * DAY_TICKS + q * DAY_TICKS / 24)
                        .precipitation
                        > 0.1
                })
            })
            .collect()
    }

    #[test]
    fn test_the_same_seed_and_moment_give_the_same_weather() {
        let a = Forecast::new(7, [1.0, 0.3], uniform(0.6, 0.5));
        let b = Forecast::new(7, [1.0, 0.3], uniform(0.6, 0.5));
        for tick in (0..4 * DAY_TICKS).step_by(DAY_TICKS as usize / 37) {
            for x in [-30_000.0, 0.0, 12_345.0] {
                assert_eq!(a.sky(x, 1_000.0, tick), b.sky(x, 1_000.0, tick));
            }
        }
        // and another seed differs somewhere
        let c = Forecast::new(8, [1.0, 0.3], uniform(0.6, 0.5));
        let differs = (0..200u64).any(|i| {
            let tick = i * DAY_TICKS / 20;
            a.sky(5_000.0, 5_000.0, tick) != c.sky(5_000.0, 5_000.0, tick)
        });
        assert!(differs);
    }

    #[test]
    fn test_deserts_stay_dry_and_jungles_get_the_most_rain() {
        let share = |moisture, warmth| {
            rainy_share(&Forecast::new(3, [1.0, 0.0], uniform(moisture, warmth)))
        };
        let desert = share(0.08, 0.8);
        let steppe = share(0.25, 0.45);
        let grassland = share(0.45, 0.45);
        let jungle = share(0.85, 0.8);
        assert_eq!(desert, 0.0, "desert {desert}");
        assert!(steppe < grassland, "steppe {steppe} grassland {grassland}");
        assert!(grassland < jungle, "grassland {grassland} jungle {jungle}");
        // rain a tenth of the time or so in jungle, and hardly any in grassland
        assert!((0.06..0.25).contains(&jungle), "jungle {jungle}");
        assert!((0.002..0.05).contains(&grassland), "grassland {grassland}");
    }

    #[test]
    fn test_grassland_is_mostly_dry_and_jungle_wet_in_spells_of_days() {
        let share = |days: &[bool]| days.iter().filter(|&&d| d).count() as f64 / days.len() as f64;
        let longest = |days: &[bool], of: bool| {
            days.split(|&d| d != of)
                .map(<[bool]>::len)
                .max()
                .unwrap_or(0)
        };
        // grassland: a wet day now and then, and long runs of dry ones
        let grassland = Forecast::new(6, [1.0, 0.2], uniform(0.5, 0.5));
        let days = wet_days(&grassland, 0.0, 0.0, 120);
        assert!(share(&days) < 0.35, "grassland wet days {}", share(&days));
        assert!(
            longest(&days, false) >= 8,
            "longest dry run {}",
            longest(&days, false)
        );
        // jungle: wet most days, in spells
        let jungle = Forecast::new(6, [1.0, 0.2], uniform(0.9, 0.8));
        let days = wet_days(&jungle, 0.0, 0.0, 120);
        let wet = share(&days);
        assert!(wet > 0.45, "jungle wet days {wet}");
        assert!(
            longest(&days, true) >= 4,
            "longest wet run {}",
            longest(&days, true)
        );
        // and its dry breaks come in runs too: spells, not dry days scattered at random
        let dry = 1.0 - wet;
        let after_dry: Vec<bool> = days.windows(2).filter(|w| !w[0]).map(|w| !w[1]).collect();
        assert!(
            share(&after_dry) > dry + 0.05,
            "{} dry after a dry day, {dry} on any",
            share(&after_dry)
        );
    }

    #[test]
    fn test_it_snows_without_thunder_in_the_cold_and_rains_in_the_warm() {
        let falling = |warmth| {
            let forecast = Forecast::new(5, [1.0, 0.0], uniform(0.9, warmth));
            (0..60 * 12u64)
                .flat_map(|step| {
                    forecast.map(DVec2::splat(-40_000.0), 8, 10_000.0, step * DAY_TICKS / 12)
                })
                .filter(|sky| sky.precipitation > 0.1)
                .collect::<Vec<_>>()
        };
        let cold = falling(0.05);
        assert!(!cold.is_empty());
        assert!(cold.iter().all(|sky| sky.snow == 1.0 && sky.thunder == 0.0));
        let warm = falling(0.8);
        assert!(!warm.is_empty());
        assert!(warm.iter().all(|sky| sky.snow == 0.0));
        assert!(warm.iter().any(|sky| sky.thunder > 0.0));
    }

    // Over two months at a spread of places, the share of samples with no cloud at all, and the mean cloud.
    fn clear_and_mean_cloud(moisture: f32, warmth: f32) -> (f64, f64) {
        let forecast = Forecast::new(3, [1.0, 0.0], uniform(moisture, warmth));
        let (mut clear, mut cloud, mut all) = (0, 0.0, 0);
        for step in 0..60 * 12u64 {
            let sky = forecast.map(DVec2::splat(-40_000.0), 16, 5_000.0, step * DAY_TICKS / 12);
            clear += sky.iter().filter(|s| s.cloud < 0.01).count();
            cloud += sky.iter().map(|s| s.cloud as f64).sum::<f64>();
            all += sky.len();
        }
        (clear as f64 / all as f64, cloud / all as f64)
    }

    #[test]
    fn test_skies_are_clearer_over_deserts() {
        let (desert, grassland, jungle) = (
            clear_and_mean_cloud(0.08, 0.8),
            clear_and_mean_cloud(0.45, 0.45),
            clear_and_mean_cloud(0.9, 0.8),
        );
        assert!(
            desert.1 < grassland.1 && grassland.1 < jungle.1,
            "{desert:?} {grassland:?} {jungle:?}"
        );
        assert!(
            desert.0 > grassland.0 && grassland.0 > jungle.0,
            "{desert:?} {grassland:?} {jungle:?}"
        );
    }

    #[test]
    fn test_the_sky_goes_completely_clear_for_stretches() {
        let (grassland, _) = clear_and_mean_cloud(0.45, 0.45);
        assert!(
            (0.3..0.8).contains(&grassland),
            "grassland clear {grassland}"
        );
        let (desert, _) = clear_and_mean_cloud(0.08, 0.8);
        assert!(desert > 0.7, "desert clear {desert}");
        // jungle clears now and then, but is mostly overcast
        let (jungle, _) = clear_and_mean_cloud(0.9, 0.8);
        assert!(jungle < 0.25, "jungle clear {jungle}");
        // and whole days go by without a cloud, and others are cloudy
        let forecast = Forecast::new(6, [1.0, 0.2], uniform(0.5, 0.5));
        let day_is_clear = |day: u64| {
            (0..24).all(|q| {
                forecast
                    .sky(0.0, 0.0, day * DAY_TICKS + q * DAY_TICKS / 24)
                    .cloud
                    < 0.01
            })
        };
        let clear_days = (0..120).filter(|&day| day_is_clear(day)).count();
        assert!(
            (12..90).contains(&clear_days),
            "{clear_days} clear days in 120"
        );
    }

    #[test]
    fn test_the_weather_changes_smoothly() {
        let forecast = Forecast::new(11, [0.4, -1.0], uniform(0.7, 0.6));
        // a game second apart, nothing jumps: a storm's edge takes minutes to pass
        let mut last = forecast.sky(0.0, 0.0, 0);
        for tick in (0..2 * DAY_TICKS).step_by(TICKS_PER_SECOND as usize) {
            let now = forecast.sky(0.0, 0.0, tick);
            for (a, b) in [
                (last.precipitation, now.precipitation),
                (last.cloud, now.cloud),
                (last.storm, now.storm),
            ] {
                assert!((a - b).abs() < 0.02, "tick {tick}: {last:?} then {now:?}");
            }
            last = now;
        }
        // and a block apart, neither does it
        for x in (-20_000..20_000).step_by(37) {
            let a = forecast.sky(x as f64, 300.0, DAY_TICKS / 2);
            let b = forecast.sky(x as f64 + 1.0, 300.0, DAY_TICKS / 2);
            assert!(
                (a.precipitation - b.precipitation).abs() < 0.01
                    && (a.cloud - b.cloud).abs() < 0.01
            );
        }
    }

    #[test]
    fn test_the_map_agrees_with_asking_each_place() {
        let forecast = Forecast::new(5, [1.0, 1.0], uniform(0.75, 0.6));
        let corner = DVec2::new(-35_000.0, -35_000.0);
        let (size, cell) = (32, 2_200.0);
        for tick in [0, DAY_TICKS / 3, DAY_TICKS * 5 / 8, 3 * DAY_TICKS] {
            let map = forecast.map(corner, size, cell, tick);
            for (k, sky) in map.iter().enumerate() {
                let p = corner + (DVec2::new((k % size) as f64, (k / size) as f64) + 0.5) * cell;
                assert_eq!(*sky, forecast.sky(p.x, p.y, tick), "cell {k} at {tick}");
            }
        }
    }

    #[test]
    fn test_storms_drift_with_the_wind() {
        let forecast = Forecast::new(2, [0.0, 1.0], uniform(0.8, 0.6));
        let t = 2.0 * DAY_SECONDS as f64;
        let storms = forecast.storms_near(DVec2::ZERO, 30_000.0, t);
        assert!(!storms.is_empty());
        for storm in storms {
            let moved = storm.centre(t + 600.0) - storm.centre(t);
            assert!(moved.x.abs() < 1e-6 && moved.y > 0.0, "{moved:?}");
            // at the seed's speed
            assert!((moved.length() / 600.0 - forecast.wind().length()).abs() < 1e-9);
        }
        assert!((DRIFT.0..=DRIFT.1).contains(&forecast.wind().length()));
    }

    #[test]
    fn test_storms_far_in_the_future_still_work() {
        // a server up for years: positions and slots stay in range
        let forecast = Forecast::new(4, [1.0, 0.0], uniform(0.8, 0.6));
        let tick = 2000 * DAY_TICKS;
        let sky = forecast.sky(0.0, 0.0, tick);
        assert!(sky.cloud.is_finite() && (0.0..=1.0).contains(&sky.cloud));
        let storms = forecast.storms_near(DVec2::ZERO, 20_000.0, seconds(tick));
        assert!(storms
            .iter()
            .all(|s| s.centre(seconds(tick)).length() < 60_000.0));
    }
}
