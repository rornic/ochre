use std::fmt::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use bevy::app::{First, Last, MainScheduleOrder, PostUpdate, RunFixedMainLoop};
use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d, Transparent3d};
use bevy::core_pipeline::prepass::{AlphaMask3dPrepass, Opaque3dPrepass};
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::pbr::Shadow;
use bevy::prelude::*;
use bevy::render::render_phase::{
    BinnedPhaseItem, BinnedRenderPhase, ViewBinnedRenderPhases, ViewSortedRenderPhases,
};
use bevy::render::{diagnostic::RenderDiagnosticsPlugin, Render, RenderApp, RenderSystems};

use crate::player::Player;
use ochre_base::block::BlockType;
use ochre_terrain::chunk::ChunkCoordinate;
use ochre_terrain::chunk_loader::Chunk;
use ochre_terrain::lod::LodNode;
use ochre_terrain::raycast::RayHit;
use ochre_terrain::world::World;

/// The block the player is looking at, which a game sets for the overlay to show.
#[derive(Resource, Default, Debug)]
pub struct LookedAt(pub Option<(RayHit, BlockType)>);

const TOGGLE_KEY: KeyCode = KeyCode::F1;
const REFRESH_SECONDS: f32 = 0.25;
const GPU_PASSES_SHOWN: usize = 8;
const CPU_PASSES_SHOWN: usize = 5;
// what a stretch of the frame's main schedules is called, between marks laid in `Main`'s order
const MAIN_STAGES: [&str; 5] = ["first", "pre", "update", "post", "last"];

/// Lines a game adds to the end of the F1 readout, for its own state.
#[derive(Resource, Default, Clone, Debug)]
pub struct DebugText(pub String);

/// F1 toggles a text readout of climate, block, frame cost and terrain; `stats` starts it shown and prints `STATS`.
#[derive(Default)]
pub struct DebugOverlayPlugin {
    pub stats: bool,
}

#[derive(Resource)]
struct Stats(bool);

impl Plugin for DebugOverlayPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<FrameTimeDiagnosticsPlugin>() {
            app.add_plugins(FrameTimeDiagnosticsPlugin::default());
        }
        if !app.is_plugin_added::<RenderDiagnosticsPlugin>() {
            app.add_plugins(RenderDiagnosticsPlugin);
        }
        // marks bracket First, RunFixedMainLoop, Update, PostUpdate and Last to time the main world's stretches
        app.init_resource::<MainTimes>();
        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        order.insert_before(First, Mark(0));
        order.insert_after(First, Mark(1));
        order.insert_after(RunFixedMainLoop, Mark(2));
        order.insert_after(Update, Mark(3));
        order.insert_after(PostUpdate, Mark(4));
        order.insert_after(Last, Mark(5));
        for i in 0..=MAIN_STAGES.len() as u8 {
            let mut schedule = Schedule::new(Mark(i));
            schedule.add_systems(move |mut times: ResMut<MainTimes>| times.mark(i as usize));
            app.add_schedule(schedule);
        }
        let counts = DrawCounts::default();
        app.insert_resource(Stats(self.stats))
            .insert_resource(counts.clone())
            .add_systems(Startup, spawn_overlay)
            .add_systems(Update, (toggle_overlay, update_overlay).chain())
            .add_systems(Update, check_holes);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(counts)
                .add_systems(Render, count_draws.in_set(RenderSystems::Prepare));
        }
    }
}

#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
struct Mark(u8);

/// Smoothed ms the main world spent in each of `MAIN_STAGES` last frames.
#[derive(Resource)]
struct MainTimes {
    since: Instant,
    ms: [f32; MAIN_STAGES.len()],
}

impl Default for MainTimes {
    fn default() -> Self {
        Self {
            since: Instant::now(),
            ms: [0.0; MAIN_STAGES.len()],
        }
    }
}

impl MainTimes {
    fn mark(&mut self, i: usize) {
        let now = Instant::now();
        if i > 0 {
            let spent = (now - self.since).as_secs_f32() * 1000.0;
            self.ms[i - 1] += (spent - self.ms[i - 1]) * 0.05;
        }
        self.since = now;
    }
}

