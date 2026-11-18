import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    view_transformations::{direction_view_to_world, frag_coord_to_ndc, position_ndc_to_view, position_world_to_clip, position_world_to_view},
};
import super::block_texture::{
    block_alpha_at, block_footprint, block_mean, block_sample, foliage_level, material_fade, material_footprint, material_layer,
    material_sample, material_triplanar, material_uv, triplanar_footprint,
};
import super::hash::value_noise;
import super::foliage::{is_flora, is_leaf, leaf_holes_filled, swayed};
import super::fog::fogged;
import super::lighting::{direct_moon, open_sky, sun_radiance};
import super::painterly::{FAR_VISIBILITY, Painterly, painterly_light, painterly_saturation};
import super::sky::SUN_LIGHT;
import super::cloud_shadow::cloud_shadow;
import super::lod_patch::patch_tint;
import super::point_lights::point_light_glow;
import super::dither::dither_threshold;
import super::curvature::{curved, eye, uncurved};
import super::frost::{frost_at, frost_receives, frosted};
import super::ice::{ice_albedo, ice_shine, is_ice};
// `nets::FACET_SPAN`: a sculpted vertex's facet, 0 to 1, is its mode over this.
const SCULPT_FACET_SPAN: f32 = 0.45;
// `nets::AO_MID` and `AO_SPREAD`: a sculpted vertex's occlusion, from its cell's fraction.
const SCULPT_AO_MID: f32 = 0.6;
const SCULPT_AO_SPREAD: f32 = 1.0;
// How bright the leaves seen through a solid tree box's holes are, to its surface's.
const HOLE_SHADE: f32 = 0.55;
// The same for far full-detail leaves, see `leaf_holes_filled`. Their holes are a texel or so
// across, and show the lit face of the next leaf in.
const FAR_HOLE_SHADE: f32 = 0.8;
// All that's left with no sky light at all, so caves aren't pitch black.
const CAVE_AMBIENT: vec3<f32> = vec3<f32>(0.02, 0.02, 0.025);
// Above 1, light falls away quickly at first under cover, then more slowly.
const SKY_LIGHT_CURVE: f32 = 2.0;
// Shadow maps only reach so far, so beyond them sunlight is kept out of caves by sky
// light instead: none gets in below this much of it.
const SUN_SKY_GATE: f32 = 0.3;
// Shadows fade out over this last part of their reach, instead of stopping at a line.
const SHADOW_FADE_START: f32 = 0.8;

// Light from the sky for sky light 0..1, see `light.rs`.
fn sky_ambient(sky: f32, normal: vec3<f32>) -> vec3<f32> {
    return CAVE_AMBIENT + open_sky(normal) * pow(clamp(sky, 0.0, 1.0), SKY_LIGHT_CURVE);
}

fn sun_shadow(world_position: vec4<f32>, normal: vec3<f32>, frag_coord: vec2<f32>) -> f32 {
    let sun = &view_bindings::lights.directional_lights[SUN_LIGHT];
    if ((*sun).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
        return 1.0;
    }
    let view_z = position_world_to_view(world_position.xyz).z;
    let reach = (*sun).cascades[(*sun).num_cascades - 1u].far_bound;
    // faded out wholly, so nothing to look up
    if -view_z >= reach {
        return FAR_VISIBILITY;
    }
    let shadow = fetch_directional_shadow(SUN_LIGHT, world_position, normal, view_z, frag_coord);
    return mix(shadow, FAR_VISIBILITY, smoothstep(reach * SHADOW_FADE_START, reach, -view_z));
}

// How far toward the sun a plant's shadow lookup is moved, in blocks, and the distances between which its shadows fade out.
const FLORA_SHADOW_BIAS: f32 = 0.3;
const FLORA_SHADOW_FADE: vec2<f32> = vec2<f32>(40.0, 120.0);

