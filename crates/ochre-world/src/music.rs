//! Music that drifts in now and then: one track picked for the camera's biome and the weather,
//! long silences between, crossfading when the mood changes under a playing track.
use bevy::{
    app::{App, Plugin, Update},
    asset::AssetServer,
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings, Volume},
    ecs::{
        component::Component,
        entity::Entity,
        schedule::IntoScheduleConfigs,
        system::{Commands, Local, Query, Res},
    },
    time::Time,
};

use crate::{
    ambience::{SenseSurroundings, Surroundings, SurroundingsPlugin},
    weather::Weather,
};

/// Plays one track at a time, picked by `Surroundings`.
pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SurroundingsPlugin>() {
            app.add_plugins(SurroundingsPlugin);
        }
        app.add_systems(Update, play_music.after(SenseSurroundings));
    }
}

/// What a track is for; the first that holds picks the track.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Storm,
    Desert,
    Cold,
    Forest,
    Rainforest,
    Savanna,
    Steppe,
    Grassland,
}

impl Mood {
    pub fn of(around: &Surroundings, rain: f32) -> Mood {
        if rain > 0.4 {
            Mood::Storm
        } else if around.desert > 0.5 {
            Mood::Desert
        } else {
            match around.biome {
                Some("tundra" | "taiga" | "glacier") => Mood::Cold,
                Some("forest") => Mood::Forest,
                Some("jungle" | "swamp") => Mood::Rainforest,
                Some("savanna") => Mood::Savanna,
                Some("steppe") => Mood::Steppe,
                _ => Mood::Grassland,
            }
        }
    }

    // shortcut: one track a mood, add a pick among several here when there are more
    fn file(self) -> &'static str {
        match self {
            Mood::Storm => "sounds/music/storm.ogg",
            Mood::Desert => "sounds/music/desert.ogg",
            Mood::Cold => "sounds/music/cold.ogg",
            Mood::Forest => "sounds/music/forest.ogg",
            Mood::Rainforest => "sounds/music/rainforest.ogg",
            Mood::Savanna => "sounds/music/savanna.ogg",
            Mood::Steppe => "sounds/music/steppe.ogg",
            Mood::Grassland => "sounds/music/grassland.ogg",
        }
    }
}

/// A playing track, how long its mood has stopped fitting, and whether it's on its way out.
#[derive(Component)]
struct Track {
    mood: Mood,
    mismatch: f32,
    fading: bool,
}

// The files are levelled to -20 LUFS with their fades baked in; this sits them under the ambience.
const MUSIC_VOLUME: f32 = 0.15;
// Seconds of silence between tracks, at the least and most, skewed short by squaring the draw.
const GAP_SECONDS: (f32, f32) = (60.0, 600.0);
// Seconds before the first track, so a run doesn't open on minutes of silence.
const FIRST_SECONDS: (f32, f32) = (10.0, 40.0);
// Seconds a track rides out a change of mood (a biome's patchy edge, a shower) before crossfading away.
const GRACE_SECONDS: f32 = 20.0;
// Seconds the volume takes to follow its target, so a fade is about three times this.
const FOLLOW_SECONDS: f32 = 4.0;
// A fading track is dropped once it's this quiet, as a linear volume.
const SILENT: f32 = 1e-3;

#[allow(clippy::too_many_arguments)]
fn play_music(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    around: Res<Surroundings>,
    weather: Option<Res<Weather>>,
    mut tracks: Query<(Entity, &mut Track, Option<&mut AudioSink>)>,
    mut wait: Local<Option<f32>>,
    mut played: Local<bool>,
) {
    let dt = time.delta_secs();
    let mood = Mood::of(
        &around,
        weather.map_or(0.0, |weather| weather.precipitation()),
    );
    let (mut live, mut handover) = (false, false);
    for (entity, mut track, sink) in &mut tracks {
        if !track.fading {
            let fits = mood == track.mood && !around.underwater;
            track.mismatch = if fits { 0.0 } else { track.mismatch + dt };
            if track.mismatch >= GRACE_SECONDS {
                track.fading = true;
                // the next track starts as this one fades, unless the cause is the water
                handover = !around.underwater;
            }
        }
        live |= !track.fading;
        // the sink arrives a frame after the player is spawned
        let Some(mut sink) = sink else {
            continue;
        };
        let wanted = if track.fading { 0.0 } else { MUSIC_VOLUME };
        let current = sink.volume().to_linear();
        let volume = current + (wanted - current) * (1.0 - (-dt / FOLLOW_SECONDS).exp());
        sink.set_volume(Volume::Linear(volume));
        if track.fading && volume < SILENT {
            commands.entity(entity).despawn();
        }
    }
    if live {
        return;
    }
    if handover {
        *wait = Some(0.0);
    }
    let left = wait.get_or_insert_with(|| {
        if *played {
            let draw: f32 = rand::random();
            GAP_SECONDS.0 + (GAP_SECONDS.1 - GAP_SECONDS.0) * draw * draw
        } else {
            // `OCHRE_MUSIC_DELAY=<seconds>` fixes it, so a recording knows when the music starts
            std::env::var("OCHRE_MUSIC_DELAY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| rand::random_range(FIRST_SECONDS.0..FIRST_SECONDS.1))
        }
    });
    *left -= dt;
    if *left > 0.0 || around.underwater {
        return;
    }
    *wait = None;
    *played = true;
    commands.spawn((
        Track {
            mood,
            mismatch: 0.0,
            fading: false,
        },
        AudioPlayer::<AudioSource>(assets.load(mood.file())),
        PlaybackSettings {
            volume: Volume::SILENT,
            ..PlaybackSettings::DESPAWN
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mood_follows_the_weather_then_the_place_then_the_biome() {
        let temperate = Surroundings::default();
        assert_eq!(Mood::Grassland, Mood::of(&temperate, 0.0));
        let biome = |name| Surroundings {
            biome: Some(name),
            ..temperate
        };
        assert_eq!(Mood::Forest, Mood::of(&biome("forest"), 0.0));
        assert_eq!(Mood::Rainforest, Mood::of(&biome("jungle"), 0.0));
        assert_eq!(Mood::Savanna, Mood::of(&biome("savanna"), 0.0));
        assert_eq!(Mood::Steppe, Mood::of(&biome("steppe"), 0.0));
        for name in ["tundra", "taiga", "glacier"] {
            assert_eq!(Mood::Cold, Mood::of(&biome(name), 0.0));
        }
        let cold_forest = Surroundings {
            warmth: 0.0,
            ..biome("forest")
        };
        assert_eq!(Mood::Forest, Mood::of(&cold_forest, 0.0));
        let desert = Surroundings {
            desert: 1.0,
            ..cold_forest
        };
        assert_eq!(Mood::Desert, Mood::of(&desert, 0.0));
        assert_eq!(Mood::Storm, Mood::of(&desert, 0.8));
    }
}
