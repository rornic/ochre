// How much of what the camera is handed is seen, headless (no window or GPU): loads the start area as
// shadow_cull does, then software-rasterizes every terrain mesh in the view frustum into a small
// depth buffer from eight headings, and counts the meshes and triangles that reach a pixel against
// those that pass the frustum, and the fragments drawn per pixel. Run with:
//   OCHRE_SPAWN=x,z[,lift] cargo run --release --example visibility -- [pitch degrees] [buffer width]
// The ceiling on what occlusion culling can save is the gap between "in frustum" and "seen". Curvature,
// sway and the buffer's resolution are ignored, so small triangles are undercounted as seen.
use std::time::{Duration, Instant};

use bevy::camera::primitives::{Aabb, Frustum};
use bevy::camera::{PerspectiveProjection, Projection};
use bevy::math::Affine3A;
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::pbr::MeshMaterial3d;
use bevy::prelude::*;
use bevy::shape::ViewFrustum;
use ochre_engine::settings::{DebugSettings, RendererSettings, Settings};
use ochre_terrain::chunk_loader::{GenerateChunkData, GenerateChunkMesh};
use ochre_terrain::group::ChunkGroup;
use ochre_terrain::lod::{GenerateLodMesh, LodNode};
use ochre_terrain::material::{
    ChunkMaterial, ShaderInputs, SprayMaterial, TerrainAssets, WaterMaterial,
};
use ochre_terrain::{insert_terrain_resources, task_pool_plugin, TerrainPlugin};
use ochre_world::worldmap::WorldConfig;

// The game's camera, see `spawn_player`.
const FOV_DEGREES: f32 = 90.0;
const ASPECT: f32 = 16.0 / 9.0;
const EYE_HEIGHT: f32 = 1.7;
const NEAR: f32 = 0.1;
const HEADINGS: usize = 8;
// The most levels counted separately, the rest are lumped together.
const LEVELS: usize = 8;

#[derive(Component)]
struct SimCamera;

#[derive(Resource)]
struct Renderer(RendererSettings, WorldConfig);

fn setup(mut commands: Commands, renderer: Res<Renderer>, mut assets: TerrainAssets) {
    let world_config = renderer.1;
    let renderer = &renderer.0;
    let world = ochre_world::load_world(renderer.lod_layout().region_reach() as u32, world_config);
    let spawn = world.spawn();
    let height = world.bounds;
    commands.insert_resource(world);
    let player = commands.spawn(Transform::from_translation(spawn)).id();
    let camera = commands
        .spawn((
            SimCamera,
            Camera::default(),
            Transform::from_translation(Vec3::Y * EYE_HEIGHT).looking_to(Vec3::X, Vec3::Y),
        ))
        .id();
    commands.entity(player).add_children(&[camera]);
    insert_terrain_resources(
        &mut commands,
        renderer,
        DebugSettings::default(),
        height,
        &ShaderInputs::default(),
        &mut assets,
    );
}

struct Piece {
    aabb: Aabb,
    affine: Affine3A,
    // world positions, as the mesh sits in the world
    positions: Vec<Vec3>,
    indices: Vec<u32>,
    // 0 for full-detail groups, then the LOD level
    class: usize,
}

fn pieces(app: &mut App) -> Vec<Piece> {
    let world = app.world_mut();
    let mut found = Vec::new();
    let mut query = world.query::<(
        &Mesh3d,
        &Aabb,
        &GlobalTransform,
        &Visibility,
        Option<&ChunkGroup>,
        Option<&LodNode>,
        Option<&ChildOf>,
        &MeshMaterial3d<ChunkMaterial>,
    )>();
    let meshes = world.resource::<Assets<Mesh>>();
    for (mesh, aabb, transform, visibility, group, node, parent, _) in query.iter(world) {
        if *visibility == Visibility::Hidden {
            continue;
        }
        let parent = parent.map(|p| world.entity(p.parent()));
        if parent
            .as_ref()
            .is_some_and(|p| p.get::<Visibility>() == Some(&Visibility::Hidden))
        {
            continue;
        }
        let group = group.or_else(|| parent.as_ref().and_then(|p| p.get::<ChunkGroup>()));
        let node = node.or_else(|| parent.as_ref().and_then(|p| p.get::<LodNode>()));
        let Some(mesh) = meshes.get(&mesh.0) else {
            continue;
        };
        let class = match (group, node) {
            (Some(_), _) => 0,
            (_, Some(node)) => node.key().level as usize,
            _ => continue,
        };
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            continue;
        };
        let affine = transform.affine();
        let positions = positions
            .iter()
            .map(|p| Vec3::from(affine.transform_point3a(Vec3::from(*p).into())))
            .collect();
        let indices = match mesh.indices() {
            Some(Indices::U16(i)) => i.iter().map(|&i| i as u32).collect(),
            Some(Indices::U32(i)) => i.clone(),
            None => (0..mesh.count_vertices() as u32).collect(),
        };
        found.push(Piece {
            aabb: *aabb,
            affine,
            positions,
            indices,
            class,
        });
    }
    found
}

