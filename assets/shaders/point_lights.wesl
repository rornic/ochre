// Light from the point lights near a face, as Bevy clusters them, for the world shader's own
// lighting: Bevy's would only light its own materials. Fireflies glow on the grass by it.
// Painted as the sun is: the falloff cut into a warm core and two rings in soft steps that brush strokes
// break and slowly stir, cooler and duller toward the rim, see docs/designs/painterly-lighting.md.

import bevy_pbr::render::{clustered_forward as clustering, mesh_view_bindings as view_bindings};
import super::hash::value_noise;
import super::painterly::luma;

// A band's span in stops, and the top of the outer ring, in stops over the light's level at its range (1 / range²).
const STOPS: f32 = 4.0;
const CORE_EDGE: f32 = 4.0;
// Each band is lit this many stops over its middle, so cutting it flat keeps about the light it had.
const LEAN: f32 = 0.15;
// How softly a band steps to the next, in bands, and how far a stroke moves its edge.
const BAND_SOFT: f32 = 0.1;
const BAND_BRUSH: f32 = 0.7;
// A stroke's size in blocks, and how fast the strokes drift and turn over, in strokes a second.
const STROKE: f32 = 1.4;
const STIR: vec2<f32> = vec2<f32>(0.3, 0.2);
// A light this bright for its reach (luminance / range²) paints its rings at their size; flaring brighter pushes them out.
const BREATHE_REF: f32 = 500.0;
const BREATHE: f32 = 0.35;
// The cosine below which a face is lit by a soft tail that reaches a little past side-on, as the sun's wrap does.
const WRAP: f32 = 0.2;
// The core's light, warmer and richer, and the outer ring's, duller and cooler, as tints kept at the light's brightness.
const CORE_WARM: vec3<f32> = vec3<f32>(1.12, 0.98, 0.72);
const RIM_COOL: vec3<f32> = vec3<f32>(0.9, 1.0, 1.15);
const RIM_DULL: f32 = 0.45;

// 0..1 brush strokes at `p`, stirring over `time`: two layers drift apart, so they turn over rather than slide,
// and a fine one, stretched along x, ragged the edge as bristles do.
fn stroke(p: vec3<f32>, seed: vec2<f32>, time: f32) -> f32 {
    let q = (p.xz + vec2<f32>(p.y * 0.61, -p.y * 0.37)) / STROKE + seed;
    let a = value_noise(q + STIR * time);
    let b = value_noise(q * 1.3 + 5.1 - STIR.yx * time);
    let fine = value_noise(q * vec2<f32>(2.5, 6.0) + 11.3 + STIR * time);
    return a * 0.45 + b * 0.3 + fine * 0.25;
}

// The point lights' light on a face at `world_position` turned to `normal`, in scene colour.
// `frag_coord` finds the face's cluster, so only the lights that reach it are looked at.
fn point_light_glow(world_position: vec3<f32>, normal: vec3<f32>, frag_coord: vec2<f32>) -> vec3<f32> {
    let view_from_world = view_bindings::view.view_from_world;
    let view_z = dot(vec4<f32>(view_from_world[0].z, view_from_world[1].z, view_from_world[2].z, view_from_world[3].z), vec4<f32>(world_position, 1.0));
    let cluster = clustering::view_fragment_cluster_index(frag_coord, view_z, false);
    let ranges = clustering::unpack_clusterable_object_index_ranges(cluster);
    let time = view_bindings::globals.time;
    var glow = vec3<f32>(0.0);
    for (var i: u32 = ranges.first_point_light_index_offset; i < ranges.first_spot_light_index_offset; i = i + 1u) {
        let light = &view_bindings::clustered_lights.data[clustering::get_clusterable_object_id(i)];
        let colour = (*light).color_inverse_square_range.rgb;
        let inverse_square_range = (*light).color_inverse_square_range.w;
        let to_light = (*light).position_radius.xyz - world_position;
        let distance_squared = max(dot(to_light, to_light), 0.0001);
        // Bevy's own falloff: the inverse square, eased to nothing at the light's range
        let reach = distance_squared * inverse_square_range;
        let fade = saturate(1.0 - reach * reach);
        let along = dot(normal, to_light * inverseSqrt(distance_squared));
        let soft = saturate(along + WRAP);
        let facing = select(soft * soft / (4.0 * WRAP), along, along > WRAP);
        // the falloff over its value at the light's range, so the bands sit alike on a firefly and a fire
        let level = fade * fade * facing / reach;
        if level <= 0.0 {
            continue;
        }
        let breathe = clamp(BREATHE * log2(luma(colour) * inverse_square_range / BREATHE_REF), -1.0, 1.0);
        let brush = (stroke(world_position, (*light).position_radius.xz * 0.37, time) - 0.5) * BAND_BRUSH;
        // 0 in the core, 1 and 2 in the rings, 3 past them, in soft steps
        let band = (CORE_EDGE - log2(level) - breathe) / STOPS + 1.0 + brush;
        let ring = smoothstep(1.0 - BAND_SOFT, 1.0 + BAND_SOFT, band) + smoothstep(2.0 - BAND_SOFT, 2.0 + BAND_SOFT, band) + smoothstep(3.0 - BAND_SOFT, 3.0 + BAND_SOFT, band);
        let painted = exp2(CORE_EDGE + LEAN + STOPS * (0.5 - min(ring, 2.0))) * (3.0 - max(ring, 2.0));
        let cool = mix(colour, vec3<f32>(luma(colour)), RIM_DULL) * RIM_COOL;
        let tinted = mix(colour * CORE_WARM, cool, saturate(ring * 0.5));
        glow += tinted * luma(colour) / max(luma(tinted), 1e-6) * painted * inverse_square_range;
    }
    return glow * view_bindings::view.exposure;
}
