#!/usr/bin/env python3
"""Resolved local build/test dependency inventory; not a release compliance verdict."""
from pathlib import Path
import argparse, csv, hashlib, json, os, re, subprocess

root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser()
parser.add_argument("--target", default="aarch64-apple-darwin")
parser.add_argument("--stem", default="dependency-inventory")
args=parser.parse_args()
assert re.fullmatch(r"[a-z0-9-]+", args.stem), "Output stem must be a plain name"
cargo=(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(root/"scripts/dev-windows.ps1")] if os.name == "nt" else [str(root/"scripts/cargo-local.sh")])
output=subprocess.run(cargo+["metadata","--format-version","1","--locked","--offline","--filter-platform",args.target],cwd=root,capture_output=True,text=True,encoding="utf8",check=True)
metadata=json.loads(output.stdout)
resolved={node["id"] for node in metadata["resolve"]["nodes"]}
lock=(root/"Cargo.lock").read_text()
checksums={}
for block in lock.split("[[package]]")[1:]:
    values=dict(re.findall(r'^([a-z]+) = "([^"]*)"$',block,re.M))
    if "checksum" in values: checksums[(values["name"],values["version"])]=values["checksum"]
packages=[]
notices=root/"reports"/("dependency-notices" if args.stem == "dependency-inventory" else args.stem+"-notices")
notices.mkdir(exist_ok=True)
for package in sorted(metadata["packages"],key=lambda p:p["name"]):
    if package["id"] not in resolved: continue
    record={key:package.get(key) for key in ["name","version","license","source","repository"]}
    if package.get("license_file"):
        license_path=Path(package["license_file"]).resolve()
        record["license_file"]=license_path.relative_to(root).as_posix() if license_path.is_relative_to(root) else license_path.name
    record["checksum_sha256"]=checksums.get((package["name"],package["version"]))
    record["notices"]=[]
    folder=Path(package["manifest_path"]).parent
    notice_paths=list(folder.iterdir())
    if package["name"] == "lcms2-sys":
        # The static feature builds the bundled C library, whose copyright
        # notice is separate from the Rust bindings' top-level license.
        notice_paths.append(folder/"vendor/LICENSE")
    for path in sorted(notice_paths):
        if path.is_file() and (path.name.upper().startswith("LICENSE") or path.name.upper().startswith("LICENCE") or path.name.upper().startswith("COPYING") or path.name.upper().startswith("NOTICE")):
            data=path.read_bytes()
            suffix=path.relative_to(folder).as_posix().replace("/", "-")
            destination=notices/(package["name"]+"-"+package["version"]+"-"+suffix)
            # Reuse identical historical notice texts without rewriting them.
            existing=next((root/"reports"/directory/destination.name for directory in
                ["dependency-notices", "dependency-inventory-windows-notices"]
                if (root/"reports"/directory/destination.name).is_file()
                and (root/"reports"/directory/destination.name).read_bytes() == data), None)
            if existing is not None:
                destination=existing
            else:
                destination.write_bytes(data)
            record["notices"].append({"file":destination.relative_to(root).as_posix(),"sha256":hashlib.sha256(data).hexdigest()})
    packages.append(record)
app_version=next(package["version"] for package in metadata["packages"] if package["name"] == "truerenderer")
report={"application":"TrueRenderer","version":app_version,"target":args.target,"scope":"Cargo-resolved build/test packages for this workspace; inventory only, not formal release SBOM or legal approval","rust":"1.98.1","cargo_lock_sha256":hashlib.sha256((root/"Cargo.lock").read_bytes()).hexdigest(),"packages":packages}
native=root/"third_party/libraw/manifest-truerenderer.json"
if native.exists():
    report["native_dependencies_outside_cargo"]=[{"name":"LibRaw","manifest":str(native.relative_to(root)).replace("\\", "/"),"manifest_sha256":hashlib.sha256(native.read_bytes()).hexdigest(),"license":"CDDL-1.0"}]
(root/"reports"/(args.stem+".json")).write_text(json.dumps(report,indent=2)+"\n",encoding="utf8")
with (root/"reports"/(args.stem+".csv")).open("w",newline="",encoding="utf8") as file:
    writer=csv.writer(file,lineterminator="\n");writer.writerow(["name","version","license","checksum_sha256","repository"])
    for p in packages:writer.writerow([p.get(k) for k in ["name","version","license","checksum_sha256","repository"]])
print(f"Inventario {args.target}: {len(packages)} package; notices in {notices.relative_to(root)}")