struct Triangle {
    piece: u32,
    id: u32,
    // pixel x, pixel y, 1 / view depth
    v: [[f32; 3]; 3],
}

struct Buffer {
    w: usize,
    h: usize,
    depth: Vec<f32>,
    id: Vec<u32>,
    drawn: usize,
    covered: usize,
}

impl Buffer {
    fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            depth: vec![0.0; w * h],
            id: vec![u32::MAX; w * h],
            drawn: 0,
            covered: 0,
        }
    }

    fn draw(&mut self, t: &Triangle) {
        let [a, b, c] = t.v;
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if area == 0.0 {
            return;
        }
        let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
        let max_x = (a[0].max(b[0]).max(c[0]).ceil().max(0.0) as usize).min(self.w - 1);
        let min_y = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
        let max_y = (a[1].max(b[1]).max(c[1]).ceil().max(0.0) as usize).min(self.h - 1);
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((b[0] - px) * (c[1] - py) - (c[0] - px) * (b[1] - py)) / area;
                let w1 = ((c[0] - px) * (a[1] - py) - (a[0] - px) * (c[1] - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let inverse = w0 * a[2] + w1 * b[2] + w2 * c[2];
                let i = y * self.w + x;
                self.covered += 1;
                if inverse > self.depth[i] {
                    self.depth[i] = inverse;
                    self.id[i] = t.id;
                    self.drawn += 1;
                }
            }
        }
    }
}

// The triangles of `pieces` in view, culled behind the near plane and by back faces, as the GPU would.
fn project(
    pieces: &[Piece],
    frustum: &Frustum,
    eye: Vec3,
    rotation: Quat,
    size: (usize, usize),
    front: &mut usize,
    back: &mut usize,
) -> (Vec<Triangle>, Vec<u32>, usize) {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let ty = (FOV_DEGREES.to_radians() / 2.0).tan();
    let tx = ty * ASPECT;
    let inverse = rotation.inverse();
    let mut triangles = Vec::new();
    let mut in_frustum = Vec::new();
    let mut frustum_triangles = 0;
    for (index, piece) in pieces.iter().enumerate() {
        if !frustum.intersects_obb(&piece.aabb, &piece.affine, true, true) {
            continue;
        }
        in_frustum.push(index as u32);
        frustum_triangles += piece.indices.len() / 3;
        let view: Vec<Vec3> = piece
            .positions
            .iter()
            .map(|p| inverse * (*p - eye))
            .collect();
        for (k, tri) in piece.indices.as_chunks::<3>().0.iter().enumerate() {
            let corners = [
                view[tri[0] as usize],
                view[tri[1] as usize],
                view[tri[2] as usize],
            ];
            // view space looks down -z, so depth is -z
            let mut polygon = Vec::with_capacity(4);
            for i in 0..3 {
                let (p, q) = (corners[i], corners[(i + 1) % 3]);
                let (dp, dq) = (-p.z - NEAR, -q.z - NEAR);
                if dp >= 0.0 {
                    polygon.push(p);
                }
                if (dp >= 0.0) != (dq >= 0.0) {
                    polygon.push(p + (q - p) * (dp / (dp - dq)));
                }
            }
            if polygon.len() < 3 {
                continue;
            }
            let screen: Vec<[f32; 3]> = polygon
                .iter()
                .map(|p| {
                    let d = -p.z;
                    [
                        (p.x / (d * tx) * 0.5 + 0.5) * w,
                        (0.5 - p.y / (d * ty) * 0.5) * h,
                        1.0 / d,
                    ]
                })
                .collect();
            for i in 1..screen.len() - 1 {
                let v = [screen[0], screen[i], screen[i + 1]];
                let area = (v[1][0] - v[0][0]) * (v[2][1] - v[0][1])
                    - (v[2][0] - v[0][0]) * (v[1][1] - v[0][1]);
                // pixel y runs down, so a counter-clockwise front face has negative area
                if area >= 0.0 {
                    *back += 1;
                    continue;
                }
                *front += 1;
                triangles.push(Triangle {
                    piece: index as u32,
                    id: ((index as u32) << 20) | (k as u32 & 0xF_FFFF),
                    v,
                });
            }
        }
    }
    (triangles, in_frustum, frustum_triangles)
}

