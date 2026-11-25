
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::{
    app::{App, FixedUpdate, Plugin, RunFixedMainLoop, RunFixedMainLoopSystems, Startup, Update},
    camera::{Camera3d, Projection},
    ecs::hierarchy::ChildOf,
    ecs::{
        component::Component,
        message::{MessageReader, MessageWriter},
        query::{With, Without},
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Commands, Query, Res},
    },
    input::{
        keyboard::KeyCode,
        mouse::{MouseButton, MouseMotion},
        ButtonInput,
    },
    log::info,
    math::{Dir3, EulerRot, Quat, Vec2, Vec3, Vec3Swizzles},
    pbr::MaterialPlugin,
    prelude::Transform,
    time::{Fixed, Time},
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use ochre_terrain::{
    floor::Floor,
    group::ChunkGroup,
    horizon::update_horizon,
    settings::RendererSettings,
    world::World,
};
use ochre_world::ambience::SurroundingsPlugin;

use crate::character::{
    step_character, CharacterState, Movement, MovementInput, StepEvents, WorldEnvironment, JUMP_SPEED, STEP_HEIGHT,
};
use crate::footsteps::{Footstep, Surface, SPRINT_LOUDER};
use crate::view::{
    apply_cold_grade, lift_cold_grade, spawn_camera, update_underwater_effects, ColdGrade,
    MurkMaterial, ViewAssets,
};

/// The player body at its feet, with the camera at its eyes; `CharacterState` is what it does, this what it shows.
#[derive(Component, Default)]
pub struct Player {
    // how far through its stride the head's bob is, in radians a pair of steps, and how big the bob is now
    bob_phase: f32,
    bob: f32,
    // how far the eyes still trail below the body after a step up
    eye_lag: f32,
    // how fast it was falling on the step it landed, else zero
    landed: f32,
    // what it pushed off on the step it jumped, else none
    jumped: Option<Surface>,
    // how much of a jump's kick is left, from 1 at take-off, and the pitch it has added to the camera
    kick: f32,
    // the same for a landing, as a multiple of a walking step's thud
    thud: f32,
    nudge: f32,
    // how fast the field of view is opening, in radians a second
    fov_speed: f32,
    // how far the last step moved the body, so the camera can trail it between steps
    moved: Vec3,
    /// Degrees a game narrows the view by, which it eases to like the rest of the field of view.
    pub zoom: f32,
    /// Where a game puts the eyes, from the feet in the body's frame, in place of `EYE_HEIGHT` and the head's bob, as from an animated head.
    pub eye: Option<Vec3>,
    /// Leaves a walking stride's footsteps to the game, as from an animated body's foot strikes; jumps and landings still sound.
    pub quiet_strides: bool,
}

impl Player {
    /// What a step of `events` that moved the body from `start` to `end` adds to the view.
    pub fn after_step(&mut self, world: &World, start: Vec3, end: Vec3, events: &StepEvents) {
        self.moved = end - start;
        // a step up moves the body a block in one go, so the eyes start where they were and ease up
        self.eye_lag = (self.eye_lag + events.rose).min(STEP_HEIGHT);
        if events.landed > 0.0 {
            self.landed = events.landed;
        }
        if let Some(at) = events.jumped {
            self.jumped = Surface::under(world, at);
        }
    }
}

const FLY_TOGGLE_KEY: KeyCode = KeyCode::KeyF;

const LOOK_SENSITIVITY: f32 = 0.1;
// Just short of straight up or down, where the view would flip over.
const MAX_PITCH: f32 = 89.0_f32.to_radians();

