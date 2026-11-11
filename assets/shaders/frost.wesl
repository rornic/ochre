// Frost: a thin crystalline skin over ground the cold would freeze but the generator left bare, thicker the colder it is, and a rime on the
// tips of plants. Angular plates up close that fade to soft patches far away, a bright rim just inside the edges, and a glitter of tiny
// facets that catch the sun. See `docs/designs/snow-and-frost.md`. Everything here is a function of where the fragment is, so it is the same in every pass and on every LOD.

import super::curvature::{cold, eye, uncurved};
import super::hash::{hash2, hash3, value_noise};

// Ground that is snow or ice already, and leaves, are left as they are. Their cells are in the vertex alpha.
fn frost_receives(vertex_alpha: f32, leaf: bool) -> bool {
    let cell = i32(floor(vertex_alpha + 0.5));
    return !leaf && cell != i32(constants::SNOW_CELL) && cell != i32(constants::ICE_CELL);
}

// The patches' sizes, in blocks, and the plates' cell, which is where a patch's edge is jagged.
const PATCH_LARGE: f32 = 29.0;
const PATCH_SMALL: f32 = 7.0;
const PLATE: f32 = 0.38;
// Blocks to a pixel over which the plates and the small patches fade out, so far frost is soft and doesn't shimmer.
const FINE_FADE: vec2<f32> = vec2<f32>(0.06, 0.26);
// Frost's albedo, a cool off-white well under fresh snow's, the rim's lift over it, and how much of the ground's own light and dark it keeps.
const FROST_ALBEDO: vec3<f32> = vec3<f32>(0.66, 0.72, 0.80);
const RIM_LIFT: vec3<f32> = vec3<f32>(1.2, 1.28, 1.4);
const GROUND_SHOWS: f32 = 0.7;
// The most frost covers, so the ground always shows a little, and how far a plate's thickness swings under it.
const MOST: f32 = 0.82;
// The glitter's cells per block, how bright, how sharp the facets' catch is.
const GLITTER_CELLS: f32 = 4.0;
const GLITTER_COLOUR: vec3<f32> = vec3<f32>(5.0, 5.5, 6.3);
const GLITTER_SHARP: f32 = 10.0;

struct Frost {
    // how much of the surface is frost, 0 to 1
    amount: f32,
    // how thin it is, 0 (a full plate) to 1 (a dusting the ground shows through)
    thin: f32,
    // the thin bright band just inside a patch's edge, 0 to 1
    rim: f32,
    // the dark seams between plates, 0 to 1
    seam: f32,
    glitter: f32,
}

const UNFROSTED: Frost = Frost(0.0, 0.0, 0.0, 0.0, 0.0);

struct Plate {
    // the distance to the nearest point and to the seam with the next, in plates
    seam: f32,
    id: f32,
    centre: vec2<f32>,
}

// Worley cells: the plate `p` is in, with a random point each.
fn plate(p: vec2<f32>) -> Plate {
    let base = floor(p);
    let at = p - base;
    var nearest = 8.0;
    var next = 8.0;
    var id = 0.0;
    var centre = vec2<f32>(0.0);
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let step = vec2<f32>(f32(i), f32(j));
            let cell = base + step;
            let point = step + vec2<f32>(hash2(cell + vec2<f32>(13.0, 7.0)), hash2(cell + vec2<f32>(29.0, 41.0)));
            let d = distance(point, at);
            if d < nearest {
                next = nearest;
                nearest = d;
                id = hash2(cell);
                centre = cell + point - step;
            } else if d < next {
                next = d;
            }
        }
    }
    return Plate(next - nearest, id, centre);
}

