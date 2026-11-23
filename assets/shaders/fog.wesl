// Fog for ChunkMaterial, WaterMaterial and CloudMaterial, which replace Bevy's PBR fragment stage; see `horizon.wesl`

import bevy_pbr::render::{mesh_view_bindings as view_bindings, pbr_functions::apply_fog};
import super::curvature::{cold, haze_toward, uncurved};
import super::horizon::horizon_haze;
import super::sky::SUN_LIGHT;

// Over this last share of the way to the fog's end, land fades into the haze so the farthest LOD nodes' edge hides.
const EDGE_FADE_START: f32 = 0.9;

// The haze thickens as 1 - exp(-d / scale), `FOG_SCALE` of the way to the fog's end per e-fold, stretched to reach
// full at the end; a quadratic ramp left all but the last stretch crisp.
const FOG_SCALE: f32 = 0.16;

// The haze is pale grey at the horizon, so over land it gets more colour, easing to the sky's by the fog's end.
const HAZE_SATURATION: f32 = 1.6;

// The haze is warmed toward the sun, more as it sinks: how much of the haze's brightness the glow adds there.
const SUN_HAZE: f32 = 0.08;
const SUN_HAZE_COLOUR: vec3<f32> = vec3<f32>(1.0, 0.62, 0.32);

// The haze thins as the camera climbs: from `THIN_FROM` over the sea it eases to `THIN_TO` by `THIN_FULL`.
const THIN_FROM: f32 = 150.0;
const THIN_FULL: f32 = 500.0;
const THIN_TO: f32 = 0.25;

// Looking steeply down from well above the ground the air is thin: haze fades over these slopes to `DOWN_TO`.
const DOWN_SLOPE: vec2<f32> = vec2<f32>(0.08, 0.5);
const DOWN_DROP: vec2<f32> = vec2<f32>(60.0, 250.0);
const DOWN_TO: f32 = 0.2;

// Mist over the fog where the camera's ground can frost (`frost.wesl`): `MIST_PER_BLOCK` at the ground,
// an e-fold thinner every `MIST_HEIGHT` blocks up; hides at most `MIST_MAX`; the haze goes `MIST_PALE` white.
const MIST_PER_BLOCK: f32 = 0.02;
const MIST_HEIGHT: f32 = 28.0;
const MIST_MAX: f32 = 0.85;
const MIST_PALE: f32 = 0.5;
const MIST_COLOUR: vec3<f32> = vec3<f32>(0.80, 0.87, 0.95);

// How misty the air at `view_position` is, 0 to 1: none with frost off, not thinned by the biome's thaw.
fn mist(view_position: vec3<f32>) -> f32 {
    let about = cold();
    let ground = about.camera.x - (view_position.y - f32(constants::WATER_LEVEL)) / about.rule.x;
    return (1.0 - smoothstep(about.rule.y, about.rule.y + about.rule.z, ground)) * step(0.5, about.camera.z);
}

// How much of the view the mist hides over `across` blocks to the real `height` (`chill` is `mist`): its mean density
fn mist_hides(view_position: vec3<f32>, height: f32, across: f32, chill: f32) -> f32 {
    // lower ground than the camera's has its own mist on it: from a ridge the lowlands aren't under a sheet of it
    let base = min(cold().camera.w, height);
    let up_here = max(view_position.y - base, 0.0) / MIST_HEIGHT;
    let up_there = max(height - base, 0.0) / MIST_HEIGHT;
    let rise = up_there - up_here;
    // level, the density at the camera, else the mean of exp(-up) between the two heights
    let mean = select((exp(-up_here) - exp(-up_there)) / select(rise, 1.0, abs(rise) < 0.01), exp(-up_here), abs(rise) < 0.01);
    return MIST_MAX * (1.0 - exp(-MIST_PER_BLOCK * chill * across * mean));
}

// `color` at `world_position` behind the fog: above water it thickens across the land only, so high flight
// doesn't grey the ground. Underwater, Bevy's linear murk closes in every way. `WATER_LEVEL` comes from `specialize`.
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
    // each country's own haze, from `ochre_world::atmosphere`: tinted, and thicker or thinner
    let country = haze_toward(across, distance);
    sky = sky * country.rgb;
    let haze_grey = dot(sky, vec3<f32>(0.2126, 0.7152, 0.0722));
    sky = mix(vec3<f32>(haze_grey), sky, mix(HAZE_SATURATION, 1.0, smoothstep(0.6, 0.95, distance / end)));
    let scale = end * FOG_SCALE / country.w;
    if view_bindings::lights.n_directional_lights > 0u {
        let to_sun = view_bindings::lights.directional_lights[SUN_LIGHT].direction_to_light;
        let toward = max(dot(normalize(across), normalize(to_sun.xz + vec2<f32>(1e-5))), 0.0);
        let low = 1.0 - smoothstep(0.0, 0.6, to_sun.y);
        let glow = toward * toward * toward * (0.25 + 0.75 * low) * smoothstep(-0.1, 0.1, to_sun.y);
        sky += SUN_HAZE * glow * SUN_HAZE_COLOUR * dot(sky, vec3<f32>(0.2126, 0.7152, 0.0722));
    }
    let amount = (1.0 - exp(-distance / scale)) / (1.0 - exp(-end / scale));
    let chill = mist(view_position);
    sky = mix(sky, MIST_COLOUR * dot(sky, vec3<f32>(0.2126, 0.7152, 0.0722)), MIST_PALE * chill);
    let thin = mix(1.0, THIN_TO, smoothstep(THIN_FROM, THIN_FULL, view_position.y - f32(constants::WATER_LEVEL)));
    let drop = view_position.y - uncurved(world_position).y;
    let down = smoothstep(DOWN_SLOPE.x, DOWN_SLOPE.y, drop / max(distance, 1.0)) * smoothstep(DOWN_DROP.x, DOWN_DROP.y, drop);
    let fog_opacity = mix(amount * fog.base_color.a * thin * mix(1.0, DOWN_TO, down), 1.0, smoothstep(EDGE_FADE_START, 1.0, distance / end));
    let mist_opacity = mist_hides(view_position, uncurved(world_position).y, distance, chill);
    let opacity = 1.0 - (1.0 - fog_opacity) * (1.0 - mist_opacity);
    return vec4<f32>(mix(color.rgb, sky, opacity), color.a);
}
