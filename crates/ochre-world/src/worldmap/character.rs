//! What kind of world a seed makes: how many plates, how high the relief, how warm, which way the
//! wind blows. Each is drawn from the seed within a range, so seeds differ in more than their shapes.

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

/// What a seed decides about its landforms, worked out where it is used rather than stored in the
/// config or the map file, so a new landform adds no file format change. `OCHRE_CHARACTER=off` does not touch them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LandformKnobs {
    /// About how much of the sedimentary lowland is tableland.
    pub tableland_share: f64,
    /// Multiplies the spacing and height of dunes.
    pub dune_scale: f64,
}

impl LandformKnobs {
    pub fn from_seed(seed: u32) -> Self {
        Self {
            tableland_share: round(draw(seed, SALT_TABLELAND, TABLELAND_SHARE), 3),
            dune_scale: round(draw(seed, SALT_DUNES, DUNE_SCALE), 2),
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

/// The config a world for `seed` is made with. `OCHRE_CHARACTER=off` keeps `config` as given.
pub fn world_config(seed: u32, config: WorldConfig) -> WorldConfig {
    let off = std::env::var("OCHRE_CHARACTER").is_ok_and(|v| v == "off");
    world_config_with(seed, config, !off)
}

pub fn world_config_with(seed: u32, config: WorldConfig, derived: bool) -> WorldConfig {
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
        assert_eq!(LandformKnobs::from_seed(5), LandformKnobs::from_seed(5));
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
