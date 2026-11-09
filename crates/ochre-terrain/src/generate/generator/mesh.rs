use super::buffers::{
    tiled_uvs, ChunkMeshes, GrassTint, MeshBuffers, MeshStyle, BOX_TEXTURE_MAX_CELL, CUBE,
    MAUVE_TINT, TEXTURED, TEXTURED_SOLID,
};
use super::greedy::{convex_sides, mesh_opaque_greedy, Shapes};
use super::shading::{
    corner_signs, vertex_shore_distance, vertex_shore_factor, vertex_water_depth, water_corner,
    WaterCorner, FACE_NORMAL_OFFSET, WATER_BANK_MARGIN, WATER_BED_MARGIN, WATER_DROP,
};
use super::source::{grid_index, BlockSource, BoxFaces, GridView};
use super::terrain::is_solid;
use crate::chunk::CHUNK_SIZE;
use crate::generate::light::SkyLight;
use crate::generate::light::MAX_LIGHT;
use crate::generate::region_store::water_block_top;
use bevy::math::IVec3;
use bevy::math::Vec3;
use bevy::math::{Vec2, Vec3Swizzles};
use ochre_base::block::BlockType;
use ochre_base::block::Face;
use ochre_base::block::Tile;
use ochre_base::noise::unit;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;

