#!/usr/bin/env python3
"""Exercise install/reinstall/uninstall using synthetic, isolated local data.

Run on the interactive Windows desktop. Refuses to replace an existing registered
installation. Does not touch the development shortcut or user's catalog.
"""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import winreg

ROOT = Path(__file__).resolve().parents[1]
UNINSTALL = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{D891B420-751F-4627-ACD6-56BAA778CE81}_is1"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("installer", type=Path)
    args = parser.parse_args()
    try:
        key = winreg.OpenKey(winreg.HKEY_CURRENT_USER, UNINSTALL, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY)
    except FileNotFoundError:
        pass
    else:
        key.Close()
        raise SystemExit("Existing registered installation: use a separate Windows test account")
    (ROOT / "var").mkdir(exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="installer-test-", dir=ROOT / "var"))
    app = work / "Programma con spazi"
    profile = work / "Profilo vuoto"
    profile.mkdir()
    report = {"passed": False, "installer_sha256": sha(args.installer), "work": work.relative_to(ROOT).as_posix(), "checks": {}}
    report_path = work / "result.json"
    report_path.write_text(json.dumps(report, indent=2))
    install = [str(args.installer.resolve()), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/TASKS=", "/GROUP=TrueRenderer Installer Test", f"/DIR={app}"]
    subprocess.run(install + [f"/LOG={work / 'install.log'}"], check=True, timeout=120)
    manifest = json.loads((app / "package-manifest.json").read_text())
    assert all(sha(app / name) == expected for name, expected in manifest["files"].items())
    report["checks"]["installed_payload_hashes"] = len(manifest["files"])
    environment = os.environ.copy()
    environment["LOCALAPPDATA"] = str(profile)
    environment["PATH"] = os.pathsep.join([str(Path(os.environ["SystemRoot"]) / "System32"), os.environ["SystemRoot"]])
    for name in ("CARGO_HOME", "RUSTUP_HOME", "TR_WORKER_BINARY"):
        environment.pop(name, None)
    # Ordinary launch has no repository path, explicit data flag or special cwd.
    data = profile / "TrueRenderer/var"
    process = subprocess.Popen([str(app / "truerenderer.exe")], cwd=profile, env=environment)
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.IsIconic.argtypes = [wintypes.HWND]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
    windows = []
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    @callback_type
    def window_callback(window, _):
        pid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(window, ctypes.byref(pid))
        title = ctypes.create_unicode_buffer(256)
        user32.GetWindowTextW(window, title, len(title))
        if pid.value == process.pid and title.value == "TrueRenderer" and user32.IsWindowVisible(window):
            windows.append(window)
        return True

    for _ in range(60):
        windows.clear()
        user32.EnumWindows(window_callback, 0)
        if windows and (data / "library.sqlite").is_file():
            break
        assert process.poll() is None, "Installed application exited before creating a window/catalog"
        time.sleep(0.5)
    assert windows and (data / "library.sqlite").is_file()
    assert not user32.IsIconic(windows[0])
    time.sleep(2)  # let the initial window/device setup finish before WM_CLOSE
    windows.clear()
    user32.EnumWindows(window_callback, 0)
    assert windows
    assert user32.PostMessageW(windows[0], 0x0010, 0, 0)  # normal WM_CLOSE
    assert process.wait(timeout=30) == 0
    report["checks"]["ordinary_launch_visible_without_development_path"] = True
    with (work / "smoke.stdout.log").open("w") as out, (work / "smoke.stderr.log").open("w") as err:
        subprocess.run([str(app / "truerenderer.exe"), "--smoke-test"], cwd=profile, env=environment, stdout=out, stderr=err, check=True, timeout=120)
    smoke = json.loads((app / "reports/smoke-windows.json").read_text())
    assert smoke["passed"], smoke
    report["checks"]["native_smoke"] = smoke
    assert (data / "smoke/library.sqlite").is_file()
    # A durable sentinel checks that the installer never recursively purges data.
    (data / "backups").mkdir(exist_ok=True)
    sentinel = data / "backups/keep.txt"
    sentinel.write_text("durable synthetic backup\n")
    before = {p.relative_to(data).as_posix(): sha(p) for p in data.rglob("*") if p.is_file()}
    subprocess.run(install + [f"/LOG={work / 'upgrade.log'}"], check=True, timeout=120)
    assert all(sha(data / name) == expected for name, expected in before.items())
    assert all(sha(app / name) == expected for name, expected in manifest["files"].items())
    report["checks"]["reinstall_preserves_data"] = True
    # Also prove uninstall does not remove an untracked file inside the app tree.
    (app / "user-created.txt").write_text("retain\n")
    subprocess.run([str(app / "unins000.exe"), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", f"/LOG={work / 'uninstall.log'}"], check=True, timeout=120)
    for _ in range(20):
        if not (app / "truerenderer.exe").exists():
            break
        time.sleep(0.5)
    assert not (app / "truerenderer.exe").exists()
    assert all(sha(data / name) == expected for name, expected in before.items())
    assert (app / "user-created.txt").read_text() == "retain\n"
    report["checks"]["uninstall_preserves_data_and_untracked_files"] = True
    report["passed"] = True
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(report_path)


if __name__ == "__main__":
    main()
