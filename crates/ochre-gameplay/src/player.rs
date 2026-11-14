use std::sync::LazyLock;

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{
    asset::{Asset, Assets, Handle},
    camera::{visibility::NoFrustumCulling, Camera},
    ecs::hierarchy::ChildOf,
    ecs::{
        bundle::Bundle,
        component::Component,
        message::{MessageReader, MessageWriter},
        query::{With, Without},
        resource::Resource,
        system::{Local, Query, Res, ResMut},
    },
    input::{
        keyboard::KeyCode,
        mouse::{MouseButton, MouseMotion},
        ButtonInput,
    },
    light::{NotShadowCaster, NotShadowReceiver, ShadowFilteringMethod},
    log::info,
    math::{Dir3, EulerRot, I64Vec3, Quat, Vec3, Vec3Swizzles},
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
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};

use crate::collision::{move_and_step, Aabb};
use crate::footsteps::{Footstep, FootstepPlugin, Surface, SPRINT_LOUDER};
use bevy::{
    anti_alias::taa::TemporalAntiAliasing,
    app::{App, Plugin, Update},
    camera::{Camera3d, Hdr, PerspectiveProjection, Projection},
    core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass},
    ecs::{
        schedule::IntoScheduleConfigs,
        system::{Commands, SystemParam},
    },
    light::VolumetricFog,
    pbr::{MaterialPlugin, ScreenSpaceTransmission},
    post_process::{bloom::Bloom, effect_stack::Vignette},
    render::view::Msaa,
};
use ochre_base::{block::BlockType, WATER_LEVEL, WORLD_HEIGHT};
use ochre_terrain::{
    chunk::{ChunkCoordinate, CHUNK_SIZE},
    horizon::update_horizon,
    settings::RendererSettings,
    world::World,
};
use ochre_world::{
    ambience::Surroundings,
    godrays,
    sky::{atmosphere_camera, FOG_MAX_OPACITY},
    tonemap::{cold_color_grading, dry_color_grading, scene_colour, TONEMAPPING},
};

