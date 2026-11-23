use ochre_base::terrain_source::TerrainSource as _;
use std::sync::Arc;

use bevy::math::Vec3;
use bevy::math::{I64Vec3, IVec3, U16Vec3};
use bevy::platform::collections::HashMap;
use bevy::render::mesh::{Indices, Mesh, VertexAttributeValues};

use ochre_base::block::{BlockType, Tile};

use super::buffers::{ChunkMeshes, GrassTint, MeshStyle};

const RAMP: f32 = 0.3;
use super::chunk::{generate_chunk, generate_chunk_mesh_with};
use super::lod::LodBlocks;
use super::shading::{
    spread_water_levels, vertex_ao, vertex_water_depth, water_corner, AO_STRENGTH,
    WATER_SPREAD_REACH,
};
use super::source::{water_depth, BlockSource, ChunkNeighbourhood};
use super::terrain::{
    freeze_cell, soil_depth, terrain_block, under_grass, FREEZE_TEMPERATURE, SAND_DEPTH, SOIL_DEPTH,
};
use crate::chunk::{neighbor_26_index, ChunkCoordinate, ChunkData, CHUNK_SIZE};
use crate::generate::light::SkyLight;
use crate::generate::region_store::RegionStore;
use crate::lod::LodKey;
use ochre_base::WATER_LEVEL;

// leaves hiding the leaves behind them, as every LOD draws them
fn style(ambient_occlusion: bool, textured: bool) -> MeshStyle {
    MeshStyle {
        ambient_occlusion,
        textured,

        tree_shapes: false,
        far_flora: None,
    }
}

fn no_neighbors() -> [Option<Arc<ChunkData>>; 26] {
    std::array::from_fn(|_| None)
}

#[test]
fn test_neighbourhood_within_chunk() {
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(4, 4, 4), BlockType::Rock);

    let block = ChunkNeighbourhood::new(&chunk, &no_neighbors()).block(IVec3::new(4, 4, 4));
    assert_eq!(BlockType::Rock, block);
}

#[test]
fn test_neighbourhood_crosses_single_axis_into_adjacent_chunk() {
    let chunk = ChunkData::default();
    let mut adjacent = ChunkData::default();
    adjacent.set_block_at(U16Vec3::new(0, 4, 4), BlockType::Rock);

    let mut neighbors = no_neighbors();
    neighbors[neighbor_26_index(1, 0, 0)] = Some(Arc::new(adjacent));
    let block =
        ChunkNeighbourhood::new(&chunk, &neighbors).block(IVec3::new(CHUNK_SIZE as i32, 4, 4));
    assert_eq!(BlockType::Rock, block);
}

#[test]
fn test_neighbourhood_crosses_two_axes_into_diagonal_chunk() {
    let chunk = ChunkData::default();
    let mut diagonal = ChunkData::default();
    diagonal.set_block_at(U16Vec3::new(0, 0, 4), BlockType::Rock);

    let mut neighbors = no_neighbors();
    neighbors[neighbor_26_index(1, 1, 0)] = Some(Arc::new(diagonal));
    let block = ChunkNeighbourhood::new(&chunk, &neighbors).block(IVec3::new(
        CHUNK_SIZE as i32,
        CHUNK_SIZE as i32,
        4,
    ));
    assert_eq!(BlockType::Rock, block);
}

#[test]
fn test_neighbourhood_missing_diagonal_chunk_is_air() {
    let chunk = ChunkData::default();
    let block = ChunkNeighbourhood::new(&chunk, &no_neighbors()).block(IVec3::new(
        CHUNK_SIZE as i32,
        CHUNK_SIZE as i32,
        4,
    ));
    assert_eq!(BlockType::Air, block);
}

#[test]
fn test_vertex_ao_no_occluders_is_unlit() {
    let chunk = ChunkData::default();
    let neighbors = no_neighbors();
    let source = ChunkNeighbourhood::new(&chunk, &neighbors);
    let ao = vertex_ao(&source, IVec3::new(4, 4, 4), 4, [0.5, 0.5, -0.5]);
    assert_eq!(1.0, ao);
}

#[test]
fn test_vertex_ao_fully_occluded_corner() {
    let mut chunk = ChunkData::default();
    // Top face (index 4) at (4,4,4): occluders are the y+1 layer's tangent/diagonal cells.
    chunk.set_block_at(U16Vec3::new(5, 5, 4), BlockType::Rock);
    chunk.set_block_at(U16Vec3::new(4, 5, 5), BlockType::Rock);
    chunk.set_block_at(U16Vec3::new(5, 5, 5), BlockType::Rock);

    let neighbors = no_neighbors();
    let source = ChunkNeighbourhood::new(&chunk, &neighbors);
    let ao = vertex_ao(&source, IVec3::new(4, 4, 4), 4, [0.5, 0.5, 0.5]);
    assert_eq!(1.0 - AO_STRENGTH, ao);
}

#[test]
fn test_neighbouring_water_columns_agree_on_shared_corner_depth() {
    let mut chunk = ChunkData::default();
    // two columns with their surface at y = 5: one 5 deep, its +x neighbour 2 deep
    chunk.set_block_at(U16Vec3::new(4, 0, 4), BlockType::Rock);
    for y in 1..=5 {
        chunk.set_block_at(U16Vec3::new(4, y, 4), BlockType::Water);
    }
    for y in 0..=3 {
        chunk.set_block_at(U16Vec3::new(5, y, 4), BlockType::Rock);
    }
    for y in 4..=5 {
        chunk.set_block_at(U16Vec3::new(5, y, 4), BlockType::Water);
    }
    let neighbors = no_neighbors();
    let source = ChunkNeighbourhood::new(&chunk, &neighbors);
    let mut depth_of = |coord: IVec3| water_depth(&source, coord) as f32;

    // the corner between them, on the +z side: seen from each column's top face
    let deep = vertex_water_depth(&mut depth_of, IVec3::new(4, 5, 4), 4, [0.5, 0.5, 0.5]);
    let shallow = vertex_water_depth(&mut depth_of, IVec3::new(5, 5, 4), 4, [-0.5, 0.5, 0.5]);
    assert_eq!(deep, shallow);
    // averaged with the two dry columns on the +z side
    assert_eq!((5.0 + 2.0) / 4.0, deep);
}

#[test]
fn test_a_rivers_surface_is_continuous_across_the_blocks_it_runs_down() {
    // a river falling 0.3 a column along x, over blocks whose tops step as it passes whole numbers
    let level = |x: i32, _z: i32| Some(10.4 - 0.3 * x as f32);
    let corner = |x: i32, sign: f32| {
        water_corner(
            &mut |x, z| level(x, z),
            (x, 0),
            level(x, 0).unwrap(),
            (if sign > 0.0 { 1 } else { -1 }, 1),
        )
        .height
    };
    // each corner is the same seen from the column either side of it
    for x in 0..8 {
        let (a, b) = (corner(x, 0.5), corner(x + 1, -0.5));
        assert!((a - b).abs() < 1e-5, "{a} {b}");
    }
    // falling all the way down, with no step at the corners where the blocks' tops change
    let heights: Vec<f32> = (0..8).map(|x| corner(x, 0.5)).collect();
    assert!(heights.windows(2).all(|w| w[0] > w[1]), "{heights:?}");
    // nor is there a step down a rapid, more than a block a column but less than `WATER_SLOPE_MAX_STEP`
    let mut rapid = |x: i32, _z: i32| Some(20.0 - 1.25 * x as f32);
    let (a, b) = (
        water_corner(&mut rapid, (0, 0), 20.0, (1, 1)).height,
        water_corner(&mut rapid, (1, 0), 18.75, (-1, 1)).height,
    );
    assert!((a - b).abs() < 1e-5 && a < 20.0 && a > 18.75, "{a} {b}");
    // a curtain, three blocks apart, is not averaged across
    let mut stepped = |x: i32, _z: i32| Some(if x <= 0 { 10.0 } else { 7.0 });
    assert_eq!(
        10.0,
        water_corner(&mut stepped, (0, 0), 10.0, (1, 1)).height
    );
}

#[test]
fn test_a_step_in_the_water_ramps_over_the_columns_round_it() {
    let reach = WATER_SPREAD_REACH;
    let wide = 6 * reach;
    // water 5 high for x < 3 * reach and 3 lower beyond
    let raw: Vec<f32> = (0..wide * wide)
        .map(|i| if i / wide < 3 * reach { 5.0 } else { 2.0 })
        .collect();
    let spread = spread_water_levels(&raw, wide);
    let inner = wide - 2 * reach;
    let at = |x: i32| spread[((x - reach) * inner + inner / 2) as usize];
    let ramp: Vec<f32> = (reach..5 * reach).map(at).collect();
    // falling all the way, by under a block a column, so it's one slope with no curtain
    assert!(
        ramp.windows(2).all(|w| w[0] >= w[1] && w[0] - w[1] < 1.0),
        "{ramp:?}"
    );
    // and only near the step
    assert_eq!((5.0, 2.0), (at(reach), at(5 * reach - 1)));
}

#[test]
fn test_desert_ground_is_sand_and_other_lowland_grass() {
    use ochre_world::worldmap::{climate::BiomeKind, WorldConfig, WorldMap, WorldTerrain};
    // Material rules should not require a particular seed to contain both climates.
    for (kind, expected) in [
        (BiomeKind::Desert, BlockType::Sand),
        (BiomeKind::Grassland, BlockType::Grass),
    ] {
        let mut map = WorldMap::empty(
            3,
            WorldConfig {
                size: 1024,
                ..Default::default()
            },
        );
        map.height.fill(50.0);
        map.continentalness.fill(0.5);
        map.temperature.fill(0.68);
        map.biome.fill(kind as u8);
        let regions = Arc::new(RegionStore::new(Arc::new(WorldTerrain::new(Arc::new(map)))));
        let top = regions.get_height(0, 0).round() as i64 - 1;
        let coord = ChunkCoordinate(I64Vec3::new(0, top.div_euclid(16), 0));
        let local = U16Vec3::new(0, top.rem_euclid(16) as u16, 0);
        assert_eq!(expected, generate_chunk(regions, coord).get_block_at(local));
    }
}

#[test]
fn test_a_chunk_knows_its_neighbours_steppe_columns() {
    // the sculpted mesh's margin draws grass from neighbours' columns: steppe must be known there or a plain strip shows
    let regions = RegionStore::new(ochre_world::worldmap::test_terrain(13));
    let terrain = regions.terrain();
    let size = CHUNK_SIZE as i32;
    let origin = (-200..200)
        .flat_map(|cx| (-200..200).map(move |cz| IVec3::new(cx * size, 0, cz * size)))
        .find(|o| terrain.steppe(o.x as f64 - 0.5, o.z as f64 + 0.5))
        .expect("no steppe on seed 13");
    let chunk = ChunkData::default();
    let source = ChunkNeighbourhood::new(&chunk, &no_neighbors()).with_water(&regions, origin);
    for (x, z) in [(-1, 0), (-1, 5), (size, 3), (4, -2), (7, size + 1)] {
        let want = terrain.steppe((origin.x + x) as f64 + 0.5, (origin.z + z) as f64 + 0.5);
        assert_eq!(want, source.steppe(x, z), "column {x},{z} past the chunk at {origin}");
    }
    assert!(source.steppe(-1, 0));
}

#[test]
fn test_trees_across_chunk_borders_are_whole() {
    use crate::generate::trees::trees_near;
    use bevy::math::I64Vec2;

    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let mut heights = regions.sampler();
    let trees = trees_near(
        regions.terrain(),
        &mut heights,
        forest_at(5) - I64Vec2::splat(800),
        forest_at(5) + I64Vec2::splat(800),
    );
    let chunk_of = |p: I64Vec3| p.div_euclid(I64Vec3::splat(16));
    let tree = trees
        .iter()
        .find(|t| {
            let (low, high) = t.bounds();
            chunk_of(low).x != chunk_of(high).x
                && chunk_of(low).z != chunk_of(high).z
                && [(-12, 0), (12, 0), (0, -12), (0, 12)]
                    .into_iter()
                    .all(|(dx, dz)| {
                        (regions
                            .terrain()
                            .height((t.base.x + dx) as f64, (t.base.z + dz) as f64)
                            - t.base.y as f64)
                            .abs()
                            < 2.0
                    })
        })
        .expect("no tree straddling a chunk corner");

    let mut chunks = HashMap::new();
    let (low, high) = tree.bounds();
    let mut checked = 0;
    tree.plant(
        low,
        1,
        (IVec3::ZERO, (high - low).as_ivec3()),
        |cell, block| {
            let p = low + cell.as_i64vec3();
            let coord = chunk_of(p);
            let chunk = chunks
                .entry(coord)
                .or_insert_with(|| generate_chunk(regions.clone(), ChunkCoordinate(coord)));
            let local = (p - coord * 16).as_u16vec3();
            let found = chunk.get_block_at(local);
            // only another tree's log may stand in for this one's leaves
            assert!(
                found == block || (block == BlockType::Leaves && found == BlockType::Log),
                "{block:?} at {p} is {found:?}"
            );
            checked += 1;
        },
    );
    assert!(
        chunks.len() >= 2 && checked > 20,
        "{} blocks over {} chunks",
        checked,
        chunks.len()
    );
}

#[test]
fn test_cold_sea_freezes_from_the_top_and_thicker_when_colder() {
    let ice_layers = |temperature| {
        (WATER_LEVEL - 10..WATER_LEVEL)
            .filter(|&y| {
                terrain_block(y, WATER_LEVEL - 20, 0.0, temperature, WATER_LEVEL) == BlockType::Ice
            })
            .count()
    };
    assert_eq!(0, ice_layers(0.5));
    assert_eq!(0, ice_layers(FREEZE_TEMPERATURE));
    assert_eq!(1, ice_layers(FREEZE_TEMPERATURE - 0.01));
    assert_eq!(3, ice_layers(0.0));
    assert_eq!(
        BlockType::Ice,
        terrain_block(WATER_LEVEL - 1, WATER_LEVEL - 20, 0.0, 0.0, WATER_LEVEL)
    );
    assert_eq!(
        BlockType::Water,
        terrain_block(WATER_LEVEL - 4, WATER_LEVEL - 20, 0.0, 0.0, WATER_LEVEL)
    );
    assert_eq!(
        BlockType::Air,
        terrain_block(WATER_LEVEL, WATER_LEVEL - 20, 0.0, 0.0, WATER_LEVEL)
    );
}

#[test]
fn test_wide_cells_freeze_when_they_reach_the_ice() {
    let top = WATER_LEVEL;
    assert_eq!(
        BlockType::Ice,
        freeze_cell(BlockType::Water, 0.0, top, (32, WATER_LEVEL))
    );
    assert_eq!(
        BlockType::Water,
        freeze_cell(BlockType::Water, 0.0, top - 32, (32, WATER_LEVEL))
    );
    assert_eq!(
        BlockType::Water,
        freeze_cell(BlockType::Water, 0.5, top, (32, WATER_LEVEL))
    );
    assert_eq!(
        BlockType::Rock,
        freeze_cell(BlockType::Rock, 0.0, top, (32, WATER_LEVEL))
    );
}

#[test]
fn test_polar_lowlands_are_snow_covered_and_glacier_ice_runs_deep_before_stone() {
    let ground = WATER_LEVEL + 20;
    assert_eq!(
        BlockType::Snow,
        terrain_block(ground - 1, ground, 0.0, 0.1, WATER_LEVEL)
    );
    assert_eq!(
        BlockType::Grass,
        terrain_block(ground - 1, ground, 0.0, 0.5, WATER_LEVEL)
    );
    // level glacier ice is a crust over snow
    assert_eq!(
        BlockType::Ice,
        under_grass(BlockType::Ice, ground - 1, ground, 1, false, SOIL_DEPTH, 0)
    );
    assert_eq!(
        BlockType::Snow,
        under_grass(BlockType::Ice, ground - 3, ground, 1, false, SOIL_DEPTH, 0)
    );
    // but a cliff of it shows ice from its top to its foot, and only then snow and stone
    assert_eq!(
        BlockType::Ice,
        under_grass(
            BlockType::Ice,
            ground - 30,
            ground,
            1,
            false,
            SOIL_DEPTH,
            30
        )
    );
    assert_eq!(
        BlockType::CompactedRock,
        under_grass(
            BlockType::Ice,
            ground - 50,
            ground,
            1,
            false,
            SOIL_DEPTH,
            30
        )
    );
}

#[test]
fn test_a_cliff_face_shows_its_surface_material_as_far_down_as_it_clings() {
    use super::terrain::skin_depth;
    let ground = WATER_LEVEL + 40;
    // level ground has none; a face shows as much as it stands over its neighbour, up to what clings
    assert_eq!(0, skin_depth(BlockType::Grass, 0));
    assert_eq!(5, skin_depth(BlockType::Grass, 5));
    assert_eq!(8, skin_depth(BlockType::Grass, 60));
    assert_eq!(40, skin_depth(BlockType::Ice, 60));
    assert_eq!(0, skin_depth(BlockType::Rock, 60));
    // a 12-block grassy face: grass for 8 blocks, then packed stone, not dirt and stone from the first block
    let at = |depth| {
        under_grass(
            BlockType::Grass,
            ground - 1 - depth,
            ground,
            1,
            false,
            2,
            12,
        )
    };
    assert_eq!(BlockType::Grass, at(8));
    assert_eq!(BlockType::CompactedRock, at(9));
    // gentle ground is as before: one block of grass over dirt
    assert_eq!(
        BlockType::Dirt,
        under_grass(BlockType::Grass, ground - 2, ground, 1, false, 2, 0)
    );
}

#[test]
fn test_terrain_block_floor_and_water() {
    // Column material sampling is independent of the world's bottom boundary.
    assert_eq!(
        BlockType::Sand,
        terrain_block(-15, -10, 0.0, 0.5, WATER_LEVEL)
    );
    assert_eq!(
        BlockType::Water,
        terrain_block(-5, -10, 0.0, 0.5, WATER_LEVEL)
    );
    assert_eq!(
        BlockType::Air,
        terrain_block(WATER_LEVEL, -10, 0.0, 0.5, WATER_LEVEL)
    );
    assert_eq!(
        BlockType::Grass,
        terrain_block(WATER_LEVEL + 10, WATER_LEVEL + 20, 0.0, 0.5, WATER_LEVEL)
    );
}

#[test]
fn test_snow_settles_on_steeper_ground_higher_up() {
    let at = |above_sea: i64, slope: f64| {
        let y = WATER_LEVEL + above_sea;
        terrain_block(y, y + 1, slope, 0.5, WATER_LEVEL)
    };
    assert_ne!(BlockType::Snow, at(900, 0.0));
    assert_eq!(BlockType::Snow, at(960, 1.0));
    assert_ne!(BlockType::Snow, at(960, 3.0));
    assert_eq!(BlockType::Snow, at(1100, 3.0));
    // cliffs stay bare however high they are
    assert_ne!(BlockType::Snow, at(1800, 6.0));
}

#[test]
fn test_snow_line_drops_in_the_cold_and_hot_ground_is_bare() {
    let at = |above_sea: i64, temperature: f32| {
        let y = WATER_LEVEL + above_sea;
        terrain_block(y, y + 1, 0.0, temperature, WATER_LEVEL)
    };
    assert_ne!(BlockType::Snow, at(100, 0.5));
    assert_eq!(BlockType::Snow, at(600, 0.35));
    assert_eq!(BlockType::Snow, at(1100, 0.5));
    // too hot for snow however high, so the peaks are rock
    assert_eq!(BlockType::Rock, at(1100, 0.8));
    assert_eq!(BlockType::Grass, at(100, 0.8));
}

#[test]
fn test_only_the_top_of_frozen_grass_is_snow() {
    let at =
        |y: i64, temperature: f32| terrain_block(y, WATER_LEVEL + 7, 0.0, temperature, WATER_LEVEL);
    let top = WATER_LEVEL + 6;
    assert_eq!(BlockType::Snow, at(top, 0.05));
    assert_eq!(BlockType::Grass, at(top, 0.3));
    assert_eq!(BlockType::Grass, at(top - 1, 0.05));
}

#[test]
fn taiga_lowlands_thaw_but_high_taiga_and_glaciers_keep_snow() {
    let biomes = ochre_base::biome::Biomes::new(1);
    let taiga = biomes.id("taiga").unwrap();
    let tundra = biomes.id("tundra").unwrap();
    let glacier = biomes.id("glacier").unwrap();
    let top = |biome, y, slope| super::terrain::column_block(&biomes, biome, y, y + 1, slope, 0.2, WATER_LEVEL);
    assert_eq!(BlockType::Grass, top(taiga, 100, 0.1));
    assert_eq!(BlockType::Gravel, top(taiga, 100, 0.8));
    assert_eq!(BlockType::Snow, top(taiga, 300, 0.1));
    assert_eq!(BlockType::Grass, top(tundra, 100, 0.1));
    assert_eq!(BlockType::Ice, top(glacier, 100, 0.1));
}

fn test_regions() -> Arc<RegionStore> {
    Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(3)))
}

#[test]
fn test_lod_level_zero_matches_full_detail_chunk() {
    let regions = test_regions();
    for coord in [
        I64Vec3::new(1, 1, 2),
        I64Vec3::new(-2, 0, 3),
        I64Vec3::new(0, 2, -1),
    ] {
        let chunk = generate_chunk(regions.clone(), ChunkCoordinate(coord));
        let lod = LodBlocks::sample(&regions, LodKey { level: 0, coord });
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    assert_eq!(
                        chunk.get_block_at(U16Vec3::new(x, y, z)),
                        lod.raw(IVec3::new(x as i32, y as i32, z as i32)),
                        "chunk {coord} block ({x},{y},{z})"
                    );
                }
            }
        }
    }
}

fn flat_lod(surface_y: i32) -> LodBlocks {
    let size = 16;
    let padded = (size + 2) as usize;
    let mut cells = vec![BlockType::Air; padded * padded * padded];
    for x in -1..=size {
        for z in -1..=size {
            for y in -1..surface_y {
                cells[LodBlocks::index(size, IVec3::new(x, y, z))] = BlockType::Rock;
            }
        }
    }
    let tops = vec![surface_y as i64; padded * padded];
    LodBlocks::new(size, cells, tops, 0, 1)
}

