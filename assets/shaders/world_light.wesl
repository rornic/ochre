// The sun, sky and cloud light on a point, for the land (`world.wesl`) and any game material lit as the land is.

import bevy_pbr::render::{
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    view_transformations::position_world_to_view,
};
import super::lighting::{direct_moon, open_sky, sun_radiance};
import super::painterly::{FAR_VISIBILITY, Painterly, painterly_light};
import super::sky::SUN_LIGHT;
import super::cloud_shadow::cloud_shadow;
import super::curvature::uncurved;
import super::hash::value_noise;

// All that's left with no sky light at all, so caves aren't pitch black.
const CAVE_AMBIENT: vec3<f32> = vec3<f32>(0.02, 0.02, 0.025);
// Above 1, light falls away quickly at first under cover, then more slowly.
const SKY_LIGHT_CURVE: f32 = 2.0;
// Shadow maps only reach so far, so beyond them sunlight is kept out of caves by sky
// light instead: none gets in below this much of it.
const SUN_SKY_GATE: f32 = 0.3;
// Shadows fade out over this last part of their reach, instead of stopping at a line.
const SHADOW_FADE_START: f32 = 0.8;

// Light from the sky for sky light 0..1, see `light.rs`.
fn sky_ambient(sky: f32, normal: vec3<f32>) -> vec3<f32> {
    return CAVE_AMBIENT + open_sky(normal) * pow(clamp(sky, 0.0, 1.0), SKY_LIGHT_CURVE);
}

// The normal bias is a share of a cascade's texel size, so a far shadow's edge moves by blocks: it eases down here
const FAR_NORMAL_BIAS: f32 = 0.33;
const NORMAL_BIAS_FADE: vec2<f32> = vec2<f32>(60.0, 400.0);

// Ground facets are about a block across and tilted, so a face the sun grazes is shadowed by neighbours in teeth
// that jump as the sun steps: the lookup is lifted toward the sun by this many blocks, past the facets.
const FACET_LIFT: f32 = 0.6;

// A world-anchored brush of this size and reach, in blocks, moves shadow outlines, so they wobble and hold still.
const BRUSH: f32 = 2.5;
const BRUSH_REACH: f32 = 0.45;
// Shade this near the light, plus a share of distance, lifts to `HALF_TONE`: a band in blobs as the brush turns it.
const BAND: f32 = 1.1;
const BAND_GROWTH: f32 = 0.01;
const HALF_TONE: f32 = 0.5;

// `offset` slid onto the face's plane, so a moved lookup doesn't sink into a slope and shadow itself.
fn along_face(offset: vec2<f32>, normal: vec3<f32>) -> vec4<f32> {
    let flat_offset = vec3<f32>(offset.x, 0.0, offset.y);
    return vec4<f32>(flat_offset - normal * dot(flat_offset, normal), 0.0);
}

fn painted_shadow(world_position: vec4<f32>, normal: vec3<f32>, view_z: f32, frag_coord: vec2<f32>) -> f32 {
    let xz = world_position.xz / BRUSH;
    let wobble = vec2<f32>(value_noise(xz), value_noise(xz + 17.3)) - 0.5;
    let at = world_position + along_face(wobble * 2.0 * BRUSH_REACH, normalize(normal));
    let core = fetch_directional_shadow(SUN_LIGHT, at, normal, view_z, frag_coord);
    if core > 0.99 {
        return core;
    }
    let turn = value_noise(xz * 0.5 + 41.7) * 6.2832;
    let reach = (BAND + BAND_GROWTH * -view_z) * vec2<f32>(cos(turn), sin(turn));
    let side = along_face(reach, normalize(normal));
    let ahead = fetch_directional_shadow(SUN_LIGHT, at + side, normal, view_z, frag_coord);
    let behind = fetch_directional_shadow(SUN_LIGHT, at - side, normal, view_z, frag_coord);
    let near_light = max(ahead, behind);
    return mix(core, max(core, HALF_TONE), smoothstep(0.3, 0.7, near_light));
}

fn sun_shadow(world_position: vec4<f32>, normal: vec3<f32>, frag_coord: vec2<f32>) -> f32 {
    let sun = &view_bindings::lights.directional_lights[SUN_LIGHT];
    if ((*sun).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
        return 1.0;
    }
    let view_z = position_world_to_view(world_position.xyz).z;
    let reach = (*sun).cascades[(*sun).num_cascades - 1u].far_bound;
    if -view_z >= reach {
        return FAR_VISIBILITY;
    }
    let lifted = vec4<f32>(world_position.xyz + (*sun).direction_to_light * FACET_LIFT, world_position.w);
    let shadow = painted_shadow(lifted, normal * mix(1.0, FAR_NORMAL_BIAS, smoothstep(NORMAL_BIAS_FADE.x, NORMAL_BIAS_FADE.y, -view_z)), view_z, frag_coord);
    return mix(shadow, FAR_VISIBILITY, smoothstep(reach * SHADOW_FADE_START, reach, -view_z));
}

// The light on a face, shadowed by land and clouds, plus unshadowed moonlight kept out of caves by sky light.
fn painterly_world_light(world_position: vec4<f32>, normal: vec3<f32>, shaded: vec3<f32>, sky: f32, frag_coord: vec2<f32>, leaf: bool) -> Painterly {
    let gate = smoothstep(0.0, SUN_SKY_GATE, sky);
    var to_sun = vec3<f32>(0.0, 1.0, 0.0);
    var sun = vec3<f32>(0.0);
    var visibility = 0.0;
    if view_bindings::lights.n_directional_lights > 0u {
        to_sun = view_bindings::lights.directional_lights[SUN_LIGHT].direction_to_light;
        sun = sun_radiance(uncurved(world_position.xyz)) * gate;
        if any(sun > vec3<f32>(0.0)) {
            visibility = sun_shadow(world_position, normal, frag_coord) * cloud_shadow(uncurved(world_position.xyz));
        }
    }
    return painterly_light(shaded, to_sun, sun, visibility, sky_ambient(sky, shaded), direct_moon(shaded) * gate, leaf);
}
