// Bends the land away from the camera as if the world were round (`curvature.rs`); drawing only, the world stays flat

struct Bend {
    // the camera's position, not the view's: the sun's shadow passes see the light's
    eye: vec3<f32>,
    // how far land drops per square block of distance across from the camera
    per_square_block: f32,
}

// What the land is told about the cold, see `ochre_base::cold::shader_values`.
struct Cold {
    // the camera's sea-level temperature, how much frost its biome keeps, 1 if frost is on, ground height under it
    camera: vec4<f32>,
    // blocks of height that cool the ground by one, ground temperature at the snow line, how far warmer frost reaches
    rule: vec4<f32>,
}

struct Bends {
    now: Bend,
    // last frame's, for motion vectors
    last: Bend,
    cold: Cold,
    // the haze toward 16 compass points, four vec4s of tint and density each: the camera's and three rings out
    haze: array<vec4<f32>, 64>,
    // how much of its sway the foliage keeps in x, see `foliage.wesl`
    sway: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(8) var<storage, read> curvature: Bends;

fn bent(world_position: vec3<f32>, bend: Bend) -> vec3<f32> {
    let across = world_position.xz - bend.eye.xz;
    return world_position - vec3<f32>(0.0, dot(across, across) * bend.per_square_block, 0.0);
}

// Where `world_position` is drawn this frame.
fn curved(world_position: vec3<f32>) -> vec3<f32> {
    return bent(world_position, curvature.now);
}

// Where `world_position` was drawn last frame.
fn curved_last(world_position: vec3<f32>) -> vec3<f32> {
    return bent(world_position, curvature.last);
}

// Where land drawn at `world_position` this frame really is.
fn uncurved(world_position: vec3<f32>) -> vec3<f32> {
    let across = world_position.xz - curvature.now.eye.xz;
    return world_position + vec3<f32>(0.0, dot(across, across) * curvature.now.per_square_block, 0.0);
}

// Where the camera is, in every pass: the sun's shadow passes' view is the light's.
fn eye() -> vec3<f32> {
    return curvature.now.eye;
}

// How much sway the foliage keeps at the camera, 0..1: less in a sheltered hollow, where the wind is quiet.
fn sway_kept() -> f32 {
    return curvature.sway.x;
}

// What the camera's climate does to the land, see `frost.wesl`.
fn cold() -> Cold {
    return curvature.cold;
}

// Distances of the rings `haze` is sensed at, in blocks.
const HAZE_RINGS: vec3<f32> = vec3<f32>(1000.0, 3000.0, 8000.0);

// The haze `distance` blocks off along `across`, between the ring points: a tint for its colour, a density for length
fn haze_toward(across: vec2<f32>, distance: f32) -> vec4<f32> {
    let turn = (atan2(across.y, across.x) / 6.2831853 + 0.5) * 16.0;
    let first = u32(floor(turn)) % 16u;
    let second = (first + 1u) % 16u;
    var ring = 3.0;
    if distance < HAZE_RINGS.x {
        ring = distance / HAZE_RINGS.x;
    } else if distance < HAZE_RINGS.y {
        ring = 1.0 + (distance - HAZE_RINGS.x) / (HAZE_RINGS.y - HAZE_RINGS.x);
    } else {
        ring = 2.0 + min((distance - HAZE_RINGS.y) / (HAZE_RINGS.z - HAZE_RINGS.y), 1.0);
    }
    let low = min(u32(floor(ring)), 2u);
    let along = ring - f32(low);
    let a = mix(curvature.haze[first * 4u + low], curvature.haze[first * 4u + low + 1u], along);
    let b = mix(curvature.haze[second * 4u + low], curvature.haze[second * 4u + low + 1u], along);
    return mix(a, b, fract(turn));
}
