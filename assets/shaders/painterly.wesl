// Painterly lighting for painted faces, see docs/designs/painterly-lighting.md. Every number to tune is a const here.

// How far past 90 degrees from the sun a face still gets light, as a share of its range.
const WRAP: f32 = 0.35;
// The shadow's visibility, 0..1, is remapped over this range so shadow edges read as soft shapes.
const SHADOW_FROM: f32 = 0.1;
const SHADOW_TO: f32 = 0.9;
// Faces turned this far from the sun, as a cosine, go from ignoring the shadow map, which shadows them by themselves, to trusting it.
const SHADOW_TRUST_FROM: f32 = -0.05;
const SHADOW_TRUST_TO: f32 = 0.1;
// What the sky's light is pushed toward in shadow, and how far, keeping its brightness.
const SHADOW_TINT: vec3<f32> = vec3<f32>(0.6, 0.55, 1.0);
const SHADOW_TINT_AMOUNT: f32 = 0.65;
// The least shadow is lit to, as a share of a fully lit face's brightness.
const SHADOW_FLOOR: f32 = 0.35;
// Terrain past the shadows' reach is lit as if this much of the sun gets through, so it isn't brighter than shadowed land nearer.
const FAR_VISIBILITY: f32 = 0.8;

fn luma(colour: vec3<f32>) -> f32 {
    return dot(colour, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// The light on a face turned to `normal`, from `sun` (its colour at normal incidence, gated by sky light) coming from `to_sun`
// and shadowed to `visibility`, the sky's `ambient`, and the `moon`, which isn't shadowed or wrapped.
fn painterly_light(normal: vec3<f32>, to_sun: vec3<f32>, sun: vec3<f32>, visibility: f32, ambient: vec3<f32>, moon: vec3<f32>) -> vec3<f32> {
    let along = dot(normal, to_sun);
    let wrap = smoothstep(0.0, 1.0, clamp((along + WRAP) / (1.0 + WRAP), 0.0, 1.0));
    let shadow = smoothstep(SHADOW_FROM, SHADOW_TO, visibility);
    let lit = wrap * mix(1.0, shadow, smoothstep(SHADOW_TRUST_FROM, SHADOW_TRUST_TO, along));
    let full = sun + ambient;
    let tinted = mix(ambient, SHADOW_TINT * luma(ambient) / luma(SHADOW_TINT), SHADOW_TINT_AMOUNT);
    let fill = tinted * max(1.0, SHADOW_FLOOR * luma(full) / max(luma(tinted), 1e-4));
    return mix(fill, full, lit) + moon;
}