// Grass two 4-block cells deep, ground 6 up the node for x < 8 and 7 beyond, both in the second cell: its top lowers.
fn sloped_lod() -> LodBlocks {
    let base_y = WATER_LEVEL + 64;
    let size = 16;
    let padded = (size + 2) as usize;
    let mut cells = vec![BlockType::Air; padded * padded * padded];
    let mut tops = vec![0; padded * padded];
    for x in -1..=size {
        for z in -1..=size {
            tops[LodBlocks::column(size, x, z)] = base_y + if x < 8 { 6 } else { 7 };
            for y in -1..=1 {
                cells[LodBlocks::index(size, IVec3::new(x, y, z))] = BlockType::Grass;
            }
        }
    }
    LodBlocks::new(size, cells, tops, base_y, 4)
}

// `lod`'s opaque mesh, lit as `generate_lod_mesh` lights it
fn mesh_lod(lod: &LodBlocks) -> Mesh {
    let top = lod.base_y + (lod.size as i64 + 1) * lod.cell;
    let opacity = lod.cells.iter().map(BlockType::light_opacity).collect();
    let light = Box::new(SkyLight::new(
        IVec3::splat(-1),
        IVec3::splat(lod.size + 2),
        super::chunk::sky_above(top, lod.cell),
        opacity,
    ));
    let tint = GrassTint::new(
        &*ochre_world::worldmap::test_terrain(3),
        Vec3::ZERO,
        lod.cell as f32,
    );
    super::mesh::mesh_blocks(lod, lod.cell, style(false, false), &tint, &light).opaque
}

fn faces(mesh: &Mesh) -> Vec<([f32; 3], Vec<[f32; 3]>)> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("missing normals");
    };
    let mut faces: Vec<([f32; 3], Vec<[f32; 3]>)> = Vec::new();
    for (p, n) in positions.iter().zip(normals) {
        match faces
            .iter_mut()
            .find(|(normal, vs)| normal == n && vs.len() < 4)
        {
            Some((_, vs)) => vs.push(*p),
            None => faces.push((*n, vec![*p])),
        }
    }
    faces
}

#[test]
fn test_lod_surface_tops_out_at_the_grounds_height() {
    let lod = sloped_lod();
    // (6 - 0) / 4 - 0.5 and (7 - 0) / 4 - 0.5, in cell units centred on the cells
    assert_eq!(Some(1.0), lod.surface_top(IVec3::new(2, 1, 2)));
    assert_eq!(Some(1.25), lod.surface_top(IVec3::new(10, 1, 2)));
    assert_eq!(None, lod.surface_top(IVec3::new(2, 0, 2)));
}

#[test]
fn test_lod_surface_under_water_fills_its_cell() {
    let LodBlocks {
        size,
        mut cells,
        tops,
        base_y,
        cell,
        ..
    } = sloped_lod();
    cells[LodBlocks::index(16, IVec3::new(2, 2, 2))] = BlockType::Water;
    // nor does one beside water, which would show a gap under the water's side
    cells[LodBlocks::index(16, IVec3::new(6, 1, 2))] = BlockType::Water;
    let lod = LodBlocks::new(size, cells, tops, base_y, cell);
    assert_eq!(None, lod.surface_top(IVec3::new(2, 1, 2)));
    assert_eq!(None, lod.surface_top(IVec3::new(5, 1, 2)));
    assert_eq!(Some(1.0), lod.surface_top(IVec3::new(4, 1, 2)));
}

fn mesh_chunk(chunk: ChunkData) -> ChunkMeshes {
    // well above the sea, under open sky
    let coord = ChunkCoordinate(I64Vec3::new(0, 40, 0));
    generate_chunk_mesh_with(
        Arc::new(chunk),
        vec![None; 26],
        coord,
        &RegionStore::new(ochre_world::worldmap::test_terrain(3)),
        false)
}


fn mesh_vertices(chunk: ChunkData) -> usize {
    mesh_chunk(chunk).opaque.count_vertices()
}

fn floor_chunk() -> ChunkData {
    let mut chunk = ChunkData::default();
    for x in 0..16 {
        for z in 0..16 {
            chunk.set_block_at(U16Vec3::new(x, 0, z), BlockType::Rock);
        }
    }
    chunk
}

#[test]
fn test_only_full_detail_grass_below_the_top_layer_is_dirt() {
    assert_eq!(
        BlockType::Dirt,
        under_grass(BlockType::Grass, 18, 20, 1, false, SOIL_DEPTH, 0)
    );
    assert_eq!(
        BlockType::Grass,
        under_grass(BlockType::Grass, 19, 20, 1, false, SOIL_DEPTH, 0)
    );
    assert_eq!(
        BlockType::Grass,
        under_grass(BlockType::Grass, 10, 20, 4, false, SOIL_DEPTH, 0)
    );
    assert_eq!(
        BlockType::Rock,
        under_grass(BlockType::Rock, 16, 20, 1, false, SOIL_DEPTH, 0)
    );
}

#[test]
fn test_grass_tint_matches_along_shared_edges() {
    let noise = ochre_world::worldmap::test_terrain(3);
    let chunk_tint =
        |x: i64, z: i64| GrassTint::new(&noise, Vec3::new(x as f32, 0.0, z as f32) * 16.0, 1.0);
    let (here, east, south) = (chunk_tint(5, 5), chunk_tint(6, 5), chunk_tint(5, 6));
    for step in 0..=16 {
        let along = step as f32 - 0.5;
        assert!(here
            .at(Vec3::new(15.5, 0.0, along))
            .abs_diff_eq(east.at(Vec3::new(-0.5, 0.0, along)), 1e-5));
        assert!(here
            .at(Vec3::new(along, 0.0, 15.5))
            .abs_diff_eq(south.at(Vec3::new(along, 0.0, -0.5)), 1e-5));
    }

    // a LOD node four chunks across, its local units 4 blocks wide, starting at chunk (4, 4)
    let node = GrassTint::new(&noise, Vec3::new(64.0, 0.0, 64.0) + Vec3::splat(1.5), 4.0);
    assert!(node
        .at(Vec3::splat(-0.5))
        .abs_diff_eq(chunk_tint(4, 4).at(Vec3::splat(-0.5)), 1e-5));
    assert!(node
        .at(Vec3::splat(15.5))
        .abs_diff_eq(chunk_tint(7, 7).at(Vec3::splat(15.5)), 1e-5));
}

// The most any channel of the grass's tint differs between full-detail chunks and the coarse node over them.
fn handover_tint_gap(seed: u32, node_corner: Vec3, scale: f32) -> f32 {
    let noise = ochre_world::worldmap::test_terrain(seed);
    // the node's local origin is half a cell back from its first block, as `GrassTint::new` takes it
    let node = GrassTint::new(&noise, node_corner + Vec3::splat(0.5 * scale), scale);
    let mut gap = 0.0f32;
    let chunks = (16.0 * scale / 16.0) as i64;
    for cz in 0..chunks {
        for cx in 0..chunks {
            let corner = node_corner + Vec3::new(cx as f32, 0.0, cz as f32) * 16.0;
            let chunk = GrassTint::new(&noise, corner, 1.0);
            for (dx, dz) in [(2.0, 2.0), (8.0, 8.0), (13.0, 5.0), (5.0, 13.0)] {
                let world = corner + Vec3::new(dx, 0.0, dz);
                let in_node = node.at((world - node_corner) / scale - Vec3::splat(0.5));
                let in_chunk = chunk.at(Vec3::new(dx, 0.0, dz));
                gap = gap.max((in_node - in_chunk).abs().max_element());
            }
        }
    }
    gap
}

#[test]
fn test_grass_tint_barely_changes_where_a_chunk_hands_over_to_a_node() {
    // levels 3 and 4 over places on two worlds, away from water (node tint misses bank wetting): gaps ~0.01
    for (seed, corner) in [(3, Vec3::new(2048.0, 0.0, 2048.0)), (3, Vec3::new(-4096.0, 0.0, 1024.0)), (5, Vec3::new(-2048.0, 0.0, -3072.0))] {
        for scale in [8.0, 16.0] {
            let gap = handover_tint_gap(seed, corner, scale);
            assert!(gap < 0.03, "tint pops by {gap} at the handover, seed {seed} {corner}, cells of {scale}");
        }
    }
}

// The middle of a forest on the test world with this seed.
fn forest_at(seed: u32) -> bevy::math::I64Vec2 {
    ochre_world::worldmap::forest_spot(&ochre_world::worldmap::test_terrain(seed))
}

// The middle of open country with a few lone trees on the test world with this seed.
fn sparse_at(seed: u32) -> bevy::math::I64Vec2 {
    ochre_world::worldmap::tree_spot(&ochre_world::worldmap::test_terrain(seed), 4, 10)
}

// The same, or None for a world with no open country with a few lone trees.
fn try_sparse_at(seed: u32) -> Option<bevy::math::I64Vec2> {
    ochre_world::worldmap::try_tree_spot(&ochre_world::worldmap::test_terrain(seed), 4, 10)
}

// top-face sky light at each vertex, by position
fn top_lights(mesh: &Mesh) -> Vec<([f32; 3], f32)> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("missing normals");
    };
    let Some(VertexAttributeValues::Float32x2(lights)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        panic!("missing light");
    };
    (0..positions.len())
        .filter(|&i| normals[i] == [0.0, 1.0, 0.0])
        .map(|i| (positions[i], lights[i][0]))
        .collect()
}

#[test]
fn test_open_ground_is_fully_lit() {
    let lights = top_lights(&mesh_chunk(floor_chunk()).opaque);
    assert!(!lights.is_empty() && lights.iter().all(|(_, l)| *l == 1.0));
}

// whether each vertex draws its block's mean colour instead of its texture, see `world.wesl`
fn untextured_flags(mesh: &Mesh) -> Vec<f32> {
    let Some(VertexAttributeValues::Float32x2(lights)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        panic!("missing light");
    };
    lights.iter().map(|l| l[1]).collect()
}

#[test]
fn test_tree_boxes_stay_textured_up_to_eight_wide_cells_with_a_tile_a_block() {
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let mut heights = regions.sampler();
    let trees = crate::generate::trees::trees_near(
        regions.terrain(),
        &mut heights,
        forest_at(5) - bevy::math::I64Vec2::splat(300),
        forest_at(5) + bevy::math::I64Vec2::splat(300),
    );
    // a coarse LOD node can put a tree on a terrace off its true foot, in another node: take the first with a mesh
    let (_, mesh) = trees
        .iter()
        .find_map(|tree| {
            let crown = tree.base + I64Vec3::Y * (tree.trunk - 1);
            let key = LodKey::containing(ChunkCoordinate(crown.div_euclid(I64Vec3::splat(16))), 3);
            assert!(key.cell_size() <= super::buffers::BOX_TEXTURE_MAX_CELL);
            let mesh = super::lod::generate_lod_mesh(regions.clone(), key, None)?;
            // and one that holds a textured box, which a node whose ground is bare of trees round the crown does not
            (untextured_flags(&mesh.opaque).last() == Some(&0.0)).then_some((tree, mesh))
        })
        .expect("no tree has a mesh with a textured box");
    let opaque = mesh.opaque;
    let flags = untextured_flags(&opaque);
    let VertexAttributeValues::Float32x2(uvs) = opaque.attribute(Mesh::ATTRIBUTE_UV_0).unwrap()
    else {
        panic!()
    };
    // boxes come last: textured, and counting more than one tile across a crown
    let boxed = flags.len() - flags.iter().rev().take_while(|&&f| f == 0.0).count();
    assert!(boxed < flags.len(), "no textured tree boxes");
    assert!(uvs[boxed..].iter().any(|uv| uv[0] > 3.0 || uv[1] > 3.0));
    // the sculpted ground packs its facet into the fraction
    assert!(flags[..boxed].iter().all(|&f| f >= 1.0));
}

#[test]
fn test_ambient_occlusion_stops_faces_merging() {
    let mut chunk = floor_chunk();
    chunk.set_block_at(U16Vec3::new(8, 1, 8), BlockType::Rock);
    // one quad per face of the floor and of the pillar would be 44; the shaded ring around it splits the top
    assert!(mesh_vertices(chunk) > 44);
}

// Open air over ground far below the node, with `blocks` set, in cells of 4 blocks.
fn lod_with(blocks: &[(IVec3, BlockType)]) -> LodBlocks {
    let size = 16;
    let padded = (size + 2) as usize;
    let mut cells = vec![BlockType::Air; padded * padded * padded];
    for &(coord, block) in blocks {
        cells[LodBlocks::index(size, coord)] = block;
    }
    LodBlocks::new(size, cells, vec![0; padded * padded], WATER_LEVEL + 64, 4)
}

fn leaf_cube(min: i32) -> Vec<(IVec3, BlockType)> {
    let mut cube = Vec::new();
    for x in min..min + 2 {
        for y in min..min + 2 {
            for z in min..min + 2 {
                cube.push((IVec3::new(x, y, z), BlockType::Leaves));
            }
        }
    }
    cube
}

// how many vertices lie on faces in the plane at `plane` across their normal's axis
fn vertices_in_plane(mesh: &Mesh, plane: f32) -> usize {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("missing normals");
    };
    positions
        .iter()
        .zip(normals)
        .filter(|(p, n)| (0..3).any(|k| n[k] != 0.0 && p[k] == plane))
        .count()
}

#[test]
fn test_untextured_leaves_hide_the_leaves_behind_them() {
    let mesh = mesh_lod(&lod_with(&leaf_cube(7)));
    // cells 7 and 8 meet at 7.5; the shell is at 6.5 and 8.5
    assert_eq!(0, vertices_in_plane(&mesh, 7.5));
    assert!(vertices_in_plane(&mesh, 6.5) > 0 && vertices_in_plane(&mesh, 8.5) > 0);
    // each side of the shell merges into one quad, a centre point and 8 round its border
    assert_eq!(6 * 9, mesh.count_vertices());
}

#[test]
fn test_untextured_leaves_still_show_other_blocks() {
    let mut blocks = leaf_cube(7);
    blocks.push((IVec3::new(6, 7, 7), BlockType::Log));
    let faces = faces(&mesh_lod(&lod_with(&blocks)));
    // the log's face into the leaves stays: only leaves hide leaves
    assert!(faces
        .iter()
        .any(|(n, vs)| *n == [1.0, 0.0, 0.0] && vs.iter().all(|v| v[0] == 6.5)));
}

#[test]
fn test_untextured_leaves_on_the_nodes_edge_keep_their_faces() {
    // past the edge the leaves may be drawn at another detail, with none just there
    let lod = lod_with(&[
        (IVec3::new(15, 4, 4), BlockType::Leaves),
        (IVec3::new(16, 4, 4), BlockType::Leaves),
    ]);
    let faces = faces(&mesh_lod(&lod));
    assert!(faces
        .iter()
        .any(|(n, vs)| *n == [1.0, 0.0, 0.0] && vs.iter().all(|v| v[0] == 15.5)));
}

fn leaf_cube_chunk() -> ChunkData {
    let mut chunk = ChunkData::default();
    for (coord, block) in leaf_cube(4) {
        chunk.set_block_at(coord.as_u16vec3(), block);
    }
    chunk
}

#[test]
fn test_leaves_on_the_chunks_edge_keep_their_faces_with_the_rest() {
    // past the edge the leaves may be drawn by a LOD node, with none just there
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(15, 4, 4), BlockType::Leaves);
    let mut east = ChunkData::default();
    east.set_block_at(U16Vec3::new(0, 4, 4), BlockType::Leaves);
    let mut neighbours = vec![None; 26];
    neighbours[neighbor_26_index(1, 0, 0)] = Some(Arc::new(east));
    let meshes = generate_chunk_mesh_with(
        Arc::new(chunk),
        neighbours,
        ChunkCoordinate(I64Vec3::new(0, 40, 0)),
        &RegionStore::new(ochre_world::worldmap::test_terrain(3)),
        false);
    assert!(vertices_in_plane(&meshes.opaque, 15.5) > 0);
}

#[test]
fn test_full_detail_solid_leaves_hide_the_leaves_behind_them() {
    let coord = ChunkCoordinate(I64Vec3::new(0, 40, 0));
    let mesh = generate_chunk_mesh_with(
        Arc::new(leaf_cube_chunk()),
        vec![None; 26],
        coord,
        &RegionStore::new(ochre_world::worldmap::test_terrain(3)),
        false);
    let mesh = mesh.opaque;
    assert_eq!(0, vertices_in_plane(&mesh, 4.5));
    assert!(vertices_in_plane(&mesh, 3.5) > 0 && vertices_in_plane(&mesh, 5.5) > 0);
}

#[test]
fn test_lod_border_exposes_surface_cells_as_skirt() {
    let lod = flat_lod(8);
    // the border's top three solid cells read as open, so the edge draws a wall
    assert_eq!(BlockType::Air, lod.block(IVec3::new(-1, 7, 4)));
    assert_eq!(BlockType::Air, lod.block(IVec3::new(-1, 6, 4)));
    assert_eq!(BlockType::Air, lod.block(IVec3::new(4, 7, 16)));
    // deeper border cells and every interior cell read as they are
    assert_eq!(BlockType::Rock, lod.block(IVec3::new(-1, 4, 4)));
    assert_eq!(BlockType::Rock, lod.block(IVec3::new(4, 7, 4)));
    // below the node isn't a horizontal border, so no skirt
    assert_eq!(BlockType::Rock, lod.block(IVec3::new(4, -1, 4)));
}

#[test]
fn test_skirt_walls_are_lit_like_the_open_ground_they_hang_from() {
    // flat ground under open sky, cells a block across, and the cells beyond it solid
    let lod = flat_lod(8);
    let noise = ochre_world::worldmap::test_terrain(1);
    let tint = super::buffers::GrassTint::new(&noise, Vec3::ZERO, 1.0);
    let opacity = lod.cells.iter().map(BlockType::light_opacity).collect();
    let light = std::cell::LazyCell::new(|| {
        crate::generate::light::SkyLight::new(
            IVec3::splat(-1),
            IVec3::splat(18),
            crate::generate::light::MAX_LIGHT,
            opacity,
        )
    });
    let mesh = super::mesh::mesh_blocks(&lod, 1, style(true, false), &tint, &light).opaque;
    let Some(bevy::render::mesh::VertexAttributeValues::Float32x2(lights)) =
        mesh.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        panic!("no light on the mesh");
    };
    // the top and the four walls, each from a corner to a corner of the node
    assert!(lights.len() >= 4 * 5);
    let darkest = lights.iter().map(|l| l[0]).fold(f32::MAX, f32::min);
    assert!(
        darkest > 0.6,
        "a vertex is lit {darkest}, so a skirt wall is dark"
    );
}

#[test]
fn test_lod_skirt_hangs_from_the_nodes_own_ground_not_its_neighbours() {
    // ground reaching the top of the node, where there's no air above to find
    let size = 16;
    let padded = (size + 2) as usize;
    let mut cells = vec![BlockType::Air; padded * padded * padded];
    for x in -1..=size {
        for z in -1..=size {
            for y in -1..=size {
                cells[LodBlocks::index(size, IVec3::new(x, y, z))] = BlockType::Rock;
            }
        }
    }
    // the ground's top is in the node above, 3 cells over the top row
    let lod = LodBlocks::new(
        size,
        cells.clone(),
        vec![size as i64 + 3; padded * padded],
        0,
        1,
    );
    assert_eq!(BlockType::Rock, lod.block(IVec3::new(-1, 8, 4)));
    // ground ending in the top row: the cells within three of it have walls, though the top row is solid
    let lod = LodBlocks::new(size, cells, vec![size as i64 + 1; padded * padded], 0, 1);
    assert_eq!(BlockType::Air, lod.block(IVec3::new(-1, 16, 4)));
    assert_eq!(BlockType::Air, lod.block(IVec3::new(-1, 15, 4)));
    assert_eq!(BlockType::Rock, lod.block(IVec3::new(-1, 13, 4)));
}

