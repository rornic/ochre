// Clouds' shadows on the land: where the sun's rays cross the cloud layer, looked up in its pattern so they drift.

import bevy_pbr::render::mesh_view_bindings as view_bindings;
import super::sky::SUN_LIGHT;
import super::cloud_bindings::{cloud_pattern, cloud_sampler, cloud_layer, sky_cover, weather_at};

// How much sun a cloud blocks, more under a storm's, and the cover its shadow darkens over; see `cloud_march.wesl`
const CLOUD_SHADE: f32 = 0.7;
const STORM_SHADE: f32 = 0.88;
const SHADOW_COVER: vec2<f32> = vec2<f32>(0.35, 0.8);

// How much sunlight reaches `world_position` past the clouds, 0..1.
fn cloud_shadow(world_position: vec3<f32>) -> f32 {
    let to_sun = view_bindings::lights.directional_lights[SUN_LIGHT].direction_to_light;
    let layer = cloud_layer.x;
    if to_sun.y <= 0.0 || world_position.y >= layer {
        return 1.0;
    }
    let under = world_position.xz + to_sun.xz * ((layer - world_position.y) / to_sun.y);
    // `globals.time` wraps on the hour, when the wind has carried the clouds a whole number of patterns
    let drift = cloud_layer.zw * view_bindings::globals.time;
    let share = textureSampleLevel(cloud_pattern, cloud_sampler, (under - drift) / cloud_layer.y, 0.0).r;
    let weather = weather_at(under);
    let cover = sky_cover(share, weather);
    return 1.0 - mix(CLOUD_SHADE, STORM_SHADE, weather.storm) * smoothstep(SHADOW_COVER.x, SHADOW_COVER.y, cover);
}
