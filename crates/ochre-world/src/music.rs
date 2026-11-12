//! Music that drifts in now and then: one track picked for where the camera is and what the sky
//! is doing, long silences between. Needs `AmbiencePlugin`, which keeps `Surroundings` current.
use bevy::{
    app::{App, Plugin, Update},
    asset::AssetServer,
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings, Volume},
    ecs::{
        component::Component,
        entity::Entity,
        system::{Commands, Local, Query, Res},
    },
    time::Time,
};

use crate::{ambience::Surroundings, weather::Weather};

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Surroundings>()
            .add_systems(Update, play_music);
    }
}

/// What a track is for; the first that holds picks the track.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Storm,
    Desert,
    Cold,
    Night,
    Day,
}

impl Mood {
    pub fn of(around: &Surroundings, rain: f32) -> Mood {
        if rain > 0.4 {
            Mood::Storm
        } else if around.desert > 0.5 {
            Mood::Desert
        } else if around.warmth < 0.5 {
            Mood::Cold
        } else if around.day < 0.4 {
            Mood::Night
        } else {
            Mood::Day
        }
    }

    // shortcut: one track a mood, add a pick among several here when there are more
    fn file(self) -> &'static str {
        match self {
            Mood::Storm => "sounds/music/storm.ogg",
            Mood::Desert => "sounds/music/desert.ogg",
            Mood::Cold => "sounds/music/cold.ogg",
            Mood::Night => "sounds/music/night.ogg",
            Mood::Day => "sounds/music/day.ogg",
        }
    }
}

/// A playing track and how long its mood has stopped fitting.
#[derive(Component)]
struct Track {
    mood: Mood,
    mismatch: f32,
}

// The files are levelled to -20 LUFS with their fades baked in; this sits them under the ambience.
const MUSIC_VOLUME: f32 = 0.15;
// Seconds of silence between tracks, at the least and most, skewed short by squaring the draw.
const GAP_SECONDS: (f32, f32) = (60.0, 600.0);
// Seconds before the first track, so a run doesn't open on minutes of silence.
const FIRST_SECONDS: (f32, f32) = (10.0, 40.0);
// Seconds a track rides out a change of mood (dusk, a desert's edge) before fading away early.
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
    let mood = Mood::of(&around, weather.map_or(0.0, |weather| weather.rain));
    if let Ok((entity, mut track, sink)) = tracks.single_mut() {
        // the sink arrives a frame after the player is spawned
        let Some(mut sink) = sink else {
            return;
        };
        let fits = mood == track.mood && !around.underwater;
        track.mismatch = if fits { 0.0 } else { track.mismatch + dt };
        let wanted = if track.mismatch < GRACE_SECONDS {
            MUSIC_VOLUME
        } else {
            0.0
        };
        let current = sink.volume().to_linear();
        let volume = current + (wanted - current) * (1.0 - (-dt / FOLLOW_SECONDS).exp());
        sink.set_volume(Volume::Linear(volume));
        if wanted == 0.0 && volume < SILENT {
            commands.entity(entity).despawn();
        }
        return;
    }
    let left = wait.get_or_insert_with(|| {
        if *played {
            let draw: f32 = rand::random();
            GAP_SECONDS.0 + (GAP_SECONDS.1 - GAP_SECONDS.0) * draw * draw
        } else {
            rand::random_range(FIRST_SECONDS.0..FIRST_SECONDS.1)
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
    fn test_mood_follows_the_weather_then_the_place_then_the_hour() {
        let day = Surroundings::default();
        assert_eq!(Mood::Day, Mood::of(&day, 0.0));
        let night = Surroundings { day: 0.0, ..day };
        assert_eq!(Mood::Night, Mood::of(&night, 0.0));
        let cold = Surroundings {
            warmth: 0.0,
            ..night
        };
        assert_eq!(Mood::Cold, Mood::of(&cold, 0.0));
        let desert = Surroundings {
            desert: 1.0,
            ..cold
        };
        assert_eq!(Mood::Desert, Mood::of(&desert, 0.0));
        assert_eq!(Mood::Storm, Mood::of(&desert, 0.8));
    }
}