#[test]
fn test_lod_water_surface_cells_over_shallow_ground_are_water() {
    use ochre_world::worldmap::{WaterCell, WorldConfig, WorldMap, WorldTerrain};
    let mut map = WorldMap::empty(
        102,
        WorldConfig {
            size: 1024,
            ..Default::default()
        },
    );
    map.height.fill(-1.5);
    map.temperature.fill(0.5);
    map.water_kind.fill(WaterCell::Ocean as u8);
    let regions = Arc::new(RegionStore::new(Arc::new(WorldTerrain::new(Arc::new(map)))));
    let mut checked = 0;
    for level in [3u8, 4, 5] {
        for x in -6..6i64 {
            for z in -6..6i64 {
                let width = 1i64 << level;
                let key = LodKey {
                    level,
                    coord: I64Vec3::new(x, (WATER_LEVEL - 1).div_euclid(16).div_euclid(width), z),
                };
                let lod = LodBlocks::sample(&regions, key);
                for cx in 0..lod.size {
                    for cz in 0..lod.size {
                        let column = LodBlocks::column(lod.size, cx, cz);
                        // a river held above the sea has its own surface to fill to
                        if lod.tops[column] >= WATER_LEVEL || lod.water_tops[column] != WATER_LEVEL
                        {
                            continue;
                        }
                        for cy in 0..lod.size {
                            let bottom = lod.base_y + cy as i64 * lod.cell;
                            if bottom < WATER_LEVEL && WATER_LEVEL <= bottom + lod.cell {
                                assert!(!super::terrain::is_solid(lod.raw(IVec3::new(cx, cy, cz))));
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 1000);
}

#[test]
fn test_water_depth_on_lod_border_ignores_skirts() {
    // ground up to y = 3 everywhere, water from 4 to 7
    let mut lod = flat_lod(4);
    for x in -1..=16 {
        for z in -1..=16 {
            for y in 4..8 {
                lod.cells[LodBlocks::index(16, IVec3::new(x, y, z))] = BlockType::Water;
            }
        }
    }
    // the border's top solid cells read as water for the skirt, but don't add depth
    assert_eq!(BlockType::Water, lod.block(IVec3::new(-1, 3, 4)));
    assert_eq!(4, water_depth(&lod, IVec3::new(4, 7, 4)));
    assert_eq!(4, water_depth(&lod, IVec3::new(-1, 7, 4)));
}

#[test]
fn test_shallow_water_in_a_coarse_lod_cell_is_drawn_to_its_real_surface() {
    // cells of 4 blocks, ground up to y = 4 and water to y = 6: its surface is the cell's middle
    let mut lod = flat_lod(1);
    lod.cell = 4;
    lod.base_y = 0;
    lod.water_tops = vec![6; (16 + 2) * (16 + 2)];
    lod.cells[LodBlocks::index(16, IVec3::new(4, 1, 4))] = BlockType::Water;
    assert_eq!(Some(0.0), lod.find_water_surface_top(IVec3::new(4, 1, 4)));
    // a cell the water fills to the top, or with water over it, has none
    lod.water_tops = vec![8; (16 + 2) * (16 + 2)];
    assert_eq!(None, lod.find_water_surface_top(IVec3::new(4, 1, 4)));
    lod.water_tops = vec![6; (16 + 2) * (16 + 2)];
    lod.cells[LodBlocks::index(16, IVec3::new(4, 2, 4))] = BlockType::Water;
    assert_eq!(None, lod.find_water_surface_top(IVec3::new(4, 1, 4)));
}

#[test]
fn test_lod_block_clamps_far_probes_to_border() {
    let lod = flat_lod(8);
    assert_eq!(
        lod.block(IVec3::new(-1, 3, 4)),
        lod.block(IVec3::new(-3, 3, 4))
    );
}

#[test]
fn test_buried_chunk_has_no_geometry() {
    let mut stone = ChunkData::default();
    for (x, y, z) in
        (0..16).flat_map(|x| (0..16).flat_map(move |y| (0..16).map(move |z| (x, y, z))))
    {
        stone.set_block_at(U16Vec3::new(x, y, z), BlockType::Rock);
    }
    let stone = Arc::new(stone);
    let meshes = generate_chunk_mesh_with(
        stone.clone(),
        vec![Some(stone); 26],
        ChunkCoordinate(I64Vec3::ZERO),
        &test_regions(),
        false);
    assert!(!meshes.has_geometry());
}

#[test]
fn test_canopy_shade_darkens_toward_the_ground() {
    let padded = 18 * 18;
    let lod = LodBlocks {
        canopy_tops: vec![40; padded],
        ..flat_lod(8)
    };
    let at = |y: f32| lod.canopy_shade(Vec3::new(4.0, y, 4.0));
    // a leaf at the canopy's top is unshaded, and the foot of a canopy is the darkest
    assert_eq!(1.0, at(39.5));
    assert!((at(19.5) - (1.0 - super::canopy::CANOPY_SHADE)).abs() < 1e-5);
    assert!(at(30.0) < at(35.0) && at(35.0) < at(39.5));
    // below the span it stops getting darker
    assert_eq!(at(19.5), at(9.5));
}

#[test]
fn test_canopy_shade_leaves_bare_columns_and_other_lods_alone() {
    let padded = 18 * 18;
    let bare = LodBlocks {
        canopy_tops: vec![i64::MIN; padded],
        ..flat_lod(8)
    };
    assert_eq!(1.0, bare.canopy_shade(Vec3::new(4.0, 2.0, 4.0)));
    assert_eq!(1.0, flat_lod(8).canopy_shade(Vec3::new(4.0, 2.0, 4.0)));
}

#[test]
fn test_a_lone_tree_is_still_there_in_every_cell_under_sixteen_wide() {
    use crate::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    // a world changes with the geology, so the first seed with a tree standing alone and clear of its cell's edges
    let (regions, tree) = (1..40u32)
        .find_map(|seed| {
            let spot = try_sparse_at(seed)?;
            let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(seed)));
            let mut heights = regions.sampler();
            let mut trees = trees_near(
                regions.terrain(),
                &mut heights,
                spot - I64Vec2::splat(700),
                spot + I64Vec2::splat(700),
            );
            let near = |a: &crate::generate::trees::Tree, b: &crate::generate::trees::Tree| {
                (a.base.x - b.base.x).abs() < 70 && (a.base.z - b.base.z).abs() < 70
            };
            let at = trees.iter().position(|t| {
                let crown = t.base + I64Vec3::Y * (t.trunk - 1);
                let key =
                    LodKey::containing(ChunkCoordinate(crown.div_euclid(I64Vec3::splat(16))), 2);
                let (min, max) = t.extent();
                let low = key.min_block();
                let high = low + I64Vec3::splat(64);
                trees.iter().filter(|o| near(t, o)).count() == 1
                    && min.cmpge(low + I64Vec3::Y * 8).all()
                    && max.cmplt(high - I64Vec3::Y * 8).all()
            })?;
            Some((regions, trees.swap_remove(at)))
        })
        .expect("no interior lone tree");
    let crown = tree.base + I64Vec3::Y * (tree.trunk - 1);
    for level in [1u8, 2, 3] {
        let key = LodKey::containing(ChunkCoordinate(crown.div_euclid(I64Vec3::splat(16))), level);
        let blocks = LodBlocks::sample(&regions, key);
        let cell = key.cell_size();
        let leaf_cells = blocks
            .cells
            .iter()
            .filter(|&&c| c == BlockType::Leaves)
            .count();
        let crown_boxes = blocks
            .boxes
            .iter()
            .filter(|b| b.block == BlockType::Leaves)
            .count();
        if cell < super::canopy::BOX_CROWN_CELL {
            assert!(
                leaf_cells > 0,
                "no leaf cells at level {level}, cells {cell} blocks wide"
            );
        } else {
            // a crown is a few big cards of its real size, never a cell's worth of cube
            assert_eq!(0, leaf_cells);
            assert!(crown_boxes > 0, "no crown cards at level {level}");
            for card in blocks.boxes.iter().filter(|b| b.block == BlockType::Leaves) {
                let width = (card.max.x - card.min.x) * cell as f32;
                assert!(width < 20.0, "a {width} block card at level {level}");
            }
        }
    }
}

#[test]
fn test_a_lone_trees_trunk_is_a_thin_box_in_cells_four_to_eight_wide() {
    use crate::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    // the first test world with open country and a few lone trees: which one has it moves with the ground
    let seed = (1..24).find(|&s| try_sparse_at(s).is_some()).expect("no test world has open country with lone trees");
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(seed)));
    let mut heights = regions.sampler();
    let trees = trees_near(
        regions.terrain(),
        &mut heights,
        sparse_at(seed) - I64Vec2::splat(700),
        sparse_at(seed) + I64Vec2::splat(700),
    );
    let near = |a: &crate::generate::trees::Tree, b: &crate::generate::trees::Tree| {
        (a.base.x - b.base.x).abs() < 70 && (a.base.z - b.base.z).abs() < 70
    };
    let tree = trees
        .iter()
        .find(|t| trees.iter().filter(|o| near(t, o)).count() == 1)
        .expect("no lone tree");
    for level in [2u8, 3] {
        let key = LodKey::containing(
            ChunkCoordinate(tree.base.div_euclid(I64Vec3::splat(16))),
            level,
        );
        let blocks = LodBlocks::sample(&regions, key);
        let cell = key.cell_size() as f32;
        let at = |block: i64, min: i64| (block - min) as f32 / cell - 0.5;
        let (min_x, min_z) = (key.min_block().x, key.min_block().z);
        let trunk = blocks
            .boxes
            .iter()
            .filter(|b| b.block == BlockType::Log)
            .find(|t| {
                (t.min.x - at(tree.base.x, min_x)).abs() < 1e-4
                    && (t.min.z - at(tree.base.z, min_z)).abs() < 1e-4
                    && (t.max.x - t.min.x - tree.width() as f32 / cell).abs() < 1e-4
            });
        assert!(
            trunk.is_some(),
            "no trunk box at level {level}: {:?}",
            blocks.boxes
        );
        let trunk = trunk.unwrap();
        assert!(((trunk.max.y - trunk.min.y) * cell - tree.trunk as f32).abs() < 1e-3);
        // and no log cells, which would be a pillar several times too thick
        assert_eq!(
            0,
            blocks
                .cells
                .iter()
                .filter(|&&c| c == BlockType::Log)
                .count()
        );
        // meshed, the box is a prism standing out of the ground's
        let lone = LodBlocks {
            boxes: vec![*trunk],
            ..LodBlocks::new(
                16,
                vec![BlockType::Air; 18 * 18 * 18],
                vec![0; 18 * 18],
                0,
                key.cell_size(),
            )
        };
        let tint = super::buffers::GrassTint::new(regions.terrain(), Vec3::ZERO, 1.0);
        let light = std::cell::LazyCell::new(|| {
            crate::generate::light::SkyLight::new(
                IVec3::splat(-1),
                IVec3::splat(18),
                crate::generate::light::MAX_LIGHT,
                vec![0; 18 * 18 * 18],
            )
        });
        let mesh =
            super::mesh::mesh_blocks(&lone, key.cell_size(), style(false, false), &tint, &light)
                .opaque;
        // its foot is in the ground: a prism of three rings of nine corners (first repeated for the seam) and a cap hub
        assert_eq!(3 * 9 + 1, mesh.count_vertices());
    }
}

// The densest forest near the origin, and the trees in it.
fn forest(regions: &RegionStore) -> (I64Vec3, Vec<crate::generate::trees::Tree>) {
    use crate::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    // the search is slow, and every caller's world is seed 5's
    static FOREST: std::sync::OnceLock<(I64Vec3, Vec<crate::generate::trees::Tree>)> =
        std::sync::OnceLock::new();
    assert_eq!(5, regions.terrain().seed());
    FOREST
        .get_or_init(|| {
            let mut heights = regions.sampler();
            let centre = forest_at(5);
            let nearby = (-3..=3)
                .flat_map(|x| (-3..=3).map(move |z| centre + I64Vec2::new(x, z) * 128 - 128));
            let corner = (-4..4)
                .flat_map(|x| (-4..4).map(move |z| I64Vec2::new(x, z) * 1024))
                .chain(nearby)
                .max_by_key(|&c| trees_near(regions.terrain(), &mut heights, c, c + 255).len())
                .unwrap();
            let trees = trees_near(regions.terrain(), &mut heights, corner, corner + 255);
            let ground = heights.height(corner.x + 128, corner.y + 128) as i64;
            (I64Vec3::new(corner.x + 128, ground, corner.y + 128), trees)
        })
        .clone()
}

#[test]
fn test_far_forests_are_a_few_canopy_boxes_not_a_box_a_tree() {
    use bevy::math::I64Vec2;
    use ochre_world::worldmap::{climate::BiomeKind, WorldConfig, WorldMap, WorldTerrain};
    let mut map = WorldMap::empty(
        5,
        WorldConfig {
            size: 2048,
            ..Default::default()
        },
    );
    map.height.fill(100.0);
    map.temperature.fill(0.5);
    map.moisture.fill(0.9);
    map.biome.fill(BiomeKind::Forest as u8);
    map.trees.fill(180);
    let regions = Arc::new(RegionStore::new(Arc::new(WorldTerrain::new(Arc::new(map)))));
    let middle = I64Vec3::new(128, 100, 128);
    let trees = crate::generate::trees::trees_near(
        regions.terrain(),
        &mut regions.sampler(),
        I64Vec2::ZERO,
        I64Vec2::splat(255),
    );
    assert!(
        trees.len() > 250,
        "only {} trees in the densest forest",
        trees.len()
    );
    for level in [4u8, 5] {
        let key = LodKey::containing(
            ChunkCoordinate(middle.div_euclid(I64Vec3::splat(16))),
            level,
        );
        let blocks = LodBlocks::sample(&regions, key);
        let cell = key.cell_size();
        let span = 16 * cell;
        let min = key.min_block();
        let inside = trees
            .iter()
            .filter(|t| {
                (min.x..min.x + span).contains(&t.base.x)
                    && (min.z..min.z + span).contains(&t.base.z)
            })
            .count();
        let leaf_boxes: Vec<_> = blocks
            .boxes
            .iter()
            .filter(|b| b.block == BlockType::Leaves)
            .collect();
        assert!(!leaf_boxes.is_empty(), "no canopy at level {level}");
        // a 35% forest merges fewer crowns than 55%: boxes need only be two thirds of trees; widest cells ask <= 2 per tree
        let allowed = if level == 4 {
            inside * 2 / 3
        } else {
            inside * 2
        };
        assert!(
            leaf_boxes.len() < allowed,
            "{} leaf boxes for {inside} trees at level {level}",
            leaf_boxes.len()
        );
        // the forest's tree tops are under the canopy's, within a step of its rounding
        let step = (cell / super::canopy::CANOPY_STEPS) as f32;
        let highest = leaf_boxes
            .iter()
            .map(|b| min.y as f32 + (b.max.y + 0.5) * cell as f32)
            .fold(f32::MIN, f32::max);
        let corner = bevy::math::I64Vec2::new(min.x, min.z);
        let node_trees = crate::generate::trees::trees_near(
            regions.terrain(),
            &mut regions.sampler(),
            corner,
            corner + span,
        );
        let tallest = node_trees
            .iter()
            .map(|t| t.crown_top() as f32)
            .fold(f32::MIN, f32::max);
        assert!(
            highest <= tallest + step,
            "canopy at {highest}, trees up to {tallest} at level {level}"
        );
        // the ground under it is darkened by how much of it is under crowns
        assert!(blocks.forest.iter().any(|&f| f > 0.8));
    }
}

#[test]
fn test_canopy_is_no_bigger_than_the_crowns_under_it() {
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let (middle, _) = forest(&regions);
    for level in [4u8, 5] {
        let key = LodKey::containing(
            ChunkCoordinate(middle.div_euclid(I64Vec3::splat(16))),
            level,
        );
        let blocks = LodBlocks::sample(&regions, key);
        let canopy: Vec<_> = blocks
            .boxes
            .iter()
            .filter(|b| b.block == BlockType::Leaves)
            .collect();
        let area = |b: &&super::source::TreeBox| (b.max.x - b.min.x) * (b.max.z - b.min.z);
        let drawn: f32 = canopy.iter().map(area).sum();
        // what's under crowns over the ground this node draws
        let (min, span) = (key.min_block().y as f32, 16.0 * key.cell_size() as f32);
        let mut under = 0.0;
        for x in 0..16 {
            for z in 0..16 {
                let column = LodBlocks::column(16, x, z);
                if (min..min + span).contains(&blocks.ground_heights[column]) {
                    under += blocks.forest[column];
                }
            }
        }
        // its edges are drawn in to the crowns, so extra is the gaps between crowns inside it, more the wider the columns
        assert!(
            drawn <= under * 1.4,
            "canopy {drawn} over {under} under crowns at level {level}"
        );
    }
}

#[test]
fn test_trees_vary_in_colour_the_same_at_every_level() {
    use crate::generate::trees::TreeTints;
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let (_, trees) = forest(&regions);
    let tints: Vec<Vec3> = trees.iter().map(|t| t.tint()).collect();
    let brightness = |t: &Vec3| t.x + t.y + t.z;
    let (dim, bright) = tints.iter().fold((f32::MAX, f32::MIN), |(lo, hi), t| {
        (lo.min(brightness(t)), hi.max(brightness(t)))
    });
    assert!(bright - dim > 0.4, "tints only span {dim}..{bright}");
    assert!(
        tints
            .iter()
            .all(|t| t.min_element() > 0.6 && t.max_element() < 1.5),
        "a tint strays too far"
    );
    // leaves take their own tree's tint, at full detail and in coarser cells
    let tree = &trees[0];
    let trunk_top = tree.base + I64Vec3::Y * tree.trunk;
    for cell in [1, 2] {
        let origin = trunk_top.div_euclid(I64Vec3::splat(16 * cell)) * 16 * cell;
        let at = (trunk_top - origin)
            .div_euclid(I64Vec3::splat(cell))
            .as_ivec3();
        assert_eq!(
            tree.tint(),
            TreeTints::new(&trees, origin, cell).at(at),
            "at cells {cell} wide"
        );
    }
}

#[test]
fn test_leaves_of_differently_tinted_trees_dont_merge() {
    let mut chunk = leaf_cube_chunk();
    // two thin-trunked trees, one each side of the cube, so it's split down the middle
    let thin = [
        crate::generate::trees::Tree::new(
            crate::generate::trees::TreeKind::Broadleaf,
            I64Vec3::ZERO,
            10,
            5.0,
            0.7,
            11,
        ),
        crate::generate::trees::Tree::new(
            crate::generate::trees::TreeKind::Broadleaf,
            I64Vec3::ZERO,
            10,
            5.0,
            0.7,
            29,
        ),
    ];
    let mut pair = vec![thin[0], thin[1]];
    pair[0].base = I64Vec3::new(4, 0, 4);
    pair[1].base = I64Vec3::new(5, 0, 4);
    assert_ne!(pair[0].tint(), pair[1].tint());
    chunk.trees = crate::generate::trees::TreeTints::new(&pair, I64Vec3::ZERO, 1);
    let mesh = mesh_chunk(chunk).opaque;
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!()
    };
    let leaf = Tile::Leaves.atlas_cell();
    let mut shades: Vec<[u32; 3]> = colors
        .iter()
        .filter(|c| c[3] == leaf)
        .map(|c| [c[0], c[1], c[2]].map(|v| (v * 1000.0) as u32))
        .collect();
    shades.sort();
    shades.dedup();
    assert!(shades.len() >= 2, "leaves all one shade: {shades:?}");
}

#[test]
fn test_forest_tints_the_ground_darker_the_more_of_it_trees_cover() {
    let padded = 18 * 18;
    let mut forest = vec![0.0; padded];
    forest[LodBlocks::column(16, 4, 4)] = 0.5;
    forest[LodBlocks::column(16, 8, 8)] = 1.0;
    let lod = LodBlocks {
        forest,
        ..flat_lod(8)
    };
    let at = |x: f32, z: f32| lod.forest_shade(Vec3::new(x, 7.5, z));
    assert_eq!(1.0, at(1.0, 1.0));
    assert!(at(4.0, 4.0) < 1.0 && at(8.0, 8.0) < at(4.0, 4.0));
    assert!((at(8.0, 8.0) - super::canopy::FOREST_DARKEN).abs() < 1e-5);
    // real trees and bare ground leave the ground as it was
    assert_eq!(1.0, flat_lod(8).forest_shade(Vec3::new(4.0, 7.5, 4.0)));
}

#[test]
fn test_cells_wider_than_real_trees_tint_forest_instead_of_drawing_leaves() {
    use crate::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let mut heights = regions.sampler();
    let trees = trees_near(
        regions.terrain(),
        &mut heights,
        forest_at(5) - I64Vec2::splat(300),
        forest_at(5) + I64Vec2::splat(300),
    );
    let tree = trees.first().expect("no trees");
    let key = LodKey::containing(ChunkCoordinate(tree.base.div_euclid(I64Vec3::splat(16))), 6);
    assert!(key.cell_size() > super::canopy::TREE_MAX_CELL);
    let blocks = LodBlocks::sample(&regions, key);
    assert_eq!(
        0,
        blocks
            .cells
            .iter()
            .filter(|&&c| c == BlockType::Leaves || c == BlockType::Log)
            .count()
    );
    assert!(blocks.boxes.is_empty());
    assert!(
        blocks.forest.iter().any(|&f| f > 0.0)
            && blocks.forest.iter().all(|&f| (0.0..=1.0).contains(&f))
    );
}

type Spots = Vec<(i64, i64)>;

// Where trees grow within a node's own area, as full detail decides, and as the node's grid does.
fn tree_spots(regions: &Arc<RegionStore>, key: LodKey) -> (Spots, Spots) {
    use crate::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    let (cell, base, size) = (key.cell_size(), key.min_block(), CHUNK_SIZE as i32);
    let span = (size + 1) as i64 * cell;
    let min = I64Vec2::new(base.x, base.z) - cell;
    let own = |t: &crate::generate::trees::Tree| {
        (base.x..base.x + size as i64 * cell).contains(&t.base.x)
            && (base.z..base.z + size as i64 * cell).contains(&t.base.z)
    };
    let mut heights = regions.sampler();
    let mut exact: Vec<_> = trees_near(regions.terrain(), &mut heights, min, min + span + cell)
        .iter()
        .filter(|t| own(t))
        .map(|t| (t.base.x, t.base.z))
        .collect();
    let grid = regions.column_grid(key);
    let mut ground = super::lod::GridGround {
        grid: &grid,
        noise: regions.terrain(),
        base: I64Vec2::new(base.x, base.z),
        cell,
        size,
        last: None,
    };
    let mut coarse: Vec<_> = trees_near(regions.terrain(), &mut ground, min, min + span + cell)
        .iter()
        .filter(|t| own(t))
        .map(|t| (t.base.x, t.base.z))
        .collect();
    exact.sort();
    coarse.sort();
    (exact, coarse)
}

#[test]
fn test_every_level_grows_the_trees_full_detail_does() {
    use bevy::math::I64Vec2;
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    for level in [2u8, 3, 4, 5] {
        // nodes over a forested stretch, found from the full-detail trees
        let mut heights = regions.sampler();
        let trees = crate::generate::trees::trees_near(
            regions.terrain(),
            &mut heights,
            forest_at(5) - I64Vec2::splat(500),
            forest_at(5) + I64Vec2::splat(500),
        );
        let mut checked = 0;
        for tree in trees.iter().step_by(trees.len() / 8 + 1) {
            let key = LodKey::containing(
                ChunkCoordinate(tree.base.div_euclid(I64Vec3::splat(16))),
                level,
            );
            let (exact, coarse) = tree_spots(&regions, key);
            assert_eq!(exact, coarse, "level {level} node {key:?}");
            checked += exact.len();
        }
        assert!(checked > 0, "no trees checked at level {level}");
    }
}

#[test]
fn test_lod_non_air_only_covers_interior() {
    let lod = flat_lod(8);
    let cells = lod.non_air();
    assert_eq!(16 * 16 * 8, cells.len());
    assert!(cells
        .iter()
        .all(|(c, _)| c.min_element() >= 0 && c.max_element() < 16));
}

fn block_at(regions: &Arc<RegionStore>, x: i64, y: i64, z: i64) -> BlockType {
    let coord = ChunkCoordinate(I64Vec3::new(x, y, z).div_euclid(I64Vec3::splat(16)));
    generate_chunk(regions.clone(), coord).get_block_at(U16Vec3::new(
        x.rem_euclid(16) as u16,
        y.rem_euclid(16) as u16,
        z.rem_euclid(16) as u16,
    ))
}

// Cell centres of the test world's map where `kind` water flows or sits, and the sample there.
fn wet_columns(kind: ochre_base::terrain_source::WaterKind) -> Vec<(i64, i64)> {
    use ochre_base::terrain_source::TerrainSource;
    let terrain = ochre_world::worldmap::test_terrain(5);
    let map = terrain.map();
    let mut found = Vec::new();
    'cells: for cz in 0..map.dim() {
        for cx in 0..map.dim() {
            let (x, z) = map.cell_centre(cx, cz);
            let (x, z) = (x.round(), z.round());
            let sample = terrain.sample(x, z);
            let depth = sample.water_surface - sample.height as f32;
            if sample.water_kind == kind
                && sample.water_surface > WATER_LEVEL as f32 + 3.0
                && depth > 2.0
            {
                found.push((x as i64, z as i64));
                if found.len() >= 6 {
                    break 'cells;
                }
            }
        }
    }
    found
}