// The frost at a fragment `world_position` (as drawn, curved), looking `up` (the lit normal's y), under baked `sky` light, `block_rate`
// blocks to a pixel, with the sun at `to_sun` and the clock at `time`. `tip` is -1 for ground, and for a plant sprite 0 at its foot to 1 at its tip.
fn frost_at(world_position: vec3<f32>, up: f32, sky: f32, block_rate: f32, tip: f32, to_sun: vec3<f32>, time: f32) -> Frost {
    let about = cold();
    if about.camera.z < 0.5 {
        return UNFROSTED;
    }
    let real = uncurved(world_position);
    // the ground's temperature here: the camera's sea level one, cooled by the height, as the snow blocks' rule has it
    let ground = about.camera.x - (real.y - f32(constants::WATER_LEVEL)) / about.rule.x;
    let freezing = 1.0 - smoothstep(about.rule.y, about.rule.y + about.rule.z, ground);
    let plant = tip >= 0.0;
    // a plant sprite is a vertical plane, where x and z alone would make vertical stripes, so height and both diagonals are mixed in
    let mapped = select(real.xz, vec2<f32>(real.x + real.z, real.y * 1.3 + real.x - real.z), plant);
    let facing = select(smoothstep(0.15, 0.6, up), 1.0, plant);
    let cover = freezing * about.camera.y * facing * smoothstep(0.3, 0.8, sky);
    if cover < 0.01 {
        return UNFROSTED;
    }
    // big patches everywhere, plates and small patches only while a pixel is well under a block
    let fine = 1.0 - smoothstep(FINE_FADE.x, FINE_FADE.y, block_rate);
    let cell = plate(mapped / PLATE);
    // away from a seam a plate is judged as a whole, at its middle, so edges follow plates; at a seam it is judged here, so neighbours blend and no edge is a step
    let inside = smoothstep(0.0, 0.4, cell.seam) * fine;
    let judged = mix(mapped, cell.centre * PLATE, inside);
    let big = value_noise(judged / PATCH_LARGE);
    let small = value_noise(judged / PATCH_SMALL + vec2<f32>(31.0, 17.0));
    // mid-sized breakup that holds out to the distance, so far frost is broken ground and not white blobs
    let mid = (value_noise(judged / 4.5 + vec2<f32>(7.0, 91.0)) - 0.5) * 0.55 * (1.0 - smoothstep(0.3, 1.6, block_rate));
    let mid2 = (value_noise(judged / 1.7 + vec2<f32>(63.0, 19.0)) - 0.5) * 0.4 * (1.0 - smoothstep(0.12, 0.7, block_rate));
    // averaging narrows the spread of the noise, so it is stretched back out around its middle
    let field = (mix(big, small, 0.4 * fine) - 0.5) * 1.6 + 0.5 + mid + mid2;
    // the colder it is, the lower the bar a patch has to clear, and each plate has a bar of its own
    // a fine grain on top, so the fade out is ragged and not a clean gradient
    let grain = (value_noise(mapped / 0.17 + vec2<f32>(5.0, 53.0)) - 0.5) * 0.45 * fine;
    let reach = field + (cell.id - 0.5) * 0.5 * inside + grain + (cover - 0.43) * 1.2 - 0.07;
    let alive = smoothstep(0.01, 0.12, cover);
    // the width of the fade from bare to frost, wide close up so an edge thins out over a few plates
    let fade = mix(0.1, 0.36, fine);
    let covered = smoothstep(0.5 - fade * 0.5, 0.5 + fade * 0.5, reach);
    var amount = covered * alive;
    let rim = covered * (1.0 - covered) * 4.0 * alive;
    // a plant's frost sits on its upper part, and is thick there
    if plant {
        amount = min(amount * 1.6, 1.0) * smoothstep(0.1, 0.6, tip);
    }
    // plates differ in thickness, thin ones let the ground show, and a seam of shade runs between plates
    var thin = mix(0.55, hash2(cell.centre + vec2<f32>(71.0, 3.0)), inside);
    if plant {
        thin = 0.1;
    }
    let seam = (1.0 - smoothstep(0.0, 0.3, cell.seam)) * fine;
    // sparkles: tiny round facets, each with a random tilt, that flash as the sun, the facet and the eye line up; none past a few blocks
    var glitter = 0.0;
    let near = 1.0 - smoothstep(0.05, 0.14, block_rate);
    if near > 0.0 && amount > 0.25 {
        let scaled = mapped * GLITTER_CELLS;
        let corner = floor(scaled);
        // a cell is a patch of the surface, told apart from the one above or below by the block height it is at
        let id = vec3<i32>(i32(corner.x), i32(floor(real.y + 0.5)), i32(corner.y));
        let jitter = vec2<f32>(hash3(id + vec3<i32>(3, 5, 7)), hash3(id + vec3<i32>(11, 13, 17)));
        // a pixel's width in cells, so a facet is never thinner than one
        let size = max(0.2, block_rate * GLITTER_CELLS * 1.3);
        let spot = 1.0 - smoothstep(0.0, size, distance(scaled - corner, jitter));
        if spot > 0.0 && hash3(id) > 0.3 {
            let facet = normalize(vec3<f32>(hash3(id + vec3<i32>(31, 0, 0)) * 1.6 - 0.8, 1.0, hash3(id + vec3<i32>(0, 0, 37)) * 1.6 - 0.8));
            let seen = normalize(eye() - real);
            let flash = pow(max(dot(facet, normalize(to_sun + seen)), 0.0), GLITTER_SHARP);
            let twinkle = 0.7 + 0.3 * sin(time * 2.6 + hash3(id) * 40.0);
            glitter = spot * flash * twinkle * near * amount * (0.35 + 0.65 * smoothstep(0.0, 0.4, to_sun.y));
        }
    }
    return Frost(amount, thin, rim, seam, glitter);
}

// `albedo` under frost: toward a cool off-white that keeps most of the ground's own light and dark and shows through where it's thin,
// with dark seams between plates, a bright rim, and the glitter added.
fn frosted(albedo: vec3<f32>, frost: Frost) -> vec3<f32> {
    let luma = dot(albedo, vec3<f32>(0.2126, 0.7152, 0.0722));
    let ground = clamp(luma / 0.22, 0.6, 1.4);
    var skin = FROST_ALBEDO * mix(1.0, ground, GROUND_SHOWS);
    // the ground's colour comes through thin plates
    skin = mix(skin, albedo * 2.4, 0.3 * frost.thin);
    skin = skin * (1.0 - 0.07 * frost.seam);
    let covered = frost.amount * MOST * mix(1.0, 0.62, frost.thin);
    var out = mix(albedo, skin, covered);
    out = mix(out, skin * RIM_LIFT, frost.rim * 0.3);
    return out + GLITTER_COLOUR * frost.glitter;
}
