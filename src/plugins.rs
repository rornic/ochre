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
    sky::SkyPlugin,
    sky_dome::{NightSkyPlugin, SunGlarePlugin},
    weather::WeatherPlugin,
};

use crate::settings::Settings;

/// Builds the world `OchrePlugins` plays in, from `settings.world`. Takes seconds, so call it before the window opens.
pub fn load_world(settings: &Settings) -> World {
    ochre_world::load_world(
        settings.renderer.lod_layout().region_reach() as u32,
        settings.world,
    )
}

/// Every Ochre plugin, for a game that wants the lot. Turn parts off with `.disable::<P>()` and swap them with
/// `.set(P { .. })`, as with Bevy's `DefaultPlugins`. Needs `DefaultPlugins`, with `task_pool_plugin()` set, added first.
///
/// `TerrainPlugin` is the one the rest stand on. The rest each work without the others: where a plugin reads another's
/// resource it makes its own blank one, so what's disabled is simply absent. `settings.renderer.frost` and
/// `godrays` start `FrostPlugin` and `GodrayPlugin` disabled.
pub struct OchrePlugins {
    pub settings: Settings,
    world: World,
}

impl OchrePlugins {
    pub fn new(settings: Settings, world: World) -> Self {
        Self { settings, world }
    }
}

impl PluginGroup for OchrePlugins {
    fn build(self) -> PluginGroupBuilder {
        let Settings { renderer, debug, .. } = self.settings;
        let mut group = PluginGroupBuilder::start::<Self>()
            .add(TerrainPlugin::new(self.world, renderer, debug))
            .add(TerrainRenderPlugin { renderer })
            .add(CurvaturePlugin)
            .add(FrostPlugin)
            .add(SkyPlugin::new(&renderer))
            .add(NightSkyPlugin)
            .add(SunGlarePlugin)
            .add(CloudPlugin {
                resolution: renderer.cloud_resolution,
            })
            .add(WeatherPlugin)
            .add(GodrayPlugin)
            .add(PlayerPlugin::default())
            .add(FootstepPlugin)
            .add(BreakingPlugin)
            .add(AmbiencePlugin)
            .add(MusicPlugin)
            .add(CritterPlugin)
            .add(BirdPlugin)
            .add(DebugOverlayPlugin)
            .add(FlythroughPlugin);
        if !renderer.frost {
            group = group.disable::<FrostPlugin>();
        }
        if !renderer.godrays {
            group = group.disable::<GodrayPlugin>();
        }
        group
    }
}