#[test]
fn test_lakes_and_rivers_above_the_sea_are_filled_to_their_own_surface() {
    use ochre_base::terrain_source::{TerrainSource, WaterKind};
    let terrain = ochre_world::worldmap::test_terrain(5);
    let regions = Arc::new(RegionStore::new(terrain.clone()));
    for kind in [WaterKind::Lake, WaterKind::River] {
        let columns = wet_columns(kind);
        assert!(
            !columns.is_empty(),
            "no {kind:?} above the sea on the test world"
        );
        for (x, z) in columns {
            let sample = terrain.sample(x as f64, z as f64);
            // the column's own water, at whole blocks, as chunk generation takes it
            let top = regions.sampler().water_top(x, z);
            let ground = regions.get_height(x, z).round() as i64;
            assert!(
                top > ground,
                "{kind:?} at ({x}, {z}): water {top} over ground {ground}"
            );
            assert!(top > WATER_LEVEL, "{sample:?}");
            let water = |y| block_at(&regions, x, y, z);
            assert_eq!(BlockType::Air, water(top + 1), "{kind:?} at ({x}, {z})");
            // the top layer may be ice in the cold, but never air, and under it is water
            assert!(matches!(water(top - 1), BlockType::Water | BlockType::Ice));
            assert_eq!(BlockType::Water, water(top - 1 - 4.min(top - ground - 1)));
            // a bed of the ground round it (`fresh_bed`), or water where a gorge's cliffs were cut open under it and flooded
            assert!(
                water(ground - 1).is_opaque() || water(ground - 1) == BlockType::Water,
                "the bed at ({x}, {z}) is {:?}",
                water(ground - 1)
            );
        }
    }
}

#[test]
fn test_ground_well_under_a_lake_is_under_its_water() {
    use ochre_base::terrain_source::TerrainSource;
    // seed 10 has mountain lakes whose terraces were cut dry pits into their floors
    for seed in [5, 10] {
        let terrain = ochre_world::worldmap::test_terrain(seed);
        let map = terrain.map();
        let mut dry = Vec::new();
        let lake = |cx: usize, cz: usize| map.water_kind[map.index(cx, cz)] == 2;
        for cz in 1..map.dim() - 1 {
            for cx in 1..map.dim() - 1 {
                // a lake cell with lake all round it
                if !(cz - 1..=cz + 1).all(|z| (cx - 1..=cx + 1).all(|x| lake(x, z))) {
                    continue;
                }
                let (x, z) = map.cell_centre(cx, cz);
                let level = map.water_surface[map.index(cx, cz)];
                let sample = terrain.sample(x, z);
                if (map.height_at(x, z) as f64) < level as f64
                    && sample.height < level.floor() as f64 - 1.0
                    && sample.water_kind == ochre_base::terrain_source::WaterKind::None
                {
                    dry.push((x, z, sample.height, level));
                }
            }
        }
        assert!(dry.is_empty(), "seed {seed}: dry ground in lakes {dry:?}");
    }
}

#[test]
fn test_soil_thins_from_flat_ground_to_bare_rock_on_steep_ground() {
    assert_eq!(SOIL_DEPTH, soil_depth(0.0));
    assert_eq!(1, soil_depth(1.2));
    let top = 600;
    assert_eq!(
        BlockType::Rock,
        under_grass(BlockType::Grass, top - 2, top, 1, false, soil_depth(1.2), 0)
    );
    assert_eq!(
        BlockType::Dirt,
        under_grass(BlockType::Grass, top - 2, top, 1, false, soil_depth(0.0), 0)
    );
}

#[test]
fn test_ground_is_soil_over_stone_over_compacted_stone_by_depth_below_the_surface() {
    use super::terrain::COMPACTED_DEPTH;
    let top = 600;
    let at = |block, depth| under_grass(block, top - 1 - depth, top, 1, false, SOIL_DEPTH, 0);
    assert_eq!(BlockType::Grass, at(BlockType::Grass, 0));
    for depth in 1..SOIL_DEPTH {
        assert_eq!(
            BlockType::Dirt,
            at(BlockType::Grass, depth),
            "depth {depth}"
        );
    }
    for depth in 1..SAND_DEPTH {
        assert_eq!(BlockType::Sand, at(BlockType::Sand, depth));
    }
    for depth in SOIL_DEPTH..COMPACTED_DEPTH {
        assert_eq!(
            BlockType::Rock,
            at(BlockType::Grass, depth),
            "depth {depth}"
        );
        if depth >= SAND_DEPTH {
            assert_eq!(BlockType::Rock, at(BlockType::Sand, depth));
        }
        assert_eq!(BlockType::Rock, at(BlockType::Rock, depth));
    }
    for depth in [COMPACTED_DEPTH, COMPACTED_DEPTH + 1, 40] {
        for block in [
            BlockType::Grass,
            BlockType::Sand,
            BlockType::Rock,
            BlockType::Snow,
        ] {
            assert_eq!(
                BlockType::CompactedRock,
                at(block, depth),
                "{block:?} {depth}"
            );
        }
    }
    // water is never packed, and wide cells keep to the surface's block
    assert_eq!(BlockType::Water, at(BlockType::Water, 9));
    // snow lies deep: full blocks of it, stone after, and packed stone only under that
    let snow_at = |block, depth| under_grass(block, top - 1 - depth, top, 1, true, SOIL_DEPTH, 0);
    for depth in 1..super::terrain::SNOW_DEPTH {
        assert_eq!(
            BlockType::Snow,
            snow_at(BlockType::Snow, depth),
            "depth {depth}"
        );
    }
    assert_eq!(
        BlockType::Rock,
        snow_at(BlockType::Snow, super::terrain::SNOW_DEPTH)
    );
    assert_eq!(
        BlockType::CompactedRock,
        snow_at(BlockType::Rock, super::terrain::SNOW_COMPACTED_DEPTH)
    );
    assert_eq!(
        BlockType::Grass,
        under_grass(BlockType::Grass, top - 20, top, 4, false, SOIL_DEPTH, 0)
    );
}

#[test]
fn test_compacted_stone_follows_the_local_surface_not_a_fixed_height() {
    use super::terrain::COMPACTED_DEPTH;
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let terrain = ochre_world::worldmap::test_terrain(5);
    let map = terrain.map();
    // dry cells a few blocks or a few hundred above the sea, spread over the world
    let columns: Vec<(i64, i64)> = (0..map.dim())
        .step_by(37)
        .flat_map(|cz| (0..map.dim()).step_by(41).map(move |cx| (cx, cz)))
        .filter(|&(cx, cz)| {
            let i = map.index(cx, cz);
            map.water_kind[i] == 0 && map.height[i] > WATER_LEVEL as f32 + 6.0
        })
        .map(|(cx, cz)| {
            let (x, z) = map.cell_centre(cx, cz);
            (x as i64, z as i64)
        })
        .collect();
    let mut seen = 0;
    for (x, z) in columns {
        // cliffs are cut into steep ground, which moves its surface from the height
        let (gx, gz) = regions.sampler().gradient(x, z);
        if ochre_base::relief::cliff_reach(gx.abs() + gz.abs()) > 0.0 {
            continue;
        }
        seen += 1;
        let ground = regions.get_height(x, z).round() as i64;
        let below = |depth: i64| block_at(&regions, x, ground - 1 - depth, z);
        // under snow the packed stone begins deeper, beneath the snow's own depth
        let packed = if below(0) == BlockType::Snow {
            super::terrain::SNOW_COMPACTED_DEPTH
        } else {
            COMPACTED_DEPTH
        };
        assert!(
            below(packed).is_compacted(),
            "({x}, {z}) at {ground}: {:?}",
            below(packed)
        );
        assert!(!below(packed - 1).is_compacted());
    }
    assert!(seen >= 2, "only {seen} dry columns found");
}

// Blocks in the chunk at (`cx`, `cy`, `cz`) with air under solid in a column, and how many columns are cliff-steep.
fn overhangs(regions: &Arc<RegionStore>, (cx, cy, cz): (i64, i64, i64)) -> (usize, usize) {
    use ochre_base::relief::cliff_reach;
    let chunk = super::chunk::generate_chunk_with(
        regions.clone(),
        ChunkCoordinate(I64Vec3::new(cx, cy, cz)),
        super::chunk::ChunkPasses {
            cliffs: true,
            trees: false,
            flora: false,
        },
    );
    let mut heights = regions.sampler();
    let (mut caves, mut steep) = (0, 0);
    for x in 0..16u16 {
        for z in 0..16u16 {
            let (wx, wz) = (cx * 16 + x as i64, cz * 16 + z as i64);
            let (gx, gz) = heights.gradient(wx, wz);
            steep += (cliff_reach(gx.abs() + gz.abs()) > 0.0) as usize;
            for y in 0..15u16 {
                let (below, above) = (
                    chunk.get_block_at(U16Vec3::new(x, y, z)),
                    chunk.get_block_at(U16Vec3::new(x, y + 1, z)),
                );
                caves += (below == BlockType::Air && super::terrain::is_solid(above)) as usize;
            }
        }
    }
    (caves, steep)
}

#[test]
fn test_steep_ground_gets_overhangs_and_gentle_ground_gets_none() {
    let terrain = ochre_world::worldmap::test_terrain(5);
    let regions = Arc::new(RegionStore::new(terrain.clone()));
    let map = terrain.map();
    let (mut steep_caves, mut steep_columns, mut gentle_caves, mut gentle_chunks) = (0, 0, 0, 0);
    'samples: for cz in (0..map.dim()).step_by(3) {
        for cx in (0..map.dim()).step_by(3) {
            let i = map.index(cx, cz);
            if map.water_kind[i] != 0 || map.height[i] < WATER_LEVEL as f32 + 6.0 {
                continue;
            }
            let (x, z) = map.cell_centre(cx, cz);
            let (x, z) = (x as i64, z as i64);
            if steep_columns >= 256 && steep_caves > 0 && gentle_chunks >= 30 {
                break 'samples;
            }
            // find steep candidates from the coarse map, not a full 256-square region per probe
            if gentle_chunks >= 30 {
                let across = (map.height[map.index((cx + 1).min(map.dim() - 1), cz)]
                    - map.height[map.index(cx.saturating_sub(1), cz)])
                .abs()
                    + (map.height[map.index(cx, (cz + 1).min(map.dim() - 1))]
                        - map.height[map.index(cx, cz.saturating_sub(1))])
                    .abs();
                if across < map.config.map_cell as f32 {
                    continue;
                }
            }
            let ground = terrain.height(x as f64, z as f64).round() as i64;
            let at = (
                x.div_euclid(16),
                (ground - 2).div_euclid(16),
                z.div_euclid(16),
            );
            let (caves, steep) = overhangs(&regions, at);
            if steep >= 8 {
                steep_caves += caves;
                steep_columns += steep;
            } else if steep == 0 && gentle_chunks < 30 {
                gentle_caves += caves;
                gentle_chunks += 1;
            }
        }
    }
    assert!(steep_columns > 0, "no steep ground on the test world");
    assert!(
        steep_caves > 0,
        "no overhangs on {steep_columns} steep columns"
    );
    assert_eq!(0, gentle_caves, "overhangs on gentle ground");
}

#[test]
fn test_cliffs_are_cut_before_the_water_fills_what_they_open() {
    use super::terrain::cut_cliff;
    let sea = WATER_LEVEL;
    // ground cut away under the water floods, and over it is open air
    assert_eq!(
        BlockType::Water,
        cut_cliff(BlockType::Rock, false, sea - 10, 0.5, sea)
    );
    assert_eq!(
        BlockType::Air,
        cut_cliff(BlockType::Rock, false, sea + 3, 0.5, sea)
    );
    // the top layer of a frozen sea freezes in the cut too
    assert_eq!(
        BlockType::Ice,
        cut_cliff(BlockType::Rock, false, sea - 1, 0.0, sea)
    );
    // rock builds up into air but never into water, and what was already right is left alone
    assert_eq!(
        BlockType::Rock,
        cut_cliff(BlockType::Air, true, sea + 2, 0.5, sea)
    );
    assert_eq!(
        BlockType::Water,
        cut_cliff(BlockType::Water, true, sea - 2, 0.5, sea)
    );
    assert_eq!(
        BlockType::Grass,
        cut_cliff(BlockType::Grass, true, sea + 2, 0.5, sea)
    );
    assert_eq!(
        BlockType::Water,
        cut_cliff(BlockType::Water, false, sea - 2, 0.5, sea)
    );
}

