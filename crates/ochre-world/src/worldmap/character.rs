//! What kind of world a seed makes: plates, relief, warmth, wind, each drawn from the seed within a range.

use ochre_base::noise::unit;

use super::WorldConfig;

const PLATES: (f64, f64) = (8.0, 17.0);
const RELIEF: (f64, f64) = (0.7, 1.4);
const WARMTH: (f64, f64) = (0.20, 0.80);
const HUMIDITY: (f64, f64) = (0.8, 1.6);
const GRADIENT: (f64, f64) = (0.10, 0.30);
const CONTINENTALITY: (f64, f64) = (-0.12, 0.12);

const SALT_PLATES: u32 = 0xC4A0_0001;
const SALT_RELIEF: u32 = 0xC4A0_0002;
const SALT_WARMTH: u32 = 0xC4A0_0003;
const SALT_GRADIENT: u32 = 0xC4A0_0004;
const SALT_GRADIENT_ANGLE: u32 = 0xC4A0_0005;
const SALT_CONTINENTALITY: u32 = 0xC4A0_0006;
const SALT_WIND: u32 = 0xC4A0_0007;
const SALT_TABLELAND: u32 = 0xC4A0_0008;
const SALT_DUNES: u32 = 0xC4A0_0009;
const SALT_CLIFFS: u32 = 0xC4A0_000A;
const SALT_FJORDS: u32 = 0xC4A0_000D;
const SALT_CONES: u32 = 0xC4A0_000B;
const SALT_INSELBERGS: u32 = 0xC4A0_000E;
const SALT_KARST: u32 = 0xC4A0_000F;
const SALT_CIRQUES: u32 = 0xC4A0_0010;
const SALT_VALLEYS: u32 = 0xC4A0_0011;
const SALT_LANDMARKS: u32 = 0xC4A0_0012;
const SALT_CANYONS: u32 = 0xC4A0_0013;
const SALT_ESKERS: u32 = 0xC4A0_0014;
const SALT_GORGES: u32 = 0xC4A0_0015;
const SALT_STACKS: u32 = 0xC4A0_0016;
const SALT_WADIS: u32 = 0xC4A0_0019;
const WADIS: (f64, f64) = (0.0, 9.0);
const SALT_CANYON_DEPTH: u32 = 0xC4A0_001A;
const SALT_CANYON_WIDTH: u32 = 0xC4A0_001B;
const CANYON_DEPTH: (f64, f64) = (0.7, 1.3);
const CANYON_WIDTH: (f64, f64) = (0.8, 1.5);
const SALT_PONDS: u32 = 0xC4A0_001C;
const PONDS: (f64, f64) = (0.0, 1500.0);
const SALT_DUNE_BEND: u32 = 0xC4A0_0017;
const SALT_DUNE_WINDWARD: u32 = 0xC4A0_0018;
const DUNE_BEND: (f64, f64) = (0.3, 1.8);
const DUNE_WINDWARD: (f64, f64) = (0.5, 0.78);
const STACKS: (f64, f64) = (0.0, 61.0);
const GORGES: (f64, f64) = (0.0, 4.0);
const ESKERS: (f64, f64) = (0.0, 7.0);
const CANYONS: (f64, f64) = (0.0, 5.0);
const INSELBERGS: (f64, f64) = (0.0, 9.0);
const KARST_GROUPS: (f64, f64) = (0.0, 5.0);
const CIRQUES: (f64, f64) = (0.0, 7.0);
const VALLEY_SHARE: (f64, f64) = (0.5, 1.0);
// Cells between steppe and tundra landmarks: about 2.2 to 3.5 km, so two or three stand in view from most places.
const LANDMARK_APART: (f64, f64) = (70.0, 110.0);
const CONE_SHARE: (f64, f64) = (0.0, 0.25);
const FJORD_SHARE: (f64, f64) = (0.0, 0.3);
const CLIFF_BOOST: (f64, f64) = (0.0, 1.0);
const DUNE_SCALE: (f64, f64) = (0.7, 1.5);
const TABLELAND_SHARE: (f64, f64) = (0.2, 0.8);
const SALT_HUMIDITY: u32 = 0xC4A0_000C;

