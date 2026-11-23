//! The feel of each kind of country, as sun, grade, haze and storms, blended smoothly by climate.

use bevy::{prelude::*, render::view::ColorGrading};
use ochre_base::{
    biome::{Climate, Modifier},
    math::smoothstep,
    terrain_source::TerrainSource,
};

/// How much the country at a point is each kind, each 0..1 and smooth in climate; they overlap where kinds meet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Feel {
    pub cold: f32,
    pub hot: f32,
    pub jungle: f32,
    pub forest: f32,
    pub steppe: f32,
    /// Over mountain, smooth across its edge.
    pub mountain: f32,
    /// Over volcanic country, which a biome's border makes a step of, and easing smooths for the camera.
    pub volcanic: f32,
}

impl Feel {
    pub fn at(terrain: &dyn TerrainSource, x: f64, z: f64) -> Self {
        let biome = terrain.biome(x, z);
        Self {
            mountain: terrain.mountain_cover(x, z),
            volcanic: (terrain.biomes().modifier(biome) == Modifier::Volcanic) as u8 as f32,
            ..Self::of(terrain.climate(x, z), terrain.temperature(x, z))
        }
    }

    /// The feel of country with this `climate`; `sea_temperature` is the map's temperature there, for judging the cold.
    pub fn of(climate: Climate, sea_temperature: f32) -> Self {
        let (t, h) = (climate.temperature, climate.humidity);
        let warm = smoothstep(0.50, 0.62, t);
        let jungle = warm * smoothstep(0.55, 0.70, h);
        Self {
            cold: 1.0 - smoothstep(0.15, 0.35, sea_temperature),
            // jungle is humid, not scorching
            hot: warm * (1.0 - smoothstep(0.45, 0.75, h)),
            jungle,
            forest: smoothstep(0.38, 0.58, h) * smoothstep(0.12, 0.26, t) * (1.0 - jungle),
            steppe: (1.0 - smoothstep(0.25, 0.35, h)) * smoothstep(0.20, 0.30, t) * (1.0 - warm),
            mountain: 0.0,
            volcanic: 0.0,
        }
    }

    /// Moves each weight `step` (0..1) of the way to `target`, to an exact zero once it is near one that is.
    pub fn ease_towards(&mut self, target: Feel, step: f32) {
        let ease = |now: &mut f32, to: f32| {
            *now += (to - *now) * step;
            if to == 0.0 && *now < 1e-3 {
                *now = 0.0;
            }
        };
        ease(&mut self.cold, target.cold);
        ease(&mut self.hot, target.hot);
        ease(&mut self.jungle, target.jungle);
        ease(&mut self.forest, target.forest);
        ease(&mut self.steppe, target.steppe);
        ease(&mut self.mountain, target.mountain);
        ease(&mut self.volcanic, target.volcanic);
    }
}

/// `Surroundings::feel` eased over a few seconds, since it is sensed in steps, for the grade, sun and haze to follow.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct EasedFeel(pub Feel);

/// What a kind of country does at full weight, as changes to a neutral look; the cold's grade is in `tonemap`.
struct Row {
    // how much more (or less) of the sun's strength
    sun: f32,
    sun_colour: [f32; 3],
    // added to the grade's white balance, and its saturation and midtone contrast as shares more
    temperature: f32,
    saturation: f32,
    contrast: f32,
    // what the haze is multiplied by, and how much more (or less) of it there is
    haze_tint: [f32; 3],
    haze_density: f32,
    // how much deeper a storm dims the light
    storm: f32,
    // the colour the sky's light is pushed toward in shadow, and how much more or less far
    shadow_tint: [f32; 3],
    shadow_amount: f32,
}