// The light on a face, shadowed by the land and the clouds, and moonlight, kept out of caves by sky light but not shadowed, see `painterly.wesl`.
fn painterly_world_light(world_position: vec4<f32>, normal: vec3<f32>, shaded: vec3<f32>, sky: f32, frag_coord: vec2<f32>, leaf: bool, flora: bool) -> Painterly {
    let gate = smoothstep(0.0, SUN_SKY_GATE, sky);
    var to_sun = vec3<f32>(0.0, 1.0, 0.0);
    var sun = vec3<f32>(0.0);
    var visibility = 0.0;
    if view_bindings::lights.n_directional_lights > 0u {
        to_sun = view_bindings::lights.directional_lights[SUN_LIGHT].direction_to_light;
        sun = sun_radiance(uncurved(world_position.xyz)) * gate;
        // no sun, nothing to shadow
        if any(sun > vec3<f32>(0.0)) {
            // a plant's own cut-out shadow falls on its quad, which the map's texels are too coarse to keep off it, so
            // its lookup is moved toward the sun and it takes the land's shadows less the further it is
            var lookup = world_position;
            var unshadowed = 0.0;
            if flora {
                lookup = vec4<f32>(world_position.xyz + to_sun * FLORA_SHADOW_BIAS, world_position.w);
                unshadowed = smoothstep(FLORA_SHADOW_FADE.x, FLORA_SHADOW_FADE.y, distance(world_position.xyz, view_bindings::view.world_position));
            }
            visibility = mix(sun_shadow(lookup, normal, frag_coord), 1.0, unshadowed) * cloud_shadow(uncurved(world_position.xyz));
        }
    }
    return painterly_light(shaded, to_sun, sun, visibility, sky_ambient(sky, shaded), direct_moon(shaded) * gate, leaf);
}

// Faces turn toward up between these distances in blocks, to this much of the way: past
// where a step is under a pixel, lit tops and dark sides shimmer into each other. LOD cells are
// a few blocks wide well before then, so it starts where shadows end, the default
// `RendererSettings::shadow_distance`, and the land is lit block by block as far as it's shadowed.
const FLATTEN_START: f32 = 1024.0;
const FLATTEN_END: f32 = 4096.0;
const FLATTEN_MAX: f32 = 0.85;

// Tiles up a pixel at which a face's texture is its mean colour: under a pixel across, the lookup
// is the coarsest mip, which `block_mean` holds.
const MEAN_BLOCK_RATE: f32 = 1.0;

// Steep ground under vegetation bares to dirt along a ragged edge: none above `STEEP_FROM` up (1 is flat), all below `STEEP_TO`.
const STEEP_FROM: f32 = 0.8;
const STEEP_TO: f32 = 0.45;

// How far the land's brightness and warmth drift from a painted material, across patches tens of blocks wide.
const DRIFT_BRIGHTNESS: f32 = 0.10;
const DRIFT_WARMTH: f32 = 0.04;

// A slow, wide factor over the land, so a painted texture's repeat doesn't line up. The wavelengths aren't
// multiples of the period, and each noise averages out to 1.
fn drift(p: vec2<f32>) -> vec3<f32> {
    let bright = DRIFT_BRIGHTNESS * (value_noise(p / 37.0) * 2.0 - 1.0);
    let warm = DRIFT_WARMTH * (value_noise(p / 91.0 + vec2<f32>(17.0, 5.0)) * 2.0 - 1.0);
    return vec3<f32>(1.0 + bright + warm, 1.0 + bright, 1.0 + bright - warm);
}

// How a painted top's blend mark is laid out, see `BLEND_BAND` in `buffers.rs`.
const BLEND_BAND: f32 = 0.125;
const BLEND_BASE: f32 = 0.0125;
const BLEND_SPAN: f32 = 0.1;
// How far toward its wall a ramp is lit, at the most: it meets the wall in a crease, not a turn.
const RAMP_LIT_TURN: f32 = 0.5;

// Neutral rock under the warm key reads peach, where grass only reads sunny, so it's cooled a little. This covers stone,
// basalt, granite, shale and their compacted forms, by material layer, and a test checks the layers against `MATERIAL_FILES`.
const ROCK_COOL: vec3<f32> = vec3<f32>(0.93, 0.99, 1.06);

