// Cloud shape and the unlit march through it, cumulus from the shadow pattern and cirrus above, see `clouds.rs`.

import super::cloud_bindings::{Weather, cloud_pattern, cloud_sampler, cloud_layer, most_storm, sky_cover, weather_at};
import super::hash::hash3;

// The cumulus slab's base and top (`CLOUD_BASE`, `CLOUD_TOP` in `clouds.rs`), and how high a storm's towers reach.
const BASE: f32 = 2400.0;
const TOP: f32 = 3000.0;
const STORM_TOP: f32 = 6000.0;
// The highest land can reach, matching `WORLD_HEIGHT` in `chunks::generate`.
const LAND_TOP: f32 = 2048.0;
// Light lost a block into the thickest cloud.
const EXTINCTION: f32 = 0.025;
// So little light still through that a march stops and counts it as none: the sun's disc is far brighter than cloud.
const OPAQUE: f32 = 0.02;
// How much of a ray through the slab is marched at full steps, in blocks, and how far on it's marched at fewer.
const NEAR_REACH: f32 = 3000.0;
const MARCH_REACH: f32 = 15000.0;
// Where light toward the sun is looked for, in blocks, for how much cloud shades each sample.
const LIGHT_REACH: vec2<f32> = vec2<f32>(50.0, 400.0);
// Deep inside, some light still gets through, scattered on and on.
const DEEP_LIGHT: f32 = 0.2;
// Sunlight scattered to the eye, and how much of the sky's light reaches the clouds' undersides.
const SUN_SCATTER: f32 = 1.6;
const UNDERSIDE: f32 = 0.5;
// Clouds fade out between these many blocks away, and the haze takes half their colour by this far.
const FADE: vec2<f32> = vec2<f32>(10000.0, 35000.0);
const HAZE: f32 = 10000.0;
// Each octave of detail past the first drops out this much farther off, in blocks.
const DETAIL_REACH: f32 = 7000.0;
// Noise cells, in blocks: a whole number of them to the pattern's width, so it drifts seamlessly.
const NOISE_CELL: f32 = 256.0;
// How far edge wisps reach into clear sky: less is crisper but shimmers between too few samples.
const SOFTNESS: f32 = 0.22;
// Cirrus: its height, streak length and width, and the swirls turning them so they never fan out (whole numbers).
const CIRRUS: f32 = 3800.0;
const CIRRUS_CELL: vec2<f32> = vec2<f32>(512.0, 256.0);
const CIRRUS_SWIRL: f32 = 3072.0;
const CIRRUS_BEND: f32 = 2500.0;
const CIRRUS_OPACITY: f32 = 0.35;
// Far off or seen edge on, streaks crowd into lines toward the horizon, so they soften and fade there.
const CIRRUS_SOFTEN: vec2<f32> = vec2<f32>(3000.0, 14000.0);
const CIRRUS_EDGE_ON: vec2<f32> = vec2<f32>(0.03, 0.25);
// Standing in for a ray's distance going on forever.
const FAR: f32 = 1e30;

// `c` wrapped into 0..period, and the cell after it: wrapping once per axis, not corner, is most of the noise's cost.
fn wrapped(c: i32, period: i32) -> vec2<i32> {
    let at = ((c % period) + period) % period;
    return vec2<i32>(at, select(at + 1, 0, at + 1 == period));
}

// Smooth 0..1 value noise, one cell a unit, wrapping every `period` cells along x and z.
fn value_noise(p: vec3<f32>, period: vec2<i32>) -> f32 {
    let cell = vec3<i32>(floor(p));
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x = wrapped(cell.x, period.x);
    let z = wrapped(cell.z, period.y);
    let x0 = mix(hash3(vec3<i32>(x.x, cell.y, z.x)), hash3(vec3<i32>(x.y, cell.y, z.x)), u.x);
    let x1 = mix(hash3(vec3<i32>(x.x, cell.y + 1, z.x)), hash3(vec3<i32>(x.y, cell.y + 1, z.x)), u.x);
    let x2 = mix(hash3(vec3<i32>(x.x, cell.y, z.y)), hash3(vec3<i32>(x.y, cell.y, z.y)), u.x);
    let x3 = mix(hash3(vec3<i32>(x.x, cell.y + 1, z.y)), hash3(vec3<i32>(x.y, cell.y + 1, z.y)), u.x);
    return mix(mix(x0, x1, u.y), mix(x2, x3, u.y), u.z);
}

// `value_noise` over a plane, for half the cost.
fn flat_noise(p: vec2<f32>, period: vec2<i32>) -> f32 {
    let cell = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x = wrapped(cell.x, period.x);
    let z = wrapped(cell.y, period.y);
    let x0 = mix(hash3(vec3<i32>(x.x, 0, z.x)), hash3(vec3<i32>(x.y, 0, z.x)), u.x);
    let x1 = mix(hash3(vec3<i32>(x.x, 0, z.y)), hash3(vec3<i32>(x.y, 0, z.y)), u.x);
    return mix(x0, x1, u.y);
}

