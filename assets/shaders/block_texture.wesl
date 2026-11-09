// ChunkMaterial's bindings and atlas lookup, shared by its main and shadow passes.

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> material_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var material_color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var material_color_sampler: sampler;

@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<uniform> material_fade: vec4<f32>;

// the painted materials, see `material_array` in material.rs
@group(constants::MATERIAL_BIND_GROUP) @binding(12) var materials: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(13) var materials_sampler: sampler;

// textures in blocks.png, see `Tile` in block.rs
const ATLAS_CELLS: f32 = 37.0;
const TILE_INSET: f32 = 0.002;

// The `materials` layer a leaf or plant draws across its tile's own uv, -1 for any other cell: `Tile::foliage_layer`.
// `LEAVES_LAYER` and `FIRST_FLORA_LAYER` come from `ChunkMaterial::specialize`.
fn foliage_layer(vertex_alpha: f32) -> i32 {
    let cell = i32(floor(vertex_alpha + 0.5));
    if cell == i32(constants::LEAF_CELL) {
        return i32(constants::LEAVES_LAYER);
    }
    if cell >= i32(constants::FIRST_FLORA_CELL) {
        return i32(constants::FIRST_FLORA_LAYER) + cell - i32(constants::FIRST_FLORA_CELL);
    }
    return -1;
}

fn is_flora_cell(vertex_alpha: f32) -> bool {
    return vertex_alpha > f32(constants::FIRST_FLORA_CELL) - 0.5;
}

// Texels along a foliage layer, which is 512px against the atlas's 16.
const FOLIAGE_TEXELS: f32 = 512.0;

// A leaf or plant's colour at `uv`: leaves repeat their layer every block, a sprite is drawn once, kept inside its
// edge so the repeat doesn't bleed its far side in. `dx` and `dy` are `uv`'s screen derivatives.
fn foliage_sample(uv: vec2<f32>, layer: i32, flora: bool, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    var p = uv;
    if flora {
        let texels = max(length(dx), length(dy)) * FOLIAGE_TEXELS;
        let coarsest = exp2(clamp(ceil(log2(max(texels, 1.0))), 0.0, log2(FOLIAGE_TEXELS)));
        let inset = max(TILE_INSET, 0.5 * coarsest / FOLIAGE_TEXELS);
        p = clamp(uv, vec2<f32>(inset), vec2<f32>(1.0 - inset));
    }
    return material_color * textureSampleGrad(materials, materials_sampler, p, layer, dx, dy);
}

// How blurred a leaf or plant is cut out at `distance` blocks from the camera: a texel a block deep is far finer
// than a pixel past a block or two, and cut at full detail it would shimmer. Both the main and depth passes cut at
// this, so they agree. `view` is the camera's.
fn foliage_level(distance: f32, viewport_height: f32, focal: f32) -> f32 {
    let blocks_per_pixel = 2.0 * distance / (viewport_height * focal);
    return clamp(log2(max(blocks_per_pixel * FOLIAGE_TEXELS, 1.0)), 0.0, log2(FOLIAGE_TEXELS));
}

// Blurred alpha loses thin leaves and stems, which average below the cut, so it's scaled up as the level climbs to cut at
// a lower coverage: a field of grass stays a field rather than fading out a few blocks away.
const COVERAGE_CUT_FAR: f32 = 0.1;
const COVERAGE_LEVELS: f32 = 5.0;
fn foliage_cut(alpha: f32, level: f32, far: f32) -> f32 {
    let cut = mix(0.5, far, clamp(level / COVERAGE_LEVELS, 0.0, 1.0));
    return min(alpha * 0.5 / cut, 1.0);
}

// Shrubs and heath are broad solid sprites, not thin blades, so the blur and the lowered cut that keep a meadow from thinning
// out would draw them as flat slabs: past level 6 or so a mip texel is half the sprite and its average passes the cut
// whole, the quad drawn solid above the mound. They are cut at no more than this level and at a higher coverage.
const BIG_FLORA_MAX_LEVEL: f32 = 5.0;
const BIG_FLORA_CUT_FAR: f32 = 0.3;