// Each knob has its own salt, so the order of draws never matters.
fn draw(seed: u32, salt: u32, (lo, hi): (f64, f64)) -> f64 {
    lo + unit(seed ^ salt, 0, 0) * (hi - lo)
}

fn round(value: f64, decimals: i32) -> f64 {
    let scale = 10f64.powi(decimals);
    (value * scale).round() / scale
}

/// What a seed decides about its landforms, derived where used and never stored, so a new landform changes no file format.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LandformKnobs {
    /// About how much of the sedimentary lowland is tableland.
    pub tableland_share: f64,
    /// Multiplies the spacing and height of dunes.
    pub dune_scale: f64,
    /// How much longer than usual cliffs on mountains and tableland reach, 0..1.
    pub cliff_boost: f64,
    /// The share of the mouths its mountainous coast has room for that are fjords, 0 to 0.3.
    pub fjord_share: f64,
    /// The share of the cone sites its volcanic ground has room for that are raised, 0 to 0.25.
    pub cone_share: f64,
    /// How many inselbergs a dry, warm world raises, 0 to 8.
    pub inselbergs: u32,
    /// How many fields of tower karst a wet world raises, 0 to 4.
    pub karst_groups: u32,
    /// How many cirques a cold world cuts, 0 to 6.
    pub cirques: u32,
    /// How much of the weight the valleys' siting gives it, 0.5 to 1.
    pub valley_share: f64,
    /// Cells between steppe and tundra landmarks, 70 to 110.
    pub landmark_apart: f64,
    /// How many canyons a desert world cuts, 0 to 4.
    pub canyons: u32,
    /// How many eskers a glaciated world raises, 0 to 6.
    pub eskers: u32,
    /// How many gorges the wetter plateaus are cut with, 0 to 3.
    pub gorges: u32,
    /// How many sea stacks cliffed coasts are given, 0 to 60.
    pub stacks: u32,
    /// How many wadis, dry beds, dry country is cut with, 0 to 8.
    pub wadis: u32,
    /// How many ponds thaw out of frozen ground, 0 to 1500.
    pub ponds: u32,
    /// How deep a seed's canyons and gorges cut against the table, 0.7 to 1.3 times.
    pub canyon_depth: f64,
    /// How wide a seed's canyons, gorges and wadis run against the table, 0.8 to 1.5 times.
    pub canyon_width: f64,
    /// How far dune rows bend, in dunes, 0.3 (near straight ridges) to 1.8 (wandering crests).
    pub dune_bend: f64,
    /// The share of a dune that is its gentle windward slope, 0.5 to 0.78.
    pub dune_windward: f64,
}

impl LandformKnobs {
    pub fn from_seed(seed: u32) -> Self {
        Self {
            tableland_share: round(draw(seed, SALT_TABLELAND, TABLELAND_SHARE), 3),
            dune_scale: round(draw(seed, SALT_DUNES, DUNE_SCALE), 2),
            cliff_boost: round(draw(seed, SALT_CLIFFS, CLIFF_BOOST), 2),
            fjord_share: round(draw(seed, SALT_FJORDS, FJORD_SHARE), 3),
            cone_share: round(draw(seed, SALT_CONES, CONE_SHARE), 3),
            inselbergs: draw(seed, SALT_INSELBERGS, INSELBERGS).floor() as u32,
            karst_groups: draw(seed, SALT_KARST, KARST_GROUPS).floor() as u32,
            cirques: draw(seed, SALT_CIRQUES, CIRQUES).floor() as u32,
            valley_share: round(draw(seed, SALT_VALLEYS, VALLEY_SHARE), 3),
            landmark_apart: draw(seed, SALT_LANDMARKS, LANDMARK_APART).floor(),
            canyons: draw(seed, SALT_CANYONS, CANYONS).floor() as u32,
            eskers: draw(seed, SALT_ESKERS, ESKERS).floor() as u32,
            gorges: draw(seed, SALT_GORGES, GORGES).floor() as u32,
            stacks: draw(seed, SALT_STACKS, STACKS).floor() as u32,
            wadis: draw(seed, SALT_WADIS, WADIS).floor() as u32,
            ponds: draw(seed, SALT_PONDS, PONDS).floor() as u32,
            canyon_depth: round(draw(seed, SALT_CANYON_DEPTH, CANYON_DEPTH), 2),
            canyon_width: round(draw(seed, SALT_CANYON_WIDTH, CANYON_WIDTH), 2),
            dune_bend: round(draw(seed, SALT_DUNE_BEND, DUNE_BEND), 2),
            dune_windward: round(draw(seed, SALT_DUNE_WINDWARD, DUNE_WINDWARD), 2),
        }
    }
}

