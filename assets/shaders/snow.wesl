// Snow falling round the camera (`weather::SnowMaterial`): a box of flakes as `rain.wesl`'s is of streaks, soft dots.

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput},
    mesh_view_bindings as view_bindings,
    view_transformations::position_world_to_clip,
};

// x: how hard it's snowing, 0..1; y: how bright the light on it is; zw: the wind it drifts on, blocks a second.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> snow: vec4<f32>;

// Matches `SNOW_BOX` in `weather.rs`.
const BOX: vec3<f32> = vec3<f32>(40.0, 24.0, 40.0);
// Fall speed in blocks a second, a flake's radius and sway either side of its path in blocks, sway speed in turns/s.
const FALL: f32 = 1.4;
const RADIUS: f32 = 0.035;
const SWAY: f32 = 0.45;
const SWAY_RATE: f32 = 0.18;
const TAU: f32 = 6.2831853;

// A number 0..1 that's the same for every corner of a flake, from where it starts.
fn hash(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let eye = view_bindings::view.world_position;
    let time = view_bindings::globals.time;
    let seed = hash(vertex.position);
    let other = hash(vertex.position.zxy + 17.0);
    // bigger flakes fall a little faster, and every one drifts on the wind
    let velocity = vec3<f32>(snow.z, -FALL * (0.7 + 0.6 * seed), snow.w);
    let fallen = vertex.position + velocity * time - eye;
    let offset = (fract(fallen / BOX + 0.5) - 0.5) * BOX;
    // swaying in a slow loop round its path, each at its own pace
    let rate = SWAY_RATE * TAU * (0.7 + 0.6 * other);
    let phase = TAU * vec2<f32>(seed, other);
    let sway = SWAY * vec3<f32>(sin(time * rate + phase.x), 0.0, cos(time * rate * 0.8 + phase.y));
    let centre = eye + offset + sway;
    // turned to face the camera, and kept big enough far off to be seen at all
    let right = view_bindings::view.world_from_view[0].xyz;
    let up = view_bindings::view.world_from_view[1].xyz;
    let corner = vec2<f32>(vertex.uv.x, vertex.uv.y * 2.0 - 1.0);
    let size = RADIUS * (0.6 + 0.8 * seed) * (1.0 + length(offset) * 0.05);
    let world = centre + (right * corner.x + up * corner.y) * size;
    out.world_position = vec4<f32>(world, 1.0);
    out.position = position_world_to_clip(world);
    out.world_normal = vec3<f32>(0.0, 1.0, 0.0);
    // how far through the box it is, to fade out before it wraps, and only some flakes show in lighter snow
    let through = abs(offset) / (BOX * 0.5);
    let edge = max(through.x, max(through.y, through.z));
    let shown = step(other, snow.x);
    out.uv = corner;
    // and before it's so near it's a blot across the view
    let near = smoothstep(0.6, 2.0, length(offset));
    out.color = vec4<f32>((1.0 - smoothstep(0.7, 1.0, edge)) * near, shown, 0.0, 0.0);
    // a flake that won't show is put outside the view, so it's never rasterised
    if shown * near == 0.0 {
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dot_shape = 1.0 - smoothstep(0.3, 1.0, length(in.uv));
    let alpha = dot_shape * in.color.x * in.color.y * 0.85;
    if alpha < 0.002 {
        discard;
    }
    let tint = vec3<f32>(0.94, 0.96, 1.0) * snow.y;
    return vec4<f32>(tint * alpha, alpha);
}
