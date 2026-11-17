// The clouds' low-res target, lit and blended over the scene wherever nothing nearer hides it, see `clouds.rs`.

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput},
    mesh_functions,
    mesh_view_bindings as view_bindings,
    view_transformations::{depth_ndc_to_view_z, position_world_to_clip},
};
import super::clouds::lit_clouds;
import super::cloud_bindings::{most_precipitation, weather_at};
import super::cloud_march::{BASE, FAR, HAZE, cloud_entry};
import super::cloud_paint::{brush, cloud_plane, paint};
import super::curvature::curvature;

// the share of light they scatter, in x, see `CloudMaterial::set_shade`
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var<uniform> cloud_shade: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var cloud_target: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var cloud_target_sampler: sampler;

// Over every pixel: it's drawn without a depth test, see `CloudMaterial::specialize`.
@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    // reversed depth: 0 is infinitely far
    out.position.z = 0.0;
    return out;
}

// How far along `direction` the scene drawn at `frag_coord` is, or `FAR` where there's only sky.
fn scene_distance(frag_coord: vec2<f32>, direction: vec3<f32>) -> f32 {
@if(DEPTH_PREPASS && !MULTISAMPLED) {
    let depth = textureLoad(view_bindings::depth_prepass_texture, vec2<i32>(frag_coord), 0);
    if depth > 0.0 {
        let forward = -view_bindings::view.world_from_view[2].xyz;
        return -depth_ndc_to_view_z(depth) / max(dot(direction, forward), 1e-4);
    }
}
    return FAR;
}

// Rain falling from the storms, seen from afar as grey shafts under their clouds (or pale ones
// of snow, below): light lost per block in the heaviest rain (5 km of it hides about 60% of
// what's behind), the most of what's behind it any ray of rain hides, so the land always shows through, how far along a ray it's
// looked for and in how many steps, and how much sun and sky it's lit by, less under a storm's
// dark cloud.
const SHAFT_EXTINCTION: f32 = 1.0 / 5000.0;
const SHAFT_MOST: f32 = 0.6;
const SHAFT_REACH: f32 = 30000.0;
const SHAFT_STEPS: i32 = 16;
const SHAFT_SUNLIT: f32 = 0.08;
const SHAFT_SKYLIT: f32 = 0.45;
const SHAFT_STORM_DARK: f32 = 0.55;
// Snow, where it falls instead, hangs pale and white: it takes more light than rain as hard, and
// scatters far more of what it's lit by back, under a storm's dark cloud too.
const SNOW_EXTINCTION: f32 = 1.0 / 3500.0;
const SNOW_SUNLIT: f32 = 0.2;
const SNOW_SKYLIT: f32 = 0.9;
const SNOW_STORM_DARK: f32 = 0.3;

// The rain along a ray from `eye` until `limit`, packed as the clouds' march packs them: sunlit,
// sky-lit and hazed light in xyz, how much it hides behind it in w. Drawn at full resolution, so
// the land's own depth cuts it off.
fn rain_shafts(eye: vec3<f32>, direction: vec3<f32>, limit: f32, bend: f32, jitter: f32) -> vec4<f32> {
    if most_precipitation() < 0.01 {
        return vec4<f32>(0.0);
    }
    let across = length(direction.xz);
    let stride = min(limit, SHAFT_REACH) / f32(SHAFT_STEPS);
    var through = 1.0;
    var gathered = vec3<f32>(0.0);
    for (var i = 0; i < SHAFT_STEPS; i++) {
        let t = (f32(i) + jitter) * stride;
        // where the sample really is, above where it's drawn
        let p = eye + direction * t + vec3<f32>(0.0, bend * t * t * across * across, 0.0);
        if p.y > BASE {
            continue;
        }
        let weather = weather_at(p.xz);
        let rain = weather.precipitation * (1.0 - smoothstep(BASE - 300.0, BASE, p.y));
        if rain <= 0.0 {
            continue;
        }
        let taken = 1.0 - exp(-rain * mix(SHAFT_EXTINCTION, SNOW_EXTINCTION, weather.snow) * stride);
        let hazed = 1.0 - exp2(-t * across / (2.0 * HAZE));
        let lit = (1.0 - hazed) * (1.0 - mix(SHAFT_STORM_DARK, SNOW_STORM_DARK, weather.snow) * weather.storm);
        let sunlit = mix(SHAFT_SUNLIT, SNOW_SUNLIT, weather.snow);
        let skylit = mix(SHAFT_SKYLIT, SNOW_SKYLIT, weather.snow);
        gathered += through * taken * vec3<f32>(lit * sunlit, lit * skylit, hazed);
        through *= 1.0 - taken;
    }
    let cover = 1.0 - through;
    return vec4<f32>(gathered, cover) * min(1.0, SHAFT_MOST / max(cover, 1e-4));
}

// The march's target at `uv` by cubic B-spline, in four taps of the linear sampler: bilinear's texels show as squares and kinks once edges are cut sharp.
fn smooth_target(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(cloud_target));
    let p = uv * size - 0.5;
    let f = fract(p);
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - f) * (1.0 - f) * (1.0 - f) / 6.0;
    let w1 = (3.0 * f3 - 6.0 * f2 + 4.0) / 6.0;
    let w3 = f3 / 6.0;
    let w2 = 1.0 - w0 - w1 - w3;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (floor(p) + 0.5 - 1.0 + w1 / g0) / size;
    let h1 = (floor(p) + 0.5 + 1.0 + w3 / g1) / size;
    let a = mix(textureSampleLevel(cloud_target, cloud_target_sampler, vec2<f32>(h0.x, h1.y), 0.0), textureSampleLevel(cloud_target, cloud_target_sampler, vec2<f32>(h1.x, h1.y), 0.0), g1.x / (g0.x + g1.x));
    let b = mix(textureSampleLevel(cloud_target, cloud_target_sampler, vec2<f32>(h0.x, h0.y), 0.0), textureSampleLevel(cloud_target, cloud_target_sampler, vec2<f32>(h1.x, h0.y), 0.0), g1.x / (g0.x + g1.x));
    return mix(b, a, g1.y / (g0.y + g1.y));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let viewport = view_bindings::view.viewport;
    let eye = view_bindings::view.world_position.xyz;
    let direction = normalize(in.world_position.xyz - eye);
    let sampled = smooth_target((in.position.xy - viewport.xy) / viewport.zw);
    // derivatives need every pixel of a quad, so before any return
    let plane = cloud_plane(eye, direction, view_bindings::globals.time);
    let each = sampled.xyz / max(sampled.w, 1e-3);
    let change = fwidth(vec3<f32>(sampled.w, each.x, each.y));
    let footprint = length(fwidth(plane));
    let bend = curvature.now.per_square_block;
    let scene = scene_distance(in.position.xy, direction);
    // interleaved gradient noise, so the shafts' few steps don't band
    let jitter = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))));
    let shafts = rain_shafts(eye, direction, scene, bend, jitter);
    let rain = select(vec4<f32>(0.0), lit_clouds(shafts, eye, direction, cloud_shade.x), shafts.w > 0.0005);
    if sampled.w < 0.002 || scene < cloud_entry(eye, direction, bend) {
        return rain;
    }
    let marched = paint(sampled, brush(plane, footprint), change, viewport.w);
    // the rain hangs under the clouds, in front of them
    return rain + lit_clouds(marched, eye, direction, cloud_shade.x) * (1.0 - rain.w);
}
