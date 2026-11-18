//! Hands the land's shaders how hot the country is round the camera, on 16 compass points and 3 rings out, so hot country far off shows in the haze.

use std::f32::consts::TAU;

use bevy::prelude::*;
use ochre_terrain::{curvature::Curvature, world::World};

use super::{heat_of, Heat};

const POINTS: usize = 16;
// Blocks out to each ring, matching `HEAT_RINGS` in `curvature.wesl`.
const RINGS: [f32; 3] = [1000.0, 3000.0, 8000.0];
// Seconds between senses, and for the rings to glide to what they last sensed.
const SENSE_SECONDS: f32 = 0.5;
const EASE_SECONDS: f32 = 2.0;

#[derive(Default)]
pub(super) struct Rings {
    since: f32,
    started: bool,
    target: [[f32; 3]; POINTS],
    now: [[f32; 3]; POINTS],
}

pub(super) fn sense_haze(
    time: Res<Time>,
    world: Res<World>,
    heat: Res<Heat>,
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
            // the shader's turn is 0 at -pi round to 16 at pi, see `heat_toward`
            let angle = (point as f32 / POINTS as f32 - 0.5) * TAU;
            for (ring, distance) in RINGS.into_iter().enumerate() {
                let (x, z) = (eye.x + angle.cos() * distance, eye.z + angle.sin() * distance);
                rings.target[point][ring] = heat_of(terrain.climate(x as f64, z as f64));
            }
        }
        if !rings.started {
            rings.now = rings.target;
            rings.started = true;
        }
    }
    let step = 1.0 - (-time.delta_secs() / EASE_SECONDS).exp();
    for point in 0..POINTS {
        for ring in 0..3 {
            let (target, now) = (rings.target[point][ring], &mut rings.now[point][ring]);
            *now += (target - *now) * step;
        }
        curvature.heat[point * 4] = heat.0;
        curvature.heat[point * 4 + 1..point * 4 + 4].copy_from_slice(&rings.now[point]);
    }
}