const SUN: [f32; 3] = [1.0, 0.9, 0.75]; // `sky::SUN_COLOR`, which a test checks
// what painterly lighting pushes the sky's light toward in shadow, and how far; `SHADOW_TINT` in `painterly.wesl`
pub const SHADOW: [f32; 3] = [0.55, 0.63, 1.0];
pub const SHADOW_AMOUNT: f32 = 0.3;
const COLD: Row = Row { sun: 0.0, sun_colour: SUN, temperature: 0.0, saturation: 0.0, contrast: 0.0, haze_tint: [0.88, 0.95, 1.08], haze_density: 0.45, storm: 0.0, shadow_tint: SHADOW, shadow_amount: 0.0 };
// a hard, hot sun on bleached land, in a thick warm haze
const HOT: Row = Row { sun: 0.5, sun_colour: [1.0, 0.86, 0.66], temperature: 0.03, saturation: 0.04, contrast: 0.06, haze_tint: [1.0, 0.86, 0.64], haze_density: 1.2, storm: 0.0, shadow_tint: [0.80, 0.60, 0.55], shadow_amount: 0.10 };
// hot, sticky and wet: vibrant colour in a thick, green haze that closes in past a kilometre, the sun filtered
const JUNGLE: Row = Row { sun: -0.12, sun_colour: [1.0, 0.92, 0.70], temperature: 0.015, saturation: 0.20, contrast: -0.04, haze_tint: [0.92, 1.0, 0.90], haze_density: 3.0, storm: 0.0, shadow_tint: [0.40, 0.65, 0.55], shadow_amount: 0.15 };
// sheltered, cosy and gloomier: a dimmer, warmer sun and a soft close haze. A taiga is this with the cold over it
const FOREST: Row = Row { sun: -0.2, sun_colour: [1.0, 0.89, 0.72], temperature: 0.01, saturation: 0.04, contrast: -0.03, haze_tint: [0.95, 0.95, 0.90], haze_density: 0.8, storm: 0.0, shadow_tint: [0.62, 0.55, 0.70], shadow_amount: 0.10 };
// vast, desolate and exposed: drained colour, a hard clear light, far views, and a sky that turns on you
const STEPPE: Row = Row { sun: 0.1, sun_colour: [0.99, 0.93, 0.82], temperature: -0.004, saturation: -0.10, contrast: 0.10, haze_tint: [0.92, 0.95, 1.0], haze_density: -0.35, storm: 0.5, shadow_tint: [0.62, 0.68, 0.85], shadow_amount: -0.10 };

// high, thin, crisp air: a clear hard light and far views, and weather that comes up fast
const MOUNTAIN: Row = Row { sun: 0.15, sun_colour: [0.99, 0.93, 0.82], temperature: -0.01, saturation: -0.04, contrast: 0.08, haze_tint: [0.92, 0.96, 1.04], haze_density: -0.4, storm: 0.3, shadow_tint: [0.50, 0.60, 1.0], shadow_amount: 0.05 };
// ashen and ominous: a dim red sun through a grey-brown veil, drained colour, maroon shadows
const VOLCANIC: Row = Row { sun: -0.2, sun_colour: [1.0, 0.78, 0.6], temperature: 0.01, saturation: -0.12, contrast: 0.1, haze_tint: [0.85, 0.78, 0.75], haze_density: 1.2, storm: 0.3, shadow_tint: [0.70, 0.50, 0.50], shadow_amount: 0.15 };

/// What the country's feel makes of the light: multipliers on a neutral look, exactly neutral at no feel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    pub sun: f32,
    pub sun_colour: [f32; 3],
    pub temperature: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub haze_tint: [f32; 3],
    pub haze_density: f32,
    pub storm: f32,
    pub shadow_tint: [f32; 3],
    pub shadow_amount: f32,
}

impl Atmosphere {
    pub fn of(feel: &Feel) -> Self {
        let mut a = Atmosphere {
            sun: 1.0,
            sun_colour: SUN,
            temperature: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            haze_tint: [1.0; 3],
            haze_density: 1.0,
            storm: 1.0,
            shadow_tint: SHADOW,
            shadow_amount: SHADOW_AMOUNT,
        };
        // the jungle's haze goes by the fourth power of its weight, so a jungle's edge isn't 3x hazy
        for (weight, haze_power, row) in [(feel.cold, 1, &COLD), (feel.hot, 1, &HOT), (feel.jungle, 4, &JUNGLE), (feel.forest, 1, &FOREST), (feel.steppe, 1, &STEPPE), (feel.mountain, 1, &MOUNTAIN), (feel.volcanic, 1, &VOLCANIC)] {
            a.sun += weight * row.sun;
            a.temperature += weight * row.temperature;
            a.saturation += weight * row.saturation;
            a.contrast += weight * row.contrast;
            a.haze_density += weight.powi(haze_power) * row.haze_density;
            a.storm += weight * row.storm;
            a.shadow_amount += weight * row.shadow_amount;
            for (i, base) in SUN.into_iter().enumerate() {
                a.sun_colour[i] += weight * (row.sun_colour[i] - base);
                a.haze_tint[i] += weight * (row.haze_tint[i] - 1.0);
                a.shadow_tint[i] += weight * (row.shadow_tint[i] - SHADOW[i]);
            }
        }
        a.sun = a.sun.max(0.4);
        a.saturation = a.saturation.max(0.6);
        a.haze_density = a.haze_density.max(0.3);
        a.shadow_amount = a.shadow_amount.clamp(0.05, 0.6);
        a
    }

    pub fn sun_color(&self) -> Color {
        Color::srgb(self.sun_colour[0], self.sun_colour[1], self.sun_colour[2])
    }

