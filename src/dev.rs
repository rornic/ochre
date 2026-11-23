use std::env::{var, var_os};

use ochre_gameplay::flythrough::FlightConfig;
use ochre_world::{SpawnAt, WorldOptions};

/// What the `OCHRE_*` environment variables set, for the sandbox and tools to pass on; the plugins read no environment.
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
    /// `OCHRE_STATS`.
    pub stats: bool,
    /// `OCHRE_FLIGHT`, with `OCHRE_FLIGHT_WARMUP` and `OCHRE_FLIGHT_FRAMES`.
    pub flight: Option<FlightConfig>,
    /// `OCHRE_REMOTE`, the port for `RemoteControlPlugin`: a port from 1024 up, off for `0`, `off` or `false`, else Bevy's 15702.
    pub remote: Option<u16>,
}

// `1` and `true` mean on, not port 1, which nothing can bind below 1024.
fn remote_port(value: &str) -> Option<u16> {
    match value.trim() {
        "" | "0" | "off" | "false" => None,
        v => Some(v.parse().ok().filter(|&p| p >= 1024).unwrap_or(bevy::remote::http::DEFAULT_PORT)),
    }
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
            stats: var_os("OCHRE_STATS").is_some(),
            flight: var("OCHRE_FLIGHT").ok().map(|file| FlightConfig {
                file,
                warmup: number("OCHRE_FLIGHT_WARMUP"),
                frames: var("OCHRE_FLIGHT_FRAMES").ok(),
            }),
            remote: var("OCHRE_REMOTE").ok().and_then(|v| remote_port(&v)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::remote_port;

    #[test]
    fn the_remote_port_is_a_port_or_the_default_and_never_a_flag_read_as_one() {
        let default = bevy::remote::http::DEFAULT_PORT;
        assert_eq!(Some(default), remote_port("1"));
        assert_eq!(Some(default), remote_port("on"));
        assert_eq!(Some(default), remote_port("true"));
        assert_eq!(Some(16000), remote_port("16000"));
        assert_eq!(Some(default), remote_port("80"));
        for off in ["", "0", "off", "false"] {
            assert_eq!(None, remote_port(off));
        }
    }
}
