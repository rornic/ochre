//! Every `OchrePlugins` plugin but the terrain's can be left out without anything else panicking, run headless with no GPU.
//! `TerrainRenderPlugin` stays: it registers the material assets `TerrainPlugin` builds on, as `init_asset` does for `lod_sim`.
use std::time::Duration;

use bevy::app::{PluginGroup, ScheduleRunnerPlugin};
use bevy::prelude::*;
use bevy::render::{settings::WgpuSettings, RenderPlugin};
use bevy::winit::WinitPlugin;
use ochre_engine::settings::Settings;
use ochre_engine::{load_world, OchrePlugins};
use ochre_gameplay::{
    birds::BirdPlugin, breaking::BreakingPlugin, critters::CritterPlugin, debug::DebugOverlayPlugin,
    flythrough::FlythroughPlugin, footsteps::FootstepPlugin, player::PlayerPlugin,
    remote::RemoteControlPlugin,
};
use ochre_terrain::{
    curvature::CurvaturePlugin, frost::FrostPlugin, task_pool_plugin,
};
use ochre_world::{
    ambience::AmbiencePlugin,
    clouds::CloudPlugin,
    godrays::GodrayPlugin,
    music::MusicPlugin,
    sky::SkyPlugin,
    sky_dome::{NightSkyPlugin, SunGlarePlugin},
    weather::WeatherPlugin,
    WorldOptions,
};

fn settings() -> Settings {
    toml::from_str("[renderer]\n[world]\nsize = 4096\nmap_cell = 32").unwrap()
}

// A few frames of the game with the plugins `trim` leaves.
fn run(name: &str, trim: impl FnOnce(bevy::app::PluginGroupBuilder) -> bevy::app::PluginGroupBuilder) {
    eprintln!("=== without {name}");
    let mut app = headless(trim);
    for _ in 0..3 {
        app.update();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn headless(trim: impl FnOnce(bevy::app::PluginGroupBuilder) -> bevy::app::PluginGroupBuilder) -> App {
    let settings = settings();
    let options = WorldOptions {
        seed: Some(1),
        dir: std::env::temp_dir().join("ochre-plugin-subsets"),
        ..default()
    };
    let world = load_world(&settings, &options);
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .build()
            .disable::<WinitPlugin>()
            .set(task_pool_plugin())
            .set(RenderPlugin {
                render_creation: WgpuSettings {
                    backends: None,
                    ..default()
                }
                .into(),
                ..default()
            }),
        ScheduleRunnerPlugin::run_once(),
    ))
    .add_plugins(trim(OchrePlugins::new(settings, world).build()));
    app.finish();
    app.cleanup();
    app
}

macro_rules! without {
    ($($plugin:ty),+) => {
        |group: bevy::app::PluginGroupBuilder| group$(.disable::<$plugin>())+
    };
}

#[test]
fn each_plugin_can_be_left_out() {
    std::env::remove_var("OCHRE_FLYTHROUGH");
    run("nothing", |group| group);
    run("CurvaturePlugin", without!(CurvaturePlugin));
    run("FrostPlugin", without!(FrostPlugin));
    run("SkyPlugin", without!(SkyPlugin));
    run("NightSkyPlugin", without!(NightSkyPlugin));
    run("SunGlarePlugin", without!(SunGlarePlugin));
    run("CloudPlugin", without!(CloudPlugin));
    run("WeatherPlugin", without!(WeatherPlugin));
    run("GodrayPlugin", without!(GodrayPlugin));
    run("PlayerPlugin", without!(PlayerPlugin));
    run("FootstepPlugin", without!(FootstepPlugin));
    run("BreakingPlugin", without!(BreakingPlugin));
    run("AmbiencePlugin", without!(AmbiencePlugin));
    run("MusicPlugin", without!(MusicPlugin));
    run("CritterPlugin", without!(CritterPlugin));
    run("BirdPlugin", without!(BirdPlugin));
    run("DebugOverlayPlugin", without!(DebugOverlayPlugin));
    run("FlythroughPlugin", without!(FlythroughPlugin));
    // everything but the terrain and what draws it
    run("all but terrain", without!(
        CurvaturePlugin,
        FrostPlugin,
        SkyPlugin,
        NightSkyPlugin,
        SunGlarePlugin,
        CloudPlugin,
        WeatherPlugin,
        GodrayPlugin,
        PlayerPlugin,
        FootstepPlugin,
        BreakingPlugin,
        AmbiencePlugin,
        MusicPlugin,
        CritterPlugin,
        BirdPlugin,
        DebugOverlayPlugin,
        FlythroughPlugin
    ));
}

// One JSON-RPC call over HTTP, its response body.
fn call(port: u16, method: &str, params: &str) -> String {
    use std::io::{Read, Write};
    let body = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{params}}}"#);
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(stream, "POST / HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn remote_control_teleports_and_reports_over_http() {
    let port = 15771;
    let mut app = headless(|group| group.set(RemoteControlPlugin { port: Some(port) }));
    let client = std::thread::spawn(move || {
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            std::thread::sleep(Duration::from_millis(20));
        }
        let teleported = call(port, "ochre.teleport", r#"{"x":100,"z":-200,"y":150,"yaw":90,"pitch":-10}"#);
        let state = call(port, "ochre.state", "null");
        let bad = call(port, "ochre.teleport", r#"{"x":"east"}"#);
        (teleported, state, bad)
    });
    while !client.is_finished() {
        app.update();
        std::thread::sleep(Duration::from_millis(5));
    }
    let (teleported, state, bad) = client.join().unwrap();
    assert!(teleported.contains(r#""eye":[100.0,150.0,-200.0]"#), "{teleported}");
    let eye = r#""eye":[100.0,150.0,-200.0]"#;
    assert!(state.contains(eye) && state.contains(r#""seed":1"#) && state.contains(r#""yaw":90"#), "{state}");
    assert!(bad.contains("-32602"), "{bad}");
}