/// The player's body, at its feet, with the camera at its eyes.
#[derive(Component, Default)]
pub struct Player {
    pub movement: Movement,
    velocity: Vec3,
    grounded: bool,
    // whether it walked into a wall last frame, so swimming against a bank climbs out
    against_wall: bool,
    // how far through its stride the head's bob is, in radians a pair of steps, and how big the bob is now
    bob_phase: f32,
    bob: f32,
    // how far the eyes still trail below the body after a step up
    eye_lag: f32,
    // seconds before the next step up is allowed, so a flight of steps isn't taken at a sprint
    step_cooldown: f32,
    // how fast it was falling on the frame it landed, else zero
    landed: f32,
    // what it pushed off on the frame it jumped, else none
    jumped: Option<Surface>,
    // how much of a jump's kick is left, from 1 at take-off, and the pitch it has added to the camera
    kick: f32,
    // the same for a landing, as a multiple of a walking step's thud
    thud: f32,
    nudge: f32,
    // how fast the field of view is opening, in radians a second
    fov_speed: f32,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Movement {
    /// Flying anywhere, through anything, the way the camera is facing.
    #[default]
    Fly,
    /// Walking under gravity, kept out of solid blocks.
    Walk,
}

const FLY_TOGGLE_KEY: KeyCode = KeyCode::KeyF;

// Blocks a second, and how much faster while Shift is held.
const MOVE_SPEED: f32 = 20.0;
const SPRINT_MULTIPLIER: f32 = 8.0;
const LOOK_SENSITIVITY: f32 = 0.1;
// Just short of straight up or down, where the view would flip over.
const MAX_PITCH: f32 = 89.0_f32.to_radians();

/// How high the camera sits above the player's feet, far enough under the top of its head
/// that the camera's near plane stays out of the ceiling.
pub const EYE_HEIGHT: f32 = 1.9;
// How close the camera's near plane is. A 90 degree view's near plane reaches `NEAR * (sin(pitch) + cos(pitch))` above the eye, at most
// `NEAR * sqrt 2`, which has to stay under the 0.1 between the eye and the top of the head, or a two-block gap shows the inside of its ceiling.
const NEAR: f32 = 0.05;
// The player's size: two blocks tall, so it just fits through a two-block gap.
const BODY: Aabb = Aabb {
    min: Vec3::new(-0.3, 0.0, -0.3),
    max: Vec3::new(0.3, 2.0, 0.3),
};
// Walking, blocks a second, and how much faster while Shift is held.
const WALK_SPEED: f32 = 5.0;
const WALK_SPRINT_MULTIPLIER: f32 = 1.5;
// Blocks a second squared, and the speed falling tops out at.
const GRAVITY: f32 = 32.0;
const TERMINAL_SPEED: f32 = 60.0;
// Enough to clear one block, not two.
const JUMP_SPEED: f32 = 9.0;
// In water: slower across, sinking slowly, Space swimming up.
const SWIM_MULTIPLIER: f32 = 0.6;
const WATER_GRAVITY: f32 = 8.0;
const SINK_SPEED: f32 = 2.0;
const SWIM_UP_SPEED: f32 = 4.0;
// Head bob walking and sprinting in blocks, blocks walked a pair of steps, and how fast it eases in and out a second.
const BOB_WALK: f32 = 0.05;
const BOB_SPRINT: f32 = 0.065;
const BOB_STRIDE: f32 = 3.6;
// Sprinting takes longer strides, so its bob is only a little quicker than walking's.
const BOB_SPRINT_STRIDE: f32 = 4.6;
const BOB_EASE: f32 = 6.0;
// Vertical field of view in degrees, how much wider it opens sprinting, and the spring it follows with: stiffness, and twice its square root in damping, which settles without overshoot.
const FOV: f32 = 90.0;
const SPRINT_FOV: f32 = 8.0;
const FOV_STIFFNESS: f32 = 36.0;
const FOV_DAMPING: f32 = 12.0;
// How fast, a share a second, the player's speed across turns toward what's wanted in the air.
const AIR_CONTROL: f32 = 3.0;
// How far up the body the water must reach for the player to be swimming.
const SWIM_CHEST: f32 = 0.6;
// Blocks are drawn centred on their coordinates, and collision takes a block to start at its coordinate.
const BLOCK_CORNER: Vec3 = Vec3::splat(0.5);
// The tallest ledge a grounded player walks up without jumping.
const STEP_HEIGHT: f32 = 1.0;
// Seconds between steps up.
const STEP_COOLDOWN: f32 = 0.1;
// How fast, a share a second, the eyes catch up with the body after a step up.
const STEP_EASE: f32 = 10.0;
// The longest frame moved in one step, so a hitch doesn't fling the player.
const MAX_STEP: f32 = 0.1;
// Gravity's multiple falling, and rising after Space is let go, so a jump arcs sharply and a tap hops.
const FALL_GRAVITY: f32 = 1.4;
const SHORT_HOP_GRAVITY: f32 = 2.5;
// A jump's push-off, as a multiple of a step's loudness, the degrees it widens the view and tips it up, and how fast, a share a second, that fades.
const TAKEOFF_LOUDNESS: f32 = 0.6;
const JUMP_FOV: f32 = 1.0;
const JUMP_PITCH: f32 = 0.5;
const KICK_DECAY: f32 = 5.0;
// How many degrees a landing narrows the view and tips it down, at a walking step's thud.
const LAND_FOV: f32 = 1.0;
const LAND_PITCH: f32 = 0.75;
// Landing: the slowest fall that counts, how far the eyes drop for each block a second it was falling and at most, and how loud the thud is, as a multiple of a step, at the softest and hardest.
const LAND_MIN_SPEED: f32 = 7.0;
const LAND_DIP: f32 = 0.012;
const LAND_DIP_MAX: f32 = 0.3;
const LAND_LOUDNESS: (f32, f32) = (0.8, 2.0);

type PlayerCamera<'a> = (&'a ChildOf, &'a mut Transform);

#[allow(clippy::type_complexity)]
fn player_move(
    time: Res<Time>,
    world: Res<World>,
    mut player_query: Query<(&mut Transform, &mut Player)>,
    mut camera_query: Query<(&ChildOf, &mut Transform), (With<Camera>, Without<Player>)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut steps: MessageWriter<Footstep>,
    mut projection: Query<&mut Projection, With<Camera>>,
) {
    let (parent, mut camera) = camera_query.single_mut().expect("camera does not exist");
    let (mut transform, mut player) = player_query
        .get_mut(parent.parent())
        .expect("player does not exist");
    if keys.just_pressed(FLY_TOGGLE_KEY) {
        player.movement = match player.movement {
            Movement::Walk => Movement::Fly,
            Movement::Fly => Movement::Walk,
        };
        player.velocity = Vec3::ZERO;
        info!("{:?}ing", player.movement);
    }
    let dt = time.delta_secs().min(MAX_STEP);
    let axis = |negative, positive| {
        if keys.pressed(negative) {
            -1.0
        } else if keys.pressed(positive) {
            1.0
        } else {
            0.0
        }
    };
    let across = Vec3::new(
        axis(KeyCode::KeyA, KeyCode::KeyD),
        0.0,
        axis(KeyCode::KeyW, KeyCode::KeyS),
    );
    let sprinting = keys.pressed(KeyCode::ShiftLeft);

    match player.movement {
        Movement::Fly => {
            let speed = MOVE_SPEED * dt * if sprinting { SPRINT_MULTIPLIER } else { 1.0 };
            let up = -axis(KeyCode::Space, KeyCode::ControlLeft);
            let step = transform.rotation * camera.rotation * across + Vec3::Y * up;
            transform.translation += step * speed;
        }
        Movement::Walk => walk(
            &world,
            &mut transform,
            &mut player,
            across,
            sprinting,
            &keys,
            dt,
        ),
    }

    // the head bobs a little walking and more sprinting, as far through a stride as the player has walked
    let across_speed = player.velocity.xz().length();
    let striding = player.movement == Movement::Walk && player.grounded && across_speed > 0.1;
    let target = match (striding, sprinting) {
        (false, _) => 0.0,
        (true, false) => BOB_WALK,
        (true, true) => BOB_SPRINT,
    };
    player.bob += (target - player.bob) * (BOB_EASE * dt).min(1.0);
    let stride = if sprinting {
        BOB_SPRINT_STRIDE
    } else {
        BOB_STRIDE
    };
    let before = player.bob_phase;
    player.bob_phase = (player.bob_phase + across_speed * dt * TAU / stride) % TAU;
    if striding && footfall(before, player.bob_phase) {
        if let Some(surface) = Surface::under(&world, transform.translation) {
            let loudness = if sprinting { SPRINT_LOUDER } else { 1.0 };
            steps.write(Footstep { surface, loudness });
        }
    }
    let rushing = sprinting
        && across != Vec3::ZERO
        && (player.movement == Movement::Fly || across_speed > 0.1);
    if let Ok(mut projection) = projection.single_mut() {
        if let Projection::Perspective(perspective) = &mut *projection {
            let wanted = (FOV + if rushing { SPRINT_FOV } else { 0.0 } + JUMP_FOV * player.kick
                - LAND_FOV * player.thud)
                .to_radians();
            let pull = (wanted - perspective.fov) * FOV_STIFFNESS - player.fov_speed * FOV_DAMPING;
            player.fov_speed += pull * dt;
            perspective.fov += player.fov_speed * dt;
        }
    }
    if let Some(surface) = player.jumped.take() {
        steps.write(Footstep {
            surface,
            loudness: TAKEOFF_LOUDNESS,
        });
        player.kick = 1.0;
    }
    player.kick -= player.kick * (KICK_DECAY * dt).min(1.0);
    player.thud -= player.thud * (KICK_DECAY * dt).min(1.0);
    let landed = std::mem::take(&mut player.landed);
    if landed > 0.0 {
        let hardness = (landed / JUMP_SPEED).clamp(LAND_LOUDNESS.0, LAND_LOUDNESS.1);
        player.thud = hardness;
        player.eye_lag = (player.eye_lag + landed * LAND_DIP).min(player.eye_lag.max(LAND_DIP_MAX));
        if let Some(surface) = Surface::under(&world, transform.translation) {
            steps.write(Footstep {
                surface,
                loudness: hardness,
            });
        }
    }
    player.eye_lag -= player.eye_lag * (STEP_EASE * dt).min(1.0);
    camera.translation = Vec3::new(
        player.bob * 0.5 * player.bob_phase.sin(),
        EYE_HEIGHT - player.eye_lag - player.bob * (1.0 - (2.0 * player.bob_phase).cos()) / 2.0,
        0.0,
    );
}

// Whether the bob's phase passed where the head is lowest, a quarter and three quarters of the way round.
fn footfall(before: f32, after: f32) -> bool {
    let side = |phase: f32| ((phase + FRAC_PI_2) / PI) as u32 % 2;
    side(before) != side(after)
}

fn walk(
    world: &World,
    transform: &mut Transform,
    player: &mut Player,
    across: Vec3,
    sprinting: bool,
    keys: &ButtonInput<KeyCode>,
    dt: f32,
) {
    // swimming until the chest is out, so the head bobs well clear of the water
    let chest = transform.translation + Vec3::Y * SWIM_CHEST;
    let swimming = world.get_block_at(chest.round().as_i64vec3()) == BlockType::Water;
    // just over the feet, so wading through a shallow river slows the player as swimming does
    let feet = transform.translation + Vec3::Y * 0.1;
    let wading = world.get_block_at(feet.round().as_i64vec3()) == BlockType::Water;
    let jumping = keys.pressed(KeyCode::Space);

    let mut speed = WALK_SPEED;
    if sprinting {
        speed *= WALK_SPRINT_MULTIPLIER;
    }
    if swimming || wading {
        speed *= SWIM_MULTIPLIER;
    }
    // where the body faces, not the camera, so looking down doesn't slow it
    let wanted = transform.rotation * across.normalize_or_zero() * speed;
    // in the air the player keeps its way and only steers toward where it's heading, as the air holds it back
    let grip = if player.grounded || swimming || wading {
        1.0
    } else {
        (AIR_CONTROL * dt).min(1.0)
    };
    player.velocity.x += (wanted.x - player.velocity.x) * grip;
    player.velocity.z += (wanted.z - player.velocity.z) * grip;

    if swimming {
        player.velocity.y = if jumping && player.against_wall {
            JUMP_SPEED
        } else if jumping {
            SWIM_UP_SPEED
        } else {
            (player.velocity.y - WATER_GRAVITY * dt).max(-SINK_SPEED)
        };
    } else {
        if jumping && player.grounded {
            player.velocity.y = JUMP_SPEED;
            player.jumped = Surface::under(world, transform.translation);
        }
        // falling with the feet still in water is gentle, so the head stays up a while, but a jump out of shallows isn't
        let gravity = if wading && player.velocity.y <= 0.0 {
            WATER_GRAVITY
        } else if player.velocity.y > 0.0 && !jumping {
            GRAVITY * SHORT_HOP_GRAVITY
        } else if player.velocity.y < 0.0 {
            GRAVITY * FALL_GRAVITY
        } else {
            GRAVITY
        };
        player.velocity.y = (player.velocity.y - gravity * dt).max(-TERMINAL_SPEED);
    }

    // Chunks not generated yet are solid, so the player waits where it is for the ground to
    // load rather than falling through it.
    let solid = |block: I64Vec3| {
        !world.is_chunk_generated(ChunkCoordinate::containing(block))
            || world.get_block_at(block).is_solid()
    };
    player.step_cooldown = (player.step_cooldown - dt).max(0.0);
    let ready = player.grounded && !swimming && player.step_cooldown == 0.0;
    let step = if ready { STEP_HEIGHT } else { 0.0 };
    let (position, hit) = move_and_step(
        transform.translation + BLOCK_CORNER,
        player.velocity * dt,
        BODY,
        step,
        solid,
    );
    // a step up moves the body a block in one frame, so the eyes start where they were and ease up
    let rose = position.y - BLOCK_CORNER.y - transform.translation.y;
    if step > 0.0 && rose > 0.0 {
        player.eye_lag = (player.eye_lag + rose).min(STEP_HEIGHT);
        player.step_cooldown = STEP_COOLDOWN;
    }
    transform.translation = position - BLOCK_CORNER;
    let fall = -player.velocity.y;
    let was_grounded = player.grounded;
    player.grounded = hit.y && player.velocity.y < 0.0;
    if player.grounded && !was_grounded && !swimming && !wading && fall >= LAND_MIN_SPEED {
        player.landed = fall;
    }
    if hit.y {
        player.velocity.y = 0.0;
    }
    if hit.x {
        player.velocity.x = 0.0;
    }
    if hit.z {
        player.velocity.z = 0.0;
    }
    player.against_wall = (hit.x || hit.z) && across != Vec3::ZERO;
}

/// Whether the cursor is held in the window, so the mouse turns the player.
pub fn cursor_grabbed(cursor: Query<&CursorOptions, With<PrimaryWindow>>) -> bool {
    cursor
        .single()
        .is_ok_and(|cursor| cursor.grab_mode != CursorGrabMode::None)
}

/// Holds the cursor in the window once it's clicked into, and lets it go on Escape or when the
/// window loses focus.
pub fn grab_cursor(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    let grabbed = cursor.grab_mode != CursorGrabMode::None;
    if grabbed && (keys.just_pressed(KeyCode::Escape) || !window.focused) {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    } else if !grabbed && window.focused && buttons.just_pressed(MouseButton::Left) {
        // X11 can't lock the cursor, so it's confined to the window there instead
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

fn player_look(
    time: Res<Time>,
    mut player_query: Query<(&mut Transform, &mut Player)>,
    mut camera_query: Query<PlayerCamera, (With<Camera>, Without<Player>)>,
    mut motion_evr: MessageReader<MouseMotion>,
    cursor: Query<&CursorOptions, With<PrimaryWindow>>,
) {
    if !cursor_grabbed(cursor) {
        motion_evr.clear();
        return;
    }
    let (parent, mut camera) = camera_query.single_mut().expect("camera does not exist");
    let (mut body, mut player) = player_query
        .get_mut(parent.parent())
        .expect("player does not exist");
    let turn = LOOK_SENSITIVITY * time.delta_secs();
    // the camera only ever turns about x, under the body's turn about y, and the jump's tip is taken off and put back on each frame
    let (tipped, _, _) = camera.rotation.to_euler(EulerRot::XYZ);
    let mut pitch = tipped - player.nudge;
    for ev in motion_evr.read() {
        body.rotate_axis(Dir3::Y, -ev.delta.x * turn);
        pitch = (pitch - ev.delta.y * turn).clamp(-MAX_PITCH, MAX_PITCH);
    }
    let tip = JUMP_PITCH * player.kick - LAND_PITCH * player.thud;
    let shown = (pitch + tip.to_radians()).clamp(-MAX_PITCH, MAX_PITCH);
    player.nudge = shown - pitch;
    camera.rotation = Quat::from_rotation_x(shown);
}

pub struct DryDefaults {
    fog: DistanceFog,
    dof: DepthOfField,
    color_grading: ColorGrading,
    clear_color: Color,
}

// The fog's colour underwater, picked as it should look on screen.
static UNDERWATER_COLOR: LazyLock<Color> =
    LazyLock::new(|| scene_colour(Color::srgb_u8(20, 90, 130)).into());
// How far round the camera the murk hangs, in blocks: past the underwater fog's end, where
// everything is already its colour, so it only hides the sky.
const MURK_DISTANCE: f32 = 48.0;
// Where the fog underwater in a river or lake is thick enough to hide everything, in blocks.
const FRESH_FOG_END: f32 = 160.0;

/// The sky isn't fogged, so underwater it's hidden behind a sphere of the fog's colour hung
/// round the camera. It's always drawn, and only shows when its alpha is 1, so going under
/// changes nothing but a uniform: taking the atmosphere off the camera instead changed what
/// every material's shaders were compiled with, which stalled the first dive recompiling
/// them all.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct MurkMaterial {
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
pub struct Murk(Handle<MurkMaterial>);

/// The murk, hidden, to hang from the camera, and the handle `update_underwater_effects`
/// shows it by.
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

// Snapshots whichever DistanceFog/DepthOfField/ClearColor were active right before the camera
// entered water, and restores that exact snapshot on exit, instead of hardcoding "dry" defaults
// that could drift out of sync with setup_scene's render-distance-derived fog settings.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_underwater_effects(
    world: Res<World>,
    murk: Res<Murk>,
    mut murk_materials: ResMut<Assets<MurkMaterial>>,
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
        With<Camera>,
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
        // Matches ClearColor to the fog color, same trick setup_scene uses above water (its
        // ClearColor equals the dry fog's color) - otherwise empty/unloaded space shows the old
        // background color straight through, breaking the underwater illusion.
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
        // The fog's colour is already the water's: a white balance shift on top, like the
        // -0.4 this once had, moves the white point off the chromaticity diagram, where Bevy's
        // balance matrix turns blue into olive.
        color_grading.global.post_saturation = 0.85;
        clear_color.0 = underwater_color;
    } else if let Some(dry) = dry_defaults.take() {
        *fog = dry.fog;
        *dof = dry.dof;
        *color_grading = dry.color_grading;
        clear_color.0 = dry.clear_color;
        show_murk(&murk, &mut murk_materials, false);
        *murk_shown = None;
    }
}

// How far the cold country has pulled the camera's grade, 0 to 1, and the grade as it was before that, kept to put back exactly.
#[derive(Resource, Default)]
struct ColdGrade {
    eased: f32,
    clean: Option<ColorGrading>,
}

// Seconds for the cold grade to cover most of the way to the country the camera is in, for the cold is sensed in steps.
const COLD_GRADE_EASE: f32 = 2.0;

// Puts back the grade under the cold's, before `update_underwater_effects` snapshots or sets it, so it sees only its own.
fn lift_cold_grade(mut cold: ResMut<ColdGrade>, mut camera: Query<&mut ColorGrading, With<Camera>>) {
    if let (Some(clean), Ok(mut grading)) = (cold.clean.take(), camera.single_mut()) {
        *grading = clean;
    }
}

// Lays the cold's grade over whatever grade the camera has now, easing in and out of it by how warm the surroundings are.
fn apply_cold_grade(
    around: Option<Res<Surroundings>>,
    time: Res<Time>,
    mut cold: ResMut<ColdGrade>,
    mut camera: Query<&mut ColorGrading, With<Camera>>,
) {
    let target = around.map_or(0.0, |around| 1.0 - around.warmth);
    let step = 1.0 - (-time.delta_secs() / COLD_GRADE_EASE).exp();
    cold.eased += (target - cold.eased) * step;
    // an exact zero in the warm, rather than a vanishing remainder
    if target == 0.0 && cold.eased < 1e-3 {
        cold.eased = 0.0;
    }
    if cold.eased == 0.0 {
        return;
    }
    if let Ok(mut grading) = camera.single_mut() {
        cold.clean = Some(grading.clone());
        *grading = cold_color_grading(&grading, cold.eased);
    }
}

fn show_murk(murk: &Murk, materials: &mut Assets<MurkMaterial>, shown: bool) {
    if let Some(mut material) = materials.get_mut(&murk.0) {
        material.color.alpha = if shown { 1.0 } else { 0.0 };
    }
}
/// Walking and flying, looking round, and the view underwater.
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((MaterialPlugin::<MurkMaterial>::default(), FootstepPlugin))
            .init_resource::<ColdGrade>()
            .add_systems(
                Update,
                (
                    player_move,
                    grab_cursor,
                    player_look,
                    update_horizon,
                    lift_cold_grade,
                    update_underwater_effects,
                    apply_cold_grade,
                )
                    .chain(),
            );
    }
}