// `octaves` of `value_noise` at `p`, each twice as fine, 0..1.
fn fbm(p: vec3<f32>, period: vec2<i32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var total = 0.0;
    var scale = 1.0;
    var wrap = period;
    for (var i = 0; i < octaves; i++) {
        // shifted, so the octaves' cells don't line up
        sum += amplitude * value_noise(p * scale + f32(i) * 7.31, wrap);
        total += amplitude;
        amplitude *= 0.5;
        scale *= 2.0;
        wrap *= 2;
    }
    return sum / total;
}

// A storm's cloud is this many times thicker than fair-weather cloud, so darker and more opaque.
const STORM_THICKENING: f32 = 2.5;
// How much of the sun's light, and of the sky's on their undersides, a storm's cloud keeps.
const STORM_SUNLIT: f32 = 0.5;
const STORM_UNDERSIDE: f32 = 0.25;
// Haze takes a storm's cloud this many times slower than fair-weather cloud and fades later, so towers show far off.
const STORM_CLEARER: f32 = 3.0;

// The top of the slab rays are marched through: storms' towers only while one is about, so fair weather keeps steps.
fn slab_top() -> f32 {
    return select(TOP, STORM_TOP, most_storm() > 0.01);
}

// How high the cloud at a place can reach, from the fair-weather slab's top to a storm's towers.
fn ceiling(weather: Weather) -> f32 {
    return mix(TOP, STORM_TOP, smoothstep(0.2, 0.9, weather.storm));
}

// Cloud density at `p` under `weather`, 0..1 (more in a storm): the pattern's cover piled into a dome, noise-eaten.
fn density(p: vec3<f32>, drift: vec2<f32>, octaves: i32, weather: Weather) -> f32 {
    let xz = p.xz - drift;
    let share = textureSampleLevel(cloud_pattern, cloud_sampler, xz / cloud_layer.y, 0.0).r;
    let cover = sky_cover(share, weather);
    let height = (p.y - BASE) / (ceiling(weather) - BASE);
    let top = 0.15 + 0.85 * cover;
    let shape = cover * smoothstep(0.0, 0.06, height) * (1.0 - smoothstep(0.4 * top, top, height));
    if shape <= 0.02 {
        return 0.0;
    }
    let cells = i32(round(cloud_layer.y / NOISE_CELL));
    let noise = fbm(vec3<f32>(xz.x, p.y, xz.y) / NOISE_CELL, vec2<i32>(cells), octaves);
    return saturate((noise + shape - 1.0) / SOFTNESS) * mix(1.0, STORM_THICKENING, weather.storm);
}

// The span of the ray below `height`, drawn sinking `bend` per square block, `FAR` both ends when never.
fn below(origin: vec3<f32>, direction: vec3<f32>, bend: f32, height: f32) -> vec2<f32> {
    let a = bend * dot(direction.xz, direction.xz);
    let b = direction.y;
    let c = origin.y - height;
    if a < 1e-12 {
        if abs(b) < 1e-9 {
            return select(vec2<f32>(FAR), vec2<f32>(-FAR, FAR), c <= 0.0);
        }
        let t = -c / b;
        return select(vec2<f32>(t, FAR), vec2<f32>(-FAR, t), b > 0.0);
    }
    // a t² + b t + c, the ray's real height over `height`, is under 0 between its roots
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return vec2<f32>(FAR);
    }
    let q = -0.5 * (b + select(-1.0, 1.0, b >= 0.0) * sqrt(disc));
    let r = vec2<f32>(q / a, c / q);
    return vec2<f32>(min(r.x, r.y), max(r.x, r.y));
}

// The span of the ray inside the cumulus, empty when y <= x, cut short where the bent ray meets the land.
fn cloud_span(origin: vec3<f32>, direction: vec3<f32>, bend: f32) -> vec2<f32> {
    let top = below(origin, direction, bend, slab_top());
    let base = below(origin, direction, bend, BASE);
    let start = max(top.x, 0.0);
    var end = top.y;
    let a = bend * dot(direction.xz, direction.xz);
    if a > 1e-12 {
        let lowest = -direction.y / (2.0 * a);
        if lowest > 0.0 && origin.y + direction.y * lowest * 0.5 < LAND_TOP {
            end = min(end, lowest);
        }
    }
    let first = vec2<f32>(start, min(end, base.x));
    let second = vec2<f32>(max(start, base.y), end);
    let span = vec2<f32>(select(second.x, first.x, first.y > first.x), select(first.y, second.y, second.y > second.x));
    return span;
}

