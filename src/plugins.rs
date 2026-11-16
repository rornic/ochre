use bevy::app::{PluginGroup, PluginGroupBuilder};
use ochre_gameplay::{
    birds::BirdPlugin, breaking::BreakingPlugin, critters::CritterPlugin, debug::DebugOverlayPlugin,
    flythrough::FlythroughPlugin, footsteps::FootstepPlugin, player::PlayerPlugin,
};
use ochre_terrain::{
    curvature::CurvaturePlugin, frost::FrostPlugin, world::World, TerrainPlugin,
    TerrainRenderPlugin,
};
use ochre_world::{
    ambience::AmbiencePlugin,
    clouds::CloudPlugin,
    godrays::GodrayPlugin,
    music::MusicPlugin,
    sky::{SkyPlugin, START_TIME},
    sky_dome::{NightSkyPlugin, SunGlarePlugin},
    weather::WeatherPlugin,
    WorldOptions,
};

use crate::dev::DevEnv;
use crate::settings::Settings;

/// Builds the world `OchrePlugins` plays in, from `settings.world`. Takes seconds, so call it before the window opens.
pub fn load_world(settings: &Settings, options: &WorldOptions) -> World {
    ochre_world::load_world(
        settings.renderer.lod_layout().region_reach() as u32,
        settings.world,
        options,
    )
}

/// Every Ochre plugin, for a game that wants the lot. Turn parts off with `.disable::<P>()` and swap them with
/// `.set(P { .. })`, as with Bevy's `DefaultPlugins`. Needs `DefaultPlugins`, with `task_pool_plugin()` set, added first.
///
/// `TerrainPlugin` is the one the rest stand on. The rest each work without the others: where a plugin reads another's
/// resource it makes its own blank one, so what's disabled is simply absent. `settings.plugins` starts `FrostPlugin` and
/// `GodrayPlugin` disabled, as the two that cost frame time or look.
pub struct OchrePlugins {
    pub settings: Settings,
    world: World,
    dev: DevEnv,
}

impl OchrePlugins {
    pub fn new(settings: Settings, world: World) -> Self {
        Self {
            settings,
            world,
            dev: DevEnv::default(),
        }
    }

    /// With the sky, weather, music, overlay and recording options `dev` carries.
    pub fn with_dev(mut self, dev: DevEnv) -> Self {
        self.dev = dev;
        self
    }
}

impl PluginGroup for OchrePlugins {
    fn build(self) -> PluginGroupBuilder {
        let Settings {
            renderer,
            sky,
            plugins,
            debug,
            ..
        } = self.settings;
        let dev = self.dev;
        let mut group = PluginGroupBuilder::start::<Self>()
            .add(TerrainPlugin::new(self.world, renderer, debug))
            .add(TerrainRenderPlugin { renderer })
            .add(CurvaturePlugin)
            .add(FrostPlugin)
            .add(SkyPlugin {
                start: dev.time.unwrap_or(START_TIME),
                time_scale: dev.time_scale.unwrap_or(1.0),
                ..SkyPlugin::new(sky)
            })
            .add(NightSkyPlugin)
            .add(SunGlarePlugin)
            .add(CloudPlugin {
                resolution: sky.cloud_resolution,
            })
            .add(WeatherPlugin {
                clear_sky: dev.clear_sky,
            })
            .add(GodrayPlugin)
            .add(PlayerPlugin::default())
            .add(FootstepPlugin)
            .add(BreakingPlugin)
            .add(AmbiencePlugin)
            .add(MusicPlugin {
                first_delay: dev.music_delay,
            })
            .add(CritterPlugin)
            .add(BirdPlugin)
            .add(DebugOverlayPlugin { stats: dev.stats })
            .add(FlythroughPlugin { flight: dev.flight });
        if !plugins.frost {
            group = group.disable::<FrostPlugin>();
        }
        if !plugins.godrays {
            group = group.disable::<GodrayPlugin>();
        }
        group
    }
}
