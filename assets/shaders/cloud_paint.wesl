// The clouds repainted at display resolution from the low-res march: cut to brushed shapes, shaded in dabbed bands, see `cloud_dome.wesl`.

import super::cloud_bindings::cloud_layer;
import super::cloud_march::{BASE, TOP, UNDERSIDE};
import super::cloud_edge::{EDGE, EDGE_SOFT, THICK};
import super::hash::value_noise;

// How far a brush stroke moves a shape's edge, see `cloud_edge.wesl` for where it falls.
const EDGE_BRUSH: f32 = 0.4;
// A stroke's length and width along and across the wind, in blocks, and the swirl that bends them: its size and how far it carries them.
const STROKE: vec2<f32> = vec2<f32>(360.0, 130.0);
const SWIRL: f32 = 1400.0;
const SWIRL_CARRY: f32 = 500.0;
// Levels the light is cut into, for sun and sky, how softly it steps between them, and how far a stroke moves each step.
const SUN_BANDS: f32 = 4.0;
// The march's lowest sunlight, even deep in shadow, matching `DEEP_LIGHT` in `cloud_march.wesl`: the sun's darkest band keeps it, or shade turns to cold ambient.
const SUN_FLOOR: f32 = 0.04;
const SKY_BANDS: f32 = 2.0;
// How far a painter pushes the lights, so rounding to a band doesn't darken a cloud on the whole.
const LEAN: f32 = 0.05;
const BAND_SOFT: f32 = 0.06;
const BAND_BRUSH: f32 = 0.3;
// Shading's strokes are this many times broader than the edges', or the light would speckle where it hovers at a step.
const BROAD: f32 = 3.0;
// The wash's opacity, the cover it grows over, and the change in cover a pixel (at 1080 rows) it gives way over.
const VEIL: f32 = 0.45;
const WISP: vec2<f32> = vec2<f32>(0.06, 0.2);
const WISP_RATE: vec2<f32> = vec2<f32>(0.012, 0.03);
// How much the strokes lighten and darken the paint itself, as brush grain.
const GRAIN: f32 = 0.07;
// How far haze softens a shape's edge in cover and the steps between its bands, and thins its body, at the most.
const HAZE_EDGE: f32 = 0.22;
const HAZE_BAND: f32 = 0.3;
const HAZE_THIN: f32 = 0.3;
// How far a cloud's shadows are lifted toward the lit side.
const LIFT: f32 = 0.15;
// The lowest a ray looks to meet the clouds' plane, so strokes stop stretching to the horizon, where the clouds have faded.
const FLATTEST: f32 = 0.1;

// The point a ray meets the clouds' plane at, drifting with them, so strokes stay put on a cloud as it blows.
fn cloud_plane(eye: vec3<f32>, direction: vec3<f32>, time: f32) -> vec2<f32> {
    let reach = (0.5 * (BASE + TOP) - eye.y) / max(direction.y, FLATTEST);
    return eye.xz + direction.xz * reach - cloud_layer.zw * time;
}

// 0..1 strokes at `xz`, long along the wind and bent by a swirl, thinning to 0.5 where they're smaller than a pixel (`footprint` blocks): fine ones in x, broad in y.
fn brush(xz: vec2<f32>, footprint: f32) -> vec2<f32> {
    let swirl = value_noise(xz / SWIRL) - 0.5;
    let q = (xz + vec2<f32>(swirl, -swirl) * SWIRL_CARRY) / STROKE;
    let fine = value_noise(q);
    let broad = value_noise(q / BROAD + 9.7);
    return mix(vec2<f32>(0.5), vec2<f32>(fine, broad), 1.0 - smoothstep(0.5, 1.5, footprint / STROKE.y));
}

// `x` rounded to the nearest of `bands` levels over 0..1, stepping over `soft` of a band: one pixel's change in `x` at least, so steps stay crisp whatever the resolution.
fn banded(x: f32, bands: f32, change: f32, haze: f32) -> f32 {
    let scaled = x * (bands - 1.0);
    let soft = clamp(0.75 * change * (bands - 1.0) + HAZE_BAND * haze, BAND_SOFT, 0.5);
    let step = smoothstep(0.5 - soft, 0.5 + soft, fract(scaled));
    return (floor(scaled) + step) / (bands - 1.0);
}

// `marched` (premultiplied, see `march_clouds`) as a painter would: its cover cut to a brushed edge, and its sun and sky light in bands. `change` is how much cover, sun and sky light change over a pixel of a view `rows` tall.
fn paint(marched: vec4<f32>, stroke: vec2<f32>, change: vec3<f32>, rows: f32) -> vec4<f32> {
    // the light per unit of cloud, and how much of it is haze, which blurs what's far off
    let each = marched.xyz / max(marched.w, 1e-3);
    let haze = smoothstep(0.1, 0.7, each.z);
    let width = max(EDGE_SOFT, 1.5 * change.x) + HAZE_EDGE * haze;
    let edge = smoothstep(EDGE - width, EDGE + width, marched.w + (stroke.x - 0.5) * EDGE_BRUSH);
    // thin, evenly spread cloud, the cirrus, is a pale wash of its own, but not at a cumulus's edge, where cover changes fast
    let wash = VEIL * smoothstep(WISP.x, WISP.y, marched.w) * (1.0 - smoothstep(WISP_RATE.x, WISP_RATE.y, change.x * rows / 1080.0));
    let alpha = max(edge * THICK * (1.0 - HAZE_THIN * haze), wash);
    // how much of the light is not haze
    let clear = max(1.0 - each.z, 0.05);
    let wobble = (stroke.y - 0.5) * BAND_BRUSH + LEAN;
    let sun = mix(SUN_FLOOR, 1.0, LIFT + (1.0 - LIFT) * banded(saturate((each.x / clear - SUN_FLOOR) / (1.0 - SUN_FLOOR) + wobble), SUN_BANDS, change.y / (clear * (1.0 - SUN_FLOOR)), haze));
    // the march lights undersides from half up, so the sky's bands span that
    let sky = mix(UNDERSIDE, 1.0, banded(saturate((each.y / clear - UNDERSIDE) / (1.0 - UNDERSIDE) + wobble), SKY_BANDS, change.z / (clear * (1.0 - UNDERSIDE)), haze));
    let grain = 1.0 + (stroke.x - 0.5) * 2.0 * GRAIN;
    return vec4<f32>(alpha * (clear * grain * vec3<f32>(sun, sky, 0.0) + vec3<f32>(0.0, 0.0, each.z)), alpha);
}
