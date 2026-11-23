use std::sync::LazyLock;


use bevy::{
    asset::{Asset, Assets, Handle},
    camera::{
        visibility::{NoFrustumCulling, Visibility},
        Camera,
    },
    ecs::hierarchy::ChildOf,
    ecs::{
        bundle::Bundle,
        change_detection::DetectChangesMut,
        query::With,
        resource::Resource,
        system::{Local, Query, Res, ResMut},
    },
    light::{NotShadowCaster, NotShadowReceiver, ShadowFilteringMethod},
    mesh::{Mesh, Mesh3d, MeshVertexBufferLayoutRef},
    pbr::{
        DistanceFog, FogFalloff, Material, MaterialPipeline, MaterialPipelineKey, MeshMaterial3d,
    },
    post_process::dof::{DepthOfField, DepthOfFieldMode},
    prelude::{default, Alpha, ClearColor, Color, LinearRgba, Meshable, Sphere, Transform},
    reflect::TypePath,
    render::{
        render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError},
        view::ColorGrading,
    },
    shader::ShaderRef,
    time::Time,
};

use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    camera::{Camera3d, Hdr, PerspectiveProjection, Projection},
    core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass},
    ecs::system::{Commands, SystemParam},
    pbr::ScreenSpaceTransmission,
    post_process::{bloom::Bloom, effect_stack::Vignette},
    render::view::Msaa,
};
use ochre_base::{block::BlockType, WATER_LEVEL, WORLD_HEIGHT};
use ochre_terrain::{
    chunk::CHUNK_SIZE,
    settings::RendererSettings,
    world::World,
};
use ochre_world::{
    ambience::Surroundings,
    atmosphere::{Atmosphere, EasedFeel},
    clouds::CloudDome,
    sky::{atmosphere_camera, FOG_MAX_OPACITY},
    tonemap::{cold_color_grading, dry_color_grading, scene_colour, TONEMAPPING},
};

use crate::player::{Player, EYE_HEIGHT, FOV, NEAR};
use bevy::ecs::entity::Entity;

#[derive(SystemParam)]
pub(crate) struct ViewAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    murk: ResMut<'w, Assets<MurkMaterial>>,
}

/// Spawns the camera as a child of `player`: HDR, atmosphere and anti-aliasing, seeing out to the fog distance.
pub(crate) fn spawn_camera(
    commands: &mut Commands,
    assets: &mut ViewAssets,
    renderer: &RendererSettings,
    player: Entity,
) {
    let fog_distance = renderer.fog_distance();
    let camera = commands
        .spawn((
            Transform::from_xyz(0.0, EYE_HEIGHT, 0.0),
            Camera3d::default(),
            atmosphere_camera(),
            Projection::Perspective(PerspectiveProjection {
                // vertical, as Minecraft's field of view setting is
                fov: FOV.to_radians(),
                near: NEAR,
                far: fog_distance + WORLD_HEIGHT as f32,
                ..default()
            }),
            // Depth of field, bloom and sunlight brighter than white all need HDR.
            Camera::default(),
            Hdr,
            TONEMAPPING,
            // Just a soft glow around the brightest sunlit faces and sky.
            Bloom {
                intensity: 0.08,
                ..Bloom::NATURAL
            },
            // TAA for the thin faces and one-block steps that shimmer at a distance
            Msaa::Off,
            // the transmission copy of the scene is what fresh water refracts and reflects
            (
                DepthPrepass,
                MotionVectorPrepass,
                ScreenSpaceTransmission::default(),
            ),
            TemporalAntiAliasing::default(),
            DistanceFog {
                color: Color::NONE.with_alpha(FOG_MAX_OPACITY),
                falloff: FogFalloff::Linear {
                    // half fogged where the farthest LOD level starts, hiding the swap to it
                    start: renderer.lod_layout().last_level_start() * CHUNK_SIZE as f32,
                    end: fog_distance,
                },
                ..default()
            },
        ))
        .insert((
            // In focus 8 blocks out, blurred ~1px at 16 blocks and ~2px from 150 on, so LOD seams soften. Gaussian is cheaper than Bokeh.
            DepthOfField {
                mode: DepthOfFieldMode::Gaussian,
                focal_distance: 8.0,
                aperture_f_stops: 2.2,
                ..default()
            },
            dry_color_grading(),
            // TAA averages the randomised filter over frames, which hides what's left of the shadow edges' crawl
            ShadowFilteringMethod::Temporal,
            // a light darkening toward the corners, which draws the eye in
            Vignette {
                intensity: 0.35,
                radius: 0.9,
                smoothness: 3.0,
                ..default()
            },
            ChildOf(player),
        ))
        .id();
    let (murk, handle) = murk(&mut assets.meshes, &mut assets.murk);
    commands.spawn((murk, ChildOf(camera)));
    commands.insert_resource(handle);
}