/// `config` with the knobs a seed decides replaced by that seed's draws.
pub fn derive(seed: u32, mut config: WorldConfig) -> WorldConfig {
    config.tectonics.plates = draw(seed, SALT_PLATES, PLATES).floor() as u8;
    config.tectonics.relief = round(draw(seed, SALT_RELIEF, RELIEF), 2) as f32;
    config.climate.warmth = round(draw(seed, SALT_WARMTH, WARMTH), 3) as f32;
    config.climate.humidity = round(draw(seed, SALT_HUMIDITY, HUMIDITY), 2) as f32;
    config.climate.gradient = round(draw(seed, SALT_GRADIENT, GRADIENT), 3) as f32;
    let degrees = draw(seed, SALT_GRADIENT_ANGLE, (0.0, 360.0)).floor();
    config.climate.gradient_angle = round(degrees.to_radians(), 4) as f32;
    config.climate.continentality =
        round(draw(seed, SALT_CONTINENTALITY, CONTINENTALITY), 3) as f32;
    // whole degrees, stored as a unit vector: `climate_fields` only uses the direction
    let angle = draw(seed, SALT_WIND, (0.0, 360.0)).floor().to_radians();
    config.climate.wind_x = round(libm::cos(angle), 4) as f32;
    config.climate.wind_z = round(libm::sin(angle), 4) as f32;
    config
}

/// The config a world for `seed` is made with, for the tools: `OCHRE_CHARACTER=off` keeps `config` as given.
pub fn world_config(seed: u32, config: WorldConfig) -> WorldConfig {
    let off = std::env::var("OCHRE_CHARACTER").is_ok_and(|v| v == "off");
    world_config_with(seed, config, !off)
}