// A march's gathering: sun, moon-and-sky and haze in xyz, transmittance, and the distance to what it gathered.
struct Marched {
    gathered: vec3<f32>,
    through: f32,
    depth: f32,
}

// `so_far` on through the cumulus over `span`, steps widening with distance for detail close by.
fn march(so_far: Marched, origin: vec3<f32>, direction: vec3<f32>, bend: f32, span: vec2<f32>, steps: i32, octaves: i32, jitter: f32, drift: vec2<f32>, to_sun: vec3<f32>) -> Marched {
    var out = so_far;
    if span.y <= span.x {
        return out;
    }
    let across = length(direction.xz);
    let reach = span.y - span.x;
    let squared = f32(steps * steps);
    for (var i = 0; i < steps; i++) {
        let t = span.x + reach * pow(f32(i) + jitter, 2.0) / squared;
        let stride = reach * f32(2 * i + 1) / squared;
        // where the sample really is, above where it's drawn
        let p = origin + direction * t + vec3<f32>(0.0, bend * t * t * across * across, 0.0);
        // finer octaves only near enough to be seen, or far off they'd shimmer as they drift
        let away = t * across;
        // the weather changes over kilometres, so the light samples share it
        let weather = weather_at(p.xz);
        let filled = density(p, drift, clamp(octaves - i32(away / DETAIL_REACH), 1, octaves), weather);
        if filled > 0.0 {
            let height = saturate((p.y - BASE) / (ceiling(weather) - BASE));
            // how much cloud lies between here and the sun
            let toward = density(p + to_sun * LIGHT_REACH.x, drift, 1, weather) * LIGHT_REACH.x
                + density(p + to_sun * LIGHT_REACH.y, drift, 1, weather) * (LIGHT_REACH.y - LIGHT_REACH.x);
            let depth = toward * EXTINCTION;
            // a storm's cloud is dark through and through, more than its thickness alone makes it
            let lit = max(exp(-depth), DEEP_LIGHT * exp(-0.2 * depth)) * mix(1.0, STORM_SUNLIT, weather.storm);
            let underside = mix(UNDERSIDE, 1.0, height) * mix(1.0, STORM_UNDERSIDE, weather.storm);
            // far off, the haze between takes them over, and then they fade away, a storm's towers less
            let clearer = mix(1.0, STORM_CLEARER, weather.storm);
            let hazed = 1.0 - exp2(-away / (HAZE * clearer));
            let fading = min(FADE.x * clearer, 0.8 * FADE.y);
            let taken = (1.0 - exp(-filled * EXTINCTION * stride)) * (1.0 - smoothstep(fading, FADE.y, away));
            let weight = out.through * taken;
            out.gathered += weight * vec3<f32>((1.0 - hazed) * lit, (1.0 - hazed) * underside, hazed);
            out.depth += weight * t;
            out.through *= 1.0 - taken;
            if out.through < OPAQUE {
                break;
            }
        }
    }
    return out;
}

// `so_far` on through the cirrus, `t` along the ray, if that's before `limit`.
fn cirrus(so_far: Marched, origin: vec3<f32>, direction: vec3<f32>, t: f32, limit: f32, drift: vec2<f32>) -> Marched {
    var out = so_far;
    let away = t * length(direction.xz);
    if t <= 0.0 || t >= limit || away >= FADE.y || out.through < OPAQUE {
        return out;
    }
    // short streaks, turned by swirls across the sky and broken into patches a few streaks across
    let xz = origin.xz + direction.xz * t - drift;
    let cells = vec2<i32>(round(cloud_layer.y / CIRRUS_CELL));
    let swirls = i32(round(cloud_layer.y / CIRRUS_SWIRL));
    let swirl = flat_noise(xz / CIRRUS_SWIRL, vec2<i32>(swirls));
    let patches = flat_noise(xz / CIRRUS_CELL.x * 0.5, cells.xx / 2);
    let along = xz.x + (patches - 0.5) * CIRRUS_CELL.x;
    let across = xz.y + (swirl - 0.5) * CIRRUS_BEND;
    let streak = vec2<f32>(along / CIRRUS_CELL.x, across / CIRRUS_CELL.y);
    let streaks = (2.0 * flat_noise(streak, cells) + flat_noise(streak * 2.0 + 7.31, cells * 2)) / 3.0;
    let sharpness = mix(2.5, 0.8, smoothstep(CIRRUS_SOFTEN.x, CIRRUS_SOFTEN.y, away));
    let edge_on = mix(0.3, 1.0, smoothstep(CIRRUS_EDGE_ON.x, CIRRUS_EDGE_ON.y, abs(direction.y)));
    // none under a clear sky
    let clouded = smoothstep(0.0, 0.25, weather_at(origin.xz + direction.xz * t).cloud);
    let wisp = saturate((streaks - 0.45) * sharpness) * saturate((patches - 0.35) * 2.5) * edge_on * CIRRUS_OPACITY * clouded * (1.0 - smoothstep(FADE.x, FADE.y, away));
    let hazed = 1.0 - exp2(-away / HAZE);
    // thin, so lit straight through, with no more sunlight scattered to the eye than off a cumulus's face
    out.gathered += out.through * wisp * vec3<f32>((1.0 - hazed) / SUN_SCATTER, 1.0 - hazed, hazed);
    out.depth += out.through * wisp * t;
    out.through *= 1.0 - wisp;
    return out;
}