#[derive(SystemParam)]
pub struct PlayerAssets<'w> {
    meshes: ResMut<'w, Assets<Mesh>>,
    murk: ResMut<'w, Assets<MurkMaterial>>,
}

/// Spawns the player at `position` facing `look`, with a camera that sees out to the renderer's
/// fog distance, and the murk the camera sees through underwater.
pub fn spawn_player(
    commands: &mut Commands,
    assets: &mut PlayerAssets,
    renderer: &RendererSettings,
    position: Vec3,
    look: Vec3,
) {
    let fog_distance = renderer.fog_distance();
    let player = commands
        .spawn((
            Player::default(),
            Transform::from_translation(position).looking_to(look, Vec3::Y),
        ))
        .id();
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
                    // half fogged where the farthest LOD level starts, hiding the swap to it,
                    // see `fog.wesl`
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
    if renderer.godrays {
        // no ambient: it'd veil the whole view, the land has its own
        commands.entity(camera).insert(VolumetricFog {
            ambient_intensity: 0.0,
            jitter: 4.0,
            ..default()
        });
        commands.spawn((godrays::volume(), ChildOf(camera)));
    }
    let (murk, handle) = murk(&mut assets.meshes, &mut assets.murk);
    commands.spawn((murk, ChildOf(camera)));
    commands.insert_resource(handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_footfalls_a_stride_where_the_head_is_lowest() {
        let count = (0..2000)
            .filter(|i| {
                footfall(
                    *i as f32 * TAU / 2000.0,
                    (*i + 1) as f32 * TAU / 2000.0 % TAU,
                )
            })
            .count();
        assert_eq!(count, 2);
        assert!(footfall(FRAC_PI_2 - 0.01, FRAC_PI_2 + 0.01));
        assert!(!footfall(0.1, 0.2));
    }

    // In a two-block gap the camera is `BODY.max.y - EYE_HEIGHT` under the ceiling, and a view of `fov` degrees at `pitch` has its near
    // plane's top edge `NEAR * (sin(pitch) + cos(pitch) * tan(fov / 2))` above the eye: it must never reach it, however far up the
    // player looks, or the camera is inside the block above.
    #[test]
    fn test_the_near_plane_stays_out_of_the_ceiling_of_a_two_block_gap() {
        let headroom = BODY.max.y - EYE_HEIGHT;
        let reach = |pitch: f32| NEAR * (pitch.sin() + pitch.cos() * ((FOV + SPRINT_FOV + JUMP_FOV).to_radians() / 2.0).tan());
        for degrees in 0..=89 {
            let up = reach((degrees as f32).to_radians());
            assert!(up < headroom, "at {degrees} degrees up the near plane reaches {up}, headroom {headroom}");
        }
        assert!(reach(MAX_PITCH) < headroom);
        // the eye is inside the body, which fits a two-block gap exactly
        const { assert!(EYE_HEIGHT < BODY.max.y && BODY.max.y == 2.0) };
    }

    #[derive(Resource, Default)]
    struct Wet(bool);

    // Stands in for `update_underwater_effects`: saturation absolute while wet, the dry grade put back once on leaving the water.
    fn fake_underwater(wet: Res<Wet>, mut was: Local<bool>, mut camera: Query<&mut ColorGrading, With<Camera>>) {
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
        app.world_mut().spawn((Camera::default(), dry_color_grading()));
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
