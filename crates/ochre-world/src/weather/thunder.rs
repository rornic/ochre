//! The sound of a clap of thunder, made from a distance and a seed.
use std::f32::consts::TAU;
use std::time::Duration;

use crate::ambience::SAMPLE_RATE;
use bevy::{
    audio::{ChannelCount, Decodable, SampleRate, Source},
    math::ops,
    prelude::*,
    reflect::TypePath,
};

// xorshift: plenty for noise
pub(super) fn next_random(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
}

/// One clap of thunder from `distance` blocks off: close by, a rapid run of sharp cracks over a
/// deep boom; farther off, only a softer rumble. Then a long roll that swells again as the far
/// parts of the bolt are heard, echoes, and dies away.
#[derive(Asset, TypePath)]
pub(crate) struct Thunder {
    pub(super) distance: f32,
    pub(super) seed: u64,
}

// How long a clap lasts, in seconds, and its loudness from the nearest strikes.
const THUNDER_SECONDS: f32 = 7.0;
const THUNDER_GAIN: f32 = 0.46;
// Strikes within the first distance are heard at full force, fading by the log out to the second.
const THUNDER_HEARD: (f32, f32) = (120.0, 520.0);
// The roll's gain from the farthest strikes and the nearest.
const ROLL_GAIN: (f32, f32) = (0.30, 1.16);
// How far back, in frames, the two echoes come from, ~120 ms and ~270 ms; the first heard in
// the other ear.
const ECHO_TAPS: (usize, usize) = (5_200, 11_800);
const ECHO_FRAMES: usize = ECHO_TAPS.1 + 1;

impl Decodable for Thunder {
    type Decoder = ThunderDecoder;

    fn decoder(&self) -> ThunderDecoder {
        // by the log of the distance, as loudness falls off
        let near = 1.0
            - (ops::ln(self.distance / THUNDER_HEARD.0)
                / ops::ln(THUNDER_HEARD.1 / THUNDER_HEARD.0))
            .clamp(0.0, 1.0);
        let mut rng = (self.seed << 1) | 1;
        let mut uniform = || next_random(&mut rng) * 0.5 + 0.5;

        // the roll swells again round 0.7, 1.5, 2.5 and 3.7 seconds in, each later one broader,
        // quieter and wider apart in the ears
        let swells: [Swell; 4] = [
            Swell {
                at: 0.72 + uniform() * 0.20,
                rise: 0.15 + uniform() * 0.06,
                decay: 0.75 + uniform() * 0.25,
                size: 0.65 + uniform() * 0.28,
                pan: (uniform() * 2.0 - 1.0) * 0.55,
            },
            Swell {
                at: 1.48 + uniform() * 0.32,
                rise: 0.18 + uniform() * 0.08,
                decay: 0.90 + uniform() * 0.30,
                size: 0.70 + uniform() * 0.32,
                pan: (uniform() * 2.0 - 1.0) * 0.75,
            },
            Swell {
                at: 2.45 + uniform() * 0.42,
                rise: 0.22 + uniform() * 0.10,
                decay: 1.05 + uniform() * 0.35,
                size: 0.50 + uniform() * 0.28,
                pan: (uniform() * 2.0 - 1.0) * 0.85,
            },
            Swell {
                at: 3.65 + uniform() * 0.50,
                rise: 0.28 + uniform() * 0.12,
                decay: 1.20 + uniform() * 0.40,
                size: 0.35 + uniform() * 0.22,
                pan: (uniform() * 2.0 - 1.0) * 0.85,
            },
        ];

        // the crack: five shocks in the first third of a second, each longer and lower than the
        // last, leaning left and right in turn
        let shocks: [Shock; 5] = [
            Shock {
                at: 0.0,
                duration: 0.0028,
                size: 4.6,
                pan: -0.15,
            },
            Shock {
                at: 0.026 + uniform() * 0.004,
                duration: 0.0042,
                size: 5.2,
                pan: 0.25,
            },
            Shock {
                at: 0.058 + uniform() * 0.008,
                duration: 0.0060,
                size: 4.8,
                pan: -0.20,
            },
            Shock {
                at: 0.105 + uniform() * 0.012,
                duration: 0.0095,
                size: 6.2,
                pan: 0.30,
            },
            Shock {
                at: 0.265 + uniform() * 0.020,
                duration: 0.0140,
                size: 5.4,
                pan: -0.35,
            },
        ];

        ThunderDecoder {
            rng,
            frame: 0,
            channel: 0,
            near,
            swells,
            shocks,
            low: [[0.0; 4]; 2],
            heave: [[0.0; 4]; 2],
            echo: Box::new([[0.0; 2]; ECHO_FRAMES]),
            echo_low: [0.0; 2],
            dc: [0.0; 2],
            air: [[0.0; 2]; 2],
        }
    }
}

