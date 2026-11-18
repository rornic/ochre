//! Music that drifts in now and then: one track picked for the camera's biome and the weather,
//! long silences between, crossfading when the mood changes under a playing track.
use bevy::{
    app::{App, Plugin, Update},
    asset::AssetServer,
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, AudioSource, PlaybackSettings, Volume},
    ecs::{
        component::Component,
        entity::Entity,
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Commands, Local, Query, Res},
    },
    time::Time,
};

use crate::{
    ambience::{SenseSurroundings, Surroundings, SurroundingsPlugin},
    weather::Weather,
};

/// Plays one track at a time, picked by `Surroundings`. `first_delay` fixes the seconds before the first track, and `track` the
/// one played, by what its file name has in it (`steppe4`), so a recording knows when the music starts and which it is.
#[derive(Default)]
pub struct MusicPlugin {
    pub first_delay: Option<f32>,
    pub track: Option<String>,
}

#[derive(Resource)]
struct FirstDelay(Option<f32>);

#[derive(Resource)]
struct ForcedTrack(Option<String>);

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SurroundingsPlugin>() {
            app.add_plugins(SurroundingsPlugin);
        }
        app.insert_resource(FirstDelay(self.first_delay))
            .insert_resource(ForcedTrack(self.track.clone()))
            .add_systems(Update, play_music.after(SenseSurroundings));
    }
}

/// What a track is for; the first that holds picks the track.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mood {
    Storm,
    SteppeFury,
    Desert,
    Cold,
    Forest,
    Rainforest,
    Savanna,
    Steppe,
    Grassland,
}

impl Mood {
    // `gloom` is how dark the storm overhead makes the day, see `Weather::gloom`: rain or none.
    pub fn of(around: &Surroundings, rain: f32, gloom: f32) -> Mood {
        if rain > 0.4 {
            Mood::Storm
        } else if gloom > FURY_GLOOM && around.steppe > 0.5 {
            // the steppe's dry storms
            Mood::SteppeFury
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

    // the tracks a mood has, one of which is picked each time music starts, see `pick`
    fn files(self) -> &'static [&'static str] {
        match self {
            Mood::Storm => &["sounds/music/storm.ogg"],
            Mood::SteppeFury => &["sounds/music/steppe-fury.ogg"],
            Mood::Desert => &["sounds/music/desert.ogg"],
            Mood::Cold => &["sounds/music/cold.ogg"],
            Mood::Forest => &["sounds/music/forest.ogg"],
            Mood::Rainforest => &["sounds/music/rainforest.ogg"],
            Mood::Savanna => &["sounds/music/savanna.ogg"],
            Mood::Steppe => &[
                "sounds/music/steppe.ogg",
                "sounds/music/steppe2.ogg",
                "sounds/music/steppe3.ogg",
                "sounds/music/steppe4.ogg",
            ],
            Mood::Grassland => &["sounds/music/grassland.ogg"],
        }
    }
}

// The first of `files` with `name` in its file name, if any.
fn named(files: &'static [&'static str], name: &str) -> Option<&'static str> {
    files.iter().copied().find(|f| f.contains(name))
}

// One of `files` for a `draw` in 0..1, never the `last` played unless it is the only one.
fn pick(files: &'static [&'static str], last: Option<&str>, draw: f32) -> &'static str {
    let others: Vec<&'static str> = files.iter().copied().filter(|f| Some(*f) != last).collect();
    let choices = if others.is_empty() { files } else { &others[..] };
    choices[((draw * choices.len() as f32) as usize).min(choices.len() - 1)]
}

/// A playing track, how long its mood has stopped fitting, and whether it's on its way out.
#[derive(Component)]
struct Track {
    mood: Mood,
    mismatch: f32,
    fading: bool,
}

// How dark a storm over the steppe must make the day for its own music to come in.
const FURY_GLOOM: f32 = 0.5;
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

/// What a track is picked and paced by.
#[derive(bevy::ecs::system::SystemParam)]
struct Context<'w> {
    around: Res<'w, Surroundings>,
    first_delay: Res<'w, FirstDelay>,
    forced: Res<'w, ForcedTrack>,
    weather: Option<Res<'w, Weather>>,
}

// Whether a first track has played, and which was the last, so the next is a different one.
#[derive(Default)]
struct Played {
    any: bool,
    last: Option<&'static str>,
}

