// The clouds marched for water to reflect, and how much of the sun's and moon's discs they leave showing.

import super::cloud_edge::body;

@group(constants::MATERIAL_BIND_GROUP) @binding(9) var cloud_reflection: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(10) var cloud_reflection_sampler: sampler;

// A disc's light passes the clouds as their transmittance to this power: discs far outshine the sky a cloud thins.
const DISC_HIDING: f32 = 4.0;
// The moon's disc is only a few times brighter than the night sky round it, so thin cloud lets it through.
const MOON_HIDING: f32 = 1.5;

// How cold the ground under the camera is for the aurora, 0..1, from the map's third texel (`aurora_chill`).
fn aurora_chill() -> f32 {
    return textureLoad(cloud_reflection, vec2<i32>(2, 0), 0).a;
}

// How much of the sun's or moon's disc shows past the clouds, 0..1, from the map's first two texels.
fn disc_showing(moon: bool) -> f32 {
    let marched = textureLoad(cloud_reflection, vec2<i32>(select(0, 1, moon), 0), 0).a;
    // the clouds are painted opaque past their edge, so a disc behind that is hidden however thin the march says it is
    let cover = max(marched, body(marched));
    return pow(1.0 - cover, select(DISC_HIDING, MOON_HIDING, moon));
}