#[test]
fn test_rivers_leave_no_dry_crevice_beside_their_water() {
    use ochre_base::terrain_source::{TerrainSource, WaterKind};
    let terrain = ochre_world::worldmap::test_terrain(5);
    let columns = wet_columns(WaterKind::River);
    assert!(
        !columns.is_empty(),
        "no river above the sea on the test world"
    );
    let mut checked = 0;
    for (x, z) in columns {
        let here = terrain.sample(x as f64, z as f64);
        // across the river each way: dry ground beside the water is never lower than the water
        for (dx, dz) in [(1i64, 0i64), (0, 1)] {
            for step in 1..=24 {
                for sign in [-1i64, 1] {
                    let (px, pz) = (x + sign * dx * step, z + sign * dz * step);
                    let s = terrain.sample(px as f64, pz as f64);
                    if s.water_kind == ochre_base::terrain_source::WaterKind::None
                        && s.height < here.water_surface as f64 - 1.5
                    {
                        // lower than this river's water: only fine if it has fallen away downhill, out of the banks, more than a bank wide
                        let near_water = (1..=3).any(|k| {
                            let q = terrain
                                .sample((px - sign * dx * k) as f64, (pz - sign * dz * k) as f64);
                            q.water_kind != ochre_base::terrain_source::WaterKind::None
                                && s.height < q.water_surface as f64 - 1.5
                        });
                        assert!(
                            !near_water,
                            "dry ground {:.1} lower than water {:.1} right beside it at ({px}, {pz})",
                            s.height, here.water_surface
                        );
                    }
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 100);
}

#[test]
fn test_a_stream_joining_a_river_has_water_all_the_way_to_it() {
    use ochre_base::terrain_source::{TerrainSource, WaterKind};
    use ochre_world::worldmap::{WaterCell, NEIGHBOURS, NO_FLOW};
    // bigger worlds than the usual test one, which has too few rivers that join
    let (mut checked, mut broken) = (0, Vec::new());
    for seed in [7, 1, 2] {
        let terrain = ochre_world::worldmap::WorldTerrain::new(std::sync::Arc::new(
            ochre_world::worldmap::generate(
                seed,
                ochre_world::worldmap::WorldConfig {
                    size: 32_768,
                    map_cell: 32,
                    ..Default::default()
                },
            ),
        ));
        let map = terrain.map();
        let dim = map.dim();
        for i in 0..map.height.len() {
            if map.water_kind[i] != WaterCell::River as u8 || map.down[i] == NO_FLOW {
                continue;
            }
            let (dx, dz) = NEIGHBOURS[map.down[i] as usize];
            let j = ((i / dim) as i64 + dz) as usize * dim + ((i % dim) as i64 + dx) as usize;
            // a stream a few blocks wide running into a river at least twice as wide
            let joins = map.water_kind[j] == WaterCell::River as u8
                && (4.0..25.0).contains(&map.river_width[i])
                && map.river_width[j] >= 1.4 * map.river_width[i]
                && map.river_width[j] < 80.0;
            if !joins || map.water_surface[j] < WATER_LEVEL as f32 + 3.0 {
                continue;
            }
            // the water in a window round the junction, as one piece or two
            let (cx, cz) = map.cell_centre(j % dim, j / dim);
            let (cx, cz) = (cx as i64, cz as i64);
            let wet = |x: i64, z: i64| {
                let s = terrain.sample(x as f64, z as f64);
                s.water_kind == WaterKind::River
            };
            let span = 56;
            let mut seen = std::collections::HashSet::new();
            let mut pieces = 0;
            for z in -span..=span {
                for x in -span..=span {
                    if !wet(cx + x, cz + z) || seen.contains(&(x, z)) {
                        continue;
                    }
                    pieces += 1;
                    let mut stack = vec![(x, z)];
                    seen.insert((x, z));
                    while let Some((px, pz)) = stack.pop() {
                        for (nx, nz) in [(px + 1, pz), (px - 1, pz), (px, pz + 1), (px, pz - 1)] {
                            if nx.abs() <= span
                                && nz.abs() <= span
                                && !seen.contains(&(nx, nz))
                                && wet(cx + nx, cz + nz)
                            {
                                seen.insert((nx, nz));
                                stack.push((nx, nz));
                            }
                        }
                    }
                }
            }
            checked += 1;
            // the junction's cell must be wet, or there is no river there to join
            if pieces > 1 {
                broken.push((cx, cz, pieces));
            }
            if checked >= 12 {
                break;
            }
        }
    }
    assert!(checked >= 2, "only {checked} junctions on the test world");
    assert!(
        broken.len() * 4 <= checked,
        "water in pieces at {} of {checked} junctions: {broken:?}",
        broken.len()
    );
}

#[test]
fn test_meadow_clings_to_a_mountain_face_where_the_rest_is_rock() {
    use ochre_base::biome::Biomes;
    let biomes = Biomes::new(1);
    let mountain = biomes.with_modifier(
        ochre_base::biome::BiomeId::BASE,
        ochre_base::biome::Modifier::Mountain,
    );
    let above = WATER_LEVEL + 110;
    let steep = 4.4;
    let (mut meadow, mut rock) = (0, 0);
    for i in 0..3000 {
        let (x, z) = (i * 23, i * 7 - 9000);
        let gradient = super::terrain::block_gradient(&biomes, mountain, 1, x, z, steep, 0.0);
        let top = super::terrain::column_block(
            &biomes,
            mountain,
            above - 1,
            above,
            gradient,
            0.55,
            WATER_LEVEL,
        );
        match top {
            BlockType::Grass => {
                meadow += 1;
                assert!(
                    gradient < steep,
                    "a meadow that is no gentler than the face"
                );
            }
            BlockType::Rock => rock += 1,
            other => panic!("{other:?} on a mountain face"),
        }
    }
    // a third or so is meadow, in patches, the rest rock
    assert!(meadow > 500 && rock > 1000, "{meadow} meadow, {rock} rock");
    // a mountain over a climate biome with its own ground has no meadow patches
    let taiga = biomes.with_modifier(
        biomes.id("taiga").unwrap(),
        ochre_base::biome::Modifier::Mountain,
    );
    for i in 0..500 {
        let gradient =
            super::terrain::block_gradient(&biomes, taiga, 1, i * 23, i * 7 - 9000, steep, 0.0);
        assert!(
            gradient >= steep * 0.5,
            "{gradient}: a meadow on a taiga mountain"
        );
    }
    // gentle ground and other biomes are left as they were
    let gentle = super::terrain::block_gradient(&biomes, mountain, 1, 5, 5, 0.3, 0.0);
    assert!(gentle <= 0.3, "{gentle}: eased, or at most an outcrop");
    let plain = ochre_base::biome::BiomeId::BASE;
    assert_eq!(
        steep,
        super::terrain::block_gradient(&biomes, plain, 1, 5, 5, steep, 0.0)
    );
    // a valley wall makes ground that much steeper to choose blocks by, and none makes none
    let walled = super::terrain::block_gradient(&biomes, plain, 1, 5, 5, 1.5, 1.0);
    assert!((walled - 2.5).abs() < 1e-9, "{walled}");
    assert_eq!(1.5, super::terrain::block_gradient(&biomes, plain, 1, 5, 5, 1.5, 0.0));
}

#[test]
fn test_pines_grow_on_a_mountains_lower_slopes() {
    use crate::generate::trees::{trees_near, TreeKind};
    // the first small world with a stretch of mountain in the upper conifer belt: which one has it moves with the ground
    let (terrain, belt) = (1..16u32)
        .find_map(|seed| {
            let terrain = ochre_world::worldmap::WorldTerrain::new(std::sync::Arc::new(
                ochre_world::worldmap::generate(
                    seed,
                    ochre_world::worldmap::WorldConfig {
                        size: 32_768,
                        map_cell: 32,
                        ..Default::default()
                    },
                ),
            ));
            let map = terrain.map().clone();
            let dim = map.dim();
            // the middle of a stretch of mountain in the upper conifer belt, whatever the climate
            let belt = (20..dim - 20)
                .step_by(5)
                .flat_map(|z| (20..dim - 20).step_by(5).map(move |x| (x, z)))
                .find(|&(x, z)| {
                    (-4..=4).step_by(2).all(|dz| {
                        (-4..=4).step_by(2).all(|dx| {
                            let i = map.index((x as i64 + dx) as usize, (z as i64 + dz) as usize);
                            map.landform[i] == ochre_world::worldmap::landform::MOUNTAIN
                                && (130.0..260.0).contains(&(map.height[i] - WATER_LEVEL as f32))
                                && map.water_kind[i] == 0
                        })
                    })
                })?;
            Some((terrain, belt))
        })
        .expect("no mountain slopes in the conifer belt");
    let map = terrain.map().clone();
    let belt = Some(belt);
    let (cx, cz) = belt.expect("no mountain slopes in the conifer belt");
    let (x, z) = map.cell_centre(cx, cz);
    let regions = RegionStore::new(std::sync::Arc::new(terrain));
    let mut heights = regions.sampler();
    let min = bevy::math::I64Vec2::new(x as i64 - 250, z as i64 - 250);
    let trees = trees_near(regions.terrain(), &mut heights, min, min + 500);
    let pines = trees.iter().filter(|t| t.kind == TreeKind::Pine).count();
    assert!(trees.len() > 20, "only {} trees on the slope", trees.len());
    // 45%: the lie of the land thins some cells and not others, which moved this window from over half to 47%
    assert!(
        pines * 20 > trees.len() * 9,
        "{pines} of {} trees are pines",
        trees.len()
    );
}

#[test]
fn test_no_pines_stand_in_savanna_jungle_or_desert() {
    use crate::generate::trees::{trees_near, TreeKind};
    use ochre_world::worldmap::climate::BiomeKind;
    let terrain = ochre_world::worldmap::WorldTerrain::new(std::sync::Arc::new(
        ochre_world::worldmap::generate(1, ochre_world::worldmap::WorldConfig::default()),
    ));
    let map = terrain.map().clone();
    let dim = map.dim();
    let regions = RegionStore::new(std::sync::Arc::new(terrain));
    let mut heights = regions.sampler();
    let (mut hot_trees, mut high_hot_trees, mut places) = (0, 0, 0);
    for kind in [BiomeKind::Savanna, BiomeKind::Jungle] {
        // stretches of this biome well over the sea (cooler air), with windows round them taking in edges where biomes meet
        let spots = (30..dim - 30)
            .step_by(7)
            .flat_map(|z| (30..dim - 30).step_by(7).map(move |x| (x, z)))
            .filter(|&(x, z)| {
                let i = map.index(x, z);
                BiomeKind::from_u8(map.biome[i]) == kind
                    && (110.0..260.0).contains(&(map.height[i] - WATER_LEVEL as f32))
                    && map.water_kind[i] == 0
            });
        for (cx, cz) in spots.step_by(5).take(12) {
            let (x, z) = map.cell_centre(cx, cz);
            let min = bevy::math::I64Vec2::new(x as i64 - 200, z as i64 - 200);
            let trees = trees_near(regions.terrain(), &mut heights, min, min + 400);
            places += 1;
            for tree in &trees {
                let name = regions
                    .terrain()
                    .biomes()
                    .name(heights.biome(tree.base.x, tree.base.z));
                // a mountain's upper slopes grow pines in any climate, so only the lowlands are checked
                let lowland = regions
                    .terrain()
                    .biomes()
                    .modifier(heights.biome(tree.base.x, tree.base.z))
                    == ochre_base::biome::Modifier::None;
                if lowland && matches!(name, Some("savanna" | "jungle" | "desert")) {
                    hot_trees += 1;
                    high_hot_trees += (tree.base.y - WATER_LEVEL >= 110) as usize;
                    assert_ne!(
                        TreeKind::Pine,
                        tree.kind,
                        "a pine in {name:?} at {} ({} over the sea)",
                        tree.base,
                        tree.base.y - WATER_LEVEL
                    );
                }
            }
        }
    }
    assert!(
        places >= 6 && hot_trees > 50 && high_hot_trees > 10,
        "{places} places, {hot_trees} hot trees, {high_hot_trees} high"
    );
}

#[test]
fn test_the_foothills_round_a_range_are_much_rougher_than_the_lowland_far_from_one() {
    let terrain = ochre_world::worldmap::WorldTerrain::new(std::sync::Arc::new(
        ochre_world::worldmap::generate(1, ochre_world::worldmap::WorldConfig::default()),
    ));
    let map = terrain.map().clone();
    let dim = map.dim();
    let (mut near, mut far) = (Vec::new(), Vec::new());
    for cz in (0..dim).step_by(11) {
        for cx in (0..dim).step_by(11) {
            let i = map.index(cx, cz);
            let above = map.height[i] - WATER_LEVEL as f32;
            if !(40.0..200.0).contains(&above)
                || map.water_kind[i] != 0
                || map.landform[i] == ochre_world::worldmap::landform::MOUNTAIN
            {
                continue;
            }
            let range = map.range[i] as f32 / 255.0;
            let (x, z) = map.cell_centre(cx, cz);
            // match the cached f32 height samples directly, as four columns each don't need hundreds of complete regions
            let h = |x, z| terrain.height(x, z) as f32 as f64;
            let (gx, gz) = (
                (h(x + 6.0, z) - h(x - 6.0, z)) / 6.0,
                (h(x, z + 6.0) - h(x, z - 6.0)) / 6.0,
            );
            let gradient = gx.abs() + gz.abs();
            if range > 0.5 {
                near.push(gradient);
            } else if range < 0.05 {
                far.push(gradient);
            }
        }
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    assert!(
        near.len() > 200 && far.len() > 200,
        "{} foothill and {} lowland columns",
        near.len(),
        far.len()
    );
    let (near, far) = (median(&mut near), median(&mut far));
    assert!(
        near > far * 3.0 && near > 0.9,
        "foothills {near} against lowland {far}"
    );
}

#[test]
fn test_lakes_and_the_sea_have_no_open_block_beside_or_under_their_water() {
    use ochre_base::terrain_source::{TerrainSource, WaterKind};
    // still water spills into any air or flora beside or under it once an edit nearby stirs it, see `flow`
    let open = |block: BlockType| block == BlockType::Air || block.is_flora();
    let size = I64Vec3::splat(CHUNK_SIZE as i64);
    use ochre_world::worldmap::{WaterCell, WorldConfig, WorldMap, WorldTerrain};
    let mut lake = WorldMap::empty(
        777,
        WorldConfig {
            size: 2048,
            ..Default::default()
        },
    );
    lake.height.fill(40.0);
    lake.temperature.fill(0.6);
    for z in 16..48 {
        for x in 16..48 {
            let i = lake.index(x, z);
            lake.height[i] = 10.0;
            lake.water_kind[i] = WaterCell::Lake as u8;
            lake.water_surface[i] = 20.0;
        }
    }
    let mut terrains: Vec<_> = [3, 5, 10]
        .map(ochre_world::worldmap::test_terrain)
        .into_iter()
        .collect();
    terrains.push(Arc::new(WorldTerrain::new(Arc::new(lake))));
    let mut covered = [false; 2];
    for terrain in terrains {
        let seed = terrain.seed();
        let regions = Arc::new(RegionStore::new(terrain.clone()));
        let map = terrain.map();
        for kind in [WaterKind::Ocean, WaterKind::Lake] {
            let mut sites = Vec::new();
            'cells: for cz in (0..map.dim()).step_by(3) {
                for cx in (0..map.dim()).step_by(3) {
                    let (x, z) = map.cell_centre(cx, cz);
                    let sample = terrain.sample(x.round(), z.round());
                    if sample.water_kind == kind
                        && sample.water_surface > sample.height as f32 + 1.0
                    {
                        sites.push(I64Vec3::new(
                            x as i64,
                            sample.water_surface as i64,
                            z as i64,
                        ));
                        if sites.len() >= 6 {
                            break 'cells;
                        }
                    }
                }
            }
            // a regional seed may hold no lake: the explicit basin guarantees one, generated seeds exercise real coastlines
            if sites.is_empty() {
                continue;
            }
            covered[(kind == WaterKind::Lake) as usize] = true;
            let mut leaks = Vec::new();
            for site in sites {
                // a box of chunks round a point in the water, reaching its shore unless it's wide
                let centre = site.div_euclid(size);
                let mut chunks = HashMap::new();
                for dx in -3..=3 {
                    for dy in -1..=1 {
                        for dz in -3..=3 {
                            let coord = ChunkCoordinate(centre + I64Vec3::new(dx, dy, dz));
                            if terrain.bounds().layers().contains(&coord.0.y) {
                                chunks.insert(coord.0, generate_chunk(regions.clone(), coord));
                            }
                        }
                    }
                }
                let block_at = |block: I64Vec3| {
                    chunks.get(&block.div_euclid(size)).map(|data: &ChunkData| {
                        data.get_block_at(block.rem_euclid(size).as_u16vec3())
                    })
                };
                for (coord, data) in &chunks {
                    for (local, block) in data.iter_non_air() {
                        let at = *coord * size + local.as_i64vec3();
                        if block == BlockType::Water
                            && terrain.sample(at.x as f64, at.z as f64).water_kind == kind
                            && [
                                I64Vec3::X,
                                I64Vec3::NEG_X,
                                I64Vec3::Z,
                                I64Vec3::NEG_Z,
                                I64Vec3::NEG_Y,
                            ]
                            .into_iter()
                            .any(|side| block_at(at + side).is_some_and(open))
                        {
                            leaks.push(at);
                        }
                    }
                }
            }
            assert!(
                leaks.is_empty(),
                "seed {seed}: {} {kind:?} water blocks open beside or under, {:?}",
                leaks.len(),
                &leaks[..leaks.len().min(5)]
            );
        }
    }
    assert!(
        covered.into_iter().all(|covered| covered),
        "both ocean and lake boundaries must be checked"
    );
}

#[test]
fn test_flowing_water_stands_over_its_floor_and_falls_away_from_its_source() {
    let floor = 4;
    let mut chunk = ChunkData::default();
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            chunk.set_block_at(U16Vec3::new(x, floor, z), BlockType::Rock);
        }
    }
    // a channel's levels, highest beside its source at x = 1
    let channel = 2u16..9;
    for x in channel.clone() {
        chunk.set_flow_at(U16Vec3::new(x, floor + 1, 8), Some(9 - x as u8));
    }
    let meshes = generate_chunk_mesh_with(
        Arc::new(chunk),
        vec![None; 26],
        ChunkCoordinate(I64Vec3::new(0, 30, 0)),
        &test_regions(),
        false);
    let water = meshes.water.expect("the channel's water is drawn");
    let Some(VertexAttributeValues::Float32x3(positions)) =
        water.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("water positions");
    };
    // the highest the water reaches along the line where the block at `x` meets the one before
    let top = |x: u16| {
        positions
            .iter()
            .filter(|p| (p[0] - (x as f32 - 0.5)).abs() < 0.01)
            .map(|p| p[1])
            .fold(f32::MIN, f32::max)
    };
    let floor_top = floor as f32 + 0.5;
    for x in channel.clone() {
        assert!(
            top(x) > floor_top + 0.05,
            "water at x = {x} is down in the floor, top {}",
            top(x)
        );
    }
    assert!(top(channel.start) > top(channel.end - 1) + 0.3);
}

#[test]
fn test_water_falling_down_a_drop_draws_no_side_over_another() {
    use crate::world::World;
    // a channel at y = 8 from still water at x = 0, over a drop of three into a wide basin at y = 5
    let (channel, basin) = (8u16, 5u16);
    let mut data = ChunkData::from_cells(std::iter::repeat_n(
        BlockType::Rock,
        (CHUNK_SIZE as usize).pow(3),
    ));
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            for y in channel + 1..CHUNK_SIZE {
                data.set_block_at(U16Vec3::new(x, y, z), BlockType::Air);
            }
        }
    }
    for x in 2..5 {
        data.set_block_at(U16Vec3::new(x, channel, 8), BlockType::Air);
    }
    for x in 5..15 {
        for z in 6..=10 {
            for y in basin..=channel {
                data.set_block_at(U16Vec3::new(x, y, z), BlockType::Air);
            }
        }
    }
    data.set_flow_at(U16Vec3::new(0, channel, 8), None);
    let mut world = World::with_terrain(1, 16, ochre_world::worldmap::test_terrain(1));
    world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), data);
    // the dam between the still water and the channel
    world.set_block_at(I64Vec3::new(1, channel as i64, 8), BlockType::Air);
    assert!((1..100).any(|_| crate::flow::step(&mut world) == 0));
    for y in basin..channel {
        assert_eq!(
            Some(crate::flow::FALLING),
            world.flow_at(I64Vec3::new(5, y as i64, 8))
        );
    }

    let chunk = world
        .get_chunk_data(ChunkCoordinate(I64Vec3::ZERO))
        .unwrap();
    let meshes = generate_chunk_mesh_with(
        chunk,
        vec![None; 26],
        ChunkCoordinate(I64Vec3::ZERO),
        &test_regions(),
        false);
    let water = meshes.water.expect("the water is drawn");
    let Some(VertexAttributeValues::Float32x3(positions)) =
        water.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("water positions");
    };
    // each upright face by its axis, plane and run across it, in hundredths, with the heights it spans
    type Side = (usize, i32, i32, i32);
    let mut sides: HashMap<Side, Vec<(f32, f32)>> = HashMap::new();
    let Some(Indices::U32(indices)) = water.indices() else {
        panic!("water indices");
    };
    // polygon vertices are pushed together, so a run of triangles under the last's highest vertex is one polygon
    let mut polygons = Vec::new();
    let mut at = 0;
    while at < indices.len() {
        let low = |at: usize| *indices[at..at + 3].iter().min().unwrap() as usize;
        let start = low(at);
        let mut end = start;
        loop {
            end = end.max(*indices[at..at + 3].iter().max().unwrap() as usize + 1);
            at += 3;
            if at >= indices.len() || low(at) >= end {
                break;
            }
        }
        polygons.push((start, end));
    }
    for quad in polygons.iter().filter(|(a, b)| b - a == 4).map(|&(a, b)| &positions[a..b]) {
        for axis in [0, 2] {
            let across = 2 - axis;
            let at = |v: &[f32; 3], a: usize| (v[a] * 100.0).round() as i32;
            if quad.iter().all(|v| at(v, axis) == at(&quad[0], axis)) {
                let (low, high) = quad.iter().fold((i32::MAX, i32::MIN), |(lo, hi), v| {
                    (lo.min(at(v, across)), hi.max(at(v, across)))
                });
                let ys = quad.iter().map(|v| v[1]);
                let span = (
                    ys.clone().fold(f32::MAX, f32::min),
                    ys.fold(f32::MIN, f32::max),
                );
                sides
                    .entry((axis, at(&quad[0], axis), low, high))
                    .or_default()
                    .push(span);
            }
        }
    }
    for (side, mut spans) in sides {
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        for pair in spans.windows(2) {
            assert!(
                pair[1].0 >= pair[0].1 - 0.001,
                "water sides overlap at {side:?}: {pair:?}"
            );
        }
    }
}

#[test]
fn test_flowing_water_under_still_water_leaves_the_surface_to_the_still_water() {
    let mut chunk = ChunkData::default();
    // a hole under a river, flooded: still water over falling water
    chunk.set_flow_at(U16Vec3::new(5, 6, 5), None);
    chunk.set_flow_at(U16Vec3::new(5, 5, 5), Some(crate::flow::FALLING));
    // and open to the sky beside it
    chunk.set_flow_at(U16Vec3::new(6, 5, 5), Some(5));
    let source = ChunkNeighbourhood::new(&chunk, &no_neighbors());
    assert!(!source.flowing(5, 5));
    assert!(source.flowing(6, 5));
    assert_eq!(
        Some(5.0 + crate::flow::surface(5)),
        source.water_level(6, 5)
    );
}

#[test]
fn test_stone_is_the_rock_the_geology_has_and_packs_as_that_rock() {
    use super::terrain::rocked;
    let rock = || BlockType::Limestone;
    assert_eq!(BlockType::Limestone, rocked(BlockType::Rock, rock));
    assert_eq!(
        BlockType::CompactedLimestone,
        rocked(BlockType::CompactedRock, rock)
    );
    assert_eq!(BlockType::Dirt, rocked(BlockType::Dirt, || unreachable!()));
    // a world's mountains show more than one rock
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let terrain = regions.terrain();
    let rocks: std::collections::HashSet<_> = (0..4000)
        .map(|i| {
            let (x, z) = (i * 53 - 100_000, (i * 97) % 40_000 - 20_000);
            terrain.rock_column(x, z).at(300) as u8
        })
        .collect();
    assert!(rocks.len() >= 3, "only {rocks:?}");
}

// Closed surface: every edge is shared by two triangles. Vertices match to 0.001 blocks; a T-junction counts as pieces.
fn open_edges(mesh: &Mesh) -> Vec<String> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    let Some(Indices::U32(indices)) = mesh.indices() else {
        panic!("missing indices");
    };
    let key = |i: u32| positions[i as usize].map(|c| (c * 1000.0).round() as i64);
    let mut vertices: Vec<[i64; 3]> = positions
        .iter()
        .map(|p| p.map(|c| (c * 1000.0).round() as i64))
        .collect();
    vertices.sort();
    vertices.dedup();
    // the vertices on the segment `a`..`b` between its ends, in order from `a`
    let along = |a: [i64; 3], b: [i64; 3]| {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let length = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let mut on: Vec<(i64, [i64; 3])> = vertices
            .iter()
            .filter_map(|&v| {
                let e = [v[0] - a[0], v[1] - a[1], v[2] - a[2]];
                let cross = [
                    d[1] * e[2] - d[2] * e[1],
                    d[2] * e[0] - d[0] * e[2],
                    d[0] * e[1] - d[1] * e[0],
                ];
                let t = d[0] * e[0] + d[1] * e[1] + d[2] * e[2];
                (cross == [0, 0, 0] && t > 0 && t < length).then_some((t, v))
            })
            .collect();
        on.sort();
        let mut points = vec![a];
        points.extend(on.into_iter().map(|(_, v)| v));
        points.push(b);
        points
    };
    // each edge's owning triangles, and whether each goes along it from the lesser end
    type Owners = Vec<(usize, bool)>;
    let mut edges: HashMap<([i64; 3], [i64; 3]), Owners> = HashMap::default();
    for (n, t) in indices.chunks(3).enumerate() {
        let corners = [key(t[0]), key(t[1]), key(t[2])];
        if corners[0] == corners[1] || corners[1] == corners[2] || corners[0] == corners[2] {
            continue;
        }
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            for pair in along(corners[a], corners[b]).windows(2) {
                let edge = (pair[0].min(pair[1]), pair[0].max(pair[1]));
                edges.entry(edge).or_default().push((n, pair[0] <= pair[1]));
            }
        }
    }
    // an edge on another triangle's face (as the grass lip's ledge met the wall) is joined; its own triangle doesn't
    let triangles: Vec<[Vec3; 3]> = indices
        .chunks(3)
        .map(|t| {
            t.iter()
                .map(|&i| Vec3::from(positions[i as usize]))
                .collect::<Vec<_>>()
        })
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    let on_a_face = |p: Vec3, own: usize| {
        triangles.iter().enumerate().any(|(n, &[a, b, c])| {
            let normal = (b - a).cross(c - a);
            if n == own {
                return false;
            }
            if normal.length() < 1e-6 || (p - a).dot(normal.normalize()).abs() > 2e-3 {
                return false;
            }
            let side = |u: Vec3, v: Vec3| (v - u).cross(p - u).dot(normal) >= -1e-5;
            side(a, b) && side(b, c) && side(c, a)
        })
    };
    edges
        .iter()
        .filter(|&(&(a, b), owners)| {
            // two triangles going the same way along an edge are one of them wound the wrong way round
            if owners.len() == 2 {
                return owners[0].1 == owners[1].1;
            }
            // three faces on an edge is a fin meeting a wall, as a lip's end against a step: closed
            if owners.len() != 1 {
                return false;
            }
            let (a, b) = (
                Vec3::from(a.map(|c| c as f32 / 1000.0)),
                Vec3::from(b.map(|c| c as f32 / 1000.0)),
            );
            !(0..=4).all(|k| on_a_face(a.lerp(b, k as f32 / 4.0), owners[0].0))
        })
        .map(|(&(a, b), owners)| {
            let show = |p: [i64; 3]| {
                format!(
                    "({:.2}, {:.2}, {:.2})",
                    p[0] as f32 / 1000.0,
                    p[1] as f32 / 1000.0,
                    p[2] as f32 / 1000.0
                )
            };
            format!("{} to {} owners {:?}", show(a), show(b), owners)
        })
        .collect()
}

fn blocks_chunk_of(block: BlockType, blocks: &[(u16, u16, u16)]) -> ChunkData {
    let mut chunk = ChunkData::default();
    for &(x, y, z) in blocks {
        chunk.set_block_at(U16Vec3::new(x, y, z), block);
    }
    chunk
}

#[test]
fn test_surfaces_stay_closed() {
    for (name, blocks) in [
        ("a lone block", vec![(4, 4, 4)]),
        (
            "a slab",
            (4..7)
                .flat_map(|x| (4..7).map(move |z| (x, 4, z)))
                .collect(),
        ),
        (
            "an L",
            vec![(4, 4, 4), (5, 4, 4), (6, 4, 4), (4, 4, 5), (4, 4, 6)],
        ),
        ("a step", vec![(4, 4, 4), (5, 4, 4), (5, 5, 4)]),
        (
            "a pillar by a wall",
            vec![(4, 4, 4), (5, 4, 4), (5, 5, 4), (5, 6, 4), (4, 4, 5)],
        ),
        (
            "stairs",
            vec![
                (4, 4, 4),
                (5, 4, 4),
                (5, 5, 4),
                (6, 4, 4),
                (6, 5, 4),
                (6, 6, 4),
            ],
        ),
        (
            "a plus",
            vec![(5, 4, 5), (4, 4, 5), (6, 4, 5), (5, 4, 4), (5, 4, 6)],
        ),
        (
            "a ring round a pit",
            (4..7)
                .flat_map(|x| (4..7).map(move |z| (x, 4, z)))
                .filter(|&b| b != (5, 4, 5))
                .collect(),
        ),
        // xyz / xxy / xxx from above: x two high, y one, on a floor, and z as high as y, as x, or down at the floor
        ("a staircase corner, z as y", staircase_corner(1)),
        ("a staircase corner, z as x", staircase_corner(2)),
        ("a staircase corner, z lowest", staircase_corner(0)),
        (
            "an inner corner",
            vec![
                (4, 4, 4),
                (5, 4, 4),
                (4, 4, 5),
                (5, 4, 5),
                (5, 5, 4),
                (4, 5, 5),
                (5, 5, 5),
            ],
        ),
        (
            "a trench",
            vec![(4, 4, 4), (5, 4, 4), (6, 4, 4), (4, 5, 4), (6, 5, 4)],
        ),
        (
            "a pit",
            (4..7)
                .flat_map(|x| (4..7).flat_map(move |z| [(x, 4, z), (x, 5, z)]))
                .filter(|&b| b != (5, 5, 5))
                .collect(),
        ),
        (
            "an outer corner of a wall",
            vec![(4, 4, 4), (5, 4, 4), (4, 4, 5), (5, 4, 5), (5, 5, 5)],
        ),
        (
            "a tall pillar on a slab",
            (3..8)
                .flat_map(|x| (3..8).map(move |z| (x, 4, z)))
                .chain((5..8).map(|y| (5, y, 5)))
                .collect(),
        ),
        (
            "a wall",
            (4..9)
                .flat_map(|x| (4..6).map(move |y| (x, y, 4)))
                .collect(),
        ),
        (
            "a terraced slope",
            (4..8)
                .flat_map(|x| (4..4 + (x - 3)).map(move |y| (x, y, 4)))
                .collect(),
        ),
    ] {
        // stone is cut back, and grass is left sharp
        for block in [BlockType::Rock, BlockType::Grass] {
            let mesh = mesh_chunk(blocks_chunk_of(block, &blocks)).opaque;
            let open = open_edges(&mesh);
            assert!(
                open.is_empty(),
                "{name} of {block:?} has open edges:\n{}",
                open.join("\n")
            );
        }
    }
}