// The grasses, which steep ground bares to dirt, see `STEEP_FROM`. `GRASS_LAYER` and `STEPPE_LAYER` come from `specialize`.
fn is_grass(layer: i32) -> bool {
    return layer == i32(constants::GRASS_LAYER) || layer == i32(constants::STEPPE_LAYER);
}

fn is_neutral_rock(layer: i32) -> bool {
    return layer == 6 || layer == 8 || layer == 9 || layer == 17 || layer == 20 || layer == 21;
}

// The normal to light by: turned toward up with distance, which evens out the sun's and sky's contrast between faces.
fn lit_normal(normal: vec3<f32>, world_position: vec3<f32>) -> vec3<f32> {
    let flatten = FLATTEN_MAX * smoothstep(FLATTEN_START, FLATTEN_END, distance(world_position, view_bindings::view.world_position));
    return normalize(mix(normal, vec3<f32>(0.0, 1.0, 0.0), flatten));
}

// Bevy's mesh vertex shader, for meshes that are never skinned or morphed, with leaves
// swaying in the wind and the land curving away. Block meshes always carry vertex colours,
// see `MeshBuffers`.
@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    var coarse = false;
@if(VERTEX_UVS_B) {
    coarse = vertex.uv_b.y > 0.5;
}
    // painted faces carry their plane coordinates in uv, see `material_uv`
    var uv = vertex.uv;
    // sculpted ground keeps its mark, and projects three ways in the fragment shader
    if !coarse && material_layer(vertex.color.a) >= 0 && vertex.uv.x > -512.0 {
        uv = material_uv(vertex.position, world_from_local[3].xyz, vertex.normal, vertex.uv);
    }
    let swaying = swayed(out.world_position.xyz, vertex.color.a, vertex.uv, coarse, view_bindings::globals.time);
    // bent where it's drawn, so shadows and fog are looked up where it's seen
    out.world_position = vec4<f32>(curved(swaying), 1.0);
    out.color = vertex.color;
    out.position = position_world_to_clip(out.world_position.xyz);
    // a mesh fading out sits a little farther than it is, so what replaces it wins where they coincide
    out.position.z *= 1.0 - material_fade.z;
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.uv = uv;
@if(VERTEX_UVS_B) {
    out.uv_b = vertex.uv_b;
}
@if(VERTEX_OUTPUT_INSTANCE_INDEX) {
    out.instance_index = vertex.instance_index;
}
@if(VISIBILITY_RANGE_DITHER) {
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
}
    return out;
}

struct FragmentOutput {
  @location(0) color: vec4<f32>
}

