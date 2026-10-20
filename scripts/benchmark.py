#!/usr/bin/env python3
"""Reproduce a small, local encoder/pipeline comparison; no universal speed claim."""
from pathlib import Path
import argparse
import json
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('--runtime', type=Path, default=Path(__file__).resolve().parent.parent / 'runtime')
parser.add_argument('--output', type=Path, default=Path(__file__).resolve().parent.parent / 'target/benchmark')
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
ffmpeg = args.runtime.resolve() / 'bin/ffmpeg'
ffprobe = args.runtime.resolve() / 'bin/ffprobe'
source = args.output / '1080p-source.mp4'

def run(options):
    started = time.perf_counter()
    subprocess.run([str(ffmpeg), '-hide_banner', '-loglevel', 'error', '-nostdin', '-y', *options],
                   check=True, capture_output=True, text=True, timeout=120)
    return time.perf_counter() - started

if not source.exists():
    run(['-f', 'lavfi', '-i', 'testsrc2=size=1920x1080:rate=30:duration=12',
         '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000:duration=12',
         '-c:v', 'libx264', '-preset', 'ultrafast', '-crf', '18', '-c:a', 'aac', str(source)])
results = []
for repeat in range(2):
    for mode in ['cpu', 'hardware', 'hardware_decode', 'hardware_pipeline']:
        output = args.output / f'{mode}-{repeat}.mp4'
        hardware = mode != 'cpu'
        prefix = ['-hwaccel', 'videotoolbox'] if mode in ['hardware_decode', 'hardware_pipeline'] else []
        if mode == 'hardware_pipeline':
            prefix += ['-hwaccel_output_format', 'videotoolbox_vld']
        filters = 'fps=24,scale_vt=w=1280:h=720' if mode == 'hardware_pipeline' else 'fps=24,scale=1280:720:flags=lanczos'
        codec = ['-c:v', 'h264_videotoolbox', '-allow_sw', '0', '-prio_speed', '1', '-power_efficient', '0'] if hardware else ['-c:v', 'libx264', '-preset', 'medium']
        pix = [] if mode == 'hardware_pipeline' else ['-pix_fmt', 'yuv420p']
        options = [*prefix, '-i', str(source), '-map', '0:V:0', '-map', '0:a:0', '-vf', filters,
                   *pix, *codec, '-b:v', '2000000', '-c:a', 'aac', '-b:a', '96k', str(output)]
        try:
            seconds = run(options)
            info = json.loads(subprocess.check_output([str(ffprobe), '-v', 'error', '-show_entries',
                              'format=duration,size', '-of', 'json', str(output)], text=True))['format']
            run(['-xerror', '-i', str(output), '-map', '0', '-f', 'null', '-'])
            item = {'mode': mode, 'repeat': repeat, 'seconds': round(seconds, 3),
                    'bytes': int(info['size']), 'duration': float(info['duration']), 'decoded': True}
        except subprocess.CalledProcessError as error:
            item = {'mode': mode, 'repeat': repeat, 'unavailable': str(error.stderr)[-2000:]}
        results.append(item)
        print(json.dumps(item), flush=True)
(args.output / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
