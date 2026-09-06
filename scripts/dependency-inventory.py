#!/usr/bin/env python3
"""Resolved local build/test dependency inventory; not a release compliance verdict."""
from pathlib import Path
import csv, hashlib, json, re, subprocess

root=Path(__file__).resolve().parents[1]
output=subprocess.run([str(root/"scripts/cargo-local.sh"),"metadata","--format-version","1","--locked","--offline","--filter-platform","aarch64-apple-darwin"],capture_output=True,text=True,check=True)
metadata=json.loads(output.stdout)
resolved={node["id"] for node in metadata["resolve"]["nodes"]}
lock=(root/"Cargo.lock").read_text()
checksums={}
for block in lock.split("[[package]]")[1:]:
    values=dict(re.findall(r'^([a-z]+) = "([^"]*)"$',block,re.M))
    if "checksum" in values: checksums[(values["name"],values["version"])]=values["checksum"]
packages=[]
notices=root/"reports/dependency-notices"
notices.mkdir(exist_ok=True)
for package in sorted(metadata["packages"],key=lambda p:p["name"]):
    if package["id"] not in resolved: continue
    record={key:package.get(key) for key in ["name","version","license","source","repository"]}
    record["checksum_sha256"]=checksums.get((package["name"],package["version"]))
    record["notices"]=[]
    folder=Path(package["manifest_path"]).parent
    for path in sorted(folder.iterdir()):
        if path.is_file() and (path.name.upper().startswith("LICENSE") or path.name.upper().startswith("LICENCE") or path.name.upper().startswith("COPYING") or path.name.upper().startswith("NOTICE")):
            data=path.read_bytes()
            destination=notices/(package["name"]+"-"+package["version"]+"-"+path.name)
            destination.write_bytes(data)
            record["notices"].append({"file":str(destination.relative_to(root)),"sha256":hashlib.sha256(data).hexdigest()})
    packages.append(record)
app_version=next(package["version"] for package in metadata["packages"] if package["name"] == "truerenderer")
report={"application":"TrueRenderer","version":app_version,"target":"aarch64-apple-darwin","scope":"Cargo-resolved build/test packages for this workspace; inventory only, not formal release SBOM or legal approval","rust":"1.98.1","cargo_lock_sha256":hashlib.sha256(lock.encode()).hexdigest(),"packages":packages}
(root/"reports/dependency-inventory.json").write_text(json.dumps(report,indent=2)+"\n")
with (root/"reports/dependency-inventory.csv").open("w",newline="") as file:
    writer=csv.writer(file,lineterminator="\n");writer.writerow(["name","version","license","checksum_sha256","repository"])
    for p in packages:writer.writerow([p.get(k) for k in ["name","version","license","checksum_sha256","repository"]])
print(f"Inventario: {len(packages)} package; notices copiati in reports/dependency-notices")