// Columns for corner xyz / xxy / xxx, rows along z, columns along x, on a stone floor: x two high, y one, z `z_height`.
fn staircase_corner(z_height: u16) -> Vec<(u16, u16, u16)> {
    let height = |row: usize, col: usize| match (row, col) {
        (0, 0) | (1, 0) | (1, 1) | (2, 0) | (2, 1) | (2, 2) => 2,
        (0, 1) | (1, 2) => 1,
        _ => z_height,
    };
    let mut blocks = Vec::new();
    for row in 0..3 {
        for col in 0..3 {
            blocks.push((4 + col as u16, 3, 4 + row as u16));
            for y in 0..height(row, col) {
                blocks.push((4 + col as u16, 4 + y, 4 + row as u16));
            }
        }
    }
    blocks
}

// 3x3 column heights, row by row, same under turns and flips of the square, so only the smallest of each set is meshed.
fn canonical_grid(heights: [u8; 9]) -> [u8; 9] {
    let mut best = heights;
    for flip in [false, true] {
        for turns in 0..4 {
            let mut grid = [0; 9];
            for row in 0..3 {
                for col in 0..3 {
                    let (mut r, mut c) = (row, if flip { 2 - col } else { col });
                    for _ in 0..turns {
                        (r, c) = (c, 2 - r);
                    }
                    grid[row * 3 + col] = heights[r * 3 + c];
                }
            }
            best = best.min(grid);
        }
    }
    best
}

// The blocks of a 3x3 of columns `heights` high (0 to 2) on a floor, each block's kind picked from its place.
fn grid_chunk(heights: [u8; 9], kind: impl Fn(usize, usize, u16) -> BlockType) -> ChunkData {
    let mut chunk = ChunkData::default();
    for row in 0..3 {
        for col in 0..3 {
            for y in 0..=heights[row * 3 + col] as u16 {
                let at = U16Vec3::new(4 + col as u16, 3 + y, 4 + row as u16);
                chunk.set_block_at(at, kind(row, col, y));
            }
        }
    }
    chunk
}

// Calls `check` with the kind, heights and mesh of every 3x3 height arrangement up to symmetry: stone, grass, mixed.
fn for_each_column_mesh(mut check: impl FnMut(&str, [u8; 9], &Mesh)) {
    let regions = test_regions();
    let coord = ChunkCoordinate(I64Vec3::new(0, 40, 0));
    let mesh = |chunk: ChunkData| {
        generate_chunk_mesh_with(Arc::new(chunk), vec![None; 26], coord, &regions, false).opaque
    };
    let mut grids = Vec::new();
    for n in 0..3usize.pow(9) {
        let mut heights = [0u8; 9];
        let mut rest = n;
        for h in &mut heights {
            *h = (rest % 3) as u8;
            rest /= 3;
        }
        if canonical_grid(heights) == heights {
            grids.push(heights);
        }
    }
    for (i, &heights) in grids.iter().enumerate() {
        let hash = |row: usize, col: usize, y: u16| {
            let h = (i as u32 + 1)
                .wrapping_mul(0x9E37_79B1)
                .wrapping_add((row * 3 + col) as u32 * 0x85EB_CA77)
                .wrapping_add(y as u32 * 0xC2B2_AE3D);
            (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D) >> 7
        };
        type Kind<'a> = Box<dyn Fn(usize, usize, u16) -> BlockType + 'a>;
        let kinds: [(&str, Kind); 3] = [
            ("stone", Box::new(|_, _, _| BlockType::Rock)),
            ("grass", Box::new(|_, _, _| BlockType::Grass)),
            (
                "mixed",
                Box::new(move |r, c, y| {
                    if hash(r, c, y) & 1 == 0 {
                        BlockType::Rock
                    } else {
                        BlockType::Grass
                    }
                }),
            ),
        ];
        for (name, kind) in &kinds {
            if *name == "mixed" && i % 6 != 0 {
                continue;
            }
            check(name, heights, &mesh(grid_chunk(heights, kind)));
        }
    }
}

// Every 3x3 height arrangement up to symmetry, in stone, grass and a mix, meshes a closed surface: no sky-visible hole.
#[test]
fn test_no_arrangement_of_columns_leaves_a_hole() {
    let mut failures: Vec<(u32, String, &str)> = Vec::new();
    let mut meshed = 0;
    for_each_column_mesh(|name, heights, mesh| {
        meshed += 1;
        let open = open_edges(mesh);
        if !open.is_empty() {
            let weight = heights.iter().map(|&h| h as u32).sum::<u32>();
            let name: &'static str = ["stone", "grass", "mixed"]
                .into_iter()
                .find(|k| *k == name)
                .unwrap();
            failures.push((
                weight,
                format!(
                    "{name} {:?}: {} open:\n    {}",
                    heights,
                    open.len(),
                    open.join("\n    ")
                ),
                name,
            ));
        }
    });
    failures.sort();
    let by_kind = |k: &str| failures.iter().filter(|f| f.2 == k).count();
    assert!(
        failures.is_empty(),
        "{} of {meshed} meshes have holes ({} stone, {} grass, {} mixed), the simplest:\n{}",
        failures.len(),
        by_kind("stone"),
        by_kind("grass"),
        by_kind("mixed"),
        ["stone", "grass", "mixed"]
            .iter()
            .flat_map(|k| failures.iter().filter(move |f| f.2 == *k).take(3))
            .map(|f| f.1.clone())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn ice_steps_ramp_and_close_for_all_small_column_arrangements() {
    let mut raised = false;
    for n in 0..3usize.pow(9) {
        let mut heights = [0u8; 9];
        let mut rest = n;
        for h in &mut heights {
            *h = (rest % 3) as u8;
            rest /= 3;
        }
        if canonical_grid(heights) != heights { continue; }
        let mesh = mesh_chunk(grid_chunk(heights, |_, _, _| BlockType::Ice)).opaque;
        let open = open_edges(&mesh);
        assert!(open.is_empty(), "ice {heights:?}: {}", open.join("\n"));
        if let Some(VertexAttributeValues::Float32x3(positions)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            raised |= positions.iter().any(|p| {
                let y = p[1] - 0.5 - RAMP;
                (y - y.round()).abs() < 0.001
            });
        }
    }
    assert!(raised, "no ramp vertices on ice");
}

#[test]
fn ice_and_snow_terraces_have_no_open_edges() {
    for heights in [[0, 1, 2, 0, 1, 2, 0, 1, 2], [2, 1, 0, 1, 0, 1, 0, 1, 2]] {
        let chunk = grid_chunk(heights, |_, col, _| {
            if col == 0 { BlockType::Snow } else { BlockType::Ice }
        });
        let mesh = mesh_chunk(chunk).opaque;
        let open = open_edges(&mesh);
        assert!(open.is_empty(), "{heights:?}: {}", open.join("\n"));
    }
}

// Rolling ground in layers, with its own surface block in each column, as the game's is: no hole in it.
#[test]
fn test_layered_ground_with_mixed_surfaces_leaves_no_hole() {
    let mut seed = 777u32;
    let mut rand = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 16) as u16
    };
    let surfaces = [
        BlockType::Grass,
        BlockType::Sand,
        BlockType::Rock,
        BlockType::Dirt,
        BlockType::Snow,
    ];
    let mut holes = Vec::new();
    for range in [3u16, 4, 6] {
        for trial in 0..4 {
            let mut chunk = ChunkData::default();
            let mixed = trial % 2 == 0;
            for x in 3..13u16 {
                for z in 3..13u16 {
                    let top = 4 + rand() % range;
                    let surface = if mixed {
                        surfaces[rand() as usize % surfaces.len()]
                    } else {
                        BlockType::Grass
                    };
                    for y in 1..=top {
                        let block = if y == top {
                            surface
                        } else if y + 2 >= top {
                            BlockType::Dirt
                        } else {
                            BlockType::Rock
                        };
                        chunk.set_block_at(U16Vec3::new(x, y, z), block);
                    }
                }
            }
            let open = open_edges(&mesh_chunk(chunk).opaque);
            if !open.is_empty() {
                holes.push(format!(
                    "range {range} trial {trial}: {} open, {}",
                    open.len(),
                    open[0]
                ));
            }
        }
    }
    assert!(holes.is_empty(), "{}", holes.join("\n"));
}

// Rolling ground where eight chunks meet, each meshed alone with the rest as neighbours, joined: no hole along seams.
#[test]
fn test_chunk_seams_leave_no_hole() {
    let regions = test_regions();
    let mut seed = 4242u32;
    let mut rand = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 16) as u16
    };
    let surfaces = [
        BlockType::Grass,
        BlockType::Sand,
        BlockType::Rock,
        BlockType::Dirt,
    ];
    let size = CHUNK_SIZE as usize;
    let mut holes = Vec::new();
    for trial in 0..3 {
        // the ground's top and surface at each column of x 8..24, z 8..24, rising over the seam at y 16
        let mut ground = [[(0u16, BlockType::Air); 32]; 32];
        for column in ground[8..24].iter_mut() {
            for cell in column[8..24].iter_mut() {
                *cell = (12 + rand() % 6, surfaces[rand() as usize % surfaces.len()]);
            }
        }
        let chunk_of = |n: usize| ((n & 1) as i32, (n >> 2) as i32, ((n >> 1) & 1) as i32);
        let chunks: Vec<Arc<ChunkData>> = (0..8)
            .map(|n| {
                let (cx, cy, cz) = chunk_of(n);
                let mut chunk = ChunkData::default();
                for x in 0..size {
                    for z in 0..size {
                        let (top, surface) = ground[cx as usize * size + x][cz as usize * size + z];
                        for y in 1..=top as usize {
                            if y / size != cy as usize {
                                continue;
                            }
                            let block = if y == top as usize {
                                surface
                            } else if y + 2 >= top as usize {
                                BlockType::Dirt
                            } else {
                                BlockType::Rock
                            };
                            chunk.set_block_at(
                                U16Vec3::new(x as u16, (y % size) as u16, z as u16),
                                block,
                            );
                        }
                    }
                }
                Arc::new(chunk)
            })
            .collect();
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        for n in 0..8usize {
            let (cx, cy, cz) = chunk_of(n);
            let mut adjacent: Vec<Option<Arc<ChunkData>>> = vec![None; 26];
            for m in (0..8usize).filter(|&m| m != n) {
                let (mx, my, mz) = chunk_of(m);
                adjacent[neighbor_26_index(mx - cx, my - cy, mz - cz)] = Some(chunks[m].clone());
            }
            let coord = ChunkCoordinate(I64Vec3::new(cx as i64, 40 + cy as i64, cz as i64));
            let mesh =
                generate_chunk_mesh_with(chunks[n].clone(), adjacent, coord, &regions, false).opaque;
            let (Some(VertexAttributeValues::Float32x3(p)), Some(Indices::U32(i))) =
                (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.indices())
            else {
                continue;
            };
            let base = positions.len() as u32;
            let offset = Vec3::new(cx as f32, cy as f32, cz as f32) * CHUNK_SIZE as f32;
            positions.extend(p.iter().map(|&p| (Vec3::from(p) + offset).to_array()));
            indices.extend(i.iter().map(|&i| i + base));
        }
        let mut joined = Mesh::new(
            bevy::render::mesh::PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::all(),
        );
        joined.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            VertexAttributeValues::Float32x3(positions),
        );
        joined.insert_indices(Indices::U32(indices));
        let open = open_edges(&joined);
        if !open.is_empty() {
            holes.push(format!("trial {trial}: {} open, {}", open.len(), open[0]));
        }
    }
    assert!(holes.is_empty(), "{}", holes.join("\n"));
}

// the joined meshes of the chunks at x 0, z 0 and z 1, blocks given in world coordinates (x, y, z) of one block type
fn seam_mesh(blocks: &[(u16, u16, u16)], block: BlockType) -> Mesh {
    let regions = test_regions();
    let size = CHUNK_SIZE;
    let chunks: Vec<Arc<ChunkData>> = (0..2u16)
        .map(|cz| {
            let mut chunk = ChunkData::default();
            for &(x, y, z) in blocks {
                if z / size == cz {
                    chunk.set_block_at(U16Vec3::new(x, y, z % size), block);
                }
            }
            Arc::new(chunk)
        })
        .collect();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for n in 0..2usize {
        let mut adjacent: Vec<Option<Arc<ChunkData>>> = vec![None; 26];
        adjacent[neighbor_26_index(0, 0, if n == 0 { 1 } else { -1 })] =
            Some(chunks[1 - n].clone());
        let coord = ChunkCoordinate(I64Vec3::new(0, 40, n as i64));
        let mesh = generate_chunk_mesh_with(chunks[n].clone(), adjacent, coord, &regions, false).opaque;
        let (Some(VertexAttributeValues::Float32x3(p)), Some(Indices::U32(i))) =
            (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.indices())
        else {
            continue;
        };
        let base = positions.len() as u32;
        let offset = Vec3::new(0.0, 0.0, n as f32 * CHUNK_SIZE as f32);
        positions.extend(p.iter().map(|&p| (Vec3::from(p) + offset).to_array()));
        indices.extend(i.iter().map(|&i| i + base));
    }
    let mut joined = Mesh::new(
        bevy::render::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::all(),
    );
    joined.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::Float32x3(positions),
    );
    joined.insert_indices(Indices::U32(indices));
    joined
}

// Every arrangement of heights in a 3x2 of columns across a seam between chunks has a closed surface.
#[test]
fn test_columns_across_a_chunk_seam_leave_no_hole() {
    // columns x 4..7, z 15..17 (across the seam at 15.5), heights 0..2 over a floor at y 3
    let mut failures: Vec<(u32, [u8; 6], usize, String)> = Vec::new();
    for n in 0..3usize.pow(6) {
        let mut heights = [0u8; 6];
        let mut rest = n;
        for h in &mut heights {
            *h = (rest % 3) as u8;
            rest /= 3;
        }
        let mut blocks = Vec::new();
        for col in 0..3u16 {
            for row in 0..2u16 {
                for y in 0..=heights[(row * 3 + col) as usize] as u16 {
                    blocks.push((4 + col, 3 + y, 15 + row));
                }
            }
        }
        for block in [BlockType::Rock, BlockType::Ice] {
            let open = open_edges(&seam_mesh(&blocks, block));
            if !open.is_empty() {
                failures.push((
                    heights.iter().map(|&h| h as u32).sum(),
                    heights,
                    open.len(),
                    format!("{block:?}: {}", open[0]),
                ));
            }
        }
    }
    failures.sort();
    assert!(
        failures.is_empty(),
        "{} arrangements have holes, the simplest:\n{}",
        failures.len(),
        failures
            .iter()
            .take(6)
            .map(|f| format!("{:?}: {} open, {}", f.1, f.2, f.3))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// Ground that climbs a block a step, along an axis or diagonal, in one block, in layers and per column: no hole.
#[test]
fn test_steep_ground_a_block_a_step_leaves_no_hole() {
    let surfaces = [
        BlockType::Grass,
        BlockType::Sand,
        BlockType::Rock,
        BlockType::Dirt,
    ];
    let mut seed = 99u32;
    let mut rand = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 16) as usize
    };
    let mut holes = Vec::new();
    for (a, b) in [
        (1i32, 0i32),
        (0, 1),
        (1, 1),
        (1, -1),
        (2, 1),
        (1, 2),
        (2, 2),
        (2, 0),
    ] {
        for mode in ["stone", "grass-dirt-stone", "random-surfaces"] {
            let mut chunk = ChunkData::default();
            for x in 3..10i32 {
                for z in 3..10i32 {
                    let top = (6 + a * (x - 6) + b * (z - 6)).clamp(1, 14) as u16;
                    let surface = surfaces[rand() % surfaces.len()];
                    for y in 1..=top {
                        let block = match mode {
                            "stone" => BlockType::Rock,
                            "grass-dirt-stone" => {
                                if y == top {
                                    BlockType::Grass
                                } else if y + 2 >= top {
                                    BlockType::Dirt
                                } else {
                                    BlockType::Rock
                                }
                            }
                            _ => {
                                if y == top {
                                    surface
                                } else {
                                    BlockType::Rock
                                }
                            }
                        };
                        chunk.set_block_at(U16Vec3::new(x as u16, y, z as u16), block);
                    }
                }
            }
            let open = open_edges(&mesh_chunk(chunk).opaque);
            if !open.is_empty() {
                holes.push(format!(
                    "slope ({a}, {b}) {mode}: {} open, {}",
                    open.len(),
                    open[0]
                ));
            }
        }
    }
    assert!(holes.is_empty(), "{}", holes.join("\n"));
}

fn tree_meshes(chunk: ChunkData, tree_shapes: bool) -> ChunkMeshes {
    generate_chunk_mesh_with(
        Arc::new(chunk),
        vec![None; 26],
        ChunkCoordinate(I64Vec3::ZERO),
        &RegionStore::new(ochre_world::worldmap::test_terrain(1)),
        tree_shapes)
}

// Grass 4 blocks up over a 12 by 12 square, with a step of grass up beside where the logs go, and `logs` on it.
fn ground_with_logs(logs: &[U16Vec3]) -> ChunkData {
    let mut chunk = ChunkData::default();
    for x in 0..12 {
        for z in 0..12 {
            chunk.set_block_at(U16Vec3::new(x, 3, z), BlockType::Grass);
        }
    }
    chunk.set_block_at(U16Vec3::new(7, 4, 8), BlockType::Grass);
    for &log in logs {
        chunk.set_block_at(log, BlockType::Log);
    }
    chunk
}

fn mesh_positions(mesh: &Mesh) -> Vec<Vec3> {
    mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(VertexAttributeValues::as_float3)
        .unwrap()
        .iter()
        .map(|&p| Vec3::from(p))
        .collect()
}

#[test]
fn trunk_prisms_are_lit_by_the_air_beside_them() {
    let logs = [U16Vec3::new(8, 4, 8), U16Vec3::new(8, 5, 8)];
    let meshes = tree_meshes(ground_with_logs(&logs), true);
    let Some(VertexAttributeValues::Float32x2(lights)) =
        meshes.opaque.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        panic!("no light");
    };
    let at = mesh_positions(&meshes.opaque);
    let ring: Vec<f32> = at
        .iter()
        .zip(lights)
        .filter(|(p, _)| {
            (p.x - 8.0).abs() < 0.7 && (p.z - 8.0).abs() < 0.7 && p.y > 4.4 && p.y < 4.6
        })
        .map(|(_, light)| light[0])
        .collect();
    assert!(
        !ring.is_empty(),
        "no prism vertices at the joint of the two logs"
    );
    assert!(ring.iter().all(|&light| light > 0.6), "trunk lit {ring:?}");
}

#[test]
fn tree_shapes_off_draws_logs_as_cubes() {
    let logs = [U16Vec3::new(8, 4, 8)];
    let cubes = tree_meshes(ground_with_logs(&logs), false);
    let prism = tree_meshes(ground_with_logs(&logs), true);
    // a cube is six faces of four, the prism eight sides and two ends
    assert_ne!(cubes.opaque.count_vertices(), prism.opaque.count_vertices());
}

#[test]
fn logs_that_touch_only_at_a_corner_are_joined_by_prisms_meeting_between_them() {
    let meshes = tree_meshes(
        ground_with_logs(&[U16Vec3::new(4, 4, 4), U16Vec3::new(5, 5, 4)]),
        true,
    );
    let between = Vec3::new(4.5, 4.5, 4.0);
    let meet = mesh_positions(&meshes.opaque)
        .into_iter()
        .filter(|p| p.distance(between) < 0.3)
        .count();
    // the ring each log's reach ends in, halfway to the other, is eight corners from each
    assert!(meet >= 16, "{meet} corners where the two reaches meet");
}

fn leaf_vertices(mesh: &Mesh) -> usize {
    let leaves = BlockType::Leaves.tile(ochre_base::block::Face::Side).atlas_cell();
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("no colours");
    };
    colors.iter().filter(|c| c[3] == leaves).count()
}

#[test]
fn a_surface_leaf_draws_a_card_and_a_leaf_with_no_open_side_draws_nothing() {
    let mut chunk = ground_with_logs(&[]);
    for x in 5..8 {
        for y in 5..8 {
            for z in 5..8 {
                chunk.set_block_at(U16Vec3::new(x, y, z), BlockType::Leaves);
            }
        }
    }
    let cards = leaf_vertices(&tree_meshes(chunk, true).opaque);
    // 26 leaves touch the air and each is one card of a middle and eight rim points; the middle one touches none
    assert_eq!(26 * 7, cards);
}

#[test]
fn leaves_are_drawn_as_cubes_with_tree_shapes_off() {
    let mut chunk = ground_with_logs(&[]);
    chunk.set_block_at(U16Vec3::new(6, 6, 6), BlockType::Leaves);
    let cubes = tree_meshes(chunk, false);
    // one cube of six faces, none of them cards
    assert_eq!(24, leaf_vertices(&cubes.opaque));
}