pub(crate) fn world_config_with(seed: u32, config: WorldConfig, derived: bool) -> WorldConfig {
    if derived {
        derive(seed, config)
    } else {
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldmap::{generate, WorldMap};
    use ochre_base::WATER_LEVEL;

    #[test]
    fn test_derive_is_a_pure_function_of_the_seed() {
        let base = WorldConfig::default();
        assert_eq!(derive(7, base), derive(7, base));
        assert_ne!(derive(7, base), derive(8, base));
    }

    #[test]
    fn test_derive_keeps_the_size_and_bounds() {
        let base = WorldConfig {
            size: 4096,
            ..Default::default()
        };
        let derived = derive(3, base);
        assert_eq!((derived.size, derived.map_cell), (base.size, base.map_cell));
        assert_eq!(derived.bounds, base.bounds);
    }

    #[test]
    fn test_a_thousand_seeds_give_valid_configs_that_fill_each_range() {
        let configs: Vec<_> = (0..1000)
            .map(|s| derive(s, WorldConfig::default()))
            .collect();
        for c in &configs {
            c.validate().unwrap();
            let wind = c.climate.wind_x.hypot(c.climate.wind_z);
            assert!((wind - 1.0).abs() < 0.001, "wind length {wind}");
        }
        let spread = |f: &dyn Fn(&WorldConfig) -> f64, (lo, hi): (f64, f64)| {
            let values: Vec<f64> = configs.iter().map(f).collect();
            let min = values.iter().copied().fold(f64::INFINITY, f64::min);
            let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let tolerance = 0.05 * (hi - lo);
            assert!(min - lo < tolerance, "min {min} not near {lo}");
            assert!(hi - max < tolerance, "max {max} not near {hi}");
        };
        spread(&|c| c.tectonics.plates as f64, (8.0, 16.0));
        spread(&|c| c.tectonics.relief as f64, RELIEF);
        spread(&|c| c.climate.warmth as f64, WARMTH);
        spread(&|c| c.climate.humidity as f64, HUMIDITY);
        spread(&|c| c.climate.gradient as f64, GRADIENT);
        spread(&|c| c.climate.continentality as f64, CONTINENTALITY);
    }

    #[test]
    fn test_landform_knobs_fill_their_range() {
        let shares: Vec<f64> = (0..1000)
            .map(|s| LandformKnobs::from_seed(s).tableland_share)
            .collect();
        let (min, max) = (
            shares.iter().copied().fold(f64::INFINITY, f64::min),
            shares.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        assert!(min >= TABLELAND_SHARE.0 && min < 0.23, "min {min}");
        assert!(max <= TABLELAND_SHARE.1 && max > 0.77, "max {max}");
        let scales: Vec<f64> = (0..1000)
            .map(|s| LandformKnobs::from_seed(s).dune_scale)
            .collect();
        assert!(scales
            .iter()
            .all(|&s| (DUNE_SCALE.0..=DUNE_SCALE.1).contains(&s)));
        assert!(scales.iter().any(|&s| s < 0.75) && scales.iter().any(|&s| s > 1.45));
        let boosts: Vec<f64> = (0..1000)
            .map(|s| LandformKnobs::from_seed(s).cliff_boost)
            .collect();
        assert!(boosts.iter().all(|&b| (0.0..=1.0).contains(&b)));
        assert!(boosts.iter().any(|&b| b < 0.03) && boosts.iter().any(|&b| b > 0.97));
        let valleys: Vec<f64> = (0..1000)
            .map(|s| LandformKnobs::from_seed(s).valley_share)
            .collect();
        assert!(valleys.iter().all(|&v| (VALLEY_SHARE.0..=VALLEY_SHARE.1).contains(&v)));
        assert!(valleys.iter().any(|&v| v < 0.53) && valleys.iter().any(|&v| v > 0.97));
        let cones: Vec<f64> = (0..1000)
            .map(|s| LandformKnobs::from_seed(s).cone_share)
            .collect();
        assert!(cones.iter().all(|&c| (0.0..=CONE_SHARE.1).contains(&c)));
        assert!(cones.iter().any(|&c| c < 0.01) && cones.iter().any(|&c| c > 0.24));
        let apart: Vec<f64> = (0..1000).map(|s| LandformKnobs::from_seed(s).landmark_apart).collect();
        assert!(apart.iter().all(|&a| (LANDMARK_APART.0..LANDMARK_APART.1).contains(&a)));
        assert!(apart.iter().any(|&a| a < 72.0) && apart.iter().any(|&a| a > 107.0));
        assert_eq!(LandformKnobs::from_seed(5), LandformKnobs::from_seed(5));
    }

    // two knobs on one salt would draw the same, so a seed's knobs would move together
    #[test]
    fn test_every_knob_has_its_own_salt() {
        let salts = [
            SALT_PLATES, SALT_RELIEF, SALT_WARMTH, SALT_GRADIENT, SALT_GRADIENT_ANGLE, SALT_CONTINENTALITY, SALT_WIND, SALT_TABLELAND,
            SALT_DUNES, SALT_CLIFFS, SALT_FJORDS, SALT_CONES, SALT_INSELBERGS, SALT_KARST, SALT_CIRQUES, SALT_VALLEYS, SALT_LANDMARKS,
            SALT_HUMIDITY,
        ];
        let unique: std::collections::BTreeSet<u32> = salts.iter().copied().collect();
        assert_eq!(salts.len(), unique.len());
    }

    #[test]
    fn test_off_returns_the_config_unchanged() {
        let base = WorldConfig::default();
        assert_eq!(world_config_with(5, base, false), base);
        assert_ne!(world_config_with(5, base, true), base);
    }

    // The extremes of relief and warmth together still make a map with land to stand on.
    #[test]
    fn test_corner_configs_make_land() {
        for relief in [0.7, 1.4] {
            for warmth in [0.2, 0.8] {
                let mut config = WorldConfig {
                    size: 2048,
                    ..Default::default()
                };
                config.tectonics.relief = relief;
                config.climate.warmth = warmth;
                let map: WorldMap = generate(1, config);
                let land = map.height.iter().any(|&h| h > WATER_LEVEL as f32);
                assert!(land, "no land at relief {relief} and warmth {warmth}");
            }
        }
    }
}