#[derive(Default)]
struct Totals {
    frustum_meshes: f64,
    seen_meshes: f64,
    frustum_triangles: f64,
    front_triangles: f64,
    seen_triangles: f64,
    covered: f64,
    submitted: f64,
    sorted: f64,
    views: f64,
    by_class: [[f64; 2]; LEVELS + 1],
}

fn main() {
    let settings = Settings::read("assets/settings.toml").expect("failed to read settings");
    let (renderer, world) = (settings.renderer, settings.world);
    let args: Vec<String> = std::env::args().collect();
    let pitch: f32 = args.get(1).map_or(0.0, |s| s.parse().unwrap());
    let width: usize = args.get(2).map_or(480, |s| s.parse().unwrap());
    let size = (width, (width as f32 / ASPECT) as usize);
    let far = renderer.fog_distance();

    ochre_terrain::pack::set_packed(false);
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(task_pool_plugin()),
        AssetPlugin::default(),
        TransformPlugin,
        TerrainPlugin,
    ))
    .insert_resource(Renderer(renderer, world))
    .init_asset::<Mesh>()
    .init_asset::<Image>()
    .init_asset::<ChunkMaterial>()
    .init_asset::<WaterMaterial>()
    .init_asset::<SprayMaterial>()
    .add_systems(Startup, setup);
    app.update();

    let start = Instant::now();
    let mut quiet_since = None;
    loop {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
        let world = app.world_mut();
        let pending = world.query::<&GenerateChunkData>().iter(world).count()
            + world.query::<&GenerateChunkMesh>().iter(world).count()
            + world.query::<&GenerateLodMesh>().iter(world).count();
        if pending == 0 {
            let since = *quiet_since.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_secs(4) {
                break;
            }
        } else {
            quiet_since = None;
        }
        if start.elapsed() > Duration::from_secs(300) {
            println!("gave up waiting for the start area after 300 s");
            break;
        }
    }
    println!("loaded in {:.0} s", start.elapsed().as_secs_f32());

    let pieces = pieces(&mut app);
    let all: usize = pieces.iter().map(|p| p.indices.len() / 3).sum();
    println!(
        "{} terrain meshes, {:.2}M triangles in all; view reaches {far}, buffer {}x{}",
        pieces.len(),
        all as f32 / 1e6,
        size.0,
        size.1
    );

    let eye = {
        let world = app.world_mut();
        world
            .query_filtered::<&GlobalTransform, With<SimCamera>>()
            .single(world)
            .unwrap()
            .translation()
    };
    let projection = Projection::Perspective(PerspectiveProjection {
        fov: FOV_DEGREES.to_radians(),
        aspect_ratio: ASPECT,
        near: NEAR,
        far,
        ..default()
    });
    let mut totals = Totals::default();
    for heading in 0..HEADINGS {
        let yaw = (heading as f32 / HEADINGS as f32) * std::f32::consts::TAU;
        let (pitch_sin, pitch_cos) = pitch.to_radians().sin_cos();
        let direction = Vec3::new(pitch_cos * yaw.cos(), pitch_sin, pitch_cos * yaw.sin());
        let transform = Transform::from_translation(eye).looking_to(direction, Vec3::Y);
        let view = GlobalTransform::from(transform);
        let clip_from_world = projection.get_clip_from_view() * view.to_matrix().inverse();
        let frustum = Frustum(ViewFrustum::from_clip_from_world(&clip_from_world));
        let (mut front, mut back) = (0, 0);
        let (triangles, in_frustum, frustum_triangles) = project(
            &pieces,
            &frustum,
            eye,
            transform.rotation,
            size,
            &mut front,
            &mut back,
        );
        let mut submitted = Buffer::new(size.0, size.1);
        for t in &triangles {
            submitted.draw(t);
        }
        // nearest first, by each mesh's nearest corner
        let nearest = |piece: u32| {
            let piece = &pieces[piece as usize];
            (Vec3::from(piece.affine.transform_point3a(piece.aabb.center)) - eye).length()
        };
        let mut order: Vec<&Triangle> = triangles.iter().collect();
        order.sort_by(|a, b| nearest(a.piece).total_cmp(&nearest(b.piece)));
        let mut sorted = Buffer::new(size.0, size.1);
        for t in order {
            sorted.draw(t);
        }
        let mut seen_pieces = std::collections::HashSet::new();
        let mut seen_triangles = std::collections::HashSet::new();
        for &id in &submitted.id {
            if id != u32::MAX {
                seen_pieces.insert(id >> 20);
                seen_triangles.insert(id);
            }
        }
        let pixels = (size.0 * size.1) as f64;
        println!(
            "heading {:3.0}: {:5} meshes in frustum, {:4} seen ({:.0}%); {:7.0}k triangles in frustum, {:7.0}k front-facing, {:6.0}k seen ({:.1}%); fragments per pixel: {:.1} drawn, {:.1} passing as submitted, {:.1} front to back",
            yaw.to_degrees(),
            in_frustum.len(),
            seen_pieces.len(),
            100.0 * seen_pieces.len() as f64 / in_frustum.len().max(1) as f64,
            frustum_triangles as f64 / 1000.0,
            front as f64 / 1000.0,
            seen_triangles.len() as f64 / 1000.0,
            100.0 * seen_triangles.len() as f64 / frustum_triangles.max(1) as f64,
            submitted.covered as f64 / pixels,
            submitted.drawn as f64 / pixels,
            sorted.drawn as f64 / pixels,
        );
        totals.views += 1.0;
        totals.frustum_meshes += in_frustum.len() as f64;
        totals.seen_meshes += seen_pieces.len() as f64;
        totals.frustum_triangles += frustum_triangles as f64;
        totals.front_triangles += front as f64;
        totals.seen_triangles += seen_triangles.len() as f64;
        totals.covered += submitted.covered as f64 / pixels;
        totals.submitted += submitted.drawn as f64 / pixels;
        totals.sorted += sorted.drawn as f64 / pixels;
        for &index in &in_frustum {
            let piece = &pieces[index as usize];
            let class = piece.class.min(LEVELS);
            totals.by_class[class][0] += (piece.indices.len() / 3) as f64;
            if seen_pieces.contains(&index) {
                totals.by_class[class][1] += (piece.indices.len() / 3) as f64;
            }
        }
    }
    let n = totals.views;
    println!(
        "\nmean over {HEADINGS} headings at pitch {pitch}: {:.0} meshes in frustum, {:.0} seen ({:.0}%); {:.0}k triangles in frustum, {:.0}k front-facing, {:.0}k seen ({:.1}% of the frustum's)",
        totals.frustum_meshes / n,
        totals.seen_meshes / n,
        100.0 * totals.seen_meshes / totals.frustum_meshes.max(1.0),
        totals.frustum_triangles / n / 1000.0,
        totals.front_triangles / n / 1000.0,
        totals.seen_triangles / n / 1000.0,
        100.0 * totals.seen_triangles / totals.frustum_triangles.max(1.0),
    );
    println!(
        "fragments per pixel: {:.1} rasterized, {:.1} passing the depth test as submitted, {:.1} front to back",
        totals.covered / n,
        totals.submitted / n,
        totals.sorted / n
    );
    println!("triangles in meshes that are seen, by level, of those in the frustum:");
    for (class, [frustum, seen]) in totals.by_class.iter().enumerate() {
        if *frustum > 0.0 {
            let name = if class == 0 {
                "full".to_string()
            } else {
                format!("L{class}")
            };
            println!(
                "  {name:5} {:7.0}k in frustum, {:7.0}k in seen meshes ({:.0}%)",
                frustum / n / 1000.0,
                seen / n / 1000.0,
                100.0 * seen / frustum
            );
        }
    }
}
