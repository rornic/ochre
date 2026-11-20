# Sound credits

Every recording here is CC0 1.0 (public domain), so none needs crediting, but thanks to:

| File | Source | Author |
|---|---|---|
| `forest.ogg` | [Birds in the Slovenian Forest - Processed w/ Generated Noise](https://freesound.org/people/GammaGool/sounds/850507/) | GammaGool |
| `crickets.ogg` | [Crickets At Night - Raw sound](https://freesound.org/people/Defelozedd94/sounds/522299/) | Defelozedd94 |
| `waves.ogg` | [Sea Waves](https://bigsoundbank.com/sea-waves_s0698.html) | Joseph Sardin (BigSoundBank) |
| `underwater.ogg` | [Underwater Ambience.wav](https://freesound.org/people/Tim_Verberne/sounds/482167/) | Tim_Verberne |
| `owl.ogg` | [Tawny owl hooting](https://freesound.org/people/Patrick_Corra/sounds/745208/) | Patrick Corrà |
| `leaves.ogg` | [Bushes Rustling.WAV](https://freesound.org/people/G.Lamont/sounds/117884/) | G.Lamont |
| `rain.ogg` | [Soft rain.WAV](https://freesound.org/people/barkenov/sounds/640655/) | barkenov |
| `footsteps/rock_*.ogg` | [Footsteps - Stone, Rock, Concrete, Cement](https://freesound.org/people/SecureSubset/sounds/813622/) | SecureSubset |
| `footsteps/snow_*.ogg` | [footsteps in fresh snow](https://freesound.org/people/florianreichelt/sounds/453168/) | florianreichelt |
| `footsteps/sand_*.ogg` | [footsteps in wet coarse sand.wav](https://freesound.org/people/bruno.auzet/sounds/539175/) | bruno.auzet |
| `footsteps/ice_*.ogg` | [Footsteps on frozen lake](https://freesound.org/people/SoundsLikeYukon/sounds/732880/) | SoundsLikeYukon |
| `footsteps/grass_*.ogg` | [Grassy Footstep 2](https://freesound.org/people/OwlStorm/sounds/151229/) | OwlStorm |
| `footsteps/wood_*.ogg` | [wooden floor, old, creaking, footsteps, walking.WAV](https://freesound.org/people/VMan533/sounds/575898/) | VMan533 |
| `footsteps/foliage_*.ogg` | cut from `leaves.ogg` (G.Lamont's bushes, above) | G.Lamont |

Each loop is a 30 s window of its recording, crossfaded into a seamless loop and levelled.
The crickets are high-passed at 2.5 kHz to take out the storm behind them, and the owl is
one hoot with the recording's background noise gated out. The leaves are a close recording
of bushes, with the rumble cut, the top softened and a short reverb spread over it so it
sounds like a canopy overhead.

The rain is the whole recording, high-passed at 90 Hz, lightly compressed (3:1) so its drips stay drips, peak-levelled, with its last 4 s crossfaded into its first 4 s (equal power, since rain is noise), so the loop is 57 s.

Each footstep set is four single steps cut from a longer recording, 0.5 s with a short fade and mono, every clip levelled to -28 LUFS, grass to -34 (gain capped at -1.5 dBTP, so `wood_2` sits at -31.7), played at -52 (grass -58) by `footsteps.rs:VOLUME`. The grass is one step, low-passed at 5 kHz and pitched down up to 12% for its four variants. The foliage rustles are three 0.8 s windows of `leaves.ogg` with a fast fade in and a slow fade out, mono, peak-levelled to -6 dB, played over a step taken inside ground flora.

## Music

`music/*.ogg` are not committed (gitignored) except `cold`, `forest`, `grassland`, `rainforest`, `savanna` and `steppe.ogg`: the others are Suno free-tier tracks, so non-commercial, and the repo is public, while those six were made on a Suno paid plan and are committed. Without a mood's file there, the biome's own track plays in its place, or grassland's (`Present` in `music.rs` looks for the files at startup); a biome with no track of its own falls back to grassland's. Each should be levelled to -20 LUFS with a 6 s fade in and 10 s fade out baked in (`ffmpeg -af volume=<gain>dB,afade=t=in:d=6,afade=t=out:st=<length-10>:d=10 -c:a libvorbis -q:a 5`), and `MUSIC_VOLUME` in `crates/ochre-world/src/music.rs` sets them under the ambience.

| File | Suno title | Plays |
|---|---|---|
| `grassland.ogg` | Suno paid plan | grassland, and any biome without a track |
| `forest.ogg` | Suno paid plan | forest |
| `rainforest.ogg` | Suno paid plan | jungle and swamp |
| `savanna.ogg` | Suno paid plan | savanna |
| `steppe.ogg` | Suno paid plan | steppe |
| `cold.ogg` | Tyst vind (Suno paid plan) | tundra, taiga and glacier |
| `desert.ogg` | Heat-Haze Modulation | desert |
| `storm.ogg` | Deep Sub-Bass | heavy rain |

A playing track whose mood stops fitting for 20 s (a new biome, rain) fades out over about 12 s while the next mood's track fades in, so a crossing is a crossfade rather than silence.