/// Draws the render world queued last frame, which Bevy only issues one per mesh here.
#[derive(Resource, Clone, Default)]
struct DrawCounts {
    main: Arc<AtomicU32>,
    prepass: Arc<AtomicU32>,
    blended: Arc<AtomicU32>,
    shadow: Arc<AtomicU32>,
}

// a multi-draw batch set is one draw call however many meshes it holds
fn binned_draws<T: BinnedPhaseItem>(phases: &ViewBinnedRenderPhases<T>) -> u32 {
    let draws = |phase: &BinnedRenderPhase<T>| {
        phase.multidrawable_meshes.len()
            + phase.batchable_meshes.len()
            + phase
                .unbatchable_meshes
                .values()
                .map(|v| v.entities.len())
                .sum::<usize>()
    };
    phases.values().map(draws).sum::<usize>() as u32
}

#[allow(clippy::too_many_arguments)]
fn count_draws(
    counts: Res<DrawCounts>,
    opaque: Res<ViewBinnedRenderPhases<Opaque3d>>,
    masked: Res<ViewBinnedRenderPhases<AlphaMask3d>>,
    opaque_prepass: Res<ViewBinnedRenderPhases<Opaque3dPrepass>>,
    masked_prepass: Res<ViewBinnedRenderPhases<AlphaMask3dPrepass>>,
    shadow: Res<ViewBinnedRenderPhases<Shadow>>,
    blended: Res<ViewSortedRenderPhases<Transparent3d>>,
) {
    counts.main.store(
        binned_draws(&opaque) + binned_draws(&masked),
        Ordering::Relaxed,
    );
    counts.prepass.store(
        binned_draws(&opaque_prepass) + binned_draws(&masked_prepass),
        Ordering::Relaxed,
    );
    counts
        .shadow
        .store(binned_draws(&shadow), Ordering::Relaxed);
    counts.blended.store(
        blended
            .values()
            .map(|phase| phase.items.len())
            .sum::<usize>() as u32,
        Ordering::Relaxed,
    );
}

#[derive(Component)]
struct DebugOverlay;

fn spawn_overlay(mut commands: Commands, stats: Res<Stats>) {
    commands.spawn((
        DebugOverlay,
        Text::new(""),
        TextFont {
            font_size: bevy::text::FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(6.0),
            left: Val::Px(6.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        if stats.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        },
    ));
}

fn toggle_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    mut overlay: Query<&mut Visibility, With<DebugOverlay>>,
) {
    if !keys.just_pressed(TOGGLE_KEY) {
        return;
    }
    for mut visibility in &mut overlay {
        visibility.toggle_inherited_hidden();
    }
}

// The render passes' `kind` ("elapsed_cpu" or "elapsed_gpu") in ms, the costliest first, and the top-level ones' sum.
fn render_passes(diagnostics: &DiagnosticsStore, kind: &str) -> (f64, Vec<(String, f64)>) {
    let suffix = format!("/{kind}");
    let mut passes: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.path().as_str().ends_with(&suffix))
        .filter_map(|d| {
            Some((
                d.path()
                    .as_str()
                    .trim_start_matches("render/")
                    .trim_end_matches(&suffix)
                    .to_string(),
                d.smoothed()?,
            ))
        })
        .collect();
    passes.sort_by(|a, b| b.1.total_cmp(&a.1));
    // nested spans are inside their parent's time already
    let total = passes
        .iter()
        .filter(|(pass, _)| !pass.contains('/'))
        .map(|(_, ms)| ms)
        .sum();
    (total, passes)
}