// One swell in the roll: when it peaks and how long it takes to, how fast it dies, all in
// seconds, how loud, and which way it leans, -1 left to 1 right.
#[derive(Clone, Copy)]
struct Swell {
    at: f32,
    rise: f32,
    decay: f32,
    size: f32,
    pan: f32,
}

// One shock in the crack: a single cycle of a sine that dies away as it goes, sharp up then
// down. When it comes and how long it lasts, in seconds, how loud, and which way it leans.
#[derive(Clone, Copy)]
struct Shock {
    at: f32,
    duration: f32,
    size: f32,
    pan: f32,
}

pub(crate) struct ThunderDecoder {
    rng: u64,
    frame: u32,
    channel: usize,
    // 0 for the farthest strikes, 1 for the nearest
    near: f32,
    swells: [Swell; 4],
    shocks: [Shock; 5],
    // per ear: the roll's three low-pass stages, then one finding the rumble below ~25 Hz to
    // take away
    low: [[f32; 4]; 2],
    // per ear: two slow wanderings that make the roll heave and tumble, each two low-passes,
    // round 2 Hz and 7 Hz
    heave: [[f32; 4]; 2],
    // what's been heard, for the echoes, and a low-pass dulling them
    echo: Box<[[f32; 2]; ECHO_FRAMES]>,
    echo_low: [f32; 2],
    // per ear: what drifts below ~25 Hz, to take away
    dc: [f32; 2],
    // per ear: two low-passes, the air dulling the sound as it travels
    air: [[f32; 2]; 2],
}