/// Camera height above the feet, far enough under the head that the near plane stays out of the ceiling.
pub const EYE_HEIGHT: f32 = 1.9;
// The near plane reaches at most `NEAR * sqrt 2` above the eye, which must stay under the 0.1 to the head's top.
pub(crate) const NEAR: f32 = 0.05;
// Head bob walking and sprinting in blocks, blocks walked a pair of steps, and how fast it eases in and out a second.
const BOB_WALK: f32 = 0.05;
const BOB_SPRINT: f32 = 0.065;
const BOB_STRIDE: f32 = 3.6;
const BOB_EASE: f32 = 6.0;
// Sprinting takes longer strides, so its bob is only a little quicker than walking's.
const BOB_SPRINT_STRIDE: f32 = 4.6;
// Vertical FOV in degrees, the extra when sprinting, and the follow spring: stiffness, damping 2 * sqrt(stiffness).
pub(crate) const FOV: f32 = 90.0;
const SPRINT_FOV: f32 = 8.0;
const FOV_STIFFNESS: f32 = 36.0;
const FOV_DAMPING: f32 = 12.0;
// How fast, a share a second, the eyes catch up with the body after a step up.
const STEP_EASE: f32 = 10.0;
// The longest frame the view moves in one go, so a hitch doesn't fling it.
const MAX_STEP: f32 = 0.1;
// A jump's push-off loudness as a multiple of a step's, degrees it widens and tips the view, fade rate per second
const TAKEOFF_LOUDNESS: f32 = 0.6;
const JUMP_FOV: f32 = 1.0;
const JUMP_PITCH: f32 = 0.5;
const KICK_DECAY: f32 = 5.0;
// How many degrees a landing narrows the view and tips it down, at a walking step's thud.
const LAND_FOV: f32 = 1.0;
const LAND_PITCH: f32 = 0.75;
// Landing: eye drop per block/s of fall and its cap, and thud loudness as a multiple of a step, softest and hardest.
const LAND_DIP: f32 = 0.012;
const LAND_DIP_MAX: f32 = 0.3;
const LAND_LOUDNESS: (f32, f32) = (0.8, 2.0);

type PlayerCamera<'a> = (&'a ChildOf, &'a mut Transform);

/// Reads the keys and the way the player faces into what the next steps do.
fn capture_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut player: Query<(&mut MovementInput, &Transform, &Player)>,
    camera: Query<&Transform, (With<Camera3d>, Without<Player>)>,
) {
    let (Ok((mut input, body, player)), Ok(eye)) = (player.single_mut(), camera.single()) else {
        return;
    };
    let axis = |negative, positive| {
        if keys.pressed(negative) {
            -1.0
        } else if keys.pressed(positive) {
            1.0
        } else {
            0.0
        }
    };
    // the jump's tip is taken off, so only where the player looks steers a flight
    let (pitch, _, _) = eye.rotation.to_euler(EulerRot::XYZ);
    *input = MovementInput {
        across: Vec2::new(axis(KeyCode::KeyA, KeyCode::KeyD), axis(KeyCode::KeyW, KeyCode::KeyS)),
        lift: -axis(KeyCode::Space, KeyCode::ControlLeft),
        sprint: keys.pressed(KeyCode::ShiftLeft),
        jump: keys.pressed(KeyCode::Space),
        // latched until a step takes it, as a press can fall between two steps
        toggle_fly: input.toggle_fly || keys.just_pressed(FLY_TOGGLE_KEY),
        yaw: body.rotation.to_euler(EulerRot::YXZ).0,
        pitch: pitch - player.nudge,
        ..MovementInput::default()
    };
}