// `cell_size` is how many blocks wide each cell of `source` is.
pub(super) fn mesh_blocks(
    source: &impl BlockSource,
    cell_size: i64,
    style: MeshStyle,
    grass_tint: &GrassTint,
    light: &impl Deref<Target = SkyLight>,
) -> ChunkMeshes {
    let MeshStyle { textured, .. } = style;
    let mut opaque = MeshBuffers {
        untextured: !textured,
        ..Default::default()
    };
    let mut water = MeshBuffers::default();
    let mut spray = MeshBuffers::default();
    let mut inner_leaves = MeshBuffers::default();

    // water's surface sits below a full block's height, see `WATER_TOP`
    // how far the sides of a sloped top block reach down into the block under, to meet a lower surface
    const WATER_SKIRT: f32 = 0.25;
    let lowering = WATER_DROP / cell_size as f32;
    // `top_at` is a sloped surface's height at a corner and its slope there, which the top's normals follow
    let water_face =
        |i: usize,
         top: f32,
         skirt: bool,
         top_at: &mut dyn FnMut([f32; 3]) -> Option<(f32, [f32; 2])>| {
            CUBE[i].map(|mut v| {
                if v.position[1] == 0.5 {
                    let at = top_at(v.position);
                    v.position[1] = at.map_or(top, |(height, _)| height);
                    if let Some((_, [x, z])) = at {
                        v.normal = Vec3::new(-x, 1.0, -z).normalize().to_array();
                    }
                } else if skirt && i != 4 && i != 5 {
                    v.position[1] = (v.position[1] - WATER_SKIRT)
                        .min(top_at(v.position).map_or(top, |(height, _)| height) - WATER_SKIRT);
                }
                v
            })
        };

    // columns' water depths, shared between the corners that sample them
    let mut depths = HashMap::new();
    let mut depth_of = |coord: IVec3| {
        // a sloped column is measured from its own top block, so blocks either side of a step agree on it,
        // and to at most a chunk down, all any chunk the top could be drawn by sees (the swell's full by then)
        let level = source.water_block_level(coord.x, coord.z);
        let key = if level.is_some() {
            coord.with_y(i32::MIN)
        } else {
            coord
        };
        *depths.entry(key).or_insert_with(|| match level {
            Some(level) => {
                let top = coord.with_y(water_block_top(level) as i32 - 1);
                (0..CHUNK_SIZE as i32)
                    .take_while(|&d| source.terrain(top - IVec3::Y * d) == BlockType::Water)
                    .count() as f32
            }
            None => source.water_depth_blocks(coord),
        })
    };

    let size = CHUNK_SIZE as i32;
    let mut blocks = vec![BlockType::Air; (size * size * size) as usize];
    let non_air = source.non_air();
    for &(coord, block) in &non_air {
        blocks[grid_index(size, coord)] = block;
    }

    // each column's surface at each of its corners and the columns joining it there, shared by its blocks
    let corners = RefCell::new(HashMap::new());
    let corner_of = |block_coord: IVec3, level: f32, position: [f32; 3]| {
        let (dx, dz) = corner_signs(position);
        let key = (block_coord.x, block_coord.z, dx, dz);
        if let Some(&found) = corners.borrow().get(&key) {
            return found;
        }
        let mut corner = water_corner(
            &mut |x, z| source.water_level(x, z),
            (block_coord.x, block_coord.z),
            level,
            (dx, dz),
        );
        // held over the bed of every column joining it, or a ramp down a steep run dips under the beds and they show through
        let bed = corner
            .columns()
            .iter()
            .filter_map(|&(x, z)| {
                let top = water_block_top(source.water_block_level(x, z)?) as i32 - 1;
                let water = (0..CHUNK_SIZE as i32)
                    .take_while(|&d| source.terrain(IVec3::new(x, top - d, z)) == BlockType::Water)
                    .count() as i32;
                (water > 0).then_some((top - water + 1) as f32)
            })
            .fold(f32::NEG_INFINITY, f32::max);
        corner.height = corner.height.max(bed + WATER_DROP + WATER_BED_MARGIN);
        // held under the ground of any dry column solid just under the corner, which its blocks all see alike
        let under = (corner.height - WATER_DROP).floor() as i32 - 1;
        let bank = [(dx, 0), (0, dz), (dx, dz)]
            .into_iter()
            .map(|(ox, oz)| IVec3::new(block_coord.x + ox, under, block_coord.z + oz))
            .filter(|c| source.water_level(c.x, c.z).is_none() && is_solid(source.block(*c)))
            .map(|c| {
                (1..)
                    .find(|&up| !is_solid(source.block(c + IVec3::Y * up)))
                    .unwrap() as f32
                    + c.y as f32
            })
            .fold(f32::INFINITY, f32::min);
        if !source.flowing(block_coord.x, block_coord.z) {
            corner.height = corner.height.min(bank + WATER_DROP - WATER_BANK_MARGIN);
        }
        corners.borrow_mut().insert(key, corner);
        corner
    };

    for &(block_coord, block) in &non_air {
        if block != BlockType::Water {
            continue;
        }

        // shallow water in a coarse cell is drawn to its real surface, not the cell's top
        let surface_top = source.water_surface_top(block_coord);
        let top = surface_top.map_or(0.5 - lowering, |top| (top - lowering).max(-0.45));
        let sheet = cell_size == 1 && source.water_sheet(block_coord.x, block_coord.z);
        let hush = cell_size == 1 && source.water_hush(block_coord.x, block_coord.z);
        // a sheet's sides are whole blocks, but its top follows the corners it shares with the surface beside it
        let top_level = if sheet {
            source.water_level(block_coord.x, block_coord.z)
        } else {
            None
        };
        // the sides of a fall's sheet reach the block above, or stacked blocks show a seam between them
        let top = if sheet && source.block(block_coord + IVec3::Y) == BlockType::Water {
            0.5
        } else {
            top
        };
        // fresh water at full detail is drawn to its spread surface, a slope down the river, not to whole blocks
        let level = if cell_size == 1
            && surface_top.is_none()
            && !source.water_sheet(block_coord.x, block_coord.z)
        {
            source.water_level(block_coord.x, block_coord.z)
        } else {
            None
        };
        // the block the real surface lies in, as `water_block_top` fills to: the surface may leave it, the blocks under it are cut to it
        let top_block = source
            .water_block_level(block_coord.x, block_coord.z)
            .is_some_and(|level| water_block_top(level) as i32 - 1 == block_coord.y);
        let open = |block: BlockType| {
            matches!(block, BlockType::Air | BlockType::Leaves) || block.is_flora()
        };
        let under = block_coord - IVec3::Y;
        let water_face_at = |i: usize| {
            // no skirt where flowing water under has a side of its own there, as down a fall, or the two overlap
            let skirt = level.is_some()
                && top_block
                && !(source.flowing(block_coord.x, block_coord.z)
                    && source.block(under) == BlockType::Water
                    && open(source.block(under + FACE_NORMAL_OFFSET[i])));
            // the top block's sides follow the corners too, so the sheet's edge is at the surface beside it
            let at_level = if i == 4 || top_block {
                level.or(top_level)
            } else {
                level
            };
            water_face(i, top, skirt, &mut |position| {
                let corner = corner_of(block_coord, at_level?, position);
                // unclamped to the block, so blocks either side of a step meet at their shared corner, even outside it
                let height = corner.height - WATER_DROP - block_coord.y as f32 - 0.5;
                Some((
                    if top_block { height } else { height.min(0.5) },
                    corner.slope,
                ))
            })
        };
        for (i, offset) in FACE_NORMAL_OFFSET.into_iter().enumerate() {
            if !open(source.block(block_coord + offset)) {
                continue;
            }
            let mut face = water_face_at(i);
            // a sheet's sides face straight out, whatever the slope of the corner their top edge follows
            if i < 4 && sheet {
                for (v, flat) in face.iter_mut().zip(CUBE[i]) {
                    v.normal = flat.normal;
                }
            }
            // the foot of a fall's sheet reaches down to the pool's surface, which lies a little under the top of its block
            if i < 4 && sheet && source.block(block_coord + offset - IVec3::Y) == BlockType::Water {
                for v in face.iter_mut().filter(|v| v.position[1] <= -0.5) {
                    v.position[1] = -0.5 - lowering - 1.0;
                }
            }
            // a step down to a column whose surface meets this one's is closed by the sloped tops
            let beside = (block_coord.x + offset.x, block_coord.z + offset.z);
            if i < 4
                && level.is_some_and(|own| {
                    CUBE[i]
                        .iter()
                        .filter(|v| v.position[1] == 0.5)
                        .all(|v| corner_of(block_coord, own, v.position).joins(beside))
                })
            {
                continue;
            }
            // a side of a block the spread surface is lowered under
            if i < 4 && !top_block && face.iter().all(|v| v.position[1] <= -0.5) {
                continue;
            }
            // Only the top face carries the shore, depth, top flag and shore distance (R, G, B,
            // A), as foam and waves are surface-only. Waves are gated by the flag alone, so
            // neighbouring columns of different depths move their shared edge alike.
            let colors = face.map(|v| {
                if i != 4 {
                    return [0.0, 0.0, 0.0, 1.0];
                }
                // the swell the surface keeps at this corner, from the water in the columns round it, so the
                // surface stands still where it meets a fall's sheet and shared corners move alike
                let calm = if cell_size == 1 {
                    let (sx, sz) = corner_signs(v.position);
                    let (mut total, mut count) = (0.0, 0.0);
                    for (dx, dz) in [(0, 0), (sx, 0), (0, sz), (sx, sz)] {
                        let (cx, cz) = (block_coord.x + dx, block_coord.z + dz);
                        if source.water_level(cx, cz).is_some() {
                            total += source.water_calm(cx, cz);
                            count += 1.0;
                        }
                    }
                    if count > 0.0 {
                        total / count
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };
                // a negative distance marks the top of a fall's water, whose back is not drawn
                [
                    if sheet {
                        0.0
                    } else {
                        vertex_shore_factor(source, block_coord, i, v.position)
                    },
                    vertex_water_depth(&mut depth_of, block_coord, i, v.position)
                        / CHUNK_SIZE as f32,
                    calm,
                    if hush {
                        -1.0
                    } else {
                        vertex_shore_distance(source, block_coord, i, v.position)
                    },
                ]
            });
            water.push_face(
                &face,
                block_coord.as_vec3(),
                Tile::Water.atlas_cell(),
                &colors,
            );
            if let Some(own) = level.filter(|_| top_block) {
                let height = face.iter().map(|v| v.position[1]).sum::<f32>() / 4.0;
                if i == 4 && source.spring_pool(block_coord.x, block_coord.z) {
                    push_bubbles(&mut spray, block_coord, height);
                }
                push_spray(&mut spray, block_coord, i, own, height, source, &corner_of);
            }
        }
    }

    let shapes = Shapes::new(style.bevel);
    mesh_opaque_greedy(
        &GridView {
            size,
            blocks: &blocks,
            source,
        },
        &CUBE,
        &mut opaque,
        &mut inner_leaves,
        style,
        if textured { cell_size as f32 } else { 1.0 },
        grass_tint,
        light,
        &shapes,
    );
    if style.bevel {
        super::seam::mesh_seams(&non_air, &mut opaque, grass_tint, source, light, &shapes);
    }
    // flora only grows at full detail, so coarser cells have none to look for
    if cell_size == 1 {
        mesh_flora(&non_air, &mut opaque, grass_tint, source, light);
        // with the bevels, which they cover the edges of
        if style.bevel {
            mesh_grass_fringe(&non_air, &mut opaque, grass_tint, source, light);
        }
    }
    // sky light there is rarely bright under a canopy, and these boxes are too small to shade smoothly
    const BOX_LIGHT: f32 = 0.8;
    // past them, so wide a box would show through to its back where leaves' holes line up, so
    // they're filled
    let box_holes = cell_size <= BOX_TEXTURE_MAX_CELL;
    opaque.boxes = Some((
        opaque.vertices.len(),
        if box_holes { TEXTURED } else { TEXTURED_SOLID },
    ));
    for tree_box in source.tree_boxes() {
        let (centre, extent) = (
            (tree_box.min + tree_box.max) / 2.0,
            tree_box.max - tree_box.min,
        );
        let tint = if tree_box.block.lod_tile().tinted() {
            grass_tint.at(centre) * tree_box.tint
        } else {
            Vec3::ONE
        };
        let faces = match tree_box.faces {
            BoxFaces::All => &CUBE[..],
            BoxFaces::Sides => &CUBE[..4],
        };
        for face in faces {
            let tile = tree_box.block.tile(Face::of_normal_y(face[0].normal[1]));
            let positions = std::array::from_fn(|j| {
                let mut corner = Vec3::from(face[j].position);
                if corner.y > 0.0 {
                    corner *= Vec3::new(1.0 - tree_box.taper, 1.0, 1.0 - tree_box.taper);
                }
                (centre + corner * extent).to_array()
            });
            let uvs = tiled_uvs(
                &positions,
                &std::array::from_fn(|j| face[j].uv),
                cell_size as f32,
            );
            opaque.push_tiled_quad(
                positions,
                face[0].normal,
                uvs,
                [tint.extend(tile.atlas_cell()).to_array(); 4],
                [BOX_LIGHT; 4],
            );
        }
    }

    let water_mesh = if water.vertices.is_empty() {
        None
    } else {
        Some(water.build_mesh())
    };

    ChunkMeshes {
        opaque: opaque.build_mesh(),
        cutout: None,
        water: water_mesh,
        spray: (!spray.vertices.is_empty()).then(|| spray.build_mesh()),
        inner_leaves: (!inner_leaves.vertices.is_empty()).then(|| inner_leaves.build_mesh()),
    }
}

// Fresh water throws up spray where a run steeper than this, as a drop of about 3 blocks spreads to, eases off, and where it falls.
const SPRAY_SLOPE: f32 = 0.3;
// Rising plumes and scattering droplets thrown up at each.
const SPRAY_PLUMES: u32 = 2;
const SPRAY_DROPLETS: u32 = 4;
// Bubbles rising through each block of a spring's pool.
const SPRING_BUBBLES: u32 = 3;
// The share of them left out at random.
const SPRAY_GAPS: f32 = 0.45;

// Three values 0..1 for the `k`th particle off face `i` of the block at `block_coord`.
fn spray_random(block_coord: IVec3, i: usize, k: u32) -> [f32; 3] {
    let seed = (block_coord.y as u32).wrapping_mul(97) ^ (i as u32 * 64 + k * 3);
    [0, 1, 2].map(|j| unit(seed + j, block_coord.x, block_coord.z) as f32)
}

// A spring's pool bubbles up from its bed through the surface `height` up the block at `block_coord`.
fn push_bubbles(spray: &mut MeshBuffers, block_coord: IVec3, height: f32) {
    for k in 0..SPRING_BUBBLES {
        let [u, v, seed] = spray_random(block_coord, 4, k + 16);
        let at = block_coord.as_vec3() + Vec3::new(u - 0.5, height - 0.6, v - 0.5);
        spray.push_particle(
            at,
            Vec3::Y * 0.9,
            seed,
            0.06,
            false,
            spray_random(block_coord, 4, k + 20)[0],
        );
    }
}

// Spray off a sloped top block's face, its surface `height` up it: at the foot of a steep run, and of a fall down a side.
fn push_spray(
    spray: &mut MeshBuffers,
    block_coord: IVec3,
    i: usize,
    own: f32,
    height: f32,
    source: &impl BlockSource,
    corner_of: &impl Fn(IVec3, f32, [f32; 3]) -> WaterCorner,
) {
    let random = |k: u32| spray_random(block_coord, i, k);
    let slope_of = |column: IVec3, level: f32| {
        CUBE[4].iter().fold(Vec2::ZERO, |sum, v| {
            sum + Vec2::from(corner_of(column, level, v.position).slope) / 4.0
        })
    };
    // where on the face, which way it's thrown and how hard
    let (base, down, strength) = if i == 4 {
        let slope = slope_of(block_coord, own);
        let steep = slope.length();
        if steep <= SPRAY_SLOPE {
            return;
        }
        let down = -slope / steep;
        // only at the foot, a band where one of the next two columns downhill runs gentler, or there are none
        let step = if down.x.abs() > down.y.abs() {
            IVec3::new(down.x.signum() as i32, 0, 0)
        } else {
            IVec3::new(0, 0, down.y.signum() as i32)
        };
        let easing = [1, 2].into_iter().any(|n| {
            let next = block_coord + step * n;
            source
                .water_level(next.x, next.z)
                .is_none_or(|level| slope_of(next, level).length() < 0.75 * steep)
        });
        if !easing {
            return;
        }
        (
            block_coord.as_vec3() + Vec3::Y * height + down.extend(0.0).xzy() * 0.3,
            down,
            steep,
        )
    } else if i < 4 {
        // a fall, onto the lower water beside
        let offset = FACE_NORMAL_OFFSET[i];
        let Some(below) = source.water_level(block_coord.x + offset.x, block_coord.z + offset.z)
        else {
            return;
        };
        let foot = below - WATER_DROP - 0.5;
        let base = block_coord.with_y(0).as_vec3() + offset.as_vec3() * 0.6 + Vec3::Y * foot;
        (base, offset.as_vec3().xz(), (own - below).max(1.0))
    } else {
        return;
    };
    let along = Vec3::new(-down.y, 0.0, down.x);
    let burst = random(15)[0];
    for k in 0..SPRAY_PLUMES + SPRAY_DROPLETS {
        let [u, v, seed] = random(k);
        // some left out at random, so a foot's spray is scattered, not a row along it
        if random(k + 8)[0] < SPRAY_GAPS {
            continue;
        }
        let at = base + along * (u - 0.5) + down.extend(0.0).xzy() * (v - 0.5);
        if k < SPRAY_PLUMES {
            // a plume rises off the foot, leaning a little downstream
            let flow = down.extend(0.0).xzy() * 0.4 + Vec3::Y * (0.8 + 0.6 * strength.min(2.0));
            spray.push_particle(at, flow, seed, 0.16, false, burst);
        } else {
            // droplets scatter, mostly downstream, the shader spreading each life another way
            let flow = down.extend(0.0).xzy() * (1.2 + strength.min(2.0))
                + Vec3::Y * (1.5 + strength.min(2.0));
            spray.push_particle(at, flow, seed, 0.06, true, burst);
        }
    }
}

// How much of the way to the mauve tint the long grasses go, where it is coldest.
const MAUVE_STRENGTH: f32 = 0.7;

// How far in from the block's corners flora's quads reach, so neighbouring plants don't touch.
const FLORA_INSET: f32 = 0.05;
// How far a plant may stand off its block's centre, and how much shorter than a block grass
// and other plants may be: grass varies far more, so a meadow isn't cut level.
const FLORA_JITTER: f32 = 0.18;
const GRASS_SHORTEN: f32 = 0.45;
const FLORA_SHORTEN: f32 = 0.2;
// How dark a plant is at its roots, to its tips, as the grass round it shades them.
const FLORA_ROOT_SHADE: f32 = 0.65;
// How much of its block's height and width a plant is, at the least and the most: grasses may
// stand a little over a block, where nothing is above them.
const SIZE_RANGE: (f32, f32) = (0.7, 1.25);
const WIDTH_RANGE: (f32, f32) = (0.6, 1.3);
// How far a block's further plants stand off its centre, at the most.
const FLORA_EXTRA_REACH: f32 = 0.3;
// Shrubs and heath are one plant to a block, drawn as six planes in a star that reach well past it: their quad's height
// and half width in blocks. The sprite fills nearly all of the width and, for a shrub, 70% of the height, and a heath's
// low mound 43%, so a shrub stands up to about three blocks across and one and three quarters high, and heath lies
// low and wide. They grow in clusters, see `flora.rs`, so the planes of neighbours overlap into a carpet.
const SHRUB_HEIGHT: (f32, f32) = (1.8, 2.5);
const SHRUB_HALF_WIDTH: (f32, f32) = (1.3, 1.8);
const HEATH_HEIGHT: (f32, f32) = (1.6, 2.2);
const HEATH_HALF_WIDTH: (f32, f32) = (1.2, 1.6);
// How likely the grass at the edge of a grass top is to have tufts hanging over it, and how likely it has a
// further one.
const FRINGE_CHANCE: f32 = 0.95;
const FRINGE_EXTRA_CHANCE: f32 = 0.5;

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// 0 to 1, smooth across the world in patches a few blocks wide, for where the meadow is thick.
fn patchiness(world: IVec3) -> f32 {
    const PATCH: f32 = 5.0;
    let (x, z) = (world.x as f32 / PATCH, world.z as f32 / PATCH);
    let (cx, cz) = (x.floor(), z.floor());
    let (fx, fz) = (smoothstep(0.0, 1.0, x - cx), smoothstep(0.0, 1.0, z - cz));
    let corner = |dx: f32, dz: f32| hash01(IVec3::new((cx + dx) as i32, 0, (cz + dz) as i32), 900);
    let bottom = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * fx;
    let top = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * fx;
    bottom + (top - bottom) * fz
}

// Between 0 and 1 for the block at `coord`, the same every time it's meshed, so what grows where
// doesn't change when the chunk is meshed again.
fn hash01(coord: IVec3, salt: u32) -> f32 {
    let h = (coord.x as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((coord.z as u32).wrapping_mul(0x85EB_CA77))
        .wrapping_add((coord.y as u32).wrapping_mul(0x27D4_EB2F))
        .wrapping_add(salt.wrapping_mul(0xC2B2_AE3D));
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    (h ^ (h >> 13)) as f32 / u32::MAX as f32
}

fn between(range: (f32, f32), unit: f32) -> f32 {
    range.0 + (range.1 - range.0) * unit
}

// One plant: two quads crossed at right angles, turned by `angle`, rooted at `root`, with its
// tips leaning by `lean`.
struct Tuft {
    root: Vec3,
    height: f32,
    half_width: f32,
    angle: f32,
    lean: Vec3,
}

// Flora as two quads crossed, both sides drawn as block meshes cull one, v = 0 at the top so
// `foliage.wesl` sways the tips, and facing up to light like the grass.
fn push_tuft(buffers: &mut MeshBuffers, tuft: &Tuft, atlas_cell: f32, tint: Vec3, lit: f32) {
    let (root, tip) = (
        (tint * FLORA_ROOT_SHADE).extend(atlas_cell).to_array(),
        tint.extend(atlas_cell).to_array(),
    );
    let (sin, cos) = libm::sincosf(tuft.angle);
    for across in [Vec3::new(cos, 0.0, sin), Vec3::new(-sin, 0.0, cos)] {
        let corner = |side: f32, up: bool| {
            let leaning = if up { tuft.lean } else { Vec3::ZERO };
            let height = if up { tuft.height } else { 0.0 };
            (tuft.root + across * (side * tuft.half_width) + Vec3::Y * height + leaning).to_array()
        };
        let (top_a, bottom_a, top_b, bottom_b) = (
            corner(-1.0, true),
            corner(-1.0, false),
            corner(1.0, true),
            corner(1.0, false),
        );
        buffers.push_double_sided_quad(
            [top_a, bottom_a, top_b, bottom_b],
            [0.0, 1.0, 0.0],
            [[0.0, 0.0], [0.0, 1.0], [1.0, 0.0], [1.0, 1.0]],
            [tip, root, tip, root],
            [lit; 4],
        );
    }
}

// The plants in flora blocks: a first of the block's kind, as tall and wide as it varies, and
// more beside it, so a block is a clump, not one sprite.
fn mesh_flora(
    non_air: &[(IVec3, BlockType)],
    buffers: &mut MeshBuffers,
    grass_tint: &GrassTint,
    source: &impl BlockSource,
    light: &impl Deref<Target = SkyLight>,
) {
    for &(coord, block) in non_air {
        if !block.is_flora() {
            continue;
        }
        // by where it is in the world, so every chunk's flora isn't the same
        let world = coord + grass_tint.origin;
        let jitter = |salt: u32| hash01(world, salt);
        let grass = matches!(
            block,
            BlockType::TallGrass | BlockType::ShortGrass | BlockType::SeedGrass
        );
        let shorten = if grass { GRASS_SHORTEN } else { FLORA_SHORTEN };
        // grasses may stand a little over a block, where the cell above is open
        let room = !source.block(coord + IVec3::Y).is_opaque();
        let tallest = if grass && room { SIZE_RANGE.1 } else { 1.0 };
        let base_height =
            ((1.0 - jitter(3) * shorten) * between(SIZE_RANGE, jitter(4))).min(tallest);
        let reach = (0.5 - FLORA_INSET) * std::f32::consts::SQRT_2;
        let tile = block.tile(Face::Side);
        // tinted just as the grass it grows from, see `mesh_opaque_greedy`
        let ground = coord.as_vec3() - Vec3::Y * 0.5;
        let tint = if tile.tinted() {
            let tint = grass_tint.at(ground);
            // the long grasses in the cold go mauve, to bridge between the heath, the short ones less so
            let mauve = match block {
                BlockType::TallGrass | BlockType::SeedGrass => MAUVE_STRENGTH,
                BlockType::ShortGrass => MAUVE_STRENGTH * 0.4,
                _ => 0.0,
            } * grass_tint.mauve_at(ground);
            tint.lerp(MAUVE_TINT, mauve) * source.forest_shade(ground)
        } else {
            Vec3::ONE
        };
        let lit = light.level(coord) as f32 / MAX_LIGHT as f32;
        let big = match block {
            BlockType::Shrub => Some((SHRUB_HEIGHT, SHRUB_HALF_WIDTH)),
            BlockType::Heath => Some((HEATH_HEIGHT, HEATH_HALF_WIDTH)),
            _ => None,
        };
        if let Some((heights, widths)) = big {
            // under a roof or a branch it stays within its block
            let height = between(heights, jitter(3)).min(if room { f32::MAX } else { 1.0 });
            let angle = jitter(5) * std::f32::consts::FRAC_PI_2;
            // three crossed pairs, 30 degrees apart, so it fills in seen from above and not a star of arms
            for turn in [0.0, std::f32::consts::FRAC_PI_6, std::f32::consts::FRAC_PI_3] {
                push_tuft(
                    buffers,
                    &Tuft {
                        root: coord.as_vec3()
                            - Vec3::Y * 0.5
                            + Vec3::new((jitter(1) - 0.5) * FLORA_JITTER, 0.0, (jitter(2) - 0.5) * FLORA_JITTER),
                        height,
                        half_width: between(widths, jitter(4)),
                        angle: angle + turn,
                        lean: Vec3::ZERO,
                    },
                    tile.atlas_cell(),
                    tint,
                    lit,
                );
            }
            continue;
        }
        // the first plant is where it always was, the rest stand round it
        let extras = match block {
            _ if grass => 1 + (jitter(10) < 0.55) as u32,
            BlockType::Fern => (jitter(10) < 0.5) as u32,
            _ => (jitter(10) < 0.4) as u32,
        };
        for k in 0..=extras {
            let salt = 20 * k;
            let (x, z) = if k == 0 {
                (
                    (jitter(1) - 0.5) * 2.0 * FLORA_JITTER,
                    (jitter(2) - 0.5) * 2.0 * FLORA_JITTER,
                )
            } else {
                (
                    (jitter(salt + 1) - 0.5) * 2.0 * FLORA_EXTRA_REACH,
                    (jitter(salt + 2) - 0.5) * 2.0 * FLORA_EXTRA_REACH,
                )
            };
            let height = if k == 0 {
                base_height
            } else {
                base_height * between((0.5, 0.95), jitter(salt + 3))
            };
            push_tuft(
                buffers,
                &Tuft {
                    root: coord.as_vec3() - Vec3::Y * 0.5 + Vec3::new(x, 0.0, z),
                    height,
                    half_width: reach * between(WIDTH_RANGE, jitter(salt + 4)),
                    angle: if k == 0 {
                        std::f32::consts::FRAC_PI_4
                    } else {
                        jitter(salt + 5) * std::f32::consts::PI
                    },
                    lean: Vec3::ZERO,
                },
                tile.atlas_cell(),
                tint,
                lit,
            );
        }
    }
}

// Tufts of grass on the tops of grass blocks, with no block of flora there: along the edges they
// hang over, and down the side, and a few in the middle. Meshing only, so the world has none.
fn mesh_grass_fringe(
    non_air: &[(IVec3, BlockType)],
    buffers: &mut MeshBuffers,
    grass_tint: &GrassTint,
    source: &impl BlockSource,
    light: &impl Deref<Target = SkyLight>,
) {
    let opaque = |c: IVec3| source.block(c).is_opaque();
    for &(coord, block) in non_air {
        // under flora the edges are as bare, so they grow tufts too, but the middle has its plant
        let above = source.block(coord + IVec3::Y);
        let open_top = above == BlockType::Air;
        if block != BlockType::Grass || !(open_top || above.is_flora()) {
            continue;
        }
        // by where it is in the world, so every chunk's tufts aren't the same
        let world = coord + grass_tint.origin;
        let hash = |salt: u32| hash01(world, salt);
        let sides = convex_sides(opaque, coord, IVec3::Y, IVec3::X, IVec3::Z);
        let top = coord.as_vec3() + Vec3::Y * 0.5;
        let lit = light.level(coord + IVec3::Y) as f32 / MAX_LIGHT as f32;
        let mut plant = |salt: u32,
                         at: Vec3,
                         lean: Vec3,
                         angle: Option<f32>,
                         height: (f32, f32),
                         width: (f32, f32)| {
            let jitter = |s: u32| hash(salt + s);
            let tile = match jitter(1) {
                u if u < 0.55 => Tile::ShortGrass,
                u if u < 0.85 => Tile::TallGrass,
                _ => Tile::SeedGrass,
            };
            let tint = grass_tint.at(top) * source.forest_shade(top);
            push_tuft(
                buffers,
                &Tuft {
                    root: top + at,
                    height: between(height, jitter(2)),
                    half_width: between(width, jitter(3)),
                    angle: angle.unwrap_or(jitter(4) * std::f32::consts::PI),
                    lean,
                },
                tile.atlas_cell(),
                tint,
                lit,
            );
        };
        for (k, side) in [-IVec3::X, IVec3::X, -IVec3::Z, IVec3::Z]
            .into_iter()
            .enumerate()
        {
            if !sides[k] || hash(100 + k as u32) >= FRINGE_CHANCE {
                continue;
            }
            let out = side.as_vec3();
            let along = Vec3::new(out.z, 0.0, out.x);
            let count = 3 + (hash(110 + k as u32) < FRINGE_EXTRA_CHANCE) as u32;
            for t in 0..count {
                let salt = 200 + 40 * k as u32 + 10 * t;
                let slide = (hash(salt + 5) - 0.5) * 0.9;
                // Rooted at the edge itself, so all of it is outside the block and none behind the
                // side, hanging out and down the side as a curtain facing out. The tip is the far
                // end, so it sways there.
                let reach = between((0.1, 0.32), hash(salt + 7));
                let drop = between((0.33, 0.73), hash(salt + 8));
                let facing = libm::atan2f(along.z, along.x) + (hash(salt + 9) - 0.5) * 0.7;
                plant(
                    salt,
                    out * 0.5 + along * slide,
                    out * reach - Vec3::Y * drop,
                    Some(facing),
                    (0.0, 0.0),
                    (0.28, 0.5),
                );
            }
            // short and upright just inside the edge, so the curtain grows out of grass
            let band = 2 + (hash(120 + k as u32) < FRINGE_EXTRA_CHANCE) as u32;
            for t in 0..band {
                let salt = 600 + 40 * k as u32 + 10 * t;
                let slide = (hash(salt + 5) - 0.5) * 0.95;
                let inside = between((0.1, 0.35), hash(salt + 6));
                let lean = out * between((0.03, 0.12), hash(salt + 7));
                plant(
                    salt,
                    out * (0.5 - inside) + along * slide,
                    lean,
                    None,
                    (0.15, 0.4),
                    (0.15, 0.3),
                );
            }
        }
        if !open_top {
            continue;
        }
        // thick in patches and bare between them, like a meadow
        let patch = patchiness(world);
        let chance = 0.04 + 0.6 * smoothstep(0.4, 0.7, patch);
        if hash(400) < chance {
            let count = 1 + (patch > 0.65 && hash(403) < 0.5) as u32;
            for t in 0..count {
                let salt = 410 + 10 * t;
                let at = Vec3::new(hash(salt + 5) - 0.5, 0.0, hash(salt + 6) - 0.5) * 0.8;
                plant(salt, at, Vec3::ZERO, None, (0.15, 0.5), (0.16, 0.32));
            }
        }
    }
}
