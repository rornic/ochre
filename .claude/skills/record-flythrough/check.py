#!/usr/bin/env python3
"""Checks a recording from record.sh: its size and frame rate, that the frames are not repeats, that it has sound, and that the music is in it.

usage: check.py <video.mp4> [--music <track.ogg>] [--music-at <seconds>] [--listen-at <seconds>]

--music-at is when the track starts in the video: record.sh starts it 3 s into the flight (the default). --listen-at is where in the
video the 8 s that are compared begin, 12 s after the music starts unless told, so a video shorter than 20 s needs it.
Needs numpy. The sound is checked by cross-correlating 8 s of the video's audio with the track's first 60 s: the peak should be many
standard deviations over the rest, and sit where the track would be if it started at --music-at.
"""
import argparse
import subprocess
import sys

import numpy as np


def run(cmd):
    return subprocess.run(cmd, capture_output=True, check=True).stdout


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("video")
    ap.add_argument("--music", default="assets/sounds/music/cold.ogg")
    ap.add_argument("--music-at", type=float, default=3.0)
    ap.add_argument("--listen-at", type=float)
    args = ap.parse_args()
    ok = True

    probe = run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,avg_frame_rate,nb_frames",
                 "-of", "csv=p=0", args.video]).decode().strip()
    width, height, rate, frames = probe.split(",")
    print(f"video: {width}x{height}, {rate} fps, {frames} frames")
    ok &= (width, height) == ("1920", "1080")

    # frames that differ from the one before, on a small grey copy: a capture that missed updates repeats frames
    w, h = 320, 180
    raw = run(["ffmpeg", "-v", "error", "-i", args.video, "-vf", f"scale={w}:{h}", "-pix_fmt", "gray", "-f", "rawvideo", "-"])
    grey = np.frombuffer(raw, dtype=np.uint8).reshape(-1, h, w).astype(np.int16)
    change = np.abs(np.diff(grey, axis=0)).mean(axis=(1, 2))
    distinct = float((change >= 0.02).mean())
    print(f"frames that differ from the one before: {100 * distinct:.0f}% (a screen grab got 3%; a clean render is 100%)")
    ok &= distinct > 0.97

    # the stretches where the picture stalls, which a hitching game would leave
    still = np.flatnonzero(change < 0.02)
    if len(still):
        print(f"  repeated frames at {', '.join(f'{i / 60:.1f}s' for i in still[:8])}{' ...' if len(still) > 8 else ''}")

    level = subprocess.run(["ffmpeg", "-hide_banner", "-nostats", "-i", args.video, "-af", "volumedetect", "-f", "null", "-"],
                           capture_output=True, text=True).stderr
    mean = [line.split("mean_volume:")[1].split()[0] for line in level.splitlines() if "mean_volume:" in line]
    print(f"sound: mean volume {mean[0] if mean else 'none'} dB")
    ok &= bool(mean) and float(mean[0]) > -40

    def pcm(path, start=0.0, seconds=None):
        cmd = ["ffmpeg", "-v", "error", "-ss", str(start), "-i", path, "-ac", "1", "-ar", "48000"]
        cmd += ["-t", str(seconds)] if seconds else []
        return np.frombuffer(run(cmd + ["-f", "f32le", "-"]), dtype=np.float32)

    track = pcm(args.music, 0, 60)
    start = args.listen_at if args.listen_at is not None else args.music_at + 12  # well after the track's 6 s fade in
    heard = pcm(args.video, start, 8)
    n = 1 << 23
    a, b = np.fft.rfft(track - track.mean(), n), np.fft.rfft(heard - heard.mean(), n)
    cc = np.abs(np.fft.irfft(a * np.conj(b), n)[: len(track)])
    sigma = (cc.max() - cc.mean()) / cc.std()
    where = cc.argmax() / 48000
    expect = start - args.music_at
    print(f"music: correlation peak {sigma:.1f} standard deviations over the rest, at {where:.2f} s into the track (expected {expect:.2f} s)")
    ok &= sigma > 8 and abs(where - expect) < 0.5

    print("OK" if ok else "CHECK FAILED")
    sys.exit(0 if ok else 1)


main()