#[allow(clippy::type_complexity)]
fn player_step(
    time: Res<Time>,
    world: Res<World>,
    mut player_query: Query<(&mut Transform, &mut CharacterState, &mut MovementInput, &mut Player)>,
    floors: Query<(&ChunkGroup, &Transform, &Floor), Without<Player>>,
) {
    let Ok((mut transform, mut state, mut input, mut player)) = player_query.single_mut() else {
        return;
    };
    let env = WorldEnvironment::around(&world, transform.translation, floors.iter());

    let start = transform.translation;
    let mut position = start;
    let events = step_character(&mut state, &mut position, &input, &env, time.delta_secs());
    input.toggle_fly = false;
    transform.translation = position;
    player.after_step(&world, start, position, &events);
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn player_present(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    world: Res<World>,
    mut player_query: Query<(&Transform, &CharacterState, &MovementInput, &mut Player)>,
    mut camera_query: Query<(&ChildOf, &mut Transform), (With<Camera3d>, Without<Player>)>,
    mut steps: MessageWriter<Footstep>,
    mut projection: Query<&mut Projection, With<Camera3d>>,
) {
    let (parent, mut camera) = camera_query.single_mut().expect("camera does not exist");
    let (transform, state, input, mut player) = player_query
        .get_mut(parent.parent())
        .expect("player does not exist");
    let dt = time.delta_secs().min(MAX_STEP);
    let sprinting = input.sprint;

    // the head bobs a little walking and more sprinting, as far through a stride as the player has walked
    let across_speed = state.velocity.xz().length();
    let striding = state.movement == Movement::Walk && state.grounded && across_speed > 0.1;
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
    if striding && !player.quiet_strides && footfall(before, player.bob_phase) {
        if let Some(surface) = Surface::under(&world, transform.translation) {
            let loudness = if sprinting { SPRINT_LOUDER } else { 1.0 };
            steps.write(Footstep { surface, loudness, at: None });
        }
    }
    let rushing = sprinting
        && input.across != Vec2::ZERO
        && (state.movement == Movement::Fly || across_speed > 0.1);
    if let Ok(mut projection) = projection.single_mut() {
        if let Projection::Perspective(perspective) = &mut *projection {
            let wanted = (FOV + if rushing { SPRINT_FOV } else { 0.0 } + JUMP_FOV * player.kick
                - LAND_FOV * player.thud
                - player.zoom)
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
            at: None,
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
                at: None,
            });
        }
    }
    player.eye_lag -= player.eye_lag * (STEP_EASE * dt).min(1.0);
    // the body moves in steps, so the eyes sit where it was and catch up by the time the next step lands
    let trail = transform.rotation.inverse() * (-player.moved * (1.0 - fixed.overstep_fraction()));
    let eye = player.eye.unwrap_or_else(|| {
        Vec3::new(player.bob * 0.5 * player.bob_phase.sin(), EYE_HEIGHT - player.bob * (1.0 - (2.0 * player.bob_phase).cos()) / 2.0, 0.0)
    });
    camera.translation = trail + eye - Vec3::Y * player.eye_lag;
}

// Whether the bob's phase passed where the head is lowest, a quarter and three quarters of the way round.
fn footfall(before: f32, after: f32) -> bool {
    let side = |phase: f32| ((phase + FRAC_PI_2) / PI) as u32 % 2;
    side(before) != side(after)
}

/// Whether the cursor is held in the window, so the mouse turns the player.
pub fn cursor_grabbed(cursor: Query<&CursorOptions, With<PrimaryWindow>>) -> bool {
    cursor
        .single()
        .is_ok_and(|cursor| cursor.grab_mode != CursorGrabMode::None)
}

/// Holds the cursor in the window once clicked, releasing it on Escape or lost focus.
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
    limit: Res<LookLimit>,
    mut player_query: Query<(&mut Transform, &mut Player)>,
    mut camera_query: Query<PlayerCamera, (With<Camera3d>, Without<Player>)>,
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
    // the camera only turns about x, under the body's turn about y; the jump's tip is taken off and put back each frame
    let (tipped, _, _) = camera.rotation.to_euler(EulerRot::XYZ);
    let mut pitch = tipped - player.nudge;
    for ev in motion_evr.read() {
        body.rotate_axis(Dir3::Y, -ev.delta.x * turn);
        pitch = (pitch - ev.delta.y * turn).clamp(-limit.0, limit.0);
    }
    let tip = JUMP_PITCH * player.kick - LAND_PITCH * player.thud;
    let shown = (pitch + tip.to_radians()).clamp(-limit.0, limit.0);
    player.nudge = shown - pitch;
    camera.rotation = Quat::from_rotation_x(shown);
}