#[test]
fn a_pines_boughs_are_straight_prisms_from_its_trunk_to_their_tips() {
    use crate::generate::trees::{Tree, TreeKind, TreeTints};
    let tree = Tree::new(TreeKind::Pine, I64Vec3::new(8, 4, 8), 16, 4.5, 0.7, 3);
    let mut chunk = ground_with_logs(&[]);
    tree.plant(I64Vec3::ZERO, 1, (IVec3::ZERO, IVec3::splat(15)), |c, b| {
        chunk.set_block_at(U16Vec3::new(c.x as u16, c.y as u16, c.z as u16), b);
    });
    chunk.trees = TreeTints::new([&tree], I64Vec3::ZERO, 1);
    // the first whorl starts a block above the crown's foot, a quarter of the way up the trunk
    let voxel = IVec3::new(8, 4 + 16 / 4 + 1, 8);
    let look = chunk.trees.look_at(voxel).unwrap();
    let boughs = look.boughs_from(voxel);
    assert_eq!(4, boughs.len());
    let meshes = tree_meshes(chunk, true);
    let at = mesh_positions(&meshes.opaque);
    let off = super::trees_mesh::trunk_offset(&look, voxel.y as f32);
    let start = Vec3::new(8.0 + off.x, voxel.y as f32, 8.0 + off.y);
    for along in boughs {
        let tip = start + along;
        assert!(
            at.iter().any(|p| p.distance(tip) < 0.2),
            "no bough ends at {tip}"
        );
    }
}

#[test]
fn leaves_with_more_leaves_over_them_are_drawn_darker() {
    let mut chunk = ground_with_logs(&[]);
    for y in 5..11 {
        for (x, z) in [(6, 6), (7, 6), (6, 7), (7, 7)] {
            chunk.set_block_at(U16Vec3::new(x, y, z), BlockType::Leaves);
        }
    }
    let meshes = tree_meshes(chunk, true);
    let leaves = BlockType::Leaves.tile(ochre_base::block::Face::Side).atlas_cell();
    let Some(VertexAttributeValues::Float32x4(colors)) =
        meshes.opaque.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("no colours");
    };
    let mean = |wanted: &dyn Fn(f32) -> bool| {
        let shades: Vec<f32> = mesh_positions(&meshes.opaque)
            .iter()
            .zip(colors)
            .filter(|(p, c)| c[3] == leaves && wanted(p.y))
            .map(|(_, c)| c[0] + c[1] + c[2])
            .collect();
        shades.iter().sum::<f32>() / shades.len() as f32
    };
    let (low, high) = (mean(&|y| y < 6.5), mean(&|y| y > 8.5));
    assert!(low < high * 0.9, "low {low}, high {high}");
}

#[test]
fn far_crowns_are_fewer_bigger_cards_as_the_cells_widen_and_stay_over_the_crown() {
    use crate::generate::trees::{Tree, TreeKind};
    for (kind, trunk, spread) in [
        (TreeKind::Broadleaf, 10, 4.5),
        (TreeKind::Broadleaf, 20, 7.0),
        (TreeKind::Pine, 16, 4.5),
    ] {
        let tree = Tree::new(kind, I64Vec3::ZERO, trunk, spread, 0.7, 5);
        let counts: Vec<usize> = [2i64, 4, 8, 16]
            .iter()
            .map(|&cell| {
                let cards = tree.far_cards(2.0 / cell as f64);
                for card in &cards {
                    assert!(card.size >= 1.0 && card.size as f64 <= spread * 2.0 + 2.0, "{kind:?} card {} wide", card.size);
                    assert!(card.centre.x.abs() <= spread * 1.4 && card.centre.z.abs() <= spread * 1.4);
                    assert!(card.centre.y >= 0.0 && card.centre.y <= trunk as f64 + spread + 2.0);
                }
                cards.len()
            })
            .collect();
        assert!(counts.windows(2).all(|w| w[0] >= w[1]) && counts[3] >= 3, "{kind:?} {counts:?}");
    }
}

#[test]
fn far_trees_cost_fewer_vertices_than_the_planted_cubes_they_replace() {
    use crate::generate::trees::{Tree, TreeKind};
    let regions = RegionStore::new(ochre_world::worldmap::test_terrain(1));
    let mut shown = Vec::new();
    for (kind, trunk, spread) in [
        (TreeKind::Broadleaf, 10, 4.5),
        (TreeKind::Pine, 16, 4.5),
        (TreeKind::Broadleaf, 20, 7.0),
        (TreeKind::Pine, 24, 6.0),
    ] {
        for cell in [2i64, 4] {
            let size = 16i32;
            let tree = Tree::new(kind, I64Vec3::new(8 * cell, 0, 8 * cell), trunk, spread, 0.7, 5);
            let mut cells = vec![BlockType::Air; 18 * 18 * 18];
            tree.plant(I64Vec3::ZERO, cell, (IVec3::splat(-1), IVec3::splat(size)), |coord, block| {
                cells[LodBlocks::index(size, coord)] = block;
            });
            let tint = super::buffers::GrassTint::new(regions.terrain(), Vec3::ZERO, 1.0);
            let light = std::cell::LazyCell::new(|| {
                crate::generate::light::SkyLight::new(IVec3::splat(-1), IVec3::splat(18), crate::generate::light::MAX_LIGHT, vec![0; 18 * 18 * 18])
            });
            let count = |boxes: Vec<super::source::TreeBox>, shapes: bool| {
                let lone = LodBlocks {
                    boxes,
                    ..LodBlocks::new(size, cells.clone(), vec![0; 18 * 18], 0, cell)
                };
                let style = MeshStyle { tree_shapes: shapes, ..style(false, false) };
                let m = super::mesh::mesh_blocks(&lone, cell, style, &tint, &light);
                m.opaque.count_vertices()
            };
            let planted = count(Vec::new(), false);
            let far = count(super::lod::far_boxes(&tree, I64Vec3::ZERO, cell), true);
            shown.push(format!("{kind:?} {trunk}/{spread} cell {cell}: planted {planted}, far {far}"));
            // cards cost less than the planted cubes at 2 blocks, and about what a small tree costs at 4, where those are few
            assert!(far < if cell == 2 { planted } else { planted * 3 / 2 }, "{kind:?} {trunk} at {cell}: far {far} vertices, planted {planted}");
        }
    }
    println!("{}", shown.join("\n"));
}

#[test]
fn every_lod_level_draws_the_trees_of_a_forest() {
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let mut heights = regions.sampler();
    let centre = forest_at(5);
    let trees = crate::generate::trees::trees_near(
        regions.terrain(),
        &mut heights,
        centre - bevy::math::I64Vec2::splat(300),
        centre + bevy::math::I64Vec2::splat(300),
    );
    let tree = trees.first().expect("no trees");
    let chunk = tree.base.div_euclid(I64Vec3::splat(16));
    let log = BlockType::Log.tile(ochre_base::block::Face::Side).atlas_cell();
    for level in [1u8, 2, 3] {
        let mut keys = std::collections::HashSet::new();
        for x in -12..12 {
            for z in -12..12 {
                for y in -3..4 {
                    keys.insert(LodKey::containing(ChunkCoordinate(chunk + I64Vec3::new(x, y, z)), level));
                }
            }
        }
        let (mut boxes, mut leaf, mut logs, mut total) = (0, 0, 0, 0);
        for key in keys {
            let Some(m) = super::lod::generate_lod_mesh(regions.clone(), key, None) else { continue };
            boxes += LodBlocks::sample(&regions, key).boxes.len();
            leaf += leaf_vertices(&m.opaque);
            let Some(VertexAttributeValues::Float32x4(colors)) = m.opaque.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!() };
            logs += colors.iter().filter(|c| c[3] == log).count();
            total += m.opaque.count_vertices();
        }
        assert!(boxes > 0 && leaf > 0 && logs > 0, "level {level}: {boxes} boxes, {leaf} leaf and {logs} log vertices of {total}");
    }
}


// Sculpted ground over real terrain, as generated and with a pit dug, closes up across chunks, wound consistently.
#[test]
fn test_sculpted_ground_leaves_no_hole() {
    let regions = test_regions();
    let size = CHUNK_SIZE as i64;
    let mut sampler = regions.sampler();
    let land: Vec<(i64, i64)> = (0..400i64)
        .map(|i| ((i % 20) * 700 - 7000, (i / 20) * 700 - 7000))
        .filter(|&(x, z)| sampler.ground_top(x, z) > WATER_LEVEL + 3)
        .take(8)
        .collect();
    assert!(land.len() >= 6, "too few land spots");
    let mut holes = Vec::new();
    for ((sx, sz), dug) in land.into_iter().flat_map(|spot| [(spot, false), (spot, true)]) {
        let (cx, cz) = (sx.div_euclid(size), sz.div_euclid(size));
        let cy = (sampler.ground_top(sx, sz) - 1).div_euclid(size) - 1;
        let digs: Vec<I64Vec3> = if dug {
            [(0, 0, 1), (1, 0, 2), (1, 1, 1), (4, 2, 1)]
                .into_iter()
                .flat_map(|(x, z, depth)| {
                    let top = sampler.ground_top(sx + x, sz + z) - 1;
                    (0..depth).map(move |d| I64Vec3::new(sx + x, top - d, sz + z))
                })
                .collect()
        } else {
            Vec::new()
        };
        let mut chunks = HashMap::new();
        let mut chunk_at = |c: (i64, i64, i64)| {
            chunks
                .entry(c)
                .or_insert_with(|| {
                    let origin = I64Vec3::new(c.0, c.1, c.2) * size;
                    let mut chunk = generate_chunk(regions.clone(), ChunkCoordinate(I64Vec3::new(c.0, c.1, c.2)));
                    for &block in &digs {
                        let local = block - origin;
                        if local.min_element() >= 0 && local.max_element() < size {
                            chunk.set_block_at(local.as_u16vec3(), BlockType::Air);
                        }
                    }
                    Arc::new(chunk)
                })
                .clone()
        };
        let (mut positions, mut indices) = (Vec::<[f32; 3]>::new(), Vec::<u32>::new());
        let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let mut floor = Vec::new();
        for dx in 0..3 {
            for dz in 0..3 {
                for dy in 0..3 {
                    let c = (cx + dx, cy + dy, cz + dz);
                    let mut adjacent = vec![None; 26];
                    for ax in -1..=1 {
                        for ay in -1..=1 {
                            for az in -1..=1 {
                                if (ax, ay, az) != (0, 0, 0) {
                                    adjacent[neighbor_26_index(ax, ay, az)] =
                                        Some(chunk_at((c.0 + ax as i64, c.1 + ay as i64, c.2 + az as i64)));
                                }
                            }
                        }
                    }
                    let coord = ChunkCoordinate(I64Vec3::new(c.0, c.1, c.2));
                    let meshes = generate_chunk_mesh_with(chunk_at(c), adjacent, coord, &regions, false);
                    let offset = Vec3::new((dx * size) as f32, (dy * size) as f32, (dz * size) as f32);
                    floor.extend(meshes.floor.iter().map(|t| t.map(|p| p + offset)));
                    let mesh = meshes.opaque;
                    let (Some(VertexAttributeValues::Float32x3(p)), Some(Indices::U32(i))) =
                        (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.indices())
                    else {
                        continue;
                    };
                    let base = positions.len() as u32;
                    positions.extend(p.iter().map(|&p| (Vec3::from(p) + offset).to_array()));
                    indices.extend(i.iter().map(|&i| i + base));
                    low = low.min(offset - 0.5);
                    high = high.max(offset + Vec3::splat(size as f32) - 0.5);
                }
            }
        }
        // what stands on the ground finds it within a block of its top block
        if !dug {
            let mut block_at = |y: i64| chunk_at((sx.div_euclid(size), y.div_euclid(size), sz.div_euclid(size))).get_block_at(I64Vec3::new(sx, y, sz).rem_euclid(I64Vec3::splat(size)).as_u16vec3());
            let guess = sampler.ground_top(sx, sz);
            let top_block = (guess - 4..guess + 8).rev().find(|&y| {
                let block = block_at(y);
                block.is_opaque() && !matches!(block, BlockType::Leaves | BlockType::Log)
            });
            if let Some(y) = top_block {
                let top = (y - cy * size) as f32 + 0.5;
                let at = Vec3::new((sx - cx * size) as f32, top, (sz - cz * size) as f32);
                let found = crate::floor::Floor::new(floor).surface(at, 1.0, 1.0).map(|(h, _)| h);
                assert!(found.is_some_and(|f| (f - top).abs() <= 1.0), "{sx},{sz}: no floor near {top}, found {found:?}");
            }
        }
        let mut joined =
            Mesh::new(bevy::render::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::all());
        joined.insert_attribute(Mesh::ATTRIBUTE_POSITION, VertexAttributeValues::Float32x3(positions));
        joined.insert_indices(Indices::U32(indices));
        // the outside of the chunks meshed is cut through the ground, open there and a couple of blocks in
        let inside = |line: &str| {
            let numbers: Vec<f32> = line
                .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .filter_map(|t| t.parse().ok())
                .take(6)
                .collect();
            numbers.len() == 6
                && (0..3).all(|axis| {
                    [low[axis], high[axis]].iter().all(|&wall| {
                        (numbers[axis] - wall).abs() > 2.6 && (numbers[3 + axis] - wall).abs() > 2.6
                    })
                })
        };
        for line in open_edges(&joined).into_iter().filter(|line| inside(line)) {
            let numbers: Vec<f32> = line
                .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                .filter_map(|t| t.parse().ok())
                .take(6)
                .collect();
            let mid = (Vec3::new(numbers[0], numbers[1], numbers[2]) + Vec3::new(numbers[3], numbers[4], numbers[5])) / 2.0;
            let mut near = std::collections::BTreeSet::new();
            for o in [-0.3f32, 0.3] {
                for p in [-0.3f32, 0.3] {
                    for q in [-0.3f32, 0.3] {
                        let b = (mid + Vec3::new(o, p, q)).round().as_i64vec3() + I64Vec3::new(cx, cy, cz) * size;
                        let c = (b.x.div_euclid(size), b.y.div_euclid(size), b.z.div_euclid(size));
                        near.insert(format!("{:?}", chunk_at(c).get_block_at(b.rem_euclid(I64Vec3::splat(size)).as_u16vec3())));
                    }
                }
            }
            // trees are drawn by their own mesher, open where they meet what they stand in
            if !near.iter().any(|b| b == "Log" || b == "Leaves") {
                holes.push(format!("{sx},{sz} dug {dug}: {line} near {near:?}"));
            }
        }
    }
    assert!(holes.is_empty(), "{} open edges, first: {:?}", holes.len(), &holes[..holes.len().min(8)]);
}

// Sculpted LOD nodes over real terrain close up, skirts included, where the node's sides, top and bottom cut ground.
#[test]
fn test_sculpted_lod_nodes_leave_no_hole() {
    let regions = test_regions();
    let mut sampler = regions.sampler();
    let land: Vec<(i64, i64)> = (0..400i64)
        .map(|i| ((i % 20) * 700 - 7000, (i / 20) * 700 - 7000))
        .filter(|&(x, z)| sampler.ground_top(x, z) > WATER_LEVEL + 3)
        .take(4)
        .collect();
    assert!(land.len() >= 3, "too few land spots");
    let mut holes = Vec::new();
    let mut nodes = 0;
    for (sx, sz) in land {
        let chunk = I64Vec3::new(sx, sampler.ground_top(sx, sz) - 1, sz).div_euclid(I64Vec3::splat(CHUNK_SIZE as i64));
        for level in 1..=4u8 {
            let key = LodKey::containing(ChunkCoordinate(chunk), level);
            let Some(meshes) = super::lod::generate_lod_mesh(regions.clone(), key, None) else { continue };
            let mesh = meshes.opaque;
            let (Some(VertexAttributeValues::Float32x3(positions)), Some(VertexAttributeValues::Float32x2(uvs)), Some(Indices::U32(indices))) =
                (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_UV_0), mesh.indices())
            else {
                continue;
            };
            // the sculpted ground alone: cubes, water and trees are drawn open where they meet it
            let sculpted: Vec<u32> = indices.chunks(3).filter(|t| t.iter().all(|&i| uvs[i as usize][0] < -512.0)).flatten().copied().collect();
            if sculpted.is_empty() {
                continue;
            }
            nodes += 1;
            let mut ground = Mesh::new(bevy::render::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::all());
            ground.insert_attribute(Mesh::ATTRIBUTE_POSITION, VertexAttributeValues::Float32x3(positions.clone()));
            ground.insert_indices(Indices::U32(sculpted.clone()));
            let last = CHUNK_SIZE as f32 - 0.5;
            // a vertex's normal and uv, which a skirt's lower corner copies from the vertex on the side it hangs from
            let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL) else { continue };
            let look = |i: usize| (normals[i].map(f32::to_bits), uvs[i].map(f32::to_bits));
            let hung: std::collections::HashSet<_> = (0..positions.len())
                .flat_map(|i| [0usize, 2].into_iter().flat_map(move |axis| [-0.5f32, last].into_iter().map(move |wall| (i, axis, wall))))
                .filter(|&(i, axis, wall)| positions[i][axis] == wall)
                .map(|(i, axis, wall)| (axis, wall.to_bits(), look(i)))
                .collect();
            let in_side = |ps: &[[f32; 3]]| [0, 2].iter().any(|&axis| [-0.5, last].iter().any(|&wall| ps.iter().all(|p| p[axis] == wall)));
            for line in open_edges(&ground) {
                let n: Vec<f32> = line
                    .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                    .filter_map(|t| t.parse().ok())
                    .collect();
                let (a, b) = ([n[0], n[1], n[2]], [n[3], n[4], n[5]]);
                let owner: Vec<[f32; 3]> = sculpted[3 * n[6] as usize..][..3].iter().map(|&i| positions[i as usize]).collect();
                // a side is open only where a skirt ends: no surface triangle open there has a skirt under it; skirts lean in
                let lean = super::lod::MAX_SKIRT_CELLS as f32 * super::nets::SKIRT_LEAN + 0.01;
                let corners: Vec<usize> = sculpted[3 * n[6] as usize..][..3].iter().map(|&i| i as usize).collect();
                let skirt = [0, 2].iter().any(|&axis| {
                    [-0.5, last].iter().any(|&wall| {
                        corners.iter().any(|&i| positions[i][axis] == wall)
                            && corners.iter().all(|&i| {
                                let off = (positions[i][axis] - wall).abs();
                                off == 0.0 || (off <= lean && hung.contains(&(axis, wall.to_bits(), look(i))))
                            })
                    })
                });
                let skirt_end = n.len() == 7 && ((in_side(&[a, b]) && in_side(&owner)) || skirt);
                let past_top_or_bottom = [a[1], b[1]].iter().any(|&y| !(0.5..=last - 1.0).contains(&y));
                if !skirt_end && !past_top_or_bottom {
                    holes.push(format!("level {level} at {sx},{sz}: {line}"));
                }
            }
        }
    }
    assert!(nodes >= 8, "only {nodes} nodes had sculpted ground");
    assert!(holes.is_empty(), "{} open edges, first: {:?}", holes.len(), &holes[..holes.len().min(8)]);
}

#[test]
fn test_a_dig_changes_only_the_sculpted_chunks_meshed_again() {
    let regions = test_regions();
    let size = CHUNK_SIZE as i64;
    let mut sampler = regions.sampler();
    let spots: Vec<(i64, i64)> = (0..400i64)
        .map(|i| ((i % 20) * 700 - 7000, (i / 20) * 700 - 7000))
        // on land, where the dig lands in its chunk's top rows, just under the chunk over it, as digging under a ledge does
        .filter(|&(x, z)| {
            let top = sampler.ground_top(x, z);
            top > WATER_LEVEL + 3 && (top - 3).rem_euclid(size) >= 12
        })
        .take(4)
        .collect();
    assert!(spots.len() == 4, "too few spots");
    for (sx, sz) in spots {
        // the dig two blocks under the ground, as under a ledge, and further into its chunk than a cube's reach
        let dig = I64Vec3::new(sx.div_euclid(size) * size + 10, sampler.ground_top(sx, sz) - 3, sz.div_euclid(size) * size + 10);
        let centre = (dig.x.div_euclid(size), dig.y.div_euclid(size), dig.z.div_euclid(size));
        let meshes_with = |dug: bool| {
            let mut chunks = HashMap::new();
            let mut chunk_at = |c: (i64, i64, i64)| {
                chunks
                    .entry(c)
                    .or_insert_with(|| {
                        let mut chunk = generate_chunk(regions.clone(), ChunkCoordinate(I64Vec3::new(c.0, c.1, c.2)));
                        let local = dig - I64Vec3::new(c.0, c.1, c.2) * size;
                        if dug && local.min_element() >= 0 && local.max_element() < size {
                            chunk.set_block_at(local.as_u16vec3(), BlockType::Air);
                        }
                        Arc::new(chunk)
                    })
                    .clone()
            };
            let mut out = HashMap::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let c = (centre.0 + dx, centre.1 + dy, centre.2 + dz);
                        let mut adjacent = vec![None; 26];
                        for ax in -1..=1 {
                            for ay in -1..=1 {
                                for az in -1..=1 {
                                    if (ax, ay, az) != (0, 0, 0) {
                                        adjacent[neighbor_26_index(ax, ay, az)] = Some(chunk_at((c.0 + ax as i64, c.1 + ay as i64, c.2 + az as i64)));
                                    }
                                }
                            }
                        }
                        let mesh = generate_chunk_mesh_with(chunk_at(c), adjacent, ChunkCoordinate(I64Vec3::new(c.0, c.1, c.2)), &regions, false).opaque;
                        let mut positions: Vec<[i64; 3]> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                            Some(VertexAttributeValues::Float32x3(p)) => p.iter().map(|p| p.map(|v| (v * 1000.0).round() as i64)).collect(),
                            _ => Vec::new(),
                        };
                        positions.sort_unstable();
                        out.insert(c, positions);
                    }
                }
            }
            out
        };
        let (before, after) = (meshes_with(false), meshes_with(true));
        let meshed_again: Vec<(i64, i64, i64)> = crate::remesh::chunks_reading(dig).map(|c| (c.0.x, c.0.y, c.0.z)).collect();
        for (c, positions) in &before {
            if after[c] != *positions {
                assert!(meshed_again.contains(c), "{sx},{sz}: dig at {dig} changed chunk {c:?}, which isn't meshed again");
            }
        }
    }
}

