// Renders the sky and clouds to a PNG with no window, and prints the clouds' GPU times, for judging their look and cost headlessly.
// `cargo run --release --example cloud_shot -- out.png [width=1920] [height=1080] [yaw=0] [pitch=15] [eye height=200]`; `OCHRE_TIME` sets the time of day, `CLOUD_SPIN` turns the camera that many degrees a frame.
use std::time::Duration;

use bevy::{
    app::ScheduleRunnerPlugin,
    camera::{Hdr, RenderTarget},
    core_pipeline::prepass::DepthPrepass,
    diagnostic::{DiagnosticPath, DiagnosticsStore},
    prelude::*,
    render::{
        diagnostic::RenderDiagnosticsPlugin,
        render_resource::TextureFormat,
        view::{
            screenshot::{save_to_disk, Screenshot},
            Msaa,
        },
    },
    window::ExitCondition,
    winit::WinitPlugin,
};
use ochre_engine::{load_world, settings::Settings, DevEnv};
use ochre_terrain::{curvature::CurvaturePlugin, task_pool_plugin};
use ochre_world::{
    clouds::CloudPlugin,
    sky::{atmosphere_camera, SkyPlugin, FOG_MAX_OPACITY, START_TIME},
    sky_dome::{NightSkyPlugin, SunGlarePlugin},
    tonemap::{dry_color_grading, TONEMAPPING},
};

// Frames the clouds' history takes to settle, then frames timed, then frames for the file to be written.
const SETTLE: u32 = 120;
const TIMED: u32 = 120;
const DRAIN: u32 = 30;
const WARMUP: f32 = 8.0;

#[derive(Resource)]
struct Shot {
    out: String,
    image: Handle<Image>,
    view: (f32, f32, f32),
    size: UVec2,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize, default: f32| args.get(i).and_then(|a| a.parse().ok()).unwrap_or(default);
    let size = UVec2::new(arg(1, 1920.0) as u32, arg(2, 1080.0) as u32);
    let settings = Settings::read("assets/settings.toml").expect("Failed to read settings.toml");
    let dev = DevEnv::from_env();
    // the world's seed picks the clouds, and its forecast the weather
    let world = load_world(&settings, &dev.world);
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(task_pool_plugin())
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(Duration::from_millis(1)),
            RenderDiagnosticsPlugin,
            CurvaturePlugin,
            SkyPlugin {
                start: dev.time.unwrap_or(START_TIME),
                ..SkyPlugin::new(settings.sky)
            },
            NightSkyPlugin,
            SunGlarePlugin,
            CloudPlugin,
        ))
        .insert_resource(world)
        .insert_resource(Shot {
            out: args.first().cloned().unwrap_or_else(|| "cloud_shot.png".into()),
            image: default(),
            view: (arg(3, 0.0), arg(4, 15.0), arg(5, 200.0)),
            size,
        })
        .add_systems(Startup, spawn_camera)
        .add_systems(Update, run)
        .run();
}

fn spawn_camera(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut shot: ResMut<Shot>) {
    shot.image = images.add(Image::new_target_texture(
        shot.size.x,
        shot.size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let (yaw, pitch, height) = shot.view;
    commands.spawn((
        Camera3d::default(),
        Camera::default(),
        RenderTarget::Image(shot.image.clone().into()),
        Transform::from_xyz(0.0, height, 0.0).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            -yaw.to_radians(),
            pitch.to_radians(),
            0.0,
        )),
        Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            far: 60_000.0,
            ..default()
        }),
        atmosphere_camera(),
        Hdr,
        TONEMAPPING,
        dry_color_grading(),
        Msaa::Off,
        DepthPrepass,
        DistanceFog {
            color: Color::NONE.with_alpha(FOG_MAX_OPACITY),
            falloff: FogFalloff::Linear {
                start: 8000.0,
                end: 16000.0,
            },
            ..default()
        },
    ));
}

fn run(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
    mut frame: Local<u32>,
    shot: Res<Shot>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    // shaders compile in the background, so frames only count once they've had a while
    if time.elapsed_secs() < WARMUP {
        return;
    }
    *frame += 1;
    // CLOUD_SPIN degrees of yaw a frame, ending at the view's yaw at the shot, for judging the clouds in motion
    if let Some(spin) = std::env::var("CLOUD_SPIN").ok().and_then(|v| v.parse::<f32>().ok()) {
        let (yaw, pitch, _) = shot.view;
        let turned = yaw + spin * (*frame as f32 - SETTLE as f32);
        for mut at in &mut camera {
            at.rotation = Quat::from_euler(EulerRot::YXZ, -turned.to_radians(), pitch.to_radians(), 0.0);
        }
    }
    if *frame == SETTLE {
        commands
            .spawn(Screenshot::image(shot.image.clone()))
            .observe(save_to_disk(shot.out.clone()));
    }
    // a second frame a few later, for diffing how much a still camera's clouds flicker
    if *frame == SETTLE + 3 {
        commands
            .spawn(Screenshot::image(shot.image.clone()))
            .observe(save_to_disk(shot.out.replace(".png", ".next.png")));
    }
    if *frame == SETTLE + TIMED {
        for name in [
            "render/cloud_march/elapsed_gpu",
            "render/cloud_reflection/elapsed_gpu",
            "render/transparent_main_pass/elapsed_gpu",
        ] {
            let path = DiagnosticPath::new(name);
            match diagnostics.get(&path).and_then(|d| d.average()) {
                Some(ms) => println!("{name}: {ms:.3} ms"),
                None => println!("{name}: none"),
            }
        }
        for d in diagnostics.iter() {
            if d.path().as_str().ends_with("elapsed_gpu") {
                println!("  {}: {:.3} ms", d.path(), d.average().unwrap_or(0.0));
            }
        }
    }
    if *frame > SETTLE + TIMED + DRAIN {
        exit.write(AppExit::Success);
    }
}