/// Walking and flying, looking round, and the view underwater. Spawns the player at the world's spawn, facing `look`.
pub struct PlayerPlugin {
    pub look: Vec3,
    /// Starts flying, so the player hovers where it spawns.
    pub fly: bool,
    /// Leaves stepping to the game, which sets `Transform` and `CharacterState` and calls `Player::after_step`.
    pub driven: bool,
    /// How far the view turns up or down from level, in radians, at most 89 degrees.
    pub max_pitch: f32,
}

impl Default for PlayerPlugin {
    fn default() -> Self {
        Self { look: Vec3::Z, fly: false, driven: false, max_pitch: MAX_PITCH }
    }
}

#[derive(Resource)]
struct LookLimit(f32);

#[derive(Resource)]
struct PlayerSpawn {
    look: Vec3,
    fly: bool,
}

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        // past 89 degrees the view would flip over the top and the near plane reach through a two-block gap's ceiling
        app.insert_resource(LookLimit(self.max_pitch.clamp(0.0, MAX_PITCH)));
        // the cold grade follows the climate it senses
        if !app.is_plugin_added::<SurroundingsPlugin>() {
            app.add_plugins(SurroundingsPlugin);
        }
        app.add_plugins(MaterialPlugin::<MurkMaterial>::default())
            .add_message::<Footstep>()
            .insert_resource(PlayerSpawn { look: self.look, fly: self.fly })
            .init_resource::<ColdGrade>()
            .add_systems(Startup, spawn_player)
            .add_systems(RunFixedMainLoop, capture_input.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop))
            .add_systems(
                Update,
                (
                    player_present,
                    grab_cursor,
                    player_look,
                    update_horizon,
                    lift_cold_grade,
                    update_underwater_effects,
                    apply_cold_grade,
                )
                    .chain(),
            );
        if !self.driven {
            app.add_systems(FixedUpdate, player_step);
        }
    }
}

fn spawn_player(
    mut commands: Commands,
    mut assets: ViewAssets,
    renderer: Res<RendererSettings>,
    world: Res<World>,
    spawn: Res<PlayerSpawn>,
) {
    let (position, look) = (world.spawn(), spawn.look);
    info!("spawned at {position:?}");
    let player = commands
        .spawn((
            Player::default(),
            CharacterState::new(if spawn.fly { Movement::Fly } else { Movement::Walk }),
            MovementInput::default(),
            // the body turns only about the vertical, as a step does, and the camera takes the look's tilt
            Transform::from_translation(position).looking_to(look.with_y(0.0).try_normalize().unwrap_or(Vec3::NEG_Z), Vec3::Y),
        ))
        .id();
    let pitch = look.normalize_or(Vec3::NEG_Z).y.clamp(-1.0, 1.0).asin();
    spawn_camera(&mut commands, &mut assets, &renderer, player, pitch);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::BODY;

    #[test]
    fn after_step_trails_the_view_and_eases_a_step_up_and_keeps_a_landing() {
        let world = World::with_terrain(1, 16, ochre_world::worldmap::test_terrain(1));
        let mut player = Player::default();
        let events = StepEvents { rose: 1.0, landed: 3.0, jumped: None };
        player.after_step(&world, Vec3::ZERO, Vec3::new(0.0, 1.0, 0.5), &events);
        assert_eq!(player.moved, Vec3::new(0.0, 1.0, 0.5));
        assert_eq!(player.eye_lag, STEP_HEIGHT.min(1.0));
        assert_eq!(player.landed, 3.0);
        // a step with no landing leaves the last one for the view to take
        player.after_step(&world, Vec3::ZERO, Vec3::X, &StepEvents { rose: 0.0, landed: 0.0, jumped: None });
        assert_eq!(player.landed, 3.0);
    }

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

    // in a two-block gap the camera sits `BODY.max.y - EYE_HEIGHT` under a ceiling the near plane must not reach
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
}
