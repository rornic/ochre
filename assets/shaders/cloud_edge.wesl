// Where a painted cloud's body begins and how opaque it gets, shared by the painting and by what the sun and moon show through, see `cloud_paint.wesl` and `cloud_cover.wesl`.

// Where a shape's edge falls in the march's cover, and how soft it is at least in cover.
const EDGE: f32 = 0.4;
const EDGE_SOFT: f32 = 0.03;
// The most opaque the body of a cloud gets: under 1, the moon, thousands of times brighter than the sky behind, shows through it.
const THICK: f32 = 1.0;

// How opaque a painted cloud is over cover `cover`, without its brushed edge or haze.
fn body(cover: f32) -> f32 {
    return smoothstep(EDGE - 2.0 * EDGE_SOFT, EDGE + 2.0 * EDGE_SOFT, cover) * THICK;
}