fn is_big_flora_cell(vertex_alpha: f32) -> bool {
    return vertex_alpha > f32(constants::FIRST_BIG_FLORA_CELL) - 0.5;
}

// A leaf or plant's alpha from mip `level`, which averages away holes smaller than the lookup's footprint, scaled by `foliage_cut`.
// The atlas cells with no foliage layer have the alpha of their tile.
fn block_alpha_at(uv: vec2<f32>, vertex_alpha: f32, blurred: f32) -> f32 {
    let layer = foliage_layer(vertex_alpha);
    let big = is_big_flora_cell(vertex_alpha);
    let level = select(blurred, min(blurred, BIG_FLORA_MAX_LEVEL), big);
    if layer >= 0 {
        var p = uv;
        if is_flora_cell(vertex_alpha) {
            let inset = max(TILE_INSET, 0.5 * exp2(level) / FOLIAGE_TEXELS);
            p = clamp(uv, vec2<f32>(inset), vec2<f32>(1.0 - inset));
        }
        return foliage_cut(textureSampleLevel(materials, materials_sampler, p, layer, level).a, level, select(COVERAGE_CUT_FAR, BIG_FLORA_CUT_FAR, big)) * material_color.a;
    }
    let atlas_cell = floor(vertex_alpha + 0.5);
    // a coarse level's texels are wider, so the lookup stays half of one inside the tile
    let inset = max(TILE_INSET, 0.5 * exp2(level) / f32(textureDimensions(material_color_texture).y));
    let tile_uv = clamp(fract(uv), vec2<f32>(inset), vec2<f32>(1.0 - inset));
    let atlas_uv = vec2<f32>((atlas_cell + tile_uv.x) / ATLAS_CELLS, tile_uv.y);
    return textureSampleLevel(material_color_texture, material_color_sampler, atlas_uv, level).a * material_color.a;
}

// How a block face's uv covers the screen, for `block_sample`. It takes derivatives, so needs
// uniform control flow, while `block_sample` doesn't: work it out once, then sample any tiles.
struct BlockFootprint {
    dx: vec2<f32>,
    dy: vec2<f32>,
    inset: f32,
}

fn block_footprint(uv: vec2<f32>) -> BlockFootprint {
    // Blurry mip levels' texels are wider, and bilinear filtering would blend the edge ones
    // with the next tile's, so the lookup stays half a texel of the coarsest level in use inside.
    let tile_texels = f32(textureDimensions(material_color_texture).y);
    let footprint = max(length(vec2<f32>(dpdx(uv.x), dpdx(uv.y))), length(vec2<f32>(dpdy(uv.x), dpdy(uv.y)))) * tile_texels;
    let coarsest = exp2(clamp(ceil(log2(max(footprint, 1.0))), 0.0, log2(tile_texels)));
    return BlockFootprint(
        vec2<f32>(dpdx(uv.x) / ATLAS_CELLS, dpdx(uv.y)),
        vec2<f32>(dpdy(uv.x) / ATLAS_CELLS, dpdy(uv.y)),
        max(TILE_INSET, 0.5 * coarsest / tile_texels),
    );
}

// Block meshes put tile counts in uv and the atlas cell in vertex alpha, so merged faces
// repeat their texture.
fn block_sample(uv: vec2<f32>, vertex_alpha: f32, footprint: BlockFootprint) -> vec4<f32> {
    let layer = foliage_layer(vertex_alpha);
    if layer >= 0 {
        // the footprint's derivatives are in atlas cells
        return foliage_sample(uv, layer, is_flora_cell(vertex_alpha), vec2<f32>(footprint.dx.x * ATLAS_CELLS, footprint.dx.y), vec2<f32>(footprint.dy.x * ATLAS_CELLS, footprint.dy.y));
    }
    let atlas_cell = floor(vertex_alpha + 0.5);
    let tile_uv = clamp(fract(uv), vec2<f32>(footprint.inset), vec2<f32>(1.0 - footprint.inset));
    let atlas_uv = vec2<f32>((atlas_cell + tile_uv.x) / ATLAS_CELLS, tile_uv.y);
    let texel = textureSampleGrad(material_color_texture, material_color_sampler, atlas_uv, footprint.dx, footprint.dy);
    return material_color * texel;
}

