//! Senses the cold where the camera is and hands it to the land's shaders in the shared curvature buffer, see `ochre_base::cold`.

use bevy::{
    app::{App, Plugin, Update},
    camera::Camera3d,
    ecs::{
        query::With,
        resource::Resource,
        system::{Local, Query, Res, ResMut},
    },
    time::Time,
    transform::components::GlobalTransform,
};

use crate::curvature::Curvature;
use crate::world::World;
use bevy::math::Vec3;
use ochre_base::{cold, WATER_LEVEL};

// How often the camera's climate is read, and how long what the shaders see takes to ease to it, in seconds.
const SENSE_SECONDS: f32 = 0.25;
const EASE_SECONDS: f32 = 2.0;

/// Gives the land's shaders the cold at the camera, when `on`: frost needs nothing else.
pub struct FrostPlugin {
    pub on: bool,
}

impl Plugin for FrostPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FrostSwitch(self.on))
            .add_systems(Update, sense_frost);
    }
}

/// Whether frost is on, as `FrostPlugin` was given.
#[derive(Resource)]
pub struct FrostSwitch(pub bool);

/// How much of the ground at `feet` the cold frosts, 0 to 1: the shaders' cover before their patchiness, so 0.5 is about half covered.
pub fn frost_cover(world: &World, feet: Vec3) -> f32 {
    let terrain = world.region_store.terrain();
    let (x, z) = (feet.x.floor() as f64, feet.z.floor() as f64);
    let thaw = terrain.biomes().frost(terrain.biome(x, z));
    cold::frost_cold(terrain.temperature(x, z), feet.y - WATER_LEVEL as f32) * thaw
}

// What the shaders see: the camera's sea-level temperature and how much of the cold's frost its biome keeps, with where each is heading.
#[derive(Default)]
struct Seen {
    now: [f32; 2],
    target: [f32; 2],
    since: f32,
    started: bool,
}

// `now` moved toward `target` over `EASE_SECONDS` after `dt`.
fn eased(now: f32, target: f32, dt: f32) -> f32 {
    now + (target - now) * (1.0 - (-dt / EASE_SECONDS).exp())
}

fn sense_frost(
    time: Res<Time>,
    switch: Res<FrostSwitch>,
    world: Option<Res<World>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut curvature: ResMut<Curvature>,
    mut seen: Local<Seen>,
) {
    if !switch.0 {
        curvature.cold = cold::shader_values(0.0, 1.0, false);
        return;
    }
    let (Some(world), Ok(camera)) = (world, camera.single()) else {
        return;
    };
    seen.since += time.delta_secs();
    if !seen.started || seen.since >= SENSE_SECONDS {
        seen.since = 0.0;
        let eye = camera.translation();
        let terrain = world.region_store.terrain();
        let (x, z) = (eye.x.floor() as f64, eye.z.floor() as f64);
        let thaw = terrain.biomes().frost(terrain.biome(x, z));
        seen.target = [terrain.temperature(x, z), thaw];
        if !seen.started {
            seen.now = seen.target;
            seen.started = true;
        }
    }
    let dt = time.delta_secs();
    let target = seen.target;
    seen.now = [eased(seen.now[0], target[0], dt), eased(seen.now[1], target[1], dt)];
    curvature.cold = cold::shader_values(seen.now[0], seen.now[1], true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_closes_on_the_target_and_never_passes_it() {
        let (mut now, target) = (0.9, 0.1);
        let mut last = now;
        for _ in 0..600 {
            now = eased(now, target, 1.0 / 60.0);
            assert!(now <= last && now >= target, "{now}");
            last = now;
        }
        assert!((now - target).abs() < 0.01, "{now}");
        // and over a long pause it lands on it
        assert!((eased(0.9, target, 1000.0) - target).abs() < 1e-6);
        assert_eq!(0.5, eased(0.5, 0.5, 0.016));
    }

    #[test]
    fn frost_off_hands_the_shaders_a_zero_strength() {
        assert_eq!(0.0, cold::shader_values(0.1, 0.5, false)[2]);
    }
}
