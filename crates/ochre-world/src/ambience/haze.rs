//! Hands the land's shaders the haze of the country round the camera, on 16 compass points and 3 rings out, so a far biome shows in the haze.

use std::f32::consts::TAU;

use bevy::prelude::*;
use ochre_terrain::{curvature::Curvature, world::World};

use super::EasedFeel;
use crate::atmosphere::{Atmosphere, Feel};

const POINTS: usize = 16;
// Blocks out to each ring, matching `HAZE_RINGS` in `curvature.wesl`.
const RINGS: [f32; 3] = [1000.0, 3000.0, 8000.0];
// Seconds between senses, and for the rings to glide to what they last sensed.
const SENSE_SECONDS: f32 = 0.5;
const EASE_SECONDS: f32 = 2.0;

// A haze's tint and how much more or less of it there is: what a ring point hands the shader.
type Haze = [f32; 4];

fn haze_of(atmosphere: &Atmosphere) -> Haze {
    let [r, g, b] = atmosphere.haze_tint;
    [r, g, b, atmosphere.haze_density]
}

#[derive(Default)]
pub(super) struct Rings {
    since: f32,
    started: bool,
    target: [[Haze; 3]; POINTS],
    now: [[Haze; 3]; POINTS],
}

pub(super) fn sense_haze(
    time: Res<Time>,
    world: Res<World>,
    eased: Res<EasedFeel>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    curvature: Option<ResMut<Curvature>>,
    mut rings: Local<Rings>,
) {
    let (Ok(camera), Some(mut curvature)) = (camera.single(), curvature) else {
        return;
    };
    rings.since += time.delta_secs();
    if !rings.started || rings.since >= SENSE_SECONDS {
        rings.since = 0.0;
        let eye = camera.translation();
        let terrain = world.region_store.terrain();
        for point in 0..POINTS {
            // the shader's turn is 0 at -pi round to 16 at pi, see `haze_toward`
            let angle = (point as f32 / POINTS as f32 - 0.5) * TAU;
            for (ring, distance) in RINGS.into_iter().enumerate() {
                let (x, z) = ((eye.x + angle.cos() * distance) as f64, (eye.z + angle.sin() * distance) as f64);
                rings.target[point][ring] = haze_of(&Atmosphere::of(&Feel::at(terrain, x, z)));
            }
        }
        if !rings.started {
            rings.now = rings.target;
            rings.started = true;
        }
    }
    let step = 1.0 - (-time.delta_secs() / EASE_SECONDS).exp();
    let atmosphere = Atmosphere::of(&eased.0);
    let here = haze_of(&atmosphere);
    // the camera's own country sets the colour of the shadows
    let [r, g, b] = atmosphere.shadow_tint;
    curvature.shadow = [r, g, b, atmosphere.shadow_amount];
    for point in 0..POINTS {
        for ring in 0..3 {
            let target = rings.target[point][ring];
            for (now, to) in rings.now[point][ring].iter_mut().zip(target) {
                *now += (to - *now) * step;
            }
        }
        let (first, hazes) = (point * 16, [here, rings.now[point][0], rings.now[point][1], rings.now[point][2]]);
        for (i, haze) in hazes.into_iter().enumerate() {
            curvature.haze[first + i * 4..first + i * 4 + 4].copy_from_slice(&haze);
        }
    }
}