pub(crate) struct DryDefaults {
    fog: DistanceFog,
    dof: DepthOfField,
    color_grading: ColorGrading,
    clear_color: Color,
}

// The fog's colour underwater, picked as it should look on screen.
static UNDERWATER_COLOR: LazyLock<Color> =
    LazyLock::new(|| scene_colour(Color::srgb_u8(20, 90, 130)).into());
// Blocks from the camera to the murk, past the underwater fog's end, so it only hides the sky.
const MURK_DISTANCE: f32 = 48.0;
// Where the fog underwater in a river or lake is thick enough to hide everything, in blocks.
const FRESH_FOG_END: f32 = 160.0;

/// A sphere of the fog's colour round the camera that hides the unfogged sky underwater; always drawn, shown by alpha alone, since removing the atmosphere recompiled every material's shaders on the first dive.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct MurkMaterial {
    #[uniform(0)]
    color: LinearRgba,
}

impl Material for MurkMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/murk.wesl".into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    // seen from inside
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Resource)]
pub(crate) struct Murk(Handle<MurkMaterial>);

/// The murk, hidden, and the handle `update_underwater_effects` shows it by.
fn murk(meshes: &mut Assets<Mesh>, materials: &mut Assets<MurkMaterial>) -> (impl Bundle, Murk) {
    let material = materials.add(MurkMaterial {
        color: LinearRgba::from(*UNDERWATER_COLOR).with_alpha(0.0),
    });
    let bundle = (
        Mesh3d(meshes.add(Sphere::new(MURK_DISTANCE).mesh().ico(3).unwrap())),
        MeshMaterial3d(material.clone()),
        Transform::default(),
        NotShadowCaster,
        NotShadowReceiver,
        // it's round the camera, so always in view
        NoFrustumCulling,
    );
    (bundle, Murk(material))
}

