// Ice: blue where thick and cut, glassy where it faces the sky and sun. A function of where the fragment is and the
// baked sky light only, so it is the same on every LOD.

import super::curvature::{eye, uncurved};
import super::hash::value_noise;
import super::lighting::{open_sky, sun_radiance};
import super::sky::sky_colour;

// Ice's cell is in the vertex alpha.
fn is_ice(vertex_alpha: f32) -> bool {
    return i32(floor(vertex_alpha + 0.5)) == i32(constants::ICE_CELL);
}

// Red is soaked up first going through ice, so what is thick (walls, anything the sky doesn't reach) goes deep blue.
const ABSORB: vec3<f32> = vec3<f32>(0.7, 0.85, 1.15);
// Thick ice lit from within, so crevasses glow instead of going grey, at this much of the open sky's light.
const GLOW: vec3<f32> = vec3<f32>(0.03, 0.36, 0.95);
const GLOW_GAIN: f32 = 0.22;
// How deep a sunlit wall reads, against ice the sky does not reach.
const WALL_DEPTH: f32 = 0.75;
// How far the surface's tilt is rippled, and over how many blocks, so a glint breaks up instead of lighting a flat.
const RIPPLE: f32 = 0.05;
const RIPPLE_SCALE: f32 = 1.3;
// The sun's tight glint and the wide gloss round it, as shine exponents and strengths.
const GLINT_SHARP: f32 = 140.0;
const GLINT_GAIN: f32 = 6.0;
const GLOSS_SHARP: f32 = 22.0;
const GLOSS_GAIN: f32 = 0.45;
// What a glassy surface reflects looking straight at it, how much sky it shows at most, and how much is let in.
const REFLECT_NEAR: f32 = 0.05;
const REFLECT_MOST: f32 = 0.55;
const REFLECT_GAIN: f32 = 1.0;

// How hard the painting's light and dark push the surface's tilt, taking brightness as height.
const BUMP: f32 = 0.12;

// `normal` tilted by the painting's brightness slope (`rise` per pixel) over the surface's own (`across`, `down`:
// a pixel's step in world space): a screen-derivative bump needing no tangents or texture.
fn bumped(normal: vec3<f32>, across: vec3<f32>, down: vec3<f32>, rise: vec2<f32>) -> vec3<f32> {
    let r1 = cross(down, normal);
    let r2 = cross(normal, across);
    let det = dot(across, r1);
    if abs(det) < 1e-12 {
        return normal;
    }
    return normalize(normal - BUMP * sign(det) * (rise.x * r1 + rise.y * r2) / abs(det));
}

// How thick the ice reads, 0..1: what the sky light cannot reach, and walls. `up` is the lit normal's y.
fn ice_depth(sky: f32, up: f32) -> f32 {
    let shut_in = 1.0 - smoothstep(0.35, 0.95, sky);
    let wall = 1.0 - smoothstep(0.55, 0.95, up);
    return clamp(max(shut_in, WALL_DEPTH * wall), 0.0, 1.0);
}

// `albedo` with the red taken out of it by the ice's depth, keeping the painting's light and dark.
fn ice_albedo(albedo: vec3<f32>, sky: f32, up: f32) -> vec3<f32> {
    // a flat of ice reads a little deep too: even the surface is thick ice
    return mix(albedo, albedo * ABSORB, mix(0.25, 1.0, ice_depth(sky, up)));
}

// What ice adds to its lit colour: glow, the sun's glint and gloss, and mirrored sky. `world_position` is
// as drawn (curved), `normal` the face's, `lit` how much the sun reaches it, `block_rate` blocks to a pixel.
fn ice_shine(world_position: vec3<f32>, normal: vec3<f32>, sky: f32, to_sun: vec3<f32>, lit: f32, block_rate: f32, across: vec3<f32>, down: vec3<f32>, rise: vec2<f32>) -> vec3<f32> {
    let real = uncurved(world_position);
    let seen = normalize(eye() - real);
    let depth = ice_depth(sky, normal.y);
    let glow = GLOW * open_sky(vec3<f32>(0.0, 1.0, 0.0)) * (depth * GLOW_GAIN);

    // the painting's relief tilts the face; a faint ripple breaks up smooth flats, fading near a pixel a block
    let fine = 1.0 - smoothstep(0.2, 1.0, block_rate);
    let ripple = vec3<f32>(value_noise(real.xz / RIPPLE_SCALE + vec2<f32>(3.0, 9.0)) - 0.5, 0.0, value_noise(real.xz / RIPPLE_SCALE + vec2<f32>(41.0, 17.0)) - 0.5);
    let facet = normalize(bumped(normal, across, down, rise * (1.0 - smoothstep(0.08, 0.5, block_rate))) + ripple * (RIPPLE * fine));

    let half_way = normalize(to_sun + seen);
    let along = max(dot(facet, half_way), 0.0);
    let by_day = smoothstep(0.0, 0.3, to_sun.y);
    let sun = sun_radiance(real) * lit * by_day;
    let glint = sun * (pow(along, GLINT_SHARP) * GLINT_GAIN + pow(along, GLOSS_SHARP) * GLOSS_GAIN);

    // the sky's reflection, a glance at the surface shows more of it, and none where the ice is shut in
    let slant = 1.0 - max(dot(facet, seen), 0.0);
    let mirror = min(REFLECT_NEAR + (1.0 - REFLECT_NEAR) * pow(slant, 5.0), REFLECT_MOST);
    var bounced = reflect(-seen, facet);
    bounced.y = max(bounced.y, 0.02);
    let mirrored = sky_colour(normalize(bounced)) * (mirror * REFLECT_GAIN * smoothstep(0.3, 0.9, sky));

    return glow + glint + mirrored;
}
