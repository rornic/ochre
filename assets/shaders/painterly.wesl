// Painterly lighting for painted faces, see docs/designs/painterly-lighting.md. Every number to tune is a const here.

import super::curvature::shadow_tint;

// How far past 90 degrees from the sun a face still gets light, as a share of its range.
const WRAP: f32 = 0.35;
// The shadow's visibility, 0..1, is remapped over this range so shadow edges read as soft shapes.
const SHADOW_FROM: f32 = 0.1;
const SHADOW_TO: f32 = 0.9;
// Faces turned this far from the sun, as a cosine, go from ignoring the shadow map (self-shadowed) to trusting it
const SHADOW_TRUST_FROM: f32 = -0.05;
const SHADOW_TRUST_TO: f32 = 0.1;
// What the sky's light is pushed toward in shadow, and how far, keeping brightness; from the country (`atmosphere`):
// (0.55, 0.63, 1.0) and 0.3 with no feel.
// The least shadow is lit to, as a share of a fully lit face's brightness, and leaves', which light passes through.
const SHADOW_FLOOR: f32 = 0.35;
// How far shade's light leans from the full light's colour to the cool fill's, in ratio: all the way swung yellow grass teal.
const FILL_CAST: f32 = 0.3;
const LEAF_SHADOW_FLOOR: f32 = 0.55;
// The floor rises by this share while the sun is near the horizon, when its light is dim, up to `MAX_SHADOW_FLOOR`.
const LOW_SUN_FLOOR: f32 = 0.6;
const MAX_SHADOW_FLOOR: f32 = 0.85;
// Terrain past the shadows' reach is lit as if this much sun gets through, so it isn't brighter than nearer shadow.
const FAR_VISIBILITY: f32 = 0.8;

// Chroma gain on the lit colour, in the light and in shadow, so mids are rich and shadows coloured, not grey.
const SAT_LIT: f32 = 1.1;
const SAT_SHADOW: f32 = 1.2;
// Radians shade's hue turns toward the shadow tint, per unit of its amount: bounded, so yellow goes green, not grey.
const SHADE_HUE_TURN: f32 = 0.3;

fn luma(colour: vec3<f32>) -> f32 {
    return dot(colour, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// How lit a face is, 0..1, and the light on it, see `painterly_light`.
struct Painterly {
    light: vec3<f32>,
    lit: f32,
}

// The light on a face turned to `normal`, from `sun` (its colour at normal incidence, gated by sky light) at `to_sun`
// and shadowed to `visibility`, the sky's `ambient`, and the `moon`, which isn't shadowed or wrapped.
fn painterly_light(normal: vec3<f32>, to_sun: vec3<f32>, sun: vec3<f32>, visibility: f32, ambient: vec3<f32>, moon: vec3<f32>, leaf: bool) -> Painterly {
    let along = dot(normal, to_sun);
    let wrap = smoothstep(0.0, 1.0, clamp((along + WRAP) / (1.0 + WRAP), 0.0, 1.0));
    let shadow = smoothstep(SHADOW_FROM, SHADOW_TO, visibility);
    let lit = wrap * mix(1.0, shadow, smoothstep(SHADOW_TRUST_FROM, SHADOW_TRUST_TO, along));
    let full = sun + ambient;
    let country = shadow_tint();
    let tinted = mix(ambient, country.rgb * luma(ambient) / luma(country.rgb), country.w);
    let low_sun = smoothstep(-0.1, 0.0, to_sun.y) * (1.0 - smoothstep(0.0, 0.5, to_sun.y));
    let floor = min(select(SHADOW_FLOOR, LEAF_SHADOW_FLOOR, leaf) * (1.0 + LOW_SUN_FLOOR * low_sun), MAX_SHADOW_FLOOR);
    let warm = full / max(luma(full), 1e-4);
    let leaned = warm * pow(tinted / max(luma(tinted), 1e-4) / max(warm, vec3<f32>(1e-4)), vec3<f32>(FILL_CAST));
    let fill = leaned / max(luma(leaned), 1e-4) * max(luma(tinted), floor * luma(full));
    return Painterly(mix(fill, full, lit) + moon, lit);
}

// Linear sRGB to Oklab, see https://bottosson.github.io/posts/oklab/.
fn oklab(rgb: vec3<f32>) -> vec3<f32> {
    let lms = mat3x3<f32>(
        vec3<f32>(0.4122214708, 0.2119034982, 0.0883024619),
        vec3<f32>(0.5363325363, 0.6806995451, 0.2817188376),
        vec3<f32>(0.0514459929, 0.1073969566, 0.6299787005),
    ) * max(rgb, vec3<f32>(0.0));
    let c = pow(lms, vec3<f32>(1.0 / 3.0));
    return mat3x3<f32>(
        vec3<f32>(0.2104542553, 1.9779984951, 0.0259040371),
        vec3<f32>(0.7936177850, -2.4285922050, 0.7827717662),
        vec3<f32>(-0.0040720468, 0.4505937099, -0.8086757660),
    ) * c;
}

fn linear_srgb(lab: vec3<f32>) -> vec3<f32> {
    let c = mat3x3<f32>(
        vec3<f32>(1.0, 1.0, 1.0),
        vec3<f32>(0.3963377774, -0.1055613458, -0.0894841775),
        vec3<f32>(0.2158037573, -0.0638541728, -1.2914855480),
    ) * lab;
    return mat3x3<f32>(
        vec3<f32>(4.0767416621, -1.2684380046, -0.0041960863),
        vec3<f32>(-3.3077115913, 2.6097574011, -0.7034186147),
        vec3<f32>(0.2309699292, -0.3413193965, 1.7076147010),
    ) * (c * c * c);
}

// `hue` turned toward `toward` the short way round, by at most `most` radians.
fn turn_hue(hue: f32, toward: f32, most: f32) -> f32 {
    let gap = atan2(sin(toward - hue), cos(toward - hue));
    return hue + clamp(gap, -most, most);
}

fn hue_of(rgb: vec3<f32>) -> f32 {
    let lab = oklab(rgb);
    return atan2(lab.z, lab.y);
}

// `colour` graded in Oklch for how lit it was: shade turns toward the shadow hue and gains chroma, lightness kept.
fn painterly_saturation(colour: vec3<f32>, lit: f32) -> vec3<f32> {
    let lab = oklab(colour);
    let chroma = length(lab.yz);
    let shade = 1.0 - lit;
    let country = shadow_tint();
    var hue = atan2(lab.z, lab.y);
    hue = turn_hue(hue, hue_of(country.rgb), shade * SHADE_HUE_TURN * country.w);
    let graded = chroma * mix(SAT_SHADOW, SAT_LIT, lit);
    return max(linear_srgb(vec3<f32>(lab.x, graded * cos(hue), graded * sin(hue))), vec3<f32>(0.0));
}
