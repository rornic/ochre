"""Writes steppe.flight, a flyover of seed 1's steppe timed to steppe4.ogg: slow and low through the calm intro and outro, fast through the loud middle.

usage: steppe_flight.py steppe4.wav [out.flight]   (the wav is the track before its levelling, only its loudness over time is read)
"""
import subprocess
import sys

import numpy as np
from scipy.interpolate import CubicSpline
from scipy.ndimage import gaussian_filter1d

MUSIC_AT = 5.0  # seconds into the flight the music starts: record.sh sets OCHRE_MUSIC_DELAY a little ahead and the music timer runs from the game's start, which the flight's clock follows by about 2 s (measured 4.8)
TRACK = 238.4  # seconds long
END = 248.0  # the track ends at MUSIC_AT + TRACK, and the flight a few seconds after, so a late start does not cut it
END_TRACK = END - MUSIC_AT
VMIN, VMAX = 6.0, 80.0

# where the camera goes, seed 1 (x, z): the spawn, out over the valleys, a long run east over the plain, north, and back west to the small lake
ROUTE = [(444, 7396), (1050, 7850), (2000, 9100), (3350, 9900), (6200, 8800), (5800, 7900), (3800, 6250), (3400, 6000)]

# in seconds of the track (the flight is MUSIC_AT ahead of them): blocks over the ground, degrees of look to the right of the way it is going, and the time of day
HEIGHT = [(-MUSIC_AT, 16), (11, 20), (24, 26), (36, 55), (47, 28), (57, 24), (68, 90), (82, 60), (92, 35), (100, 170), (112, 40), (124, 22), (137, 140), (144, 190), (160, 70), (172, 25), (185, 120), (196, 60), (209, 30), (225, 18), (END_TRACK, 12)]
LOOK = [(-MUSIC_AT, 0), (9, -20), (21, 15), (36, 0), (52, -25), (68, 10), (82, 35), (100, 0), (112, -15), (124, 20), (137, -35), (144, 45), (160, -10), (175, 25), (187, -20), (196, 0), (212, 25), (END_TRACK, 10)]
DAY = [(-MUSIC_AT, 0.285), (36, 0.30), (100, 0.37), (144, 0.45), (196, 0.58), (END_TRACK, 0.70)]


def onset(wav):
    x = np.frombuffer(subprocess.check_output(["ffmpeg", "-v", "error", "-i", wav, "-ac", "1", "-ar", "22050", "-f", "f32le", "-"]), np.float32)
    sr, win, hop = 22050, 1024, 512
    w, prev, flux = np.hanning(win), None, []
    for f in range((len(x) - win) // hop):
        mag = np.abs(np.fft.rfft(x[f * hop:f * hop + win] * w))
        flux.append(0.0 if prev is None else np.maximum(mag - prev, 0).sum())
        prev = mag
    flux = np.array(flux)
    fps = sr / hop
    return np.array([flux[int(i * fps):int((i + 1) * fps)].mean() for i in range(len(x) // sr)])


def curve(points, t):
    p = np.array(points, float)
    return np.interp(t, p[:, 0] + MUSIC_AT, p[:, 1])


def main():
    wav, out = sys.argv[1], (sys.argv[2] if len(sys.argv) > 2 else "steppe.flight")
    t = np.arange(0, END * 10 + 1) / 10
    strength = np.interp(t - MUSIC_AT, np.arange(len(onset(wav))), onset(wav), left=0, right=0)
    # speed swells with the music's drive, eased over a few seconds
    drive = gaussian_filter1d(np.clip((strength - 20) / 68, 0, 1), 30)
    speed = VMIN + (VMAX - VMIN) * drive ** 1.3
    flown = np.cumsum(speed) / 10
    route = np.array(ROUTE, float)
    chord = np.concatenate([[0], np.cumsum(np.hypot(*np.diff(route, axis=0).T))])
    spline = CubicSpline(chord, route, bc_type="natural")
    dense = np.linspace(0, chord[-1], 20000)
    along = np.concatenate([[0], np.cumsum(np.hypot(*np.diff(spline(dense), axis=0).T))])
    scale = along[-1] / flown[-1]
    # the speeds are scaled so the whole route is flown, which the route's length was chosen to make about 1
    where = np.interp(flown * scale, along, dense)
    xz = spline(where)
    dx, dz = np.gradient(xz[:, 0]), np.gradient(xz[:, 1])
    heading = np.unwrap(np.degrees(np.arctan2(dx, -dz)), period=360)
    heading = gaussian_filter1d(heading, 20)
    look = curve(LOOK, t)
    height = curve(HEIGHT, t)
    pitch = -np.clip(4 + 0.1 * height, 0, 22)
    day = curve(DAY, t)
    keys = np.append(np.arange(0, END - 0.5, 3.0), END)
    lines = ["# steppe4.ogg over seed 1's steppe: slow and low through the intro and outro, fast through the loud middle. Made by steppe_flight.py.",
             "# seconds x z height yaw pitch day"]
    for k in keys:
        i = int(round(k * 10))
        lines.append(f"{k:g} {xz[i, 0]:.0f} {xz[i, 1]:.0f} {height[i]:.0f} {heading[i] + look[i]:.1f} {pitch[i]:.1f} {day[i]:.4f}")
    open(out, "w").write("\n".join(lines) + "\n")
    print(f"route {along[-1]:.0f} blocks, speed scale {scale:.3f}, top speed {speed.max() * scale:.0f} blocks/s, {len(keys)} keys -> {out}")
    np.savetxt(out.replace(".flight", "-path.csv"), np.column_stack([t[::10], xz[::10], height[::10]]), fmt="%.1f", delimiter=",")


main()
