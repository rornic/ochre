// Fog for ChunkMaterial, WaterMaterial and CloudMaterial, which replace Bevy's PBR fragment
// stage and so must fog themselves. Each carries the curvature buffer, see `horizon.wesl`.

import bevy_pbr::render::{mesh_view_bindings as view_bindings, pbr_functions::apply_fog};
import super::horizon::horizon_haze;
import super::sky::SUN_LIGHT;

// Over this last share of the way to the fog's end, the land fades out altogether into the
// haze just over its edge, past the fog's opacity: the square edge of the farthest LOD nodes
// never shows against the sky, and nodes coming into reach fade in as the fog eases out.
const EDGE_FADE_START: f32 = 0.9;

// The haze thickens exponentially with distance, `FOG_SCALE` of the way to the fog's end for each e-fold,
// so mountains a few kilometres off are already softened: it's 1 - exp(-d / scale), stretched to just
// reach full at the end. A quadratic ramp to half at the farthest LOD level's start left all but the
// last stretch looking crisp.
const FOG_SCALE: f32 = 0.16;

// The haze is warmed toward the sun, more as the sun sinks: how much of the haze's own brightness the glow adds there.
const SUN_HAZE: f32 = 0.35;
const SUN_HAZE_COLOUR: vec3<f32> = vec3<f32>(1.0, 0.62, 0.32);

// `color` at `world_position` behind the camera's fog. Above water it thickens only with
// distance across the land, not up or down, so it hides the far edge of the world in the
// haze along the horizon without greying out the ground below when flying high.
// Underwater, Bevy's linear murk closes in every way. `WATER_LEVEL` comes from the
// material's `specialize`.
fn fogged(color: vec4<f32>, world_position: vec3<f32>, frag_coord: vec2<f32>) -> vec4<f32> {
    let fog = view_bindings::fog;
    let view_position = view_bindings::view.world_position.xyz;
    // under a river or lake too, over the sea's level: only the underwater fog starts at the camera
    if view_position.y < f32(constants::WATER_LEVEL) || fog.be.x <= 0.0 {
        return apply_fog(fog, color, world_position, view_position, frag_coord);
    }
    let end = max(fog.be.y, fog.be.x + 1.0);
    let across = world_position.xz - view_position.xz;
    let distance = length(across);
    var sky = horizon_haze(across);
    let scale = end * FOG_SCALE;
    if view_bindings::lights.n_directional_lights > 0u {
        let to_sun = view_bindings::lights.directional_lights[SUN_LIGHT].direction_to_light;
        let toward = max(dot(normalize(across), normalize(to_sun.xz + vec2<f32>(1e-5))), 0.0);
        let low = 1.0 - smoothstep(0.0, 0.6, to_sun.y);
        let glow = toward * toward * toward * (0.25 + 0.75 * low) * smoothstep(-0.1, 0.1, to_sun.y);
        sky += SUN_HAZE * glow * SUN_HAZE_COLOUR * dot(sky, vec3<f32>(0.2126, 0.7152, 0.0722));
    }
    let amount = (1.0 - exp(-distance / scale)) / (1.0 - exp(-end / scale));
    let opacity = mix(amount * fog.base_color.a, 1.0, smoothstep(EDGE_FADE_START, 1.0, distance / end));
    return vec4<f32>(mix(color.rgb, sky, opacity), color.a);
}
