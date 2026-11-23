//! `RemoteControlPlugin` serves the Bevy Remote Protocol on localhost, with `ochre.*` methods for driving the game.
use bevy::{
    prelude::*,
    remote::{error_codes, http::RemoteHttpPlugin, BrpError, BrpResult, RemotePlugin},
    render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured},
};
use serde_json::{json, Value};

use crate::flythrough::{facing, ground_height};
use crate::character::{CharacterState, Movement};
use crate::player::{Player, EYE_HEIGHT};
use ochre_terrain::chunk_loader::{Chunk, DirtyChunk, GenerateChunkData, GenerateChunkMesh};
use ochre_terrain::lod::{GenerateLodMesh, LodNode};
use ochre_terrain::world::World;
use ochre_world::sky::TimeOfDay;
use ochre_world::weather::Weather;

/// With a port, serves BRP plus the `ochre.*` methods on 127.0.0.1 only; render sub-app port 15703 allows one game.
#[derive(Default)]
pub struct RemoteControlPlugin {
    pub port: Option<u16>,
}

impl Plugin for RemoteControlPlugin {
    fn build(&self, app: &mut App) {
        let Some(port) = self.port else {
            return;
        };
        app.add_plugins((
            RemotePlugin::default()
                .with_method_main("ochre.teleport", teleport)
                .with_method_main("ochre.time", set_time)
                .with_method_main("ochre.state", state)
                .with_method_main("ochre.screenshot", screenshot),
            RemoteHttpPlugin::default().with_port(port),
        ))
        .init_resource::<Remote>()
        .add_systems(Update, (track_settling, take_shots).chain());
        info!("remote control on http://127.0.0.1:{port}");
    }
}

#[derive(Resource, Default)]
struct Remote {
    // frames in a row with no chunk or LOD work pending, and real seconds since the camera was last moved by a method
    quiet_frames: u32,
    since_move: f32,
    // shots waiting for the next frame, or for loading to settle if they ask to, and the last one written
    queued: Vec<(String, bool)>,
    last_shot: Option<String>,
}

// shortcut: "settled" is no chunk or LOD task for a run of frames, not every node drawn; upgrade if shots hit holes.
const QUIET_FRAMES: u32 = 30;
const MIN_SETTLE_SECONDS: f32 = 2.0;
// A shot is taken anyway after this long, as moving water can keep chunks remeshing.
const MAX_SETTLE_SECONDS: f32 = 90.0;
// a page in the user's browser can reach this server, so it may not queue shots without end
const MAX_QUEUED_SHOTS: usize = 16;

impl Remote {
    fn settled(&self) -> bool {
        self.since_move >= MIN_SETTLE_SECONDS && self.quiet_frames >= QUIET_FRAMES
    }
}

type Pending = Or<(
    With<GenerateChunkData>,
    With<DirtyChunk>,
    With<GenerateChunkMesh>,
    With<GenerateLodMesh>,
)>;

fn track_settling(time: Res<Time<Real>>, pending: Query<(), Pending>, mut remote: ResMut<Remote>) {
    remote.since_move += time.delta_secs();
    remote.quiet_frames = if pending.is_empty() { remote.quiet_frames + 1 } else { 0 };
}

fn take_shots(mut commands: Commands, mut remote: ResMut<Remote>) {
    let ready = remote.settled() || remote.since_move > MAX_SETTLE_SECONDS;
    let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut remote.queued).into_iter().partition(|&(_, settle)| ready || !settle);
    remote.queued = later;
    for (path, _) in now {
        let mut save = save_to_disk(path.clone());
        commands
            .spawn(Screenshot::primary_window())
            .observe(move |captured: On<ScreenshotCaptured>, mut remote: ResMut<Remote>| {
                save(captured);
                remote.last_shot = Some(path.clone());
            });
    }
}

fn number(params: &Option<Value>, key: &str) -> BrpResult<Option<f32>> {
    match params.as_ref().and_then(|p| p.get(key)) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().map(|n| Some(n as f32)).ok_or_else(|| invalid(format!("`{key}` must be a number"))),
    }
}

fn invalid(message: String) -> BrpError {
    BrpError { code: error_codes::INVALID_PARAMS, message, data: None }
}

fn teleport(
    In(params): In<Option<Value>>,
    world: Res<World>,
    mut remote: ResMut<Remote>,
    mut player: Query<(&mut Transform, &mut CharacterState), With<Player>>,
    mut camera: Query<&mut Transform, (With<Camera3d>, Without<Player>)>,
) -> BrpResult {
    let (Some(x), Some(z)) = (number(&params, "x")?, number(&params, "z")?) else {
        return Err(invalid("want `x` and `z`".into()));
    };
    let (Ok((mut body, mut player)), Ok(mut eye)) = (player.single_mut(), camera.single_mut()) else {
        return Err(BrpError { code: error_codes::INTERNAL_ERROR, message: "no player".into(), data: None });
    };
    let y = match number(&params, "y")? {
        Some(y) => y,
        None => ground_height(&world, Vec3::new(x, 0.0, z)) + number(&params, "height")?.unwrap_or(3.0),
    };
    let (yaw, pitch) = compass(eye.rotation, body.rotation);
    let look = facing(number(&params, "yaw")?.unwrap_or(yaw), number(&params, "pitch")?.unwrap_or(pitch));
    // flying, so it holds still in the air while the ground under it loads
    player.movement = Movement::Fly;
    body.translation = Vec3::new(x, y - EYE_HEIGHT, z);
    body.rotation = Quat::from_rotation_y(f32::atan2(-look.x, -look.z));
    eye.rotation = Quat::from_rotation_x(look.y.clamp(-1.0, 1.0).asin());
    remote.since_move = 0.0;
    remote.quiet_frames = 0;
    Ok(json!({ "eye": [x, y, z] }))
}