// The clouds along a ray before `limit`, packed as `Marched` with coverage in w, jittered and bent with the world.
fn march_clouds(origin: vec3<f32>, direction: vec3<f32>, bend: f32, limit: f32, steps: i32, octaves: i32, jitter: f32, to_sun: vec3<f32>, time: f32) -> vec4<f32> {
    let out = march_ray(origin, direction, bend, limit, steps, octaves, jitter, to_sun, time);
    return vec4<f32>(out.gathered, 1.0 - out.through);
}

// `march_clouds` unpacked.
fn march_ray(origin: vec3<f32>, direction: vec3<f32>, bend: f32, limit: f32, steps: i32, octaves: i32, jitter: f32, to_sun: vec3<f32>, time: f32) -> Marched {
    // the clock wraps on the hour, when the wind has moved a whole number of patterns, see `clouds::WIND`
    let drift = cloud_layer.zw * time;
    var span = cloud_span(origin, direction, bend);
    span.y = min(span.y, limit);
    // the cirrus, where the ray crosses its height: in front of the cumulus seen from above
    let sheet = below(origin, direction, bend, CIRRUS);
    let above = origin.y > CIRRUS;
    var out = Marched(vec3<f32>(0.0), 1.0, 0.0);
    if above {
        out = cirrus(out, origin, direction, sheet.x, limit, drift);
    }
    // low rays graze the slab for far longer than the near part, which would otherwise hide every cloud past it
    let near = vec2<f32>(span.x, min(span.y, span.x + NEAR_REACH));
    let far = vec2<f32>(near.y, min(span.y, span.x + MARCH_REACH));
    out = march(out, origin, direction, bend, near, steps, octaves, jitter, drift, to_sun);
    out = march(out, origin, direction, bend, far, steps * 2, octaves, jitter, drift, to_sun);
    if !above {
        out = cirrus(out, origin, direction, sheet.y, limit, drift);
    }
    out.through = saturate((out.through - OPAQUE) / (1.0 - OPAQUE));
    return out;
}

// Distance to the first cloud a ray could meet, or `FAR`: land never reaches that high, so nothing nearer hides them.
fn cloud_entry(origin: vec3<f32>, direction: vec3<f32>, bend: f32) -> f32 {
    let span = cloud_span(origin, direction, bend);
    var entry = select(FAR, span.x, span.y > span.x);
    let sheet = below(origin, direction, bend, CIRRUS);
    let crossing = select(sheet.y, sheet.x, origin.y > CIRRUS);
    if crossing > 0.0 {
        entry = min(entry, crossing);
    }
    return entry;
}

// The reflections' map is two halves: the sky over the eye on a disc (stereographic) on the left, and on the right
// the clouds straight over the ground round the eye, this far each way (`reflection` in `cloud_target.wesl`).
const REFLECTION_TEXELS: f32 = 512.0;
const OVERHEAD_REACH: f32 = 12288.0;

// Where an upward `direction` lies in the reflections' sky disc.
fn sky_disc(direction: vec3<f32>) -> vec2<f32> {
    return map_half(direction.xz / (1.0 + direction.y) * 0.5 + 0.5, 0.0);
}

// Where the clouds over `xz` lie in the reflections' overhead half, for an eye over `eye`.
fn overhead(xz: vec2<f32>, eye: vec2<f32>) -> vec2<f32> {
    return map_half((xz - eye) / OVERHEAD_REACH * 0.5 + 0.5, 0.5);
}

// `uv` within the map's half from `start`, kept half a texel off its edges so neither half bleeds into the other.
fn map_half(uv: vec2<f32>, start: f32) -> vec2<f32> {
    let edge = 0.5 / REFLECTION_TEXELS;
    let inside = clamp(uv, vec2<f32>(edge), vec2<f32>(1.0 - edge));
    return vec2<f32>(start + inside.x * 0.5, inside.y);
}

// The way up through `uv` in the sky disc, undoing `sky_disc`.
fn disc_sky(uv: vec2<f32>) -> vec3<f32> {
    let p = uv * 2.0 - 1.0;
    let r2 = dot(p, p);
    return vec3<f32>(2.0 * p.x, 1.0 - r2, 2.0 * p.y) / (1.0 + r2);
}
