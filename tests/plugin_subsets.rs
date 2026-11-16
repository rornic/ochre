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
};

fn settings() -> Settings {
    toml::from_str("[renderer]\nfrost = true\ngodrays = true\n[world]\nsize = 4096\nmap_cell = 32")
        .unwrap()
}

// A few frames of the game with the plugins `trim` leaves.
fn run(name: &str, trim: impl FnOnce(bevy::app::PluginGroupBuilder) -> bevy::app::PluginGroupBuilder) {
    eprintln!("=== without {name}");
    let settings = settings();
    let world = load_world(&settings);
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
    for _ in 0..3 {
        app.update();
        std::thread::sleep(Duration::from_millis(20));
    }
}

macro_rules! without {
    ($($plugin:ty),+) => {
        |group: bevy::app::PluginGroupBuilder| group$(.disable::<$plugin>())+
    };
}

#[test]
fn each_plugin_can_be_left_out() {
    std::env::set_var("OCHRE_SEED", "1");
    std::env::set_var("OCHRE_WORLD_DIR", std::env::temp_dir().join("ochre-plugin-subsets"));
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