// Restores the DistanceFog/DepthOfField/ClearColor snapshotted on entering water, not hardcoded dry defaults that could drift from setup_scene's.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn update_underwater_effects(
    world: Res<World>,
    murk: Res<Murk>,
    mut murk_materials: ResMut<Assets<MurkMaterial>>,
    mut clouds: Query<&mut Visibility, With<CloudDome>>,
    mut clear_color: ResMut<ClearColor>,
    player_query: Query<&Transform, With<Player>>,
    mut camera_query: Query<
        (
            &ChildOf,
            &Transform,
            &mut DistanceFog,
            &mut DepthOfField,
            &mut ColorGrading,
        ),
        With<Camera3d>,
    >,
    mut dry_defaults: Local<Option<DryDefaults>>,
    mut murk_shown: Local<Option<bool>>,
) {
    let (parent, camera_transform, mut fog, mut dof, mut color_grading) =
        camera_query.single_mut().expect("camera does not exist");
    let player_transform = player_query
        .get(parent.parent())
        .expect("player does not exist");

    let eye = player_transform.translation + camera_transform.translation;
    if world.get_block_at(eye.round().as_i64vec3()) == BlockType::Water {
        if dry_defaults.is_none() {
            *dry_defaults = Some(DryDefaults {
                fog: fog.clone(),
                dof: *dof,
                color_grading: color_grading.clone(),
                clear_color: clear_color.0,
            });
        }
        // rivers and lakes are clear: the sky's seen out of them, and things farther off sharp
        let fresh = eye.y > WATER_LEVEL as f32 + 1.0;
        if *murk_shown != Some(!fresh) {
            show_murk(&murk, &mut murk_materials, !fresh);
            *murk_shown = Some(!fresh);
        }
        // the clouds are blended over the scene by depth, which the murk doesn't write, so they'd show over it
        let clouds_shown = if fresh {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for mut visibility in &mut clouds {
            visibility.set_if_neq(clouds_shown);
        }
        // ClearColor matches the fog, as above water, or unloaded space shows the old background through.
        let underwater_color = *UNDERWATER_COLOR;
        *fog = DistanceFog {
            color: underwater_color,
            falloff: FogFalloff::Linear {
                start: 0.0,
                end: if fresh { FRESH_FOG_END } else { 32.0 },
            },
            ..default()
        };
        *dof = DepthOfField {
            mode: DepthOfFieldMode::Gaussian,
            focal_distance: if fresh { 4.0 } else { 1.5 },
            aperture_f_stops: if fresh { 8.0 } else { 0.5 },
            ..default()
        };
        // No white balance shift on top of the fog's water colour: it leaves the chromaticity diagram, turning blue olive.
        color_grading.global.post_saturation = 0.85;
        clear_color.0 = underwater_color;
    } else if let Some(dry) = dry_defaults.take() {
        *fog = dry.fog;
        *dof = dry.dof;
        *color_grading = dry.color_grading;
        clear_color.0 = dry.clear_color;
        show_murk(&murk, &mut murk_materials, false);
        *murk_shown = None;
        for mut visibility in &mut clouds {
            visibility.set_if_neq(Visibility::Inherited);
        }
    }
}

// How far the cold country has pulled the camera's grade, 0 to 1, and the grade as it was before that, kept to put back exactly.
#[derive(Resource, Default)]
pub(crate) struct ColdGrade {
    eased: f32,
    clean: Option<ColorGrading>,
}

// Seconds for the cold grade to cover most of the way to the country the camera is in, for the cold is sensed in steps.
const COLD_GRADE_EASE: f32 = 2.0;

// Puts back the grade under the cold's, before `update_underwater_effects` snapshots or sets it, so it sees only its own.
pub(crate) fn lift_cold_grade(mut cold: ResMut<ColdGrade>, mut camera: Query<&mut ColorGrading, With<Camera3d>>) {
    if let (Some(clean), Ok(mut grading)) = (cold.clean.take(), camera.single_mut()) {
        *grading = clean;
    }
}

// Lays the cold's grade, and the hot country's, over whatever grade the camera has now, easing in and out of the cold by how warm the surroundings are.
pub(crate) fn apply_cold_grade(
    around: Option<Res<Surroundings>>,
    feel: Option<Res<EasedFeel>>,
    time: Res<Time>,
    mut cold: ResMut<ColdGrade>,
    mut camera: Query<&mut ColorGrading, With<Camera3d>>,
) {
    let target = around.map_or(0.0, |around| 1.0 - around.warmth);
    let step = 1.0 - (-time.delta_secs() / COLD_GRADE_EASE).exp();
    cold.eased += (target - cold.eased) * step;
    // an exact zero in the warm, rather than a vanishing remainder
    if target == 0.0 && cold.eased < 1e-3 {
        cold.eased = 0.0;
    }
    let atmosphere = Atmosphere::of(&feel.map_or_else(Default::default, |feel| feel.0));
    if cold.eased == 0.0 && atmosphere == Atmosphere::of(&Default::default()) {
        return;
    }
    if let Ok(mut grading) = camera.single_mut() {
        cold.clean = Some(grading.clone());
        *grading = atmosphere.grade(&cold_color_grading(&grading, cold.eased));
    }
}

fn show_murk(murk: &Murk, materials: &mut Assets<MurkMaterial>, shown: bool) {
    if let Some(mut material) = materials.get_mut(&murk.0) {
        material.color.alpha = if shown { 1.0 } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::app::{App, Update};
    use bevy::ecs::schedule::IntoScheduleConfigs;

    #[derive(Resource, Default)]
    struct Wet(bool);

    // Stands in for `update_underwater_effects`: saturation absolute while wet, the dry grade put back once on leaving the water.
    fn fake_underwater(wet: Res<Wet>, mut was: Local<bool>, mut camera: Query<&mut ColorGrading, With<Camera3d>>) {
        let mut grading = camera.single_mut().unwrap();
        if wet.0 {
            grading.global.post_saturation = 0.85;
        } else if *was {
            *grading = dry_color_grading();
        }
        *was = wet.0;
    }

    fn grading_app() -> App {
        let mut app = App::new();
        app.init_resource::<ColdGrade>()
            .init_resource::<Surroundings>()
            .init_resource::<Wet>()
            .init_resource::<Time>()
            .add_systems(Update, (lift_cold_grade, fake_underwater, apply_cold_grade).chain());
        app.world_mut().spawn((Camera3d::default(), dry_color_grading()));
        app
    }

    fn frames(app: &mut App, count: usize) -> ColorGrading {
        for _ in 0..count {
            app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_millis(100));
            app.update();
        }
        app.world_mut().query::<&ColorGrading>().single(app.world()).unwrap().clone()
    }

    #[test]
    fn test_the_cold_grade_leaves_warm_country_exactly_as_it_was_and_returns_to_it() {
        let mut app = grading_app();
        assert_eq!(dry_color_grading(), frames(&mut app, 50));
        app.world_mut().resource_mut::<Surroundings>().warmth = 0.0;
        let mut last = dry_color_grading().global.post_saturation;
        for _ in 0..300 {
            let now = frames(&mut app, 1).global.post_saturation;
            assert!(now <= last, "saturation rose from {last} to {now} on the way into the cold");
            last = now;
        }
        let cold = cold_color_grading(&dry_color_grading(), 1.0);
        assert!((last - cold.global.post_saturation).abs() < 1e-4, "{last}");
        app.world_mut().resource_mut::<Surroundings>().warmth = 1.0;
        assert_eq!(dry_color_grading(), frames(&mut app, 400));
    }

    #[test]
    fn test_the_cold_grade_eases_in_and_does_not_jump_to_the_sensed_value() {
        let mut app = grading_app();
        app.world_mut().resource_mut::<Surroundings>().warmth = 0.0;
        let first = frames(&mut app, 1).global.post_saturation;
        let full = cold_color_grading(&dry_color_grading(), 1.0).global.post_saturation;
        let dry = dry_color_grading().global.post_saturation;
        assert!(first < dry && first > dry + (full - dry) * 0.2, "{first}");
    }

    #[test]
    fn test_the_cold_grade_composes_with_the_underwater_one_and_does_not_drift() {
        let mut app = grading_app();
        app.world_mut().resource_mut::<Surroundings>().warmth = 0.0;
        frames(&mut app, 400);
        app.world_mut().resource_mut::<Wet>().0 = true;
        let in_water = frames(&mut app, 300).global.post_saturation;
        assert!((in_water - 0.85 * 0.94).abs() < 1e-4, "{in_water}");
        assert_eq!(in_water, frames(&mut app, 600).global.post_saturation);
        app.world_mut().resource_mut::<Wet>().0 = false;
        let surfaced = frames(&mut app, 400);
        let cold = cold_color_grading(&dry_color_grading(), 1.0);
        assert!((surfaced.global.post_saturation - cold.global.post_saturation).abs() < 1e-5);
        assert!((surfaced.global.temperature - cold.global.temperature).abs() < 1e-5);
        // and wet in warm country, the underwater grade alone, then the dry one back
        app.world_mut().resource_mut::<Surroundings>().warmth = 1.0;
        frames(&mut app, 400);
        app.world_mut().resource_mut::<Wet>().0 = true;
        assert_eq!(0.85, frames(&mut app, 20).global.post_saturation);
        app.world_mut().resource_mut::<Wet>().0 = false;
        assert_eq!(dry_color_grading(), frames(&mut app, 20));
    }
}