@fragment
fn fragment(
    in: VertexOutput,
) ->  FragmentOutput {

    let world_position = in.world_position.xyz;
    // sculpted ground, see `nets.rs`: projected three ways, blending to a second material, its facets cut here
    let sculpted = in.uv.x < -512.0;
    // the triangle's own normal, in the stored normals' terms (z reversed), and how much of it a sculpted vertex takes
    // from the position relative to the camera, as world positions this far out are too coarse to difference between pixels
    let view_at = position_ndc_to_view(frag_coord_to_ndc(in.position));
    let flat_side = normalize(direction_view_to_world(cross(dpdy(view_at), dpdx(view_at))));
    var facet = 0.0;
    // coarse cells draw their mean colour, and a coarse sculpted vertex's mode is 1 more, see `nets::FACET_SPAN`
    var coarse = false;
@if(VERTEX_UVS_B) {
    facet = select(0.0, fract(in.uv_b.y) / SCULPT_FACET_SPAN, sculpted);
    coarse = in.uv_b.y > 0.5;
}
    // a sculpted blend: tinted side (1 second, 2 first, 0 both), weight toward the second, frayed cut; facets only on rock
    let sculpt_side = floor(in.uv.y / 2.0);
    let sculpt_weight = in.uv.y - sculpt_side * 2.0;
    let unbent = uncurved(world_position);
    let fray = value_noise(unbent.xz * 0.45 + vec2<f32>(unbent.y * 0.3)) + 0.5 * value_noise(unbent.xz * 1.6 + vec2<f32>(7.0, unbent.y));
    // a band between `nets::OWN_SIDE` and its mirror, frayed no further, so each vertex shows its own material alone
    let toward = smoothstep(0.38, 0.62, sculpt_weight + (fray / 1.5 - 0.5) * 0.16);
    facet *= select(select(1.0, toward, sculpt_side == 2.0), 1.0 - toward, sculpt_side == 1.0);
    var normal = normalize(in.world_normal);
    if sculpted && facet > 0.001 {
        var cut = vec3<f32>(flat_side.x, flat_side.y, -flat_side.z);
        if dot(cut, normal) < 0.0 {
            cut = -cut;
        }
        normal = normalize(mix(normal, cut, facet));
    }

    let footprint = block_footprint(in.uv);
    let painted_footprint = material_footprint(in.uv);
    // a painted top that meets a wall is drawn partly in the wall's projection, which the texture then runs on into, and
    // lit partly as the wall: how much, and which way the wall faces, are in its mode, see `BLEND_BAND` in `buffers.rs`
    var wall_blend = 0.0;
    var wall_faces_z = false;
    // the way the wall faces, as the stored normals have it, and whether this face has one
    var wall_side = vec2<f32>(0.0);
    var has_wall = false;
@if(VERTEX_UVS_B) {
    if in.uv_b.y > 0.0 && in.uv_b.y < 0.5 && !sculpted {
        let band = floor(in.uv_b.y / BLEND_BAND);
        has_wall = true;
        wall_faces_z = band >= 2.0;
        wall_blend = clamp((in.uv_b.y - band * BLEND_BAND - BLEND_BASE) / BLEND_SPAN, 0.0, 1.0);
        let facing = select(1.0, -1.0, band % 2.0 >= 1.0);
        wall_side = select(vec2<f32>(facing, 0.0), vec2<f32>(0.0, facing), wall_faces_z);
    }
}
    // the wall's own coordinates: along it the top's, and down it world up, as `material_uv` has them
    let wall_uv = vec2<f32>(select(in.uv.y, in.uv.x, wall_faces_z), -uncurved(world_position).y);
    let wall_footprint = material_footprint(wall_uv);
    let flat_position = uncurved(world_position);
    // how the texture covers the screen, from the position relative to the camera, as world positions are too coarse
    let sculpted_footprint = triplanar_footprint(direction_view_to_world(view_at));
    // blocks per pixel up the face, worked out here as derivatives need uniform control flow
    let block_rate = length(vec2<f32>(dpdx(in.uv.y), dpdy(in.uv.y)));
    // the painted material layer this face draws, if any: coarse cells and tree boxes draw their mean
    var layer = material_layer(in.color.a);
@if(VERTEX_UVS_B) {
    if in.uv_b.y > 0.5 {
        layer = -1;
    }
}
    let painted = layer >= 0;
    // a tile under a pixel across filters to its mean, so skips the lookup; leaves and plants keep
    // theirs for their holes
    var color: vec4<f32>;
    if sculpted && (painted || coarse) {
        // projected along the face's own direction where the smooth normal disagrees, or a wall between tops smears
        let smooth_facing = vec3<f32>(normal.x, normal.y, -normal.z);
        let face = flat_side * sign(dot(flat_side, smooth_facing) + 1e-6);
        let facing = normalize(mix(face, smooth_facing, smoothstep(0.6, 0.9, dot(face, smooth_facing))));
        let tinted_side = sculpt_side;
        let tint = in.color.rgb;
        if coarse {
            color = block_mean(in.color.a);
        } else {
            color = material_triplanar(flat_position, facing, layer, sculpted_footprint);
        }
        color = vec4<f32>(color.rgb * select(tint, vec3<f32>(1.0), tinted_side == 1.0), color.a);
        // toward the vertices of the triangle's second material, along a ragged edge
        let other_cell = floor(-1024.0 - in.uv.x + 0.5);
        let other = material_layer(other_cell);
        if toward > 0.001 && (other >= 0 || coarse) {
            var second: vec4<f32>;
            if coarse {
                second = block_mean(other_cell);
            } else {
                second = material_triplanar(flat_position, facing, other, sculpted_footprint);
            }
            color = mix(color, vec4<f32>(second.rgb * select(tint, vec3<f32>(1.0), tinted_side == 2.0), second.a), toward);
        }
        // steep ground bares to dirt, but only grass: rock, scree and sand keep their own look
        let vegetated = mix(select(0.0, 1.0, is_grass(layer)), select(0.0, 1.0, is_grass(other)), toward);
        let steepness = 1.0 - smoothstep(STEEP_TO, STEEP_FROM, normal.y);
        let bare = smoothstep(0.3, 0.7, steepness + (fray / 1.5 - 0.5) * 0.3) * vegetated;
        if bare > 0.001 {
            var dirt: vec4<f32>;
            if coarse {
                dirt = block_mean(f32(constants::DIRT_CELL));
            } else {
                dirt = material_triplanar(flat_position, facing, 0, sculpted_footprint);
            }
            color = vec4<f32>(mix(color.rgb, dirt.rgb, bare), color.a);
        }
        // occlusion rides in the cell's fraction, see `nets::AO_MID`
        let occlusion = (in.color.a - floor(in.color.a + 0.5)) * SCULPT_AO_SPREAD + SCULPT_AO_MID;
        color = vec4<f32>(color.rgb * occlusion, color.a);
    } else if painted {
        if block_rate < MEAN_BLOCK_RATE {
            color = material_sample(in.uv, layer, painted_footprint);
            // a ramp (stored flat) keeps its own projection: it rises only 0.3, so the wall's `-y` barely moves across it and streaks
            if wall_blend > 0.001 && normal.y <= 0.99 {
                color = mix(color, material_sample(wall_uv, layer, wall_footprint), wall_blend);
            }
        } else {
            color = block_mean(in.color.a);
        }
    } else if block_rate < MEAN_BLOCK_RATE || is_leaf(in.color.a) || is_flora(in.color.a) {
        color = block_sample(in.uv, in.color.a, footprint);
    } else {
        color = block_mean(in.color.a);
    }
    // fades and far leaves dither with the depth prepass, see `world_prepass.wesl`
    let threshold = dither_threshold(in.position.xy);
    // coarse LOD cells, where a stretched texture would read as one giant block, and wide
    // tree boxes, which keep it: leaves come out solid, and their shadows match, see
    // `world_prepass.wesl`. How a face draws is `generator::TEXTURED` and so on.
@if(VERTEX_UVS_B) {
    if sculpted {
        if coarse {
            color = vec4<f32>(color.rgb * patch_tint(world_position), color.a);
        }
    } else if in.uv_b.y > 0.875 {
        let mean = block_mean(in.color.a);
        color = vec4<f32>(mean.rgb * patch_tint(world_position), mean.a);
    } else if in.uv_b.y > 0.5 {
        // the holes show deeper leaves, in their shade
        let deeper = block_mean(in.color.a).rgb * HOLE_SHADE;
        color = vec4<f32>(mix(deeper, color.rgb, smoothstep(0.3, 0.7, color.a)), 1.0);
    } else if is_leaf(in.color.a) && threshold < leaf_holes_filled(world_position, eye()) {
        // the leaves behind aren't drawn this far out, so the holes show them in their shade,
        // cut where they'd be cut out so the rest is as bright as nearer leaves
        if color.a < 0.5 {
            color = vec4<f32>(block_mean(in.color.a).rgb * FAR_HOLE_SHADE, 1.0);
        }
        color.a = 1.0;
    } else if is_leaf(in.color.a) || is_flora(in.color.a) {
        // cut where the depth prepass does, at the same blur, so the two agree and the holes don't flicker
        color.a = block_alpha_at(in.uv, in.color.a, foliage_level(distance(world_position, eye()), view_bindings::view.viewport.w, view_bindings::view.clip_from_view[1][1]));
    }
}
    // `DebugSettings::solid_leaves`: leaves without their holes, as coarse LODs draw them
@if(SOLID_LEAVES) {
    if is_leaf(in.color.a) {
        color = block_mean(in.color.a);
    }
}
    // ambient occlusion and grass/leaf tint, which sculpted ground applied itself above
    var tinted = select(color.rgb * in.color.rgb, color.rgb, sculpted);
    if painted {
        tinted = tinted * drift(world_position.xz);
    }
    // coarse LOD cells too, which keep their block's cell in vertex alpha
    if is_neutral_rock(material_layer(in.color.a)) {
        tinted = tinted * ROCK_COOL;
    }
    color = vec4<f32>(tinted, color.a);
    // the painting's brightness and the surface's step per pixel, for ice's bump: derivatives need uniform control flow
    let across = dpdx(world_position);
    let down = dpdy(world_position);
    let rise = vec2<f32>(dpdx(dot(tinted, vec3<f32>(0.2126, 0.7152, 0.0722))), dpdy(dot(tinted, vec3<f32>(0.2126, 0.7152, 0.0722))));

    // block meshes carry smoothed sky light, all but those with no faces at all
    var sky = 1.0;
@if(VERTEX_UVS_B) {
    sky = in.uv_b.x;
}

    // a chamfer grades from the top's light to the wall's across its strip, and a ramp, stored flat, part of the way;
    // the shadow lookup keeps the faceted normal, so its bias is as it was
    var smooth_normal = normal;
    if has_wall {
        let turn = wall_blend * select(1.0, RAMP_LIT_TURN, normal.y > 0.99);
        smooth_normal = normalize(mix(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(wall_side.x, 0.0, wall_side.y), turn));
    }
    let icy = is_ice(in.color.a);
    if icy {
        color = vec4<f32>(ice_albedo(color.rgb, sky, smooth_normal.y), color.a);
    }
    // cold ground that isn't snow yet is frosted, and plants rimed, see `frost.wesl`
@if(!NO_FROST) {
    if frost_receives(in.color.a, is_leaf(in.color.a)) {
        var frost_sun = vec3<f32>(0.0, 1.0, 0.0);
        if view_bindings::lights.n_directional_lights > 0u {
            frost_sun = view_bindings::lights.directional_lights[SUN_LIGHT].direction_to_light;
        }
        let tip = select(-1.0, 1.0 - clamp(in.uv.y, 0.0, 1.0), is_flora(in.color.a));
        color = vec4<f32>(frosted(color.rgb, frost_at(world_position, smooth_normal.y, sky, block_rate, tip, frost_sun, view_bindings::globals.time)), color.a);
    }
}
    let shaded = lit_normal(smooth_normal, world_position);
    let glow = point_light_glow(world_position, normal, in.position.xy);
    let lighting = painterly_world_light(in.world_position, normal, shaded, sky, in.position.xy, is_leaf(in.color.a), is_flora(in.color.a));
    color = vec4<f32>(painterly_saturation(color.rgb * (lighting.light + glow), lighting.lit), color.a);
    if icy {
        var to_sun = vec3<f32>(0.0, 1.0, 0.0);
        if view_bindings::lights.n_directional_lights > 0u {
            to_sun = view_bindings::lights.directional_lights[SUN_LIGHT].direction_to_light;
        }
        color = vec4<f32>(color.rgb + ice_shine(world_position, smooth_normal, sky, to_sun, lighting.lit, block_rate, across, down, rise), color.a);
    }

    // so distant chunks fade into the sky instead of popping in and out
    color = fogged(color, world_position, in.position.xy);

    // after the texture samples above, which need uniform control flow
    if threshold < material_fade.x || threshold >= material_fade.y {
        discard;
    }
    // the holes in leaves: this shader replaces Bevy's, so must cut out alpha-masked texels itself
@if(!SOLID_LEAVES) {
    if color.a < 0.5 {
        discard;
    }
}
    // drawn solid, leaves still keep their shape, but a plant would be a square
@if(SOLID_LEAVES) {
    if is_flora(in.color.a) && color.a < 0.5 {
        discard;
    }
}

    var output: FragmentOutput;
    output.color = color;
    return output;
}