    /// `grading` with this feel's white balance, saturation and contrast laid over it: unchanged at a neutral feel.
    pub fn grade(&self, grading: &ColorGrading) -> ColorGrading {
        let mut graded = grading.clone();
        graded.global.temperature += self.temperature;
        graded.global.post_saturation *= self.saturation;
        graded.midtones.contrast *= self.contrast;
        graded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{sky::SUN_COLOR, tonemap::dry_color_grading};

    fn feel_of(temperature: f32, humidity: f32) -> Feel {
        Feel::of(Climate { temperature, humidity }, temperature)
    }

    #[test]
    fn no_feel_leaves_the_light_exactly_as_it_was() {
        let none = Atmosphere::of(&Feel::default());
        assert_eq!(1.0, none.sun);
        assert_eq!(SUN_COLOR, none.sun_color());
        assert_eq!([1.0; 3], none.haze_tint);
        assert_eq!(1.0, none.haze_density);
        assert_eq!((SHADOW, SHADOW_AMOUNT), (none.shadow_tint, none.shadow_amount));
        let dry = dry_color_grading();
        assert_eq!(dry, none.grade(&dry));
    }

    #[test]
    fn each_kind_of_country_is_found_where_it_grows() {
        let (hot, jungle, forest, steppe, tundra) = (feel_of(0.8, 0.05), feel_of(0.8, 0.9), feel_of(0.45, 0.8), feel_of(0.45, 0.1), feel_of(0.15, 0.1));
        assert!(hot.hot > 0.99 && hot.jungle == 0.0 && hot.steppe == 0.0);
        assert!(jungle.jungle > 0.99 && jungle.hot == 0.0);
        assert!(forest.forest > 0.99 && forest.jungle == 0.0 && forest.hot == 0.0);
        assert!(steppe.steppe > 0.99 && steppe.forest == 0.0);
        assert!(tundra.cold > 0.99 && tundra.forest == 0.0);
        // a taiga is a forest's gloom under the cold
        let taiga = feel_of(0.2, 0.8);
        assert!(taiga.forest > 0.5 && taiga.cold > 0.5);
    }

    #[test]
    fn the_feels_pull_the_light_the_way_they_are_meant_to() {
        let at = |t, h| Atmosphere::of(&feel_of(t, h));
        let (hot, jungle, forest, steppe) = (at(0.8, 0.05), at(0.8, 0.9), at(0.45, 0.8), at(0.45, 0.1));
        assert!(hot.sun > 1.3 && jungle.sun < 1.0 && forest.sun < jungle.sun && steppe.sun > 1.0);
        assert!(jungle.saturation > hot.saturation && steppe.saturation < 1.0);
        assert!(jungle.haze_density > forest.haze_density && forest.haze_density > 1.0 && steppe.haze_density < 1.0);
        assert!(steppe.storm > 1.0 && forest.storm == 1.0);
        // the shadows take their country's colour: warm where it is hot, green in the jungle, and bluer on a peak
        assert!(hot.shadow_tint[0] > SHADOW[0] && jungle.shadow_tint[1] > jungle.shadow_tint[0]);
        assert!(steppe.shadow_amount < SHADOW_AMOUNT && jungle.shadow_amount > SHADOW_AMOUNT);
        // the cold's haze is bluer than the hot's
        let cold = at(0.1, 0.1);
        assert!(cold.haze_tint[2] > cold.haze_tint[0] && hot.haze_tint[0] > hot.haze_tint[2]);
    }

    #[test]
    fn mountains_are_crisp_and_volcanic_country_is_dim_and_ashen() {
        let none = Atmosphere::of(&Feel::default());
        let mountain = Atmosphere::of(&Feel { mountain: 1.0, ..Feel::default() });
        let volcanic = Atmosphere::of(&Feel { volcanic: 1.0, ..Feel::default() });
        assert!(mountain.sun > none.sun && mountain.haze_density < 1.0 && mountain.storm > 1.0);
        assert!(volcanic.sun < none.sun && volcanic.haze_density > 1.0);
        assert!(volcanic.sun_colour[2] < none.sun_colour[2] && volcanic.haze_tint[0] < 1.0);
        // the weights add where the kinds overlap, and stay inside what the light can take
        let both = Atmosphere::of(&Feel { cold: 1.0, hot: 1.0, jungle: 1.0, forest: 1.0, steppe: 1.0, mountain: 1.0, volcanic: 1.0 });
        assert!(both.sun >= 0.4 && both.saturation >= 0.6 && both.haze_density >= 0.3);
        assert!((0.05..=0.6).contains(&both.shadow_amount));
    }
}