// The painted material layer each atlas cell draws, -1 for the leaves, flora and water that stay in the atlas: `Tile::material_layer`,
// which a test checks this against.
const MATERIAL_LAYER_TABLE: array<i32, 37> = array<i32, 37>(
    0, // stone
    2, // grass top
    3, // sand
    -1, // water
    4, // snow
    14, // log bark
    -1, // leaves
    2, // grass side
    1, // dirt
    15, // log end
    12, // gravel
    13, // ice
    11, // sandstone side
    11, // sandstone top
    4, // snow side
    16, // compacted stone
    5, // limestone
    6, // clay
    7, // basalt
    8, // marble
    17, // compacted limestone
    18, // compacted clay
    19, // compacted basalt
    20, // compacted marble
    21, // compacted sandstone
    9, // granite
    10, // shale
    22, // compacted granite
    23, // compacted shale
    -1, // tall grass
    -1, // fern
    -1, // poppy
    -1, // dandelion
    -1, // short grass
    -1, // seed grass
    -1, // shrub
    -1, // heath
);

fn material_layer(vertex_alpha: f32) -> i32 {
    var table = MATERIAL_LAYER_TABLE;
    return table[u32(floor(vertex_alpha + 0.5))];
}

// Spreads `p` about the nearest block boundary along the way a face tilts, in the plane it's projected onto, by
// `tilt` blocks of rise for each block across, so a tilted face covers as much of the texture as a flat one does: a
// chamfer's 45 degrees would stretch it by two fifths. `tilt` is the face's normal in the plane's axes over its
// normal's other part. Block boundaries are at half-integers, where the flat faces on either side of a chamfer are
// continuous with it.
fn unstretched(p: vec2<f32>, tilt: vec2<f32>) -> vec2<f32> {
    let steep = length(tilt);
    if steep < 1e-4 {
        return p;
    }
    let boundary = round(p - 0.5) + 0.5;
    let across = p - boundary;
    let along = tilt / steep;
    return boundary + across + along * dot(across, along) * (sqrt(1.0 + steep * steep) - 1.0);
}

// Where a painted face sits in its plane, in blocks: the chunk origin's remainder by the period
// is exact in f32 (the period is a power of two), so the texture holds still far from the origin.
// A mesh stores the normals of faces along z reversed. One whose mesh worked out its texture coordinates, with
// `generator::UNFOLDED` added to their `x`, has them from the chunk's corner and only needs the origin's remainder.
fn material_uv(local: vec3<f32>, origin: vec3<f32>, stored_normal: vec3<f32>, mesh_uv: vec2<f32>) -> vec2<f32> {
    let period = f32(constants::MATERIAL_PERIOD);
    let remainder = origin - floor(origin / period) * period;
    let at = local + remainder;
    let normal = vec3<f32>(stored_normal.x, stored_normal.y, -stored_normal.z);
    let n = abs(normal);
    let unfolded = mesh_uv.x > 512.0;
    let given = mesh_uv - vec2<f32>(1024.0, 0.0);
    if n.y >= n.x && n.y >= n.z {
        if unfolded {
            return given + remainder.xz;
        }
        return unstretched(at.xz, normal.xz / n.y);
    }
    // sides have world up as the texture's up
    if n.x >= n.z {
        if unfolded {
            return given + vec2<f32>(remainder.z, -remainder.y);
        }
        return unstretched(vec2<f32>(at.z, -at.y), vec2<f32>(normal.z, -normal.y) / n.x);
    }
    if unfolded {
        return given + vec2<f32>(remainder.x, -remainder.y);
    }
    return unstretched(vec2<f32>(at.x, -at.y), vec2<f32>(normal.x, -normal.y) / n.z);
}

// How a painted face's uv covers the screen, in material repeats. It takes derivatives, so needs
// uniform control flow, while `material_sample` doesn't.
struct MaterialFootprint {
    dx: vec2<f32>,
    dy: vec2<f32>,
}