#[allow(clippy::too_many_arguments)]
fn update_overlay(
    time: Res<Time>,
    mut since_refresh: Local<f32>,
    diagnostics: Res<DiagnosticsStore>,
    world: Option<Res<World>>,
    player: Query<&GlobalTransform, With<Player>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    chunks: Query<(), With<Chunk>>,
    lod_nodes: Query<&LodNode>,
    counts: Res<DrawCounts>,
    main_times: Res<MainTimes>,
    looked_at: Option<Res<LookedAt>>,
    stats: Res<Stats>,
    game: Option<Res<DebugText>>,
    mut overlay: Query<(&mut Text, &Visibility), With<DebugOverlay>>,
) {
    let Ok((mut text, visibility)) = overlay.single_mut() else {
        return;
    };
    *since_refresh += time.delta_secs();
    if *visibility == Visibility::Hidden || *since_refresh < REFRESH_SECONDS {
        return;
    }
    *since_refresh = 0.0;

    let mut out = String::new();

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let _ = writeln!(out, "fps: {fps:.0}");

    if let (Some(world), Ok(player)) = (&world, player.single()) {
        let pos = player.translation();
        let noise = world.region_store.terrain();
        let (x, z) = (pos.x as f64, pos.z as f64);
        let id = noise.sample(x, z).biome;
        let name = noise.biomes().name(id).unwrap_or("grassland");
        let biome = match noise.biomes().modifier(id) {
            ochre_base::biome::Modifier::None => name.to_string(),
            modifier => format!("{name} ({})", format!("{modifier:?}").to_lowercase()),
        };
        let climate = noise.climate(x, z);
        let _ = writeln!(
            out,
            "seed: {}  pos: {:.0} {:.0} {:.0}",
            world.seed(),
            pos.x,
            pos.y,
            pos.z
        );
        let _ = writeln!(
            out,
            "biome: {biome}  temp: {:.2}  humidity: {:.2}",
            climate.temperature, climate.humidity
        );
    }
    if let (Some(world), Ok(camera)) = (&world, camera.single()) {
        let eye = camera.translation();
        let cell = (eye + Vec3::splat(0.5)).floor().as_i64vec3();
        let _ = write!(out, "camera block: ");
        if world.is_chunk_generated(ChunkCoordinate::containing(cell)) {
            let block = world.get_block_at(cell);
            let _ = writeln!(out, "{}  solid: {}", block.name(), block.is_solid());
        } else {
            let _ = writeln!(out, "unloaded  solid: true (collision)");
        }
    }
    if let Some((hit, block)) = looked_at.and_then(|l| l.0) {
        let b = hit.block;
        let _ = writeln!(
            out,
            "looking at: {}{} ({} {} {})",
            block.name(),
            if block.is_breakable() {
                ""
            } else {
                ", unbreakable"
            },
            b.x,
            b.y,
            b.z
        );
    }

    let load = |count: &AtomicU32| count.load(Ordering::Relaxed);
    let (main, prepass, blended, shadow) = (
        load(&counts.main),
        load(&counts.prepass),
        load(&counts.blended),
        load(&counts.shadow),
    );
    let _ = writeln!(
        out,
        "draw calls: {} (main {main}, depth {prepass}, water {blended}, shadows {shadow})",
        main + prepass + blended + shadow
    );

    let mut per_level = vec![chunks.iter().count()];
    for node in &lod_nodes {
        let level = node.key().level as usize;
        if per_level.len() <= level {
            per_level.resize(level + 1, 0);
        }
        per_level[level] += 1;
    }
    let _ = write!(out, "chunks:");
    for (level, count) in per_level.iter().enumerate() {
        let _ = write!(out, " L{level}={count}");
    }

    // ms per render pass, from `RenderDiagnosticsPlugin`, the costliest first
    let frame = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let main: f32 = main_times.ms.iter().sum();
    let _ = write!(
        out,
        "\nframe: {frame:.2} ms\n  cpu main world: {main:.2} ms ("
    );
    for (i, (stage, ms)) in MAIN_STAGES.iter().zip(main_times.ms).enumerate() {
        let _ = write!(out, "{}{stage} {ms:.2}", if i > 0 { ", " } else { "" });
    }
    let (total, passes) = render_passes(&diagnostics, "elapsed_cpu");
    let _ = write!(out, ")\n  cpu render encoding: {total:.2} ms");
    for (pass, ms) in passes.iter().take(CPU_PASSES_SHOWN) {
        let _ = write!(out, "\n    {pass}: {ms:.2}");
    }
    let (total, passes) = render_passes(&diagnostics, "elapsed_gpu");
    let _ = write!(out, "\n  gpu: {total:.2} ms");
    for (pass, ms) in passes.iter().take(GPU_PASSES_SHOWN) {
        let _ = write!(out, "\n    {pass}: {ms:.2}");
    }

    if let Some(game) = game.filter(|g| !g.0.is_empty()) {
        let _ = write!(out, "\n{}", game.0);
    }

    if stats.0 {
        println!("STATS {}", out.replace('\n', " | "));
    }
    text.0 = out;
}