// Ground scrambled into thin walls, lone blocks and diagonal touches: every quad has a diagonal facing solid to air.
#[test]
fn test_sculpted_ground_never_folds_a_triangle() {
    let regions = test_regions();
    let size = CHUNK_SIZE as i64;
    let mut sampler = regions.sampler();
    let land: Vec<(i64, i64)> = (0..400i64)
        .map(|i| ((i % 20) * 700 - 7000, (i / 20) * 700 - 7000))
        .filter(|&(x, z)| sampler.ground_top(x, z) > WATER_LEVEL + 3)
        .take(4)
        .collect();
    assert!(land.len() >= 3, "too few land spots");
    let mut triangles = 0;
    for (sx, sz) in land {
        let (cx, cz) = (sx.div_euclid(size), sz.div_euclid(size));
        let cy = (sampler.ground_top(sx, sz) - 1).div_euclid(size);
        for (salt, share) in [(1u64, 3u64), (2, 5), (3, 2)] {
            let mut chunk = generate_chunk(regions.clone(), ChunkCoordinate(I64Vec3::new(cx, cy, cz)));
            for x in 0..size {
                for y in 0..size {
                    for z in 0..size {
                        let at = U16Vec3::new(x as u16, y as u16, z as u16);
                        let hash = ((x as u64 * 73856093) ^ (y as u64 * 19349663) ^ (z as u64 * 83492791) ^ (salt * 2654435761)).wrapping_mul(0x9E3779B97F4A7C15) >> 40;
                        if hash % 10 < share {
                            let to = if chunk.get_block_at(at).is_opaque() { BlockType::Air } else { BlockType::Rock };
                            chunk.set_block_at(at, to);
                        }
                    }
                }
            }
            let coord = ChunkCoordinate(I64Vec3::new(cx, cy, cz));
            let mut adjacent = vec![None; 26];
            for ax in -1..=1i64 {
                for ay in -1..=1i64 {
                    for az in -1..=1i64 {
                        if (ax, ay, az) != (0, 0, 0) {
                            adjacent[neighbor_26_index(ax as i32, ay as i32, az as i32)] =
                                Some(Arc::new(generate_chunk(regions.clone(), ChunkCoordinate(I64Vec3::new(cx + ax, cy + ay, cz + az)))));
                        }
                    }
                }
            }
            let meshes = generate_chunk_mesh_with(Arc::new(chunk), adjacent, coord, &regions, false);
            triangles += meshes.opaque.indices().map_or(0, |i| i.len() / 3);
        }
    }
    assert!(triangles > 1000, "scrambled ground meshed to only {triangles} triangles");
    let unsound = super::nets::UNSOUND_QUADS.load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(unsound, 0, "{unsound} quads have no diagonal whose triangles face from solid to air");
}

// A chunk draws faces over its own solid blocks: an all-air chunk (never meshed) draws none, rock beside it the wall.
#[test]
fn test_the_wall_against_an_empty_chunk_is_drawn_by_the_rock() {
    let regions = test_regions();
    let mut rock = ChunkData::default();
    for y in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            rock.set_block_at(U16Vec3::new(0, y, z), BlockType::Rock);
        }
    }
    let (rock, empty) = (Arc::new(rock), Arc::new(ChunkData::default()));
    let triangles = |meshes: ChunkMeshes| meshes.opaque.indices().map_or(0, |i| i.len() / 3);
    let mut beside_rock = vec![None; 26];
    beside_rock[neighbor_26_index(1, 0, 0)] = Some(rock.clone());
    let from_empty = triangles(generate_chunk_mesh_with(empty.clone(), beside_rock, ChunkCoordinate(I64Vec3::new(0, 40, 0)), &regions, false));
    let mut beside_empty = vec![Some(empty.clone()); 26];
    beside_empty[neighbor_26_index(-1, 0, 0)] = Some(empty);
    let from_rock = triangles(generate_chunk_mesh_with(rock, beside_empty, ChunkCoordinate(I64Vec3::new(1, 40, 0)), &regions, false));
    assert_eq!(from_empty, 0, "the empty chunk drew {from_empty} triangles");
    assert!(from_rock > 100, "the rock drew only {from_rock} triangles of its wall");
}

#[test]
fn test_no_tree_roots_on_a_banks_sand() {
    use crate::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    use ochre_base::terrain_source::WaterKind;

    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let mut heights = regions.sampler();
    let mut chunks: HashMap<I64Vec3, ChunkData> = HashMap::new();
    let (mut trees_seen, mut on_sand) = (0, Vec::new());
    for kind in [WaterKind::Lake, WaterKind::River] {
        for (x, z) in wet_columns(kind).into_iter().step_by(3).take(40) {
            let centre = I64Vec2::new(x, z);
            for tree in trees_near(regions.terrain(), &mut heights, centre - 40, centre + 40) {
                let under = tree.base - I64Vec3::Y;
                let coord = under.div_euclid(I64Vec3::splat(16));
                let chunk = chunks
                    .entry(coord)
                    .or_insert_with(|| generate_chunk(regions.clone(), ChunkCoordinate(coord)));
                trees_seen += 1;
                if chunk.get_block_at((under - coord * 16).as_u16vec3()) == BlockType::Sand {
                    on_sand.push(tree.base);
                }
            }
        }
    }
    assert!(trees_seen > 50, "only {trees_seen} trees by the water");
    assert!(on_sand.is_empty(), "{} of {trees_seen} trees by the water root on sand, as at {:?}", on_sand.len(), &on_sand[..on_sand.len().min(5)]);
}

#[test]
fn test_lod_level_zero_lays_a_banks_bed_where_full_detail_does() {
    use ochre_base::terrain_source::WaterKind;
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let mut heights = regions.sampler();
    // a bank's and bed's blocks, which fresh water lays from the ground round it (`fresh_bed`)
    let bed = |b: BlockType| matches!(b, BlockType::Sand | BlockType::Gravel | BlockType::Dirt);
    let (mut beds, mut wrong) = (0, Vec::new());
    for kind in [WaterKind::Lake, WaterKind::River] {
        for (x, z) in wet_columns(kind).into_iter().step_by(5).take(12) {
            let ground = regions.get_height(x, z).round() as i64;
            let coord = I64Vec3::new(x, ground, z).div_euclid(I64Vec3::splat(16));
            let chunk = generate_chunk(regions.clone(), ChunkCoordinate(coord));
            let lod = LodBlocks::sample(&regions, LodKey { level: 0, coord });
            for x in 0..16 {
                for z in 0..16 {
                    // the top block, which shows: how deep the soil runs under it is not the shore's
                    let y = heights.ground_top(coord.x * 16 + x as i64, coord.z * 16 + z as i64) - 1 - coord.y * 16;
                    if !(0..16).contains(&y) {
                        continue;
                    }
                    let (x, y, z) = (x as u16, y as u16, z as u16);
                    let (full, node) = (chunk.get_block_at(U16Vec3::new(x, y, z)), lod.raw(IVec3::new(x as i32, y as i32, z as i32)));
                    if bed(full) || bed(node) {
                        beds += 1;
                        if full != node {
                            wrong.push((coord * 16 + I64Vec3::new(x as i64, y as i64, z as i64), full, node));
                        }
                    }
                }
            }
        }
    }
    assert!(beds > 100, "only {beds} bed blocks");
    assert!(wrong.is_empty(), "{} of {beds} bed blocks differ, as {:?}", wrong.len(), &wrong[..wrong.len().min(4)]);
}




#[test]
fn test_grass_tint_barely_changes_at_the_handover_near_rivers_either() {
    // levels 3 and 4 over twelve nodes of seed 5, rivers among them: gaps over 32-block samples averaged 0.045, max 0.15
    let mut gaps = Vec::new();
    for scale in [8.0f32, 16.0] {
        for x in [-2048.0f32, -1024.0, 0.0, 384.0, 512.0, 1024.0] {
            for z in [-3072.0f32, -1024.0] {
                gaps.push(handover_tint_gap(5, Vec3::new(x, 0.0, z), scale));
            }
        }
    }
    let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
    let max = gaps.iter().cloned().fold(0.0, f32::max);
    assert!(mean < 0.015 && max < 0.1, "tint pops by {mean} on average and {max} at most at the handover");
}

#[test]
fn test_rock_a_cliff_builds_over_a_column_buries_no_grass() {
    use super::chunk::{generate_chunk_with, ChunkPasses};
    use ochre_world::worldmap::{generate, WorldConfig, WorldTerrain};
    // seed 2925398056 by 1911,6919: a one-block shell of sandstone built over grass along a slope
    let seed = 2925398056;
    let map = Arc::new(generate(seed, ochre_world::worldmap::character::world_config(seed, WorldConfig::default())));
    let regions = Arc::new(RegionStore::new(Arc::new(WorldTerrain::new(map))));
    let passes = ChunkPasses { cliffs: true, trees: false, flora: false };
    let (mut grass, mut buried) = (0, Vec::new());
    for cx in (1911 / 16 - 4)..(1911 / 16 + 4) {
        for cz in (6919 / 16 - 4)..(6919 / 16 + 4) {
            let chunks: Vec<ChunkData> =
                (0..5).map(|cy| generate_chunk_with(regions.clone(), ChunkCoordinate(I64Vec3::new(cx, cy, cz)), passes)).collect();
            let at = |y: i64, x: u16, z: u16| chunks[(y / 16) as usize].get_block_at(U16Vec3::new(x, (y % 16) as u16, z));
            for x in 0..16 {
                for z in 0..16 {
                    for y in 0..79 {
                        if at(y, x, z) != BlockType::Grass {
                            continue;
                        }
                        grass += 1;
                        let above = at(y + 1, x, z);
                        if above.is_opaque() && !matches!(above, BlockType::Grass | BlockType::Dirt | BlockType::Snow) {
                            buried.push((cx * 16 + x as i64, y, cz * 16 + z as i64, above));
                        }
                    }
                }
            }
        }
    }
    assert!(grass > 1000, "only {grass} grass blocks");
    assert!(buried.is_empty(), "{} of {grass} grass blocks lie under rock a cliff built, as {:?}", buried.len(), &buried[..buried.len().min(3)]);
}

#[test]
fn test_lake_and_river_beds_take_the_ground_round_them_and_the_sea_keeps_its_sand() {
    use ochre_base::terrain_source::{TerrainSource, WaterKind};
    use ochre_world::worldmap::{character::world_config, generate, WorldConfig, WorldTerrain};
    let map = Arc::new(generate(1, world_config(1, WorldConfig::default())));
    let terrain = Arc::new(WorldTerrain::new(map.clone()));
    let regions = Arc::new(RegionStore::new(terrain.clone()));
    let (mut fresh, mut sea): (HashMap<u8, usize>, HashMap<u8, usize>) = (HashMap::new(), HashMap::new());
    let mut rivers: HashMap<u8, usize> = HashMap::new();
    for i in (0..map.height.len()).step_by(211) {
        let (x, z) = map.cell_centre(i % map.dim(), i / map.dim());
        let s = terrain.sample(x, z);
        let tally = match s.water_kind {
            WaterKind::Lake | WaterKind::River => &mut fresh,
            WaterKind::Ocean if s.height > WATER_LEVEL as f64 - 6.0 => &mut sea,
            _ => continue,
        };
        let ground = regions.sampler().ground_top(x as i64, z as i64);
        let bed = block_at(&regions, x as i64, ground - 1, z as i64) as u8;
        *tally.entry(bed).or_default() += 1;
        if s.water_kind == WaterKind::River {
            *rivers.entry(bed).or_default() += 1;
        }
    }
    eprintln!("fresh beds by block code {fresh:?}, of them rivers {rivers:?}, shallow sea {sea:?}");
    let share = |t: &HashMap<u8, usize>| t.get(&(BlockType::Sand as u8)).copied().unwrap_or(0) as f64 / t.values().sum::<usize>().max(1) as f64;
    assert!(fresh.values().sum::<usize>() > 100 && sea.values().sum::<usize>() > 20);
    assert!(fresh.len() >= 3 && share(&fresh) < 0.5, "fresh beds are mostly sand or all one block: {fresh:?}");
    assert!(share(&sea) > 0.9, "the shallow sea's beds aren't sand: {sea:?}");
}

// A chunk lays water only where the map holds it, and in a cliff's cut beside such water: no single blocks of it stuck to a fall's cliff.
#[test]
fn test_round_falls_no_water_stands_apart_from_the_water_the_map_holds() {
    use ochre_world::worldmap::{character::world_config, generate, WorldConfig, WorldTerrain};
    let map = Arc::new(generate(6, world_config(6, WorldConfig::default())));
    let terrain = Arc::new(WorldTerrain::new(map));
    let mut falls = terrain.falls();
    falls.sort_by(|a, b| b.2.total_cmp(&a.2));
    let regions = Arc::new(RegionStore::new(terrain));
    let mut sampler = regions.sampler();
    let (mut water, mut strays) = (0, Vec::new());
    for &(fx, fz, drop, level) in falls.iter().take(12) {
        let (cx, cz) = ((fx as i64).div_euclid(16), (fz as i64).div_euclid(16));
        let (y0, y1) = ((level - drop) as i64 / 16 - 1, level as i64 / 16 + 1);
        let mut chunks = HashMap::new();
        for (x, y, z) in (cx - 1..=cx + 1).flat_map(|x| (y0..=y1).flat_map(move |y| (cz - 1..=cz + 1).map(move |z| (x, y, z)))) {
            chunks.insert((x, y, z), generate_chunk(regions.clone(), ChunkCoordinate(I64Vec3::new(x, y, z))));
        }
        let at = |x: i64, y: i64, z: i64| {
            let local = U16Vec3::new(x.rem_euclid(16) as u16, y.rem_euclid(16) as u16, z.rem_euclid(16) as u16);
            chunks[&(x.div_euclid(16), y.div_euclid(16), z.div_euclid(16))].get_block_at(local)
        };
        for x in cx * 16 - 15..cx * 16 + 31 {
            for z in cz * 16 - 15..cz * 16 + 31 {
                if sampler.water_top(x, z) > sampler.ground_top(x, z) {
                    continue;
                }
                for y in (y0 * 16).max(WATER_LEVEL + 1)..(y1 + 1) * 16 {
                    if at(x, y, z) != BlockType::Water {
                        continue;
                    }
                    water += 1;
                    if [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|&(dx, dz)| at(x + dx, y, z + dz) != BlockType::Water) {
                        strays.push((x, y, z));
                    }
                }
            }
        }
    }
    assert!(strays.is_empty(), "{} of {water} water blocks in dry columns touch no water, e.g. {:?}", strays.len(), &strays[..strays.len().min(4)]);
}

#[test]
fn test_trunks_far_into_the_negative_world_are_not_read_by_the_shader_as_sculpted_ground() {
    use crate::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    use bevy::render::mesh::VertexAttributeValues;
    let regions = Arc::new(RegionStore::new(ochre_world::worldmap::test_terrain(5)));
    let mut heights = regions.sampler();
    // the shader takes `uv.x` under -512 as sculpted ground, which a bark phase from a negative position gave it
    let trees = trees_near(regions.terrain(), &mut heights, I64Vec2::new(-7000, -3000), I64Vec2::new(-1600, 3000));
    let tree = trees.iter().find(|t| t.trunk >= 6 && t.base.x < -1600).expect("no tree far into the negative world");
    let mid = tree.base + I64Vec3::new(0, tree.trunk / 2, 0);
    let coord = ChunkCoordinate(mid.div_euclid(I64Vec3::splat(16)));
    let mut neighbours = vec![None; 26];
    for dx in -1..=1i64 {
        for dy in -1..=1i64 {
            for dz in -1..=1i64 {
                if (dx, dy, dz) != (0, 0, 0) {
                    neighbours[neighbor_26_index(dx as i32, dy as i32, dz as i32)] = Some(Arc::new(generate_chunk(regions.clone(), ChunkCoordinate(coord.0 + I64Vec3::new(dx, dy, dz)))));
                }
            }
        }
    }
    let meshes = generate_chunk_mesh_with(Arc::new(generate_chunk(regions.clone(), coord)), neighbours, coord, &regions, true);
    let (Some(VertexAttributeValues::Float32x2(uvs)), Some(VertexAttributeValues::Float32x4(colours))) =
        (meshes.opaque.attribute(Mesh::ATTRIBUTE_UV_0), meshes.opaque.attribute(Mesh::ATTRIBUTE_COLOR))
    else {
        panic!("no uvs");
    };
    let bark: Vec<f32> = uvs.iter().zip(colours).filter(|(_, c)| c[3].round() as u8 == Tile::LogBark as u8 || c[3].round() as u8 == Tile::LogEnd as u8).map(|(uv, _)| uv[0]).collect();
    assert!(bark.len() > 8, "{} bark vertices", bark.len());
    assert!(bark.iter().all(|&u| u > -512.0), "bark at uv.x {}", bark.iter().cloned().fold(f32::MAX, f32::min));
}

// A level-1 node draws full detail's own plants, and of them only the tufts that outlast `FloraFade::mid`.
#[test]
fn level_one_draws_the_tall_plants_full_detail_grows() {
    let regions = test_regions();
    let fade = crate::lod::LodLayout::new(16, 1024, 2).with_full_detail_distance(12).flora_fade();
    let (mut plants, mut agree) = (0, 0);
    let mut flora_vertices = 0;
    let (mut shrubs, mut short_shrubs) = (0, 0);
    for (x, z) in (-12..12).flat_map(|x| (-12..12).map(move |z| (x * 3, z * 3))) {
        if plants > 2000 {
            break;
        }
        let ground = regions.terrain().sample(x as f64 * 32.0 + 16.0, z as f64 * 32.0 + 16.0).height;
        let y = (ground as i64).div_euclid(32);
        for key in [y - 1, y].map(|y| LodKey { level: 1, coord: I64Vec3::new(x, y, z) }) {
            for &(world, block) in &LodBlocks::sample_with(&regions, key, true).plants {
                let chunk = generate_chunk(regions.clone(), ChunkCoordinate(world.as_i64vec3().div_euclid(I64Vec3::splat(16))));
                let local = world.rem_euclid(IVec3::splat(16)).as_u16vec3();
                plants += 1;
                agree += (chunk.get_block_at(local) == block) as usize;
            }
            let Some(meshes) = super::lod::generate_lod_mesh(regions.clone(), key, Some(fade)) else { continue };
            let Some(VertexAttributeValues::Float32x4(colors)) = meshes.opaque.attribute(Mesh::ATTRIBUTE_COLOR) else { continue };
            for alpha in colors.iter().map(|c| c[3]).filter(|&a| a > Tile::FIRST_FLORA.atlas_cell() - 0.5) {
                let code = alpha - alpha.round();
                let height = (code / super::FLORA_HEIGHT_CODE * 0.5 + 0.5) * super::FLORA_HEIGHT_SPAN;
                assert!(fade.gone_at(height + super::FLORA_HEIGHT_STEP + 0.01) > fade.mid, "a {height} block tuft is gone before {}", fade.mid);
                flora_vertices += 1;
                // a shrub in the open stands 1.8 to 2.5 blocks in full detail, and so as tall here
                if alpha.round() == Tile::Shrub.atlas_cell() {
                    shrubs += 1;
                    short_shrubs += (height < 1.7) as usize;
                }
            }
        }
    }
    assert!(plants > 500 && flora_vertices > 0, "{plants} plants, {flora_vertices} flora vertices");
    // a low bough full detail plants over the ground is the one difference, see `far_plants`
    assert!(agree * 100 >= plants * 98, "{agree} of {plants} plants agree with full detail");
    assert!(shrubs > 0 && short_shrubs * 50 <= shrubs, "{short_shrubs} of {shrubs} shrub vertices are under a roof");
}

// A plant rooted on near-vertical ground stays about as wide as tall, not stretched into a spike by the slope.
#[test]
fn tuft_on_a_cliff_keeps_its_size() {
    for ground in [Vec3::new(0.99, 0.12, 0.0), Vec3::new(0.7, 0.11, 0.7).normalize(), Vec3::new(1.0, 0.0, 0.0)] {
        let mut buffers = super::buffers::MeshBuffers::default();
        let tuft = super::mesh::Tuft { root: Vec3::ZERO, ground, height: 2.5, half_width: 1.8, angle: 0.3, lean: Vec3::ZERO };
        super::mesh::push_tuft(&mut buffers, &tuft, 1.0, Tile::Shrub.atlas_cell(), Vec3::ONE, 1.0);
        let reach = Vec3::from(buffers.vertices.iter().map(|v| v.position).fold([0.0f32; 3], |a, p| [a[0].max(p[0].abs()), a[1].max(p[1].abs()), a[2].max(p[2].abs())]));
        assert!(reach.max_element() < 6.0, "{ground}: vertices reach {reach}");
    }
}

// A rapid that runs down a ramp, however steeply and across the map's grid, is a surface, not walls of water standing in it:
// seed 6's river runs down 5 blocks in 9 columns at (-8197, 15214), which drew a fin of sheet on each column it stepped.
#[test]
fn test_a_rapid_is_a_sloped_surface_with_no_walls_of_water_in_it() {
    use bevy::render::mesh::{Indices, VertexAttributeValues};
    use ochre_world::worldmap::{character::world_config, generate, WorldConfig, WorldTerrain};
    let map = Arc::new(generate(6, world_config(6, WorldConfig::default())));
    let regions = Arc::new(RegionStore::new(Arc::new(WorldTerrain::new(map))));
    let coord = ChunkCoordinate(I64Vec3::new(-513, 13, 950));
    let mut neighbours = vec![None; 26];
    for (dx, dy, dz) in (-1..=1).flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| (x, y, z)))) {
        if (dx, dy, dz) != (0, 0, 0) {
            let at = ChunkCoordinate(coord.0 + I64Vec3::new(dx, dy, dz));
            neighbours[neighbor_26_index(dx as i32, dy as i32, dz as i32)] = Some(Arc::new(generate_chunk(regions.clone(), at)));
        }
    }
    let meshes = generate_chunk_mesh_with(Arc::new(generate_chunk(regions.clone(), coord)), neighbours, coord, &regions, true);
    let water = meshes.water.expect("the rapid has water");
    let Some(VertexAttributeValues::Float32x3(pos)) = water.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
    let Some(VertexAttributeValues::Float32x3(normal)) = water.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!() };
    let Some(Indices::U32(indices)) = water.indices() else { panic!() };
    let walls = indices
        .chunks(3)
        .filter(|t| {
            let ys = t.iter().map(|&i| pos[i as usize][1]);
            normal[t[0] as usize][1] < 0.5 && ys.clone().fold(f32::MIN, f32::max) - ys.fold(f32::MAX, f32::min) > 0.01
        })
        .count();
    assert_eq!(0, walls, "{walls} triangles of water walls in a rapid");
}
