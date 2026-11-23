// The cloud pattern every material that draws clouds or their shadows binds, see `clouds::CloudLayer`.

@group(constants::MATERIAL_BIND_GROUP) @binding(4) var cloud_pattern: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var cloud_sampler: sampler;
// the middle of the layer's height, the pattern's width in blocks, and the wind
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var<uniform> cloud_layer: vec4<f32>;

// The weather round the camera, a grid of cells, see `clouds::weather_map`.
struct WeatherMap {
    // the grid's north-west corner in xy, one over a cell's width in z, and how many cells across in w
    corner: vec4<f32>,
    // the most precipitation anywhere on the grid in x, and the most storm cloud in y
    most: vec4<f32>,
    // per cell: share of sky clouded over, a storm's dark cloud, precipitation, and its share falling as snow
    cells: array<vec4<f32>>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(11) var<storage, read> cloud_weather: WeatherMap;

// The weather at a place, each 0..1, see `storms::Sky`.
struct Weather {
    // the share of the sky clouded over
    cloud: f32,
    // how much of a storm's dark cloud is there
    storm: f32,
    // how hard rain or snow falls
    precipitation: f32,
    // the share of it that's snow
    snow: f32,
}

// The share of the sky clouded over until the weather's known: a temperate sky's fair-weather cloud.
const FAIR_CLOUD: f32 = 0.45;
// How far a storm's thick cloud fills the gaps in the pattern, and how much of its shape it keeps.
const STORM_FILL: f32 = 0.75;
// Past this share of sky clouded over, gaps close into an overcast, fully by the second, this thick at its thinnest
const OVERCAST: vec2<f32> = vec2<f32>(0.75, 0.97);
const OVERCAST_FILL: f32 = 0.8;

// The weather over `xz`, blended between the grid's cells.
fn weather_at(xz: vec2<f32>) -> Weather {
    let size = i32(cloud_weather.corner.w);
    if size < 2 {
        return Weather(FAIR_CLOUD, 0.0, 0.0, 0.0);
    }
    let uv = (xz - cloud_weather.corner.xy) * cloud_weather.corner.z - 0.5;
    let base = floor(uv);
    let f = saturate(uv - base);
    let last = vec2<i32>(size - 1);
    let i0 = clamp(vec2<i32>(base), vec2<i32>(0), last);
    let i1 = clamp(vec2<i32>(base) + vec2<i32>(1), vec2<i32>(0), last);
    let a = cloud_weather.cells[i0.y * size + i0.x];
    let b = cloud_weather.cells[i0.y * size + i1.x];
    let c = cloud_weather.cells[i1.y * size + i0.x];
    let d = cloud_weather.cells[i1.y * size + i1.x];
    let w = mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
    return Weather(w.x, w.y, w.z, w.w);
}

// The most rain or snow falling anywhere round the camera, 0 when it's dry everywhere the grid reaches.
fn most_precipitation() -> f32 {
    return select(0.0, cloud_weather.most.x, cloud_weather.corner.w >= 2.0);
}

// The most storm cloud anywhere round the camera, 0 when there's none the grid reaches.
fn most_storm() -> f32 {
    return select(0.0, cloud_weather.most.y, cloud_weather.corner.w >= 2.0);
}

// How much cloud there is, 0..1, where the pattern ranks `share` (0..1, cloudiest last): the weather's share of sky,
// closing into an overcast as it nears all, thickening under a storm.
fn sky_cover(share: f32, weather: Weather) -> f32 {
    let cloudy = weather.cloud;
    // only the divisor is floored, so the last clouds thin out to nothing instead of snapping off
    let scattered = saturate((share - (1.0 - cloudy)) / max(cloudy, 0.05));
    let overcast = smoothstep(OVERCAST.x, OVERCAST.y, cloudy) * (OVERCAST_FILL + (1.0 - OVERCAST_FILL) * share);
    let cover = max(scattered, overcast);
    return mix(cover, max(cover, STORM_FILL + (1.0 - STORM_FILL) * share), weather.storm);
}
