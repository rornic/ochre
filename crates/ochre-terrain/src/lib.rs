//! Voxel terrain: chunk storage and generation, meshing, octree LODs, and the world they make.

pub mod chunk;
pub mod chunk_loader;
pub mod floor;
pub mod curvature;
mod fade;
mod flow;
pub mod frost;
pub mod generate;
pub mod group;
pub mod horizon;
pub mod lod;
pub mod material;
pub mod pack;
pub mod raycast;
mod remesh;
pub mod settings;
mod shadow_mesh;
pub mod world;

use std::sync::Mutex;

use bevy::{
    app::{App, Plugin, PreStartup, Startup, Update},
    app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy},
    asset::{Assets, Handle},
    ecs::{
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Commands, Res, ResMut, SystemParam},
    },
    image::Image,
    pbr::MaterialPlugin,
    render::{storage::ShaderBuffer, RenderApp},
};

use crate::settings::{DebugSettings, RendererSettings};
use chunk_loader::{
    gather_chunks, generate_chunks, load_chunks, mark_chunks, unload_chunks, ChunkLoader,
};
use curvature::Curvature;
use fade::{apply_fades, FadeMaterials};
use flow::flow_water;
use group::{build_groups, load_groups};
use lod::{
    load_lods, refresh_lod_visibility, retire_lods, spawn_lods, update_lod_selection, LodLoader,
};
use material::{
    material_array, ChunkMaterial, CloudLayer, ShaderInputs, SprayMaterial,
    TerrainAssets, TerrainMaterials, WaterMaterial,
};
use world::World;
use remesh::{apply_remeshes, remesh_edits};

/// The world's terrain: inserts the `World`, then loads, meshes and unloads full-detail chunks and LOD terrain
/// around the camera. Headless-safe: with no renderer the materials are blank. Needs the material assets
/// registered, which `TerrainRenderPlugin` does and a headless run does with `init_asset`.
pub struct TerrainPlugin {
    pub renderer: RendererSettings,
    pub debug: DebugSettings,
    world: Mutex<Option<World>>,
}

impl TerrainPlugin {
    pub fn new(world: World, renderer: RendererSettings, debug: DebugSettings) -> Self {
        Self {
            renderer,
            debug,
            world: Mutex::new(Some(world)),
        }
    }
}

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        let world = self.world.lock().unwrap().take();
        app.insert_resource(world.expect("TerrainPlugin was built twice"))
            .insert_resource(self.renderer)
            .insert_resource(self.debug)
            .add_systems(Startup, setup_terrain)
            .add_systems(
                Update,
                (
                    (
                        gather_chunks,
                        generate_chunks,
                        mark_chunks,
                        load_chunks,
                        build_groups,
                        load_groups,
                    )
                        .before(unload_chunks),
                    (
                        update_lod_selection,
                        spawn_lods,
                        load_lods,
                        retire_lods,
                        refresh_lod_visibility,
                        apply_fades,
                    )
                        .chain()
                        .after(gather_chunks)
                        .before(unload_chunks),
                    // after the groups loading this frame, so one built from meshes made before a
                    // change is meshed again
                    (flow_water, remesh_edits, apply_remeshes)
                        .chain()
                        .after(load_groups)
                        .before(unload_chunks),
                    unload_chunks,
                ),
            );
    }
}

/// Draws the terrain: its textures and materials, and the renderer's mesh buffers. Needs a window and GPU, so a
/// headless run goes without. `CurvaturePlugin` and `FrostPlugin` are separate.
pub struct TerrainRenderPlugin {
    pub renderer: RendererSettings,
}

impl Plugin for TerrainRenderPlugin {
    fn build(&self, app: &mut App) {
        pack::set_packed(self.renderer.pack_meshes);
        app.add_plugins((
            MaterialPlugin::<ChunkMaterial>::default(),
            MaterialPlugin::<WaterMaterial>::default(),
            MaterialPlugin::<SprayMaterial>::default(),
        ))
        // before `setup_terrain` builds the materials from them
        .init_resource::<Curvature>()
        .add_systems(PreStartup, load_textures)
        .add_systems(Startup, blank_clouds.before(setup_terrain));
        // the terrain's meshes share a few big buffers, see `RendererSettings::mesh_slab_mb`
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.insert_resource(self.renderer.mesh_allocator());
        }
    }
}

/// The painted materials the terrain's shaders sample, as loaded by `TerrainRenderPlugin`.
#[derive(Resource)]
struct TerrainTextures {
    materials: Handle<Image>,
}

// A cloudless layer for the shaders to sample when `CloudPlugin` is left out.
fn blank_clouds(
    mut commands: Commands,
    clouds: Option<Res<CloudLayer>>,
    mut images: ResMut<Assets<Image>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    if clouds.is_none() {
        commands.insert_resource(CloudLayer::clear(&mut images, &mut buffers));
    }
}

fn load_textures(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    renderer: Res<RendererSettings>,
) {
    commands.insert_resource(TerrainTextures {
        materials: images.add(material_array(
            "assets/textures/materials",
            renderer.material_anisotropy,
        )),
    });
}

/// Terrain generation runs on the async compute pool, so it gets most of the cores.
pub fn task_pool_plugin() -> TaskPoolPlugin {
    TaskPoolPlugin {
        task_pool_options: TaskPoolOptions {
            io: TaskPoolThreadAssignmentPolicy {
                min_threads: 1,
                max_threads: 1,
                percent: 0.0,
                on_thread_spawn: None,
                on_thread_destroy: None,
            },
            async_compute: TaskPoolThreadAssignmentPolicy {
                min_threads: 1,
                max_threads: usize::MAX,
                percent: 0.6,
                on_thread_spawn: None,
                on_thread_destroy: None,
            },
            ..Default::default()
        },
    }
}

#[derive(SystemParam)]
struct ShaderSources<'w> {
    textures: Option<Res<'w, TerrainTextures>>,
    clouds: Option<Res<'w, CloudLayer>>,
    curvature: Option<Res<'w, Curvature>>,
}

/// Builds the materials and loaders `TerrainPlugin`'s systems run on. The shaders sample the textures, clouds and curvature
/// of the plugins that provide them, and a blank where one is left out.
fn setup_terrain(
    mut commands: Commands,
    world: Res<World>,
    renderer: Res<RendererSettings>,
    debug: Res<DebugSettings>,
    sources: ShaderSources,
    mut assets: TerrainAssets,
) {
    let layout = renderer.lod_layout();
    let inputs = ShaderInputs {
        materials: sources.textures.as_ref().map(|t| t.materials.clone()),
        clouds: sources.clouds.map(|c| c.clone()).unwrap_or_default(),
        curvature: sources.curvature.map(|c| c.buffer.clone()).unwrap_or_default(),
        wind: bevy::math::Vec2::from(world.region_store.terrain().prevailing_wind()),
    };
    let materials = TerrainMaterials::new(*debug, renderer.material_period(), layout.flora_fade(), &inputs, &mut assets);
    commands.insert_resource(materials.clone());
    commands.insert_resource(FadeMaterials::new(
        renderer.lod_fade_seconds,
        &materials,
        &mut assets.chunk,
        &mut assets.water,
    ));
    commands.insert_resource(LodLoader::new(
        layout,
        world.bounds,
        materials.chunk.clone(),
        materials.water.clone(),
        materials.solid_water,
    ));
    commands.insert_resource(ChunkLoader::new(
        layout,
        world.bounds,
        materials.chunk,
        materials.water,
        materials.spray,
    ));
}