// The camera's compass yaw and pitch in degrees, as `facing` takes them.
fn compass(eye: Quat, body: Quat) -> (f32, f32) {
    let forward = (body * eye) * Vec3::NEG_Z;
    (f32::atan2(forward.x, -forward.z).to_degrees().rem_euclid(360.0), forward.y.clamp(-1.0, 1.0).asin().to_degrees())
}

fn set_time(In(params): In<Option<Value>>, clock: Option<ResMut<TimeOfDay>>) -> BrpResult {
    let day = number(&params, "day")?.ok_or_else(|| invalid("want `day`".into()))?;
    let mut clock = clock.ok_or_else(|| invalid("no sky clock: SkyPlugin is off".into()))?;
    // keeps the day count, so the weather, which runs on the clock's ticks, doesn't jump back
    let whole = (clock.0 / TimeOfDay::from_days(1.0).0) as f32;
    *clock = TimeOfDay::from_days(whole + day.rem_euclid(1.0));
    Ok(json!({ "day": clock.days() }))
}

#[allow(clippy::too_many_arguments)]
fn state(
    In(_): In<Option<Value>>,
    world: Res<World>,
    remote: Res<Remote>,
    diagnostics: Option<Res<bevy::diagnostic::DiagnosticsStore>>,
    clock: Option<Res<TimeOfDay>>,
    weather: Option<Res<Weather>>,
    player: Query<&Transform, With<Player>>,
    camera: Query<(&GlobalTransform, &Transform), With<Camera3d>>,
    chunks: Query<(), With<Chunk>>,
    lods: Query<(), With<LodNode>>,
) -> BrpResult {
    let (Ok(body), Ok((eye, eye_local))) = (player.single(), camera.single()) else {
        return Err(BrpError { code: error_codes::INTERNAL_ERROR, message: "no player".into(), data: None });
    };
    let at = eye.translation();
    let (yaw, pitch) = compass(eye_local.rotation, body.rotation);
    let terrain = world.region_store.terrain();
    let sample = terrain.sample(at.x as f64, at.z as f64);
    let fps = diagnostics
        .as_ref()
        .and_then(|d| d.get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed()));
    Ok(json!({
        "seed": world.seed(),
        "eye": [at.x, at.y, at.z],
        "yaw": yaw,
        "pitch": pitch,
        "ground": sample.height,
        "biome": terrain.biomes().name(sample.biome),
        "day": clock.map(|c| c.days()),
        "weather": weather.map(|w| json!({ "rain": w.rain, "snow": w.snow, "cloud": w.cloud, "storm": w.storm })),
        "fps": fps,
        "chunks": chunks.iter().count(),
        "lod_nodes": lods.iter().count(),
        "settled": remote.settled(),
        "shots_queued": remote.queued.len(),
        "last_shot": remote.last_shot,
    }))
}

fn screenshot(In(params): In<Option<Value>>, mut remote: ResMut<Remote>) -> BrpResult {
    let dir = std::path::Path::new("target/remote");
    let name = match params.as_ref().and_then(|p| p.get("path")) {
        None | Some(Value::Null) => {
            let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
            format!("shot-{n}.png")
        }
        Some(Value::String(name)) => name.clone(),
        Some(_) => return Err(invalid("`path` must be a string".into())),
    };
    if !is_shot_name(&name) {
        return Err(invalid("`path` must be a bare .png file name, saved in target/remote".into()));
    }
    if remote.queued.len() >= MAX_QUEUED_SHOTS {
        return Err(invalid(format!("{MAX_QUEUED_SHOTS} shots are queued already")));
    }
    std::fs::create_dir_all(dir).map_err(|e| invalid(e.to_string()))?;
    let path = dir.join(name).to_string_lossy().into_owned();
    let settle = match params.as_ref().and_then(|p| p.get("settle")) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(settle)) => *settle,
        Some(_) => return Err(invalid("`settle` must be true or false".into())),
    };
    remote.queued.push((path.clone(), settle));
    Ok(json!({ "path": path }))
}

// a page in the user's browser can reach this server, so a shot may only land in target/remote
fn is_shot_name(name: &str) -> bool {
    name.ends_with(".png") && !name.starts_with('.') && !name.contains(['/', '\\', '\0'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_shot_name_cannot_leave_the_shot_directory() {
        assert!(is_shot_name("cliff.png"));
        for bad in ["../x.png", "/etc/x.png", "a/b.png", "a\\\\b.png", ".png", ".hidden.png", "x.txt", ""] {
            assert!(!is_shot_name(bad), "{bad}");
        }
    }

    #[test]
    fn test_compass_reads_back_the_yaw_and_pitch_facing_was_given() {
        for (yaw, pitch) in [(0.0, 0.0), (90.0, 10.0), (200.0, -30.0), (359.0, 60.0)] {
            let look = facing(yaw, pitch);
            let body = Quat::from_rotation_y(f32::atan2(-look.x, -look.z));
            let eye = Quat::from_rotation_x(look.y.asin());
            let (y, p) = compass(eye, body);
            assert!((y - yaw).abs() < 1e-2 && (p - pitch).abs() < 1e-2, "{yaw} {pitch} read as {y} {p}");
        }
    }
}
