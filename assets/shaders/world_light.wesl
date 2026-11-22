// The sun, sky and cloud light on a point, for the land (`world.wesl`) and any material a game adds that should be lit as the land is.

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

// The normal bias is a share of a cascade's texel size, so it moves a far shadow's edge by blocks: it eases down to this share of itself between these distances.
const FAR_NORMAL_BIAS: f32 = 0.33;
const NORMAL_BIAS_FADE: vec2<f32> = vec2<f32>(60.0, 400.0);

// The ground's facets are about a block across and tilted a little differently, so a face the sun grazes is shadowed by its own neighbours in teeth that jump as the sun steps: the lookup is lifted toward the sun by this many blocks, past the facets.
const FACET_LIFT: f32 = 0.6;

fn sun_shadow(world_position: vec4<f32>, normal: vec3<f32>, frag_coord: vec2<f32>) -> f32 {
    let sun = &view_bindings::lights.directional_lights[SUN_LIGHT];
    if ((*sun).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
        return 1.0;
    }
    let view_z = position_world_to_view(world_position.xyz).z;
    let reach = (*sun).cascades[(*sun).num_cascades - 1u].far_bound;
    // faded out wholly, so nothing to look up
    if -view_z >= reach {
        return FAR_VISIBILITY;
    }
    let lifted = vec4<f32>(world_position.xyz + (*sun).direction_to_light * FACET_LIFT, world_position.w);
    let shadow = fetch_directional_shadow(SUN_LIGHT, lifted, normal * mix(1.0, FAR_NORMAL_BIAS, smoothstep(NORMAL_BIAS_FADE.x, NORMAL_BIAS_FADE.y, -view_z)), view_z, frag_coord);
    return mix(shadow, FAR_VISIBILITY, smoothstep(reach * SHADOW_FADE_START, reach, -view_z));
}

// How far toward the sun a plant's shadow lookup is moved, in blocks, and the distances between which its shadows fade out.
const FLORA_SHADOW_BIAS: f32 = 0.3;
const FLORA_SHADOW_FADE: vec2<f32> = vec2<f32>(40.0, 120.0);

// The light on a face, shadowed by the land and the clouds, and moonlight, kept out of caves by sky light but not shadowed, see `painterly.wesl`.
fn painterly_world_light(world_position: vec4<f32>, normal: vec3<f32>, shaded: vec3<f32>, sky: f32, frag_coord: vec2<f32>, leaf: bool, flora: bool) -> Painterly {
    let gate = smoothstep(0.0, SUN_SKY_GATE, sky);
    var to_sun = vec3<f32>(0.0, 1.0, 0.0);
    var sun = vec3<f32>(0.0);
    var visibility = 0.0;
    if view_bindings::lights.n_directional_lights > 0u {
        to_sun = view_bindings::lights.directional_lights[SUN_LIGHT].direction_to_light;
        sun = sun_radiance(uncurved(world_position.xyz)) * gate;
        // no sun, nothing to shadow
        if any(sun > vec3<f32>(0.0)) {
            // a plant's own cut-out shadow falls on its quad, which the map's texels are too coarse to keep off it, so
            // its lookup is moved toward the sun and it takes the land's shadows less the further it is
            var lookup = world_position;
            var unshadowed = 0.0;
            if flora {
                lookup = vec4<f32>(world_position.xyz + to_sun * FLORA_SHADOW_BIAS, world_position.w);
                unshadowed = smoothstep(FLORA_SHADOW_FADE.x, FLORA_SHADOW_FADE.y, distance(world_position.xyz, view_bindings::view.world_position));
            }
            visibility = mix(sun_shadow(lookup, normal, frag_coord), 1.0, unshadowed) * cloud_shadow(uncurved(world_position.xyz));
        }
    }
    return painterly_light(shaded, to_sun, sun, visibility, sky_ambient(sky, shaded), direct_moon(shaded) * gate, leaf);
}
