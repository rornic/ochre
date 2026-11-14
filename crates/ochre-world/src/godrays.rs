//! Light shafts: Bevy's volumetric fog, a box of thin air round the camera that the sun's shadow map carves into beams.

use bevy::{
    app::{App, Plugin, Update},
    color::Color,
    ecs::{
        component::Component,
        system::{Query, Res},
    },
    light::{FogVolume, VolumetricFog, VolumetricLight},
    math::Vec3,
    transform::components::Transform,
};

use crate::sky::{sun_direction, TimeOfDay};
use crate::weather::Weather;
use ochre_base::math::smoothstep;

/// Half the width of the box of air, in blocks: the march is 64 steps, so a bigger box makes beams blocky.
pub const REACH: f32 = 96.0;

// Per block. Bevy's defaults (0.1) are a thick fog; shafts need air a few blocks of which you barely see.
const DENSITY_NOON: f32 = 0.003;
const DENSITY_LOW_SUN: f32 = 0.016;
const ABSORPTION: f32 = 0.02;
const SCATTERING: f32 = 0.3;
// High, so shafts show looking toward the sun and fade looking away.
const ASYMMETRY: f32 = 0.75;
// The sun's illuminance already carries `SKY_GAIN`. By estimate (unseen) 0.25 of this left shafts near 0.1 against a sky of several.
const LIGHT: f32 = 2.5;

// The march's steps a pixel while the air is clear (it still has to run: the pass can't be dropped without unsorting the sun from the
// moon), at noon, and at its thickest. The shader takes this many shadow lookups for every pixel of the view whatever the ray's length,
// and the pass was over a third of the GPU's frame at 64.
const STEPS_CLEAR: u32 = 1;
const STEPS_NOON: u32 = 12;
const STEPS_LOW_SUN: u32 = 32;

/// The box of air round the camera, see `volume`.
#[derive(Component)]
pub struct Shafts;

/// The box, for the camera to carry: a child of it, so the box turns with the view and the beams don't shift as it turns.
pub fn volume() -> impl bevy::ecs::bundle::Bundle {
    (
        Shafts,
        FogVolume {
            fog_color: Color::WHITE,
            density_factor: 0.0,
            absorption: ABSORPTION,
            scattering: SCATTERING,
            scattering_asymmetry: ASYMMETRY,
            light_intensity: LIGHT,
            ..Default::default()
        },
        Transform::from_scale(Vec3::splat(2.0 * REACH)),
    )
}

/// Marks the sun to scatter light into the air, see `GodrayPlugin`.
pub fn shining() -> VolumetricLight {
    VolumetricLight
}

pub struct GodrayPlugin;

impl Plugin for GodrayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, thicken);
    }
}

/// How thick the air is: shafts are for a low sun, there's none at night, and rain's dark enough without them.
fn density(sun_height: f32, rain: f32) -> f32 {
    let low = 1.0 - smoothstep(0.0, 0.6, sun_height);
    let day = smoothstep(-0.05, 0.1, sun_height);
    (DENSITY_NOON + (DENSITY_LOW_SUN - DENSITY_NOON) * low) * day * (1.0 - rain)
}

/// How many steps the march takes through air `thickness` thick: one when there's none, more as shafts get brighter, since fewer
/// steps band where they're strong and nowhere else.
fn steps(thickness: f32) -> u32 {
    if thickness <= 0.0 {
        return STEPS_CLEAR;
    }
    let strength = ((thickness - DENSITY_NOON) / (DENSITY_LOW_SUN - DENSITY_NOON)).clamp(0.0, 1.0);
    (STEPS_NOON as f32 + (STEPS_LOW_SUN - STEPS_NOON) as f32 * strength).round() as u32
}

fn thicken(
    clock: Res<TimeOfDay>,
    weather: Res<Weather>,
    mut air: Query<&mut FogVolume, bevy::ecs::query::With<Shafts>>,
    mut march: Query<&mut VolumetricFog>,
) {
    let thickness = density(sun_direction(clock.days()).y, weather.rain);
    for mut fog in &mut air {
        if (fog.density_factor - thickness).abs() > 1e-6 {
            fog.density_factor = thickness;
        }
    }
    let wanted = steps(thickness);
    for mut fog in &mut march {
        if fog.step_count != wanted {
            fog.step_count = wanted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_march_is_one_step_in_clear_air_and_takes_more_as_the_air_thickens() {
        assert_eq!(steps(0.0), STEPS_CLEAR);
        assert_eq!(steps(DENSITY_NOON), STEPS_NOON);
        assert_eq!(steps(DENSITY_LOW_SUN), STEPS_LOW_SUN);
        let mut last = 0;
        for i in 0..=20 {
            let now = steps(DENSITY_NOON + (DENSITY_LOW_SUN - DENSITY_NOON) * i as f32 / 20.0);
            assert!(now >= last);
            last = now;
        }
    }

    #[test]
    fn test_air_is_thickest_at_dusk_and_gone_at_night_and_in_rain() {
        assert_eq!(density(-0.3, 0.0), 0.0);
        assert_eq!(density(0.3, 1.0), 0.0);
        assert!(density(0.1, 0.0) > density(0.9, 0.0));
        assert!((density(0.9, 0.0) - DENSITY_NOON).abs() < 1e-6);
    }
}
