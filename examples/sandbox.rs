use bevy::prelude::*;
use ochre_engine::settings::Settings;
use ochre_engine::{load_world, DevEnv, OchrePlugins};
use ochre_gameplay::player::PlayerPlugin;
use ochre_terrain::task_pool_plugin;

// `OCHRE_RESOLUTION=WxH`, for reading frame times at a fixed size.
fn window_size() -> Option<(u32, u32)> {
    let value = std::env::var("OCHRE_RESOLUTION").ok()?;
    let (w, h) = value.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

fn main() {
    let settings = Settings::read("assets/settings.toml").expect("Failed to read settings.toml");
    // a flight is 1080p, and unsynced only when its frames are saved, as its real-time run keeps the desktop's compositor its share of the GPU
    let flight = std::env::var_os("OCHRE_FLIGHT").is_some();
    let flight_frames = std::env::var_os("OCHRE_FLIGHT_FRAMES").is_some();
    // made before the window exists: a window whose app is busy for seconds has wedged the desktop's shell
    let dev = DevEnv::from_env();
    let game_world = load_world(&settings, &dev.world);
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(ImagePlugin::default_nearest())
            .set(task_pool_plugin())
            .set(WindowPlugin {
                primary_window: Some(Window {
                    // uncapped, the GPU runs flat out and can starve the desktop's compositor until the PC stalls
                    // a recording runs unsynced, as the desktop throttles a hidden vsynced window to a crawl
                    present_mode: if flight_frames || std::env::var_os("OCHRE_NOVSYNC").is_some()
                    {
                        bevy::window::PresentMode::AutoNoVsync
                    } else {
                        bevy::window::PresentMode::AutoVsync
                    },
                    // a flight is recorded at video size
                    resolution: if flight {
                        bevy::window::WindowResolution::new(1920, 1080)
                    } else if let Some((w, h)) = window_size() {
                        bevy::window::WindowResolution::new(w, h)
                    } else {
                        default()
                    },
                    ..default()
                }),
                ..default()
            }),
    ))
    .add_plugins(
        OchrePlugins::new(settings, game_world)
            .with_dev(dev)
            .set(PlayerPlugin {
                look: look_direction(),
                // OCHRE_FLY starts flying, so the camera hovers at the spawn for framing a view
                fly: std::env::var_os("OCHRE_FLY").is_some(),
            }),
    )
    .insert_resource(winit_settings())
    .insert_resource(ClearColor(Color::BLACK))
    .add_systems(Update, timed_shot)
    .run();
}

// the stats runs are unfocused, which Bevy otherwise throttles to 60 fps
fn winit_settings() -> bevy::winit::WinitSettings {
    use bevy::winit::{UpdateMode, WinitSettings};
    if std::env::var("OCHRE_STATS").is_ok() {
        WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        }
    } else {
        WinitSettings::game()
    }
}

// OCHRE_LOOK=x,y,z points the camera that way at the start, for framing a place by its coordinates
fn look_direction() -> Vec3 {
    std::env::var("OCHRE_LOOK")
        .ok()
        .and_then(|s| {
            let mut parts = s.split(',').map(|p| p.trim().parse::<f32>().ok());
            Some(Vec3::new(parts.next()??, parts.next()??, parts.next()??))
        })
        .filter(|v| v.length() > 0.0)
        .unwrap_or(Vec3::Z)
}

// OCHRE_SHOT=seconds[,path] saves the window once that many seconds in, whatever is in front of it on the desktop
fn timed_shot(mut commands: Commands, time: Res<Time>, mut taken: Local<bool>) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    let Ok(spec) = std::env::var("OCHRE_SHOT") else { return };
    let (secs, path) = spec.split_once(',').unwrap_or((spec.as_str(), "shot.png"));
    if *taken || time.elapsed_secs() < secs.parse().unwrap_or(30.0) {
        return;
    }
    *taken = true;
    commands.spawn(Screenshot::primary_window()).observe(save_to_disk(String::from(path)));
}
