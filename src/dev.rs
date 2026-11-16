use std::env::{var, var_os};

use ochre_gameplay::flythrough::FlightConfig;
use ochre_world::{SpawnAt, WorldOptions};

/// What the `OCHRE_*` environment variables set, for the sandbox and tools to pass on: `OchrePlugins` and the plugins
/// themselves read no environment. `DevEnv::default()` sets none of it.
#[derive(Default)]
pub struct DevEnv {
    /// `OCHRE_SEED`, `OCHRE_WORLD_DIR`, `OCHRE_CHARACTER=off` and `OCHRE_SPAWN=x,z[,lift]`.
    pub world: WorldOptions,
    /// `OCHRE_TIME`, the time of day the clock starts at, in days.
    pub time: Option<f32>,
    /// `OCHRE_TIMESCALE`, game seconds a real second.
    pub time_scale: Option<f32>,
    /// `OCHRE_CLEAR_SKY`.
    pub clear_sky: bool,
    /// `OCHRE_MUSIC_DELAY`, seconds before the first track.
    pub music_delay: Option<f32>,
    /// `OCHRE_STATS`.
    pub stats: bool,
    /// `OCHRE_FLIGHT`, with `OCHRE_FLIGHT_WARMUP` and `OCHRE_FLIGHT_FRAMES`.
    pub flight: Option<FlightConfig>,
}

fn number<T: std::str::FromStr>(name: &str) -> Option<T> {
    var(name).ok()?.parse().ok()
}

impl DevEnv {
    pub fn from_env() -> Self {
        let defaults = WorldOptions::default();
        Self {
            world: WorldOptions {
                seed: number("OCHRE_SEED"),
                dir: var_os("OCHRE_WORLD_DIR").map_or(defaults.dir, Into::into),
                character: var("OCHRE_CHARACTER").map_or(true, |v| v != "off"),
                spawn: var("OCHRE_SPAWN").ok().and_then(|s| {
                    let mut parts = s.split(',').map(|p| p.trim().parse::<f64>().ok());
                    Some(SpawnAt {
                        x: parts.next()??,
                        z: parts.next()??,
                        lift: parts.next().flatten(),
                    })
                }),
            },
            time: number("OCHRE_TIME"),
            time_scale: number("OCHRE_TIMESCALE").filter(|s: &f32| s.is_finite() && *s >= 0.0),
            clear_sky: var_os("OCHRE_CLEAR_SKY").is_some(),
            music_delay: number("OCHRE_MUSIC_DELAY"),
            stats: var_os("OCHRE_STATS").is_some(),
            flight: var("OCHRE_FLIGHT").ok().map(|file| FlightConfig {
                file,
                warmup: number("OCHRE_FLIGHT_WARMUP"),
                frames: var("OCHRE_FLIGHT_FRAMES").ok(),
            }),
        }
    }
}
