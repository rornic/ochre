// The aurora: curtains of light that ripple and drift across the northern night sky over cold ground, for `stars.wesl`.

import bevy_pbr::render::mesh_view_bindings as view_bindings;
import super::cloud_cover::aurora_chill;
import super::hash::value_noise;

// Scene colour of the brightest green, well under the moon's face and over the Milky Way.
const AURORA_SCENE: f32 = 0.55;
const GREEN: vec3<f32> = vec3<f32>(0.12, 1.0, 0.32);
const VIOLET: vec3<f32> = vec3<f32>(0.85, 0.28, 0.95);
const CYAN: vec3<f32> = vec3<f32>(0.1, 0.75, 0.85);
// The curtains, and how far round from north they reach in radians: whole to the first, gone by the second.
const CURTAINS: i32 = 2;
const REACH: vec2<f32> = vec2<f32>(0.9, 1.8);
// How tall a curtain stands where its rays are shortest and where they're longest, in the view's y above its lower edge.
const TALL: vec2<f32> = vec2<f32>(0.1, 0.55);

// One curtain's light at `along` radians round from north and `up` the sky (the view's y), `layer` apart from its neighbours.
fn curtain(along: f32, up: f32, layer: f32, time: f32) -> vec3<f32> {
    // nothing travels: time is a noise axis, so features brighten, fray and fade where they are, a slow warp sways the broad shapes (not the fine rays) a few degrees, and everything drifts under 0.1 degrees a second
    let sway = 0.1 * (value_noise(vec2<f32>(along * 1.3 + layer * 5.0, time * 0.03 + layer)) - 0.5);
    let fixed = along + layer * 3.1 + time * 0.0015;
    let slide = fixed + sway;
    // the lower edge hangs steadily, drifting only a little
    let base = 0.08 + 0.07 * layer + 0.05 * value_noise(vec2<f32>(slide * 1.6, time * 0.03 + layer * 7.0)) + 0.012 * sin(slide * 11.0) * sin(time * 0.4 + layer * 2.0);
    // how far the rays reach up from it: stretches of the curtain, a few tens of degrees across, lengthen and retract on two slow clocks, out of step with each other
    let reach_now = smoothstep(0.2, 0.8, 0.6 * value_noise(vec2<f32>(slide * 2.4 + 9.0, time * 0.035 + layer * 3.0)) + 0.4 * value_noise(vec2<f32>(slide * 5.0 + 31.0, time * 0.06 + layer * 8.0)));
    let tall = mix(TALL.x, TALL.y, reach_now);
    // 0 along the lower edge, 1 along the upper
    let v = (up - base) / tall;
    // most of the sky is outside the curtain, and `hi` below never passes 1.3
    if v <= 0.0 || v > 1.5 {
        return vec3<f32>(0.0);
    }
    // each streak ends at its own height, so the edges follow no line: the lower fades in from a ragged start that wanders along the curtain and over tens of seconds, and the upper dissolves from a height that does too
    let column = value_noise(vec2<f32>(fixed * 26.0, time * 0.045 + layer * 5.0));
    let lo = 0.4 * value_noise(vec2<f32>(slide * 4.0 + 3.0, time * 0.037 + layer * 2.0)) * column + 0.12 * column;
    let hi = 0.7 + 0.45 * value_noise(vec2<f32>(slide * 2.6 + 21.0, time * 0.029 + layer * 9.0)) + 0.3 * (value_noise(vec2<f32>(fixed * 15.0 + 5.0, time * 0.05 + layer * 4.0)) - 0.5);
    if v > hi * 1.15 {
        return vec3<f32>(0.0);
    }
    let rise = smoothstep(lo, lo + 0.4, v);
    let fall = 1.0 - smoothstep(hi * 0.25, hi, v);
    // streaks up the curtain that lean with height, and bright stretches that wander and dim along it
    let rays = 0.35 + 0.65 * value_noise(vec2<f32>(fixed * 34.0 + v * 2.0, time * 0.12 + layer * 5.0));
    let breathe = 0.5 + 0.5 * sin(time * 0.1 + 6.0 * value_noise(vec2<f32>(slide * 1.7, layer * 3.0)));
    let bursts = 0.12 + 0.88 * smoothstep(0.25, 0.75, value_noise(vec2<f32>(slide * 2.3 + 17.0, time * 0.05 + layer * 11.0))) * mix(0.55, 1.0, breathe);
    // a cyan tint where it fades in, green through the body, violet where it dissolves
    let green = mix(CYAN, GREEN, smoothstep(0.0, 0.3, v)) * rise * fall;
    let violet = VIOLET * 0.35 * smoothstep(hi * 0.45, hi * 0.85, v) * (1.0 - smoothstep(hi * 0.85, hi * 1.15, v)) * rise;
    return (green * exp(-v * 1.3) + violet) * rays * bursts;
}

// The aurora's light towards `direction`, for the caller to scale by the night: none over warm ground.
fn aurora(direction: vec3<f32>) -> vec3<f32> {
    let along = atan2(direction.x, -direction.z);
    let reach = 1.0 - smoothstep(REACH.x, REACH.y, abs(along));
    if reach <= 0.0 || direction.y <= 0.0 {
        return vec3<f32>(0.0);
    }
    let chill = aurora_chill();
    if chill <= 0.0 {
        return vec3<f32>(0.0);
    }
    var light = vec3<f32>(0.0);
    for (var i = 0; i < CURTAINS; i++) {
        light += curtain(along, direction.y, f32(i), view_bindings::globals.time);
    }
    // thin into the haze at the horizon as the stars do
    return light * AURORA_SCENE * reach * chill * smoothstep(0.0, 0.1, direction.y);
}