fn play_music(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<AssetServer>,
    context: Context,
    mut tracks: Query<(Entity, &mut Track, Option<&mut AudioSink>)>,
    mut wait: Local<Option<f32>>,
    mut played: Local<Played>,
) {
    let Context {
        around,
        first_delay,
        forced,
        weather,
    } = context;
    let dt = time.delta_secs();
    let mood = Mood::of(
        &around,
        weather.as_ref().map_or(0.0, |weather| weather.precipitation()),
        weather.as_ref().map_or(0.0, |weather| weather.gloom()),
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
        if played.any {
            let draw: f32 = rand::random();
            GAP_SECONDS.0 + (GAP_SECONDS.1 - GAP_SECONDS.0) * draw * draw
        } else {
            first_delay
                .0
                .unwrap_or_else(|| rand::random_range(FIRST_SECONDS.0..FIRST_SECONDS.1))
        }
    });
    *left -= dt;
    if *left > 0.0 || around.underwater {
        return;
    }
    *wait = None;
    played.any = true;
    let file = forced
        .0
        .as_deref()
        .and_then(|name| named(mood.files(), name))
        .unwrap_or_else(|| pick(mood.files(), played.last, rand::random()));
    played.last = Some(file);
    commands.spawn((
        Track {
            mood,
            mismatch: 0.0,
            fading: false,
        },
        AudioPlayer::<AudioSource>(assets.load(file)),
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
    fn music_starts_on_a_different_track_each_time_and_uses_them_all() {
        let files = Mood::Steppe.files();
        assert_eq!(4, files.len());
        let mut last = None;
        let mut heard = std::collections::HashSet::new();
        for i in 0..300 {
            let file = pick(files, last, (i as f32 * 0.6180339) % 1.0);
            assert_ne!(Some(file), last, "the same track twice running");
            heard.insert(file);
            last = Some(file);
        }
        assert_eq!(4, heard.len());
        // a mood with one track plays it again, and a draw at the top of its range stays in it
        let only = Mood::Grassland.files();
        assert_eq!(only[0], pick(only, Some(only[0]), 0.999));
        assert_eq!(files[files.len() - 1], pick(files, None, 0.9999));
    }

    #[test]
    fn a_track_can_be_asked_for_by_name() {
        let files = Mood::Steppe.files();
        assert_eq!(Some("sounds/music/steppe4.ogg"), named(files, "steppe4"));
        assert_eq!(Some("sounds/music/steppe.ogg"), named(files, "steppe."));
        assert_eq!(None, named(files, "desert"));
    }

    #[test]
    fn the_committed_steppe_track_is_a_file_that_exists() {
        // the others are gitignored, see assets/sounds/CREDITS.md, so a fresh clone has only this one
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/sounds/music/steppe.ogg");
        assert!(path.exists(), "steppe.ogg is missing");
    }

    #[test]
    fn test_mood_follows_the_weather_then_the_place_then_the_biome() {
        let temperate = Surroundings::default();
        assert_eq!(Mood::Grassland, Mood::of(&temperate, 0.0, 0.0));
        let biome = |name| Surroundings {
            biome: Some(name),
            ..temperate
        };
        assert_eq!(Mood::Forest, Mood::of(&biome("forest"), 0.0, 0.0));
        assert_eq!(Mood::Rainforest, Mood::of(&biome("jungle"), 0.0, 0.0));
        assert_eq!(Mood::Savanna, Mood::of(&biome("savanna"), 0.0, 0.0));
        assert_eq!(Mood::Steppe, Mood::of(&biome("steppe"), 0.0, 0.0));
        for name in ["tundra", "taiga", "glacier"] {
            assert_eq!(Mood::Cold, Mood::of(&biome(name), 0.0, 0.0));
        }
        let cold_forest = Surroundings {
            warmth: 0.0,
            ..biome("forest")
        };
        assert_eq!(Mood::Forest, Mood::of(&cold_forest, 0.0, 0.0));
        let desert = Surroundings {
            desert: 1.0,
            ..cold_forest
        };
        assert_eq!(Mood::Desert, Mood::of(&desert, 0.0, 0.0));
        assert_eq!(Mood::Storm, Mood::of(&desert, 0.8, 0.8));
        // a dry storm over the steppe has the steppe's fury, a wet one the storm's music, and a dark sky elsewhere neither
        let steppe = Surroundings { steppe: 1.0, ..biome("steppe") };
        assert_eq!(Mood::SteppeFury, Mood::of(&steppe, 0.0, 0.7));
        assert_eq!(Mood::Steppe, Mood::of(&steppe, 0.0, 0.3));
        assert_eq!(Mood::Storm, Mood::of(&steppe, 0.8, 0.8));
        assert_eq!(Mood::Savanna, Mood::of(&biome("savanna"), 0.0, 0.7));
    }
}