impl Iterator for ThunderDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let frame = self.frame as usize;
        let t = self.frame as f32 / SAMPLE_RATE as f32;
        if t >= THUNDER_SECONDS {
            return None;
        }
        let c = self.channel;
        if c == 1 {
            self.frame += 1;
        }
        self.channel = 1 - self.channel;
        let near = self.near;
        let ear = if c == 0 { -1.0 } else { 1.0 };
        let white = next_random(&mut self.rng);

        // the crack, only from nearer strikes
        let crack_amount = near * near;
        let mut crack = 0.0f32;
        if crack_amount > 0.0 && t < 0.6 {
            for shock in &self.shocks {
                let since = t - shock.at;
                if (0.0..shock.duration).contains(&since) {
                    let phase = since / shock.duration;
                    crack += ops::sin(phase * TAU)
                        * (1.0 - phase)
                        * shock.size
                        * (1.0 + 0.25 * shock.pan * ear);
                }
            }
        }
        let crack = crack * crack_amount;

        // how much the roll swells here, and how much of that is a fresh swell cresting
        let (mut swelling, mut cresting) = (0.0f32, 0.0f32);
        for swell in &self.swells {
            let since = t - swell.at + swell.rise;
            if since > 0.0 {
                let u = (since / swell.rise).min(1.0);
                let onset = u * u * (3.0 - 2.0 * u);
                let after = (since - swell.rise).max(0.0);
                let size = swell.size * onset * ops::exp(-after / swell.decay);
                swelling += size * (1.0 + 0.35 * swell.pan * ear);
                cresting += size * ops::exp(-after / 0.35);
            }
        }

        // the roll heaving and tumbling
        let [h1, h2, g1, g2] = &mut self.heave[c];
        *h1 += (white - *h1) * 0.00032;
        *h2 += (*h1 - *h2) * 0.00032;
        *g1 += (white - *g1) * 0.00095;
        *g2 += (*g1 - *g2) * 0.00095;
        let heave = (*h2 * 120.0).clamp(-0.55, 1.45);
        let tumble = (*g2 * 70.0).clamp(-0.50, 1.40);
        let ragged = (0.85 + 0.48 * heave + 0.38 * tumble.max(-0.20)).clamp(0.55, 2.20);

        // the roll: spiky noise through three low-passes, round 150-500 Hz, brighter near, early
        // on and as it heaves or swells, with the lowest rumble taken out
        let crest = (0.35 * heave.max(0.0) + 0.25 * tumble.max(0.0) + 0.50 * cresting).min(1.4);
        let cut = (0.021 + (0.015 + 0.022 * near) * ops::exp(-t / 3.2) + 0.017 * crest)
            .clamp(0.019, 0.072);
        let mut x = (white * 0.60 + white * white * white * 0.75) * ragged;
        let [l1, l2, l3, rumble] = &mut self.low[c];
        for stage in [l1, l2, l3] {
            *stage += (x - *stage) * cut;
            x = *stage;
        }
        *rumble += (x - *rumble) * 0.0035;
        let x = (x - *rumble) * ops::sqrt(0.026 / cut);
        let roll_envelope = {
            // near, it's on top of you at once; far, it fades in, slower at first
            let attack = (t / (0.42 * (1.0 - near).max(0.002))).min(1.0);
            let attack = if near < 0.3 { attack * attack } else { attack };
            let decay = ops::exp(-t / (2.4 + 0.7 * near));
            attack * decay * (0.60 + 0.65 * swelling)
        };
        let roll_gain = ROLL_GAIN.0 * ops::powf(ROLL_GAIN.1 / ROLL_GAIN.0, near);
        let mut sample = x * 17.0 * roll_envelope * roll_gain + crack;

        // two echoes off the land and clouds, dulled, the nearer the louder, each feeding the
        // next
        let tap = |delay: usize| (frame + ECHO_FRAMES - delay) % ECHO_FRAMES;
        let heard =
            self.echo[tap(ECHO_TAPS.0)][1 - c] * 0.58 + self.echo[tap(ECHO_TAPS.1)][c] * 0.42;
        self.echo_low[c] += (heard - self.echo_low[c]) * 0.038;
        let echo = self.echo_low[c];
        self.echo[frame % ECHO_FRAMES][c] = crack * 0.40 + sample * 0.20 + echo * 0.12;
        sample += echo * (0.35 + 0.30 * near);

        self.dc[c] += (sample - self.dc[c]) * 0.0035;
        let sample = (sample - self.dc[c]) * THUNDER_GAIN;
        // fades out at the very end so it doesn't click, and saturates rather than clipping
        let tail = ((THUNDER_SECONDS - t) / 0.5).min(1.0);
        let sample = (sample * tail).tanh();

        // the air dulls it: round 2.5 kHz from the nearest strikes at first, closing to round
        // 270 Hz within half a second
        let cut = (0.038 + (0.020 + 0.240 * near) * ops::exp(-t / 0.14)).clamp(0.036, 0.300);
        let [a1, a2] = &mut self.air[c];
        *a1 += (sample - *a1) * cut;
        *a2 += (*a1 - *a2) * cut;
        Some((*a2 * 1.05).clamp(-1.0, 1.0))
    }
}

impl Source for ThunderDecoder {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(SAMPLE_RATE).unwrap()
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(THUNDER_SECONDS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::{STRIKE_DISTANCE};

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn test_thunder_ends_and_near_claps_are_louder() {
        let clap = |distance| -> Vec<f32> { Thunder { distance, seed: 9 }.decoder().collect() };
        let (near, far) = (clap(STRIKE_DISTANCE.0), clap(STRIKE_DISTANCE.1));
        assert_eq!((THUNDER_SECONDS * 44_100.0) as usize * 2, near.len());
        assert!(near.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        assert!(rms(&near) > rms(&far) * 1.5, "{} {}", rms(&near), rms(&far));
    }

    #[test]
    fn test_near_thunder_opens_with_a_bright_crack() {
        let opening = |distance| -> Vec<f32> {
            Thunder { distance, seed: 9 }
                .decoder()
                .take(44_100 / 20 * 2)
                .collect()
        };
        // how much it changes from one frame to the next, in the left ear: the brighter, the more
        let brightness = |s: &[f32]| {
            rms(&s
                .windows(3)
                .map(|w| w[2] - w[0])
                .step_by(2)
                .collect::<Vec<_>>())
        };
        let (near, far) = (opening(STRIKE_DISTANCE.0), opening(STRIKE_DISTANCE.1));
        assert!(
            rms(&near) > rms(&far) * 10.0,
            "{} {}",
            rms(&near),
            rms(&far)
        );
        assert!(
            brightness(&near) > brightness(&far) * 100.0,
            "{} {}",
            brightness(&near),
            brightness(&far)
        );
        // and it's loud from the very start
        assert!(rms(&near[..88]) > 0.1, "{}", rms(&near[..88]));
    }
}