// Every 2 s, logs ground nothing draws: a hidden group with no visible LOD node, or a drawn chunk with no group
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn check_holes(
    time: Res<Time>,
    mut last: Local<f32>,
    mut reported: Local<std::collections::HashSet<(ochre_terrain::lod::LodKey, u8)>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    loader: Option<Res<ochre_terrain::chunk_loader::ChunkLoader>>,
    groups: Query<(&ochre_terrain::group::ChunkGroup, &Visibility, Option<&Mesh3d>, Has<ochre_terrain::group::BuiltGroup>)>,
    nodes: Query<(&LodNode, &Visibility)>,
    grouped: Query<&Chunk, With<ochre_terrain::group::GroupedChunk>>,
    meshed: Query<&Chunk, With<ochre_terrain::group::MeshedChunk>>,
    states: Query<(Has<ochre_terrain::chunk_loader::DirtyChunk>, Has<ochre_terrain::chunk_loader::GenerateChunkData>, Has<ochre_terrain::chunk_loader::GenerateChunkMesh>)>,
    mut waiting_since: Local<std::collections::HashMap<ochre_terrain::lod::LodKey, f32>>,
) {
    use ochre_terrain::lod::LodKey;
    let now = time.elapsed_secs();
    if now - *last < 2.0 {
        return;
    }
    *last = now;
    let (Ok(camera), Some(loader)) = (camera.single(), loader) else { return };
    let eye = camera.translation();
    let near = |key: LodKey| (key.min_block().as_vec3() + Vec3::splat(16.0)).distance(eye) < 200.0;
    let shown_nodes: std::collections::HashSet<LodKey> = nodes.iter().filter(|(_, v)| **v != Visibility::Hidden).map(|(n, _)| n.key()).collect();
    let covered = |chunk: ChunkCoordinate| (1..=12u8).any(|l| shown_nodes.contains(&LodKey::containing(chunk, l)));
    for (group, visibility, mesh, built) in &groups {
        if !built || mesh.is_none() || *visibility != Visibility::Hidden || !near(group.key) {
            continue;
        }
        if !covered(ChunkCoordinate(group.key.min_chunk())) && reported.insert((group.key, 0)) {
            info!("hole check: group {:?} at {} is hidden with no LOD node over it", group.key.coord, group.key.min_block());
        }
    }
    // a chunk meshed and waiting for the rest of its group, still waiting 6 s on: which of the group's chunks hold it up
    let mut still: std::collections::HashSet<LodKey> = std::collections::HashSet::new();
    for chunk in &meshed {
        let key = LodKey::containing(chunk.coord, 1);
        if !near(key) {
            continue;
        }
        still.insert(key);
        let since = *waiting_since.entry(key).or_insert(now);
        if now - since > 6.0 && reported.insert((key, 2)) {
            let holding: Vec<String> = key
                .chunks()
                .filter_map(|c| match loader.chunk_entity(c) {
                    None => Some(format!("{:?} not loaded", c.0)),
                    Some(e) => match states.get(e) {
                        Ok((true, _, _)) => Some(format!("{:?} dirty", c.0)),
                        Ok((_, true, _)) => Some(format!("{:?} generating", c.0)),
                        Ok((_, _, true)) => Some(format!("{:?} meshing", c.0)),
                        _ => None,
                    },
                })
                .collect();
            info!("hole check: group {:?} at {} has waited {:.0} s to be built, held up by {:?}", key.coord, key.min_block(), now - since, holding);
        }
    }
    waiting_since.retain(|key, _| still.contains(key));
    for chunk in &grouped {
        let key = LodKey::containing(chunk.coord, 1);
        if !near(key) {
            continue;
        }
        let missing = loader.group_entity(key).is_none_or(|e| !groups.get(e).is_ok_and(|(_, _, _, built)| built));
        if missing && reported.insert((key, 1)) {
            info!("hole check: chunk {:?} is marked drawn by group {:?}, which isn't built", chunk.coord.0, key.coord);
        }
    }
}
