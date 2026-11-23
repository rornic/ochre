use bevy::app::{PluginGroup, PluginGroupBuilder};
use ochre_gameplay::{
    debug::DebugOverlayPlugin,
    flythrough::FlythroughPlugin, footsteps::FootstepPlugin, player::PlayerPlugin,
    remote::RemoteControlPlugin,
};
use ochre_terrain::{
    curvature::CurvaturePlugin, frost::FrostPlugin, world::World, TerrainPlugin,
    TerrainRenderPlugin,
};
use ochre_world::{
    ambience::AmbiencePlugin,
    clouds::CloudPlugin,
    godrays::GodrayPlugin,
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

/// Every Ochre plugin, disabled or swapped as in `DefaultPlugins` (added first, with `task_pool_plugin()`); `settings.plugins` starts `FrostPlugin` and `GodrayPlugin` disabled.
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

    /// With the sky, weather, overlay and recording options `dev` carries.
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
            .add(CloudPlugin)
            .add(WeatherPlugin {
                clear_sky: dev.clear_sky,
            })
            .add(GodrayPlugin)
            .add(PlayerPlugin::default())
            .add(FootstepPlugin)
            .add(AmbiencePlugin)
            .add(DebugOverlayPlugin { stats: dev.stats })
            .add(FlythroughPlugin { flight: dev.flight })
            .add(RemoteControlPlugin { port: dev.remote });
        if !plugins.frost {
            group = group.disable::<FrostPlugin>();
        }
        if !plugins.godrays {
            group = group.disable::<GodrayPlugin>();
        }
        group
    }
}
