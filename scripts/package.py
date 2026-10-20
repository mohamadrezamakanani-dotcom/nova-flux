#!/usr/bin/env python3
"""Assemble a locally signed macOS .app from the release binary and runtime."""
from pathlib import Path
import argparse, plistlib, re, shutil, subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--runtime', type=Path, required=True)
parser.add_argument('--destination', type=Path, required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parent.parent
version = re.search(r'^version = "([^"]+)"', (root / 'Cargo.toml').read_text(), re.M).group(1)
icon_data = (root / 'assets/icon.png').read_bytes()
if icon_data[:8] != b'\x89PNG\r\n\x1a\n' or icon_data[24:26] != bytes([8, 6]):
    raise SystemExit('The app icon must be an 8-bit RGBA PNG.')
app = args.destination.resolve()
if app.exists():
    raise SystemExit(f'Refusing to overwrite existing app: {app}')
contents = app / 'Contents'
(contents / 'MacOS').mkdir(parents=True)
(contents / 'Resources').mkdir()
shutil.copy2(root / 'target/release/nova-flux', contents / 'MacOS/nova-flux')
shutil.copy2(root / 'assets/AppIcon.icns', contents / 'Resources/AppIcon.icns')
shutil.copytree(args.runtime.resolve(), contents / 'Resources/runtime')
plist = {
 'CFBundleName': 'Nova Flux', 'CFBundleDisplayName': 'Nova Flux',
 'CFBundleExecutable': 'nova-flux', 'CFBundleIdentifier':'com.nova.flux',
 'CFBundlePackageType':'APPL', 'CFBundleShortVersionString':version,
 'CFBundleVersion':'6', 'CFBundleIconFile':'AppIcon.icns',
 'CFBundleDevelopmentRegion':'en', 'CFBundleLocalizations':['en'],
 'LSMinimumSystemVersion':'12.0', 'LSApplicationCategoryType':'public.app-category.video',
 'NSHighResolutionCapable':True,
 'NSHumanReadableCopyright':'Nova Flux · Local media tools',
}
with (contents / 'Info.plist').open('wb') as f:
    plistlib.dump(plist,f)
for p in (contents / 'Resources/runtime').rglob('*'):
    if p.is_file() and (p.suffix=='.dylib' or p.parent.name=='bin'):
        subprocess.run(['codesign','--force','--sign','-',str(p)],check=True,capture_output=True)
subprocess.run(['codesign','--force','--sign','-',str(app)],check=True)
subprocess.run(['codesign','--verify','--deep','--strict',str(app)],check=True)
subprocess.run([str(contents/'MacOS/nova-flux'),'--diagnose'],check=True)
print(app)
