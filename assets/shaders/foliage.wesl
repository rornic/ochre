// Leaves and ground flora swaying in the wind, shared by ChunkMaterial's main and shadow
// passes so they keep to their shadows.

import super::curvature::eye;

// How far, in blocks, leaves sway: little enough that they never pull far from their twigs.
const SWAY: f32 = 0.08;
// How far, in blocks, a plant's tip sways. Its roots stay put.
const FLORA_SWAY: f32 = 0.16;
// How far a plant leans with the wind on average, as a share of `FLORA_SWAY`, so a meadow
// bends one way in a gust rather than wobbling about upright.
const FLORA_LEAN: f32 = 0.35;

// The world's prevailing wind in x and z, see `ChunkMaterial::wind`: zero until set, which reads as the old fixed direction.
@group(constants::MATERIAL_BIND_GROUP) @binding(14) var<uniform> wind: vec4<f32>;
// How far apart, in blocks, the broad gust bands that cross open ground are, along the wind.
const BAND_BLOCKS: f32 = 190.0;

fn wind_direction() -> vec2<f32> {
    if dot(wind.xy, wind.xy) < 0.0001 {
        return normalize(vec2<f32>(0.846, 0.538));
    }
    return normalize(wind.xy);
}

// How hard the wind pushes at `world` at `time`, about -1.2..1.2: a quick gust plus slow broad bands rolling along the wind.
fn gust_at(world: vec2<f32>, time: f32) -> f32 {
    let along = dot(world, wind_direction());
    return 0.7 * sin(time * 1.3 + along * 0.13) + 0.5 * sin(time * 0.55 + along * 6.2831853 / BAND_BLOCKS);
}

// Whether a block mesh's vertex alpha, its atlas cell, is leaves'. `LEAF_CELL` comes from
// `ChunkMaterial::specialize`.
fn is_leaf(vertex_alpha: f32) -> bool {
    return abs(vertex_alpha - f32(constants::LEAF_CELL)) < 0.5;
}

// How much of the way leaves at `world_position` are to having their holes filled with the shade
// of the leaves behind them, 0..1: past `INNER_LEAVES_REACH` blocks of the camera at `eye`, over
// the band before it.
fn leaf_holes_filled(world_position: vec3<f32>, eye: vec3<f32>) -> f32 {
    let reach = f32(constants::INNER_LEAVES_REACH);
    let band = f32(constants::INNER_LEAVES_BAND);
    return smoothstep(reach - band, reach, distance(world_position, eye));
}

// Whether it's ground flora's, whose sprites are the atlas's last tiles. `FIRST_FLORA_CELL`
// comes from `ChunkMaterial::specialize`.
fn is_flora(vertex_alpha: f32) -> bool {
    return vertex_alpha > f32(constants::FIRST_FLORA_CELL) - 0.5;
}

// A plant's height in blocks, which `push_tuft` codes in its vertex alpha's fraction about the atlas cell.
const FLORA_HEIGHT_SPAN: f32 = 4.0;
const FLORA_HEIGHT_CODE: f32 = 0.4;

fn flora_height(vertex_alpha: f32) -> f32 {
    let code = vertex_alpha - floor(vertex_alpha + 0.5);
    return (code / FLORA_HEIGHT_CODE * 0.5 + 0.5) * FLORA_HEIGHT_SPAN;
}

// How far a plant `height` blocks tall at `world` has shrunk into its root, 0..1, the tallest lasting longest: `FloraFade::gone_at`.
const FLORA_HEIGHTS: vec2<f32> = vec2<f32>(0.4, 1.3);
const FLORA_KEEP: f32 = 0.3;
const FLORA_FIRST_GONE: f32 = 0.35;

fn flora_shrink(world: vec3<f32>, height: f32) -> f32 {
    let start = f32(constants::FLORA_FADE_START);
    let mid = f32(constants::FLORA_FADE_MID);
    let end = f32(constants::FLORA_FADE_END);
    let rank = clamp((height - FLORA_HEIGHTS.x) / (FLORA_HEIGHTS.y - FLORA_HEIGHTS.x), 0.0, 1.0);
    let first = start + (mid - start) * FLORA_FIRST_GONE;
    let gone = select(mid + (end - mid) * (rank - FLORA_KEEP) / (1.0 - FLORA_KEEP), first + (mid - first) * rank / FLORA_KEEP, rank < FLORA_KEEP);
    return smoothstep(start, gone, distance(world, eye()));
}

// Where a vertex at `world` sways to by `time`. It depends on nothing but where the vertex
// is, so corners shared by neighbouring leaves move together and leave no cracks. Coarse LOD
// cells, whose faces span many blocks, stay still: their corners would drift apart and tear.
// Flora's tops sway, by `uv`'s v, which is 0 at a plant's tip and 1 at its roots.
fn swayed(world: vec3<f32>, vertex_alpha: f32, uv: vec2<f32>, coarse: bool, time: f32) -> vec3<f32> {
    // bending most at the tip, as a stem does
    let bend = 1.0 - clamp(uv.y, 0.0, 1.0);
    var shrink = 0.0;
    var at = world;
    if is_flora(vertex_alpha) {
        // far off its tip sinks to its root, so the meadow thins out with no edge where flora stops being drawn
        let height = flora_height(vertex_alpha);
        shrink = flora_shrink(world, height);
        at.y -= shrink * bend * height;
    }
    // `DebugSettings::still_leaves`
@if(STILL_LEAVES) {
    return at;
}
    if coarse {
        return at;
    }
    // slow gusts roll across the land with the wind, see `gust_at`
    let toward = wind_direction();
    if is_flora(vertex_alpha) {
        let gust = gust_at(world.xz, time);
        // quicker than the boughs
        let flutter = sin(time * 3.3 + dot(world.xz, vec2<f32>(1.7, 1.1)));
        let push = (FLORA_LEAN + gust * 0.45 + flutter * 0.25) * bend * bend * FLORA_SWAY * (1.0 - shrink);
        // a leaning tip dips a little, as its stem doesn't stretch
        return at + vec3<f32>(toward.x * push * 1.17, -abs(push) * 0.3, toward.y * push * 1.17);
    }
    if !is_leaf(vertex_alpha) {
        return world;
    }
    let gust = gust_at(world.xz, time);
    // with a quicker flutter on top
    let flutter = sin(time * 4.1 + dot(world, vec3<f32>(0.9, 0.7, 1.3)));
    let push = gust * 0.7 + flutter * 0.3;
    return world + vec3<f32>(toward.x * push * 1.17, flutter * 0.4, toward.y * push * 1.17) * SWAY;
}
