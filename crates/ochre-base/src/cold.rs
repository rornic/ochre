//! What cold does to the land: where snow settles, and the frost that comes just before it, in one place for the mesher and the shaders.

use crate::math::smoothstep;

/// Height above the sea where snow starts to settle in a temperate climate (temperature 0.5).
pub const SNOW_LINE: i64 = 950;
/// How many blocks the snow line climbs per unit of temperature, which is also how many blocks of height cool the ground by one.
pub const SNOW_LINE_PER_DEGREE: f64 = 2600.0;
/// At or above this temperature it's too hot for snow, and the heights are bare rock.
pub const SNOW_MAX_TEMPERATURE: f32 = 0.62;
/// The lowest the snow line gets, so even the coldest lowlands are snow-covered.
pub const COLDEST_SNOW_LINE: i64 = 10;
/// Above this height over the sea the ground is snow-capped in any climate, in the tropics too.
pub const SNOWCAP_ABOVE: i64 = 1700;
/// Below this temperature even the grass is under a layer of snow.
pub const SNOWY_GRASS_TEMPERATURE: f32 = 0.25;

/// What the ground's temperature is at the snow line, in any climate: the line climbs as the climate warms at the rate height cools it.
pub const SNOW_TEMPERATURE: f32 = 0.5 - SNOW_LINE as f32 / SNOW_LINE_PER_DEGREE as f32;
/// How far warmer than snow's the ground's temperature may be and still have frost.
pub const FROST_BAND: f32 = 0.12;

/// Height above the sea where snow starts to settle, lower in the cold.
pub fn snow_line(temperature: f32) -> i64 {
    (SNOW_LINE as f64 + (temperature as f64 - 0.5) * SNOW_LINE_PER_DEGREE).max(COLDEST_SNOW_LINE as f64) as i64
}

/// The temperature of ground `above_sea` blocks up where the sea level's is `sea_temperature`.
pub fn ground_temperature(sea_temperature: f32, above_sea: f32) -> f32 {
    sea_temperature - above_sea / SNOW_LINE_PER_DEGREE as f32
}

/// How cold enough for frost the ground is, 0 to 1: full at the snow line's temperature, easing out over `FROST_BAND`. The shaders do
/// the same sum, see `frost.wesl`.
pub fn frost_cold(sea_temperature: f32, above_sea: f32) -> f32 {
    1.0 - smoothstep(SNOW_TEMPERATURE, SNOW_TEMPERATURE + FROST_BAND, ground_temperature(sea_temperature, above_sea))
}

/// What the shaders are given about the cold, in the order `Cold` has them in `curvature.wesl`: the camera's sea-level temperature,
/// how much frost its biome allows, whether frost is on, and the constants the sum needs.
pub fn shader_values(sea_temperature: f32, thaw: f32, on: bool) -> [f32; 8] {
    [
        sea_temperature,
        thaw,
        on as u8 as f32,
        0.0,
        SNOW_LINE_PER_DEGREE as f32,
        SNOW_TEMPERATURE,
        FROST_BAND,
        0.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snow_line_is_where_the_ground_cools_to_the_snow_temperature() {
        for t in [0.25_f32, 0.35, 0.5, 0.6] {
            let line = snow_line(t) as f32;
            assert!((ground_temperature(t, line) - SNOW_TEMPERATURE).abs() < 0.001, "{t}");
        }
        assert!((SNOW_TEMPERATURE - 0.1346).abs() < 0.001);
    }

    #[test]
    fn frost_runs_from_nothing_to_full_at_the_snow_line() {
        for t in [0.3_f32, 0.5, 0.6] {
            let line = snow_line(t) as f32;
            assert!(frost_cold(t, line) > 0.999, "{t}");
            assert!(frost_cold(t, line + 400.0) > 0.999, "{t}");
            // farther down than the band reaches there is none, and between the two it rises with height
            let band_blocks = FROST_BAND * SNOW_LINE_PER_DEGREE as f32;
            assert_eq!(0.0, frost_cold(t, line - band_blocks - 1.0), "{t}");
            let mut last = 0.0;
            for step in 0..=20 {
                let now = frost_cold(t, line - band_blocks + band_blocks * step as f32 / 20.0);
                assert!(now >= last - 1e-6, "{t} at step {step}");
                last = now;
            }
        }
    }

    #[test]
    fn the_shader_values_keep_their_order() {
        let v = shader_values(0.2, 0.5, true);
        assert_eq!((v[0], v[1], v[2]), (0.2, 0.5, 1.0));
        assert_eq!((v[4], v[5], v[6]), (SNOW_LINE_PER_DEGREE as f32, SNOW_TEMPERATURE, FROST_BAND));
        assert_eq!(0.0, shader_values(0.2, 0.5, false)[2]);
    }
}