fn material_footprint(uv: vec2<f32>) -> MaterialFootprint {
    let period = f32(constants::MATERIAL_PERIOD);
    return MaterialFootprint(dpdx(uv) / period, dpdy(uv) / period);
}

fn material_sample(uv: vec2<f32>, layer: i32, footprint: MaterialFootprint) -> vec4<f32> {
    let texel = textureSampleGrad(materials, materials_sampler, uv / f32(constants::MATERIAL_PERIOD), layer, footprint.dx, footprint.dy);
    return material_color * vec4<f32>(texel.rgb, 1.0);
}

// What a tile averages to: the mean linear colour of each atlas cell's opaque texels, in atlas
// order. `gen_atlas` prints these and the block-textures skill's `pack.py`
// rewrites them, and `material.rs`'s tests check them against blocks.png.
fn block_mean(vertex_alpha: f32) -> vec4<f32> {
    var means = array<vec3<f32>, 37>(
        vec3<f32>(0.1612, 0.1542, 0.1649), // stone
        vec3<f32>(0.4981, 0.4981, 0.4981), // grass top
        vec3<f32>(0.7134, 0.4024, 0.2341), // sand
        vec3<f32>(0.0665, 0.1023, 0.9149), // water
        vec3<f32>(0.6173, 0.7140, 0.8183), // snow
        vec3<f32>(0.0755, 0.0383, 0.0301), // log bark
        vec3<f32>(0.2499, 0.2499, 0.2499), // leaves
        vec3<f32>(0.5106, 0.5106, 0.5106), // grass side
        vec3<f32>(0.2403, 0.1078, 0.0716), // dirt
        vec3<f32>(0.4026, 0.2195, 0.1318), // log end
        vec3<f32>(0.2730, 0.2296, 0.2106), // gravel
        vec3<f32>(0.4181, 0.6444, 0.6924), // ice
        vec3<f32>(0.6178, 0.4855, 0.2738), // sandstone side
        vec3<f32>(0.8332, 0.3810, 0.2250), // sandstone top
        vec3<f32>(0.4256, 0.3341, 0.2906), // snow side
        vec3<f32>(0.1049, 0.1048, 0.1322), // compacted stone
        vec3<f32>(0.5822, 0.4646, 0.3619), // limestone
        vec3<f32>(0.4313, 0.1646, 0.0943), // clay
        vec3<f32>(0.0618, 0.0582, 0.0596), // basalt
        vec3<f32>(0.7205, 0.6952, 0.7124), // marble
        vec3<f32>(0.4055, 0.3244, 0.2533), // compacted limestone
        vec3<f32>(0.3013, 0.1168, 0.0676), // compacted clay
        vec3<f32>(0.0449, 0.0424, 0.0434), // compacted basalt
        vec3<f32>(0.5001, 0.4827, 0.4945), // compacted marble
        vec3<f32>(0.5776, 0.2663, 0.1587), // compacted sandstone
        vec3<f32>(0.3406, 0.3244, 0.3538), // granite
        vec3<f32>(0.1032, 0.0949, 0.1021), // shale
        vec3<f32>(0.2386, 0.2275, 0.2477), // compacted granite
        vec3<f32>(0.0739, 0.0680, 0.0731), // compacted shale
        vec3<f32>(0.4954, 0.4954, 0.4954), // tall grass
        vec3<f32>(0.3000, 0.3000, 0.3000), // fern
        vec3<f32>(0.3995, 0.0742, 0.0164), // poppy
        vec3<f32>(0.3838, 0.3697, 0.0299), // dandelion
        vec3<f32>(0.4988, 0.4988, 0.4988), // short grass
        vec3<f32>(0.4951, 0.4951, 0.4951), // seed grass
        vec3<f32>(0.2699, 0.2699, 0.2699), // shrub
        vec3<f32>(0.1522, 0.1146, 0.1861), // heath
    );
    let atlas_cell = u32(floor(vertex_alpha + 0.5));
    return material_color * vec4<f32>(means[atlas_cell], 1.0);
}
