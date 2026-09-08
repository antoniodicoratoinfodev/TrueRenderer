#!/usr/bin/env python3
"""Record the local preview measurement environment without personal paths or IDs."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess

ROOT = Path(__file__).resolve().parents[1]

def command(*args):
    return subprocess.check_output(args, text=True).strip()

def main():
    bundle = ROOT / 'dist/TrueRenderer.app'
    info = plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())
    smoke = json.loads((ROOT / 'reports/smoke-macos.json').read_text())
    # diskutil accepts a device or mount point, not an arbitrary subdirectory.
    # df resolves the actual data volume even on split APFS system/data mounts.
    device = command('/bin/df', '-P', str(ROOT)).splitlines()[-1].split()[0]
    disk = plistlib.loads(subprocess.check_output(['diskutil', 'info', '-plist', device]))
    power = command('pmset', '-g', 'batt').splitlines()[0]
    binaries = {}
    for name in ['Contents/MacOS/TrueRenderer', 'Contents/MacOS/tr-worker',
                 'Contents/XPCServices/Decoder0.xpc/Contents/MacOS/Decoder',
                 'Contents/XPCServices/Decoder1.xpc/Contents/MacOS/Decoder']:
        binaries[name] = hashlib.sha256((bundle / name).read_bytes()).hexdigest()
    report = {
        'application': 'TrueRenderer', 'version': info['CFBundleShortVersionString'],
        'recorded_at': datetime.now(timezone.utc).isoformat(),
        'cpu': command('sysctl', '-n', 'machdep.cpu.brand_string'),
        'logical_cpus': int(command('sysctl', '-n', 'hw.logicalcpu')),
        'ram_bytes': int(command('sysctl', '-n', 'hw.memsize')),
        'os_version': command('sw_vers', '-productVersion'),
        'os_build': command('sw_vers', '-buildVersion'),
        'gpu': smoke.get('adapter', 'See smoke-macos.json'),
        'surface': smoke.get('surface', 'See smoke-macos.json'),
        'pixels_per_point': smoke.get('pixels_per_point', 'See screenshot sampling reports'),
        'volume': {key: disk.get(key) for key in ['FilesystemType', 'BusProtocol', 'SolidState', 'Internal']},
        'power_source': power,
        'git_base_commit': command('git', '-C', str(ROOT), 'rev-parse', 'HEAD'),
        'uncommitted_candidate': bool(command('git', '-C', str(ROOT), 'status', '--porcelain')),
        'binary_sha256': binaries,
        'decoder': 'Apple ImageIO/CIRAWFilter, TR-linear-v1, full source fallback, persistent CPU-requested context; Metal-requested decode only in separate experiment',
        'corpus': 'Project-generated fixtures and 1000 distinct Bayer DNGs; no personal photographs',
        'quality': 'Cache fixtures: Full/Standard edge 256; navigation: Full edge 256; compute: identical generated linear graph CPU/GPU',
        'os_cache': 'Not flushed',
        'limits': 'Driver version follows the OS build; no separate driver memory or thermal/power trace. Renderer benchmark excludes source/cache and surface presentation; navigation excludes presentation. Not full project qualification.',
    }
    (ROOT / 'reports/preview-environment-macos.json').write_text(json.dumps(report, indent=2) + '\n')

if __name__ == '__main__':
    main()
