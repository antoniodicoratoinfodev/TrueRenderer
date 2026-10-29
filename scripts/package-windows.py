#!/usr/bin/env python3
"""Build a self-contained per-user x64 installer from prebuilt release binaries."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def copy(source, destination):
    if source.is_symlink() or not source.is_file():
        raise ValueError(f"Expected regular package input: {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binaries", type=Path, default=ROOT / "target/release")
    parser.add_argument("--dxc", type=Path, default=ROOT / "dist/TrueRenderer-windows/runtime/dxc")
    parser.add_argument("--iscc", type=Path, default=ROOT / ".tools/inno/ISCC.exe")
    parser.add_argument("--objdump", type=Path, default=ROOT / ".tools/mingw64/bin/objdump.exe")
    args = parser.parse_args()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    inventory_path = ROOT / "reports/dependency-inventory-windows-installer.json"
    inventory = json.loads(inventory_path.read_text())
    if inventory["cargo_lock_sha256"] != digest(ROOT / "Cargo.lock"):
        raise ValueError("Regenerate Windows installer dependency inventory for this Cargo.lock")
    output = ROOT / "dist/installers"
    output.mkdir(parents=True, exist_ok=True)
    (ROOT / "var").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="windows-package-", dir=ROOT / "var") as staging:
        payload = Path(staging)
        for name in ("truerenderer.exe", "tr-worker.exe"):
            copy(args.binaries / name, payload / name)
        for name in ("LICENSE", "NOTICE.md"):
            copy(ROOT / name, payload / name)
        (payload / "truerenderer-installed.txt").write_text("1\n", encoding="utf8")
        # Only synthetic files named in the manifest, never sample/user photos.
        manifest = json.loads((ROOT / "corpus/manifest.json").read_text())
        copy(ROOT / "corpus/manifest.json", payload / "corpus/manifest.json")
        for entry in manifest["images"]:
            name = entry["file"]
            if Path(name).name != name or digest(ROOT / "corpus" / name) != entry["sha256"]:
                raise ValueError(f"Invalid corpus member: {name}")
            copy(ROOT / "corpus" / name, payload / "corpus" / name)
        dxc_manifest = json.loads((args.dxc / "manifest.json").read_text())
        if dxc_manifest["version"] != "1.8.2505.1" or dxc_manifest["architecture"] != "x64":
            raise ValueError("Unexpected DXC runtime version/architecture")
        for name in ("dxcompiler.dll", "dxil.dll", "LICENSE-LLVM.txt", "LICENSE-MIT.txt", "LICENSE-MS.txt", "manifest.json"):
            if name != "manifest.json" and digest(args.dxc / name) != dxc_manifest["files"][name]:
                raise ValueError(f"DXC hash mismatch: {name}")
            copy(args.dxc / name, payload / "runtime/dxc" / name)
        copy(inventory_path, payload / "ThirdPartyNotices/inventory.json")
        runtime_notices = ROOT / "reports/windows-runtime-notices"
        for notice in json.loads((runtime_notices / "manifest.json").read_text(encoding="utf-8-sig")):
            source = runtime_notices / notice["file"]
            if digest(source) != notice["sha256"]:
                raise ValueError(f"Runtime notice hash mismatch: {source}")
            copy(source, payload / "ThirdPartyNotices" / source.name)
        copy(runtime_notices / "manifest.json", payload / "ThirdPartyNotices/runtime-notices.json")
        for package in inventory["packages"]:
            for notice in package["notices"]:
                source = ROOT / notice["file"]
                if digest(source) != notice["sha256"]:
                    raise ValueError(f"Notice hash mismatch: {source}")
                copy(source, payload / "ThirdPartyNotices" / source.name)
        # Include covered native sources and data, not merely a link to a repo.
        tracked = subprocess.check_output(["git", "ls-files", "-z", "third_party/libraw", "third_party/colour-robertson", "third_party/nikon-reference-data", "third_party/fuji-codec"], cwd=ROOT).decode().split("\0")
        with zipfile.ZipFile(payload / "ThirdPartyNotices/CoveredSources.zip", "w", zipfile.ZIP_DEFLATED) as archive:
            for name in filter(None, tracked):
                archive.write(ROOT / name, name)
        # Fail closed on non-system runtime imports rather than ship a package
        # that happens to work only with the build machine's PATH/toolchain.
        system = {"kernel32.dll", "user32.dll", "dwmapi.dll", "imm32.dll", "ntdll.dll", "uxtheme.dll", "bcryptprimitives.dll", "bcrypt.dll", "dxgi.dll", "gdi32.dll", "ole32.dll", "oleaut32.dll", "opengl32.dll", "setupapi.dll", "uiautomationcore.dll", "advapi32.dll", "combase.dll", "shell32.dll", "userenv.dll", "ws2_32.dll", "version.dll", "shlwapi.dll", "msvcrt.dll", "ucrtbase.dll", "rpcrt4.dll", "secur32.dll", "crypt32.dll", "winmm.dll", "dbghelp.dll", "psapi.dll"}
        imports = {}
        for binary in list(payload.glob("*.exe")) + list(payload.glob("runtime/dxc/*.dll")):
            pe = subprocess.check_output([str(args.objdump.resolve()), "-p", str(binary)], text=True)
            if "pei-x86-64" not in pe:
                raise ValueError(f"Expected Windows x64 PE: {binary.name}")
            names = sorted(set(re.findall(r"DLL Name:\s*(\S+)", pe)))
            if not names:
                raise ValueError(f"No PE imports parsed: {binary.name}")
            for name in names:
                if name.lower() not in system and not name.lower().startswith(("api-ms-win-", "ext-ms-win-")):
                    raise ValueError(f"Unbundled dependency {name} in {binary.name}")
            imports[binary.relative_to(payload).as_posix()] = names
        files = {p.relative_to(payload).as_posix(): digest(p) for p in sorted(payload.rglob("*")) if p.is_file()}
        report = {"version": version, "cargo_lock_sha256": digest(ROOT / "Cargo.lock"), "files": files, "imports": imports, "signed": False}
        (payload / "package-manifest.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf8")
        subprocess.run([str(args.iscc.resolve()), f"/DPayload={payload}", f"/DOutput={output}", f"/DAppVersion={version}", str(ROOT / "scripts/windows-installer.iss")], check=True)
        installer = output / f"TrueRenderer-{version}-Setup-x64.exe"
        report["installer_sha256"] = digest(installer)
        (output / (installer.name + ".sha256")).write_text(f"{report['installer_sha256']}  {installer.name}\n")
        (output / "package-report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf8")
        print(installer)


if __name__ == "__main__":
    main()
