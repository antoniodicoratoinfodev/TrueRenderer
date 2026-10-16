#!/usr/bin/env python3
"""Compare native equal-EV campaigns and create a private before/after gallery.

Requires NumPy. Inputs and output must remain private when they contain photos.
Metrics describe changes, not absolute scene colour accuracy. No fitting of EV,
WB, matrix, registration or masks against the corrected output takes place.
"""
import argparse
import gzip
import json
import os
from pathlib import Path

import numpy as np

ENGINES = ["TrueRendererExperimental", "Apple", "LibRawAhd", "LibRawBilinear", "TrueRenderer"]
LABELS = ["TrueRenderer autonomo", "Apple RAW", "LibRaw AHD", "LibRaw bilineare", "TrueRenderer storico"]


def load(path):
    return json.loads(path.read_text())


def floats(folder, filename, size):
    path = folder / filename
    if path.name != filename:
        raise ValueError("invalid pixel filename")
    raw = path.read_bytes() if path.exists() else gzip.decompress(path.with_suffix(path.suffix + ".gz").read_bytes())
    pixels = np.frombuffer(raw, dtype="<f4").reshape(size[1], size[0], 3)
    if not np.isfinite(pixels).all():
        raise ValueError("nonfinite linear pixel")
    return pixels


def difference(a, b):
    delta = b.astype(np.float64) - a
    return {"pixels": int(a.shape[0] * a.shape[1]),
            "changed_pixels": int(np.any(a != b, axis=2).sum()),
            "max_abs_rgb": float(np.abs(delta).max()),
            "rgb_rmse": float(np.sqrt(np.mean(delta * delta)))}


def neutral_metrics(pixels):
    low, high = pixels.min(axis=2), pixels.max(axis=2)
    valid = high > 1e-6
    if not valid.any():
        raise ValueError("neutral diagnostic region contains no signal")
    spread = (high[valid] - low[valid]) / high[valid]
    magenta = (np.minimum(pixels[:, :, 0], pixels[:, :, 2]) - pixels[:, :, 1])[valid] / high[valid]
    return {"mean_linear_rgb": pixels.mean(axis=(0, 1), dtype=np.float64).tolist(),
            "relative_channel_spread_median": float(np.median(spread)),
            "relative_magenta_excess_median": float(np.median(magenta))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--regions", type=Path, help="Fixed overview rectangles selected before analysis, keyed by source id")
    args = parser.parse_args()
    before, after = args.before.resolve(), args.after.resolve()
    bm, am = load(before / "manifest.json"), load(after / "manifest.json")
    assert bm["complete"] and bm["passed"] and am["complete"] and am["passed"]
    assert bm["protocol"] == am["protocol"] == "TRExp-engine-exposure-v1"
    assert bm["input_manifest_sha256"] == am["input_manifest_sha256"]
    assert bm["exposure_ev"] == am["exposure_ev"] and bm["exposure_ev"]
    assert [s["id"] for s in bm["sources"]] == [s["id"] for s in am["sources"]]
    regions = load(args.regions) if args.regions else {}
    rows, gallery, refusals = [], [], []
    verified_images = 0
    for source in bm["sources"]:
        a, b = load(before / source["report"]), load(after / source["report"])
        assert a["source_sha256"] == b["source_sha256"] and a["original_unchanged"] and b["original_unchanged"]
        ae, be = ({v["engine"]: v for v in r["engines"]} for r in [a, b])
        assert set(ae) == set(be)
        item = {"id": a["id"], "camera": a["camera"], "kind": a["kind"], "before": ae, "after": be}
        gallery.append(item)
        for engine in ENGINES:
            left, right = ae[engine], be[engine]
            assert left["accepted"] == right["accepted"]
            if not left["accepted"]:
                assert left["expected_refusal"] and right["expected_refusal"]
                refusals.append({"id": a["id"], "engine": engine})
                continue
            assert left["size"] == right["size"]
            assert left["overview"]["size"] == right["overview"]["size"]
            av, bv = (floats(folder, e["overview"]["linear"], e["overview"]["size"])
                      for folder, e in [(before, left), (after, right)])
            crops = []
            for zone in range(9):
                name = f'{a["id"]}-{engine}-zone-{zone}.f32'
                crops.append(difference(floats(before, name, [320, 320]), floats(after, name, [320, 320])))
            for folder, e in [(before, left), (after, right)]:
                sequences = [e["exposure_overview"], *e["exposure_crops"]]
                assert len(sequences) == 10
                for sequence in sequences:
                    assert [v["ev"] for v in sequence] == bm["exposure_ev"]
                    for v in sequence:
                        assert v["linear_gain_exact"] and v["gain"] == 2 ** v["ev"]
                        assert (folder / v["png"]).is_file()
                        verified_images += 1
            diagnostic = []
            # Regions are in each engine's own overview coordinates. They are
            # only used for within-engine before/after, never cross-engine truth.
            for region in regions.get(a["id"], []):
                x, y, w, h = region["rect"]
                assert x >= 0 and y >= 0 and x+w <= av.shape[1] and y+h <= av.shape[0]
                ar, br = av[y:y+h, x:x+w], bv[y:y+h, x:x+w]
                diagnostic.append({"name": region["name"], "rect": region["rect"],
                    "before": neutral_metrics(ar), "after": neutral_metrics(br),
                    "difference": difference(ar, br)})
            rows.append({"id": a["id"], "camera": a["camera"], "engine": engine,
                "recipe_before": left["recipe"], "recipe_after": right["recipe"],
                "overview": difference(av, bv), "crops": crops, "diagnostic_regions": diagnostic})
    report = {"protocol": "TR-equal-EV-highlight-review-v1", "exposure_ev": bm["exposure_ev"],
        "sources": len(gallery), "accepted_per_campaign": len(rows), "expected_refusals_per_campaign": len(refusals),
        "native_exposure_images_verified": verified_images, "records": rows, "refusals": refusals,
        "limits": "Same EV offset, native engine brightness retained. Overviews and 9 full-resolution crops; not full-size exports. Neutrality on selected clipped plateaus is a rendering criterion, not measured scene colour. No absolute colour or detail ranking."}
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output / "analysis.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    data = {"sources": gallery, "ev": bm["exposure_ev"], "engines": ENGINES, "labels": LABELS,
            "before": os.path.relpath(before, args.output.resolve()), "after": os.path.relpath(after, args.output.resolve())}
    template = '''<!doctype html><html lang="it"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>TrueRenderer · esposizioni uguali, prima e dopo</title>
<style>body{margin:24px;background:#17191c;color:#eee;font:15px system-ui}h1{font-size:25px;margin-bottom:8px}p{max-width:1050px;color:#bcc4d0;line-height:1.5}header{position:sticky;top:0;background:#17191cf5;padding:12px 0;z-index:2}label{display:inline-block;margin:0 20px 10px 0}select{font:inherit;background:#292e35;color:white;border:1px solid #647082;border-radius:6px;padding:6px}section{display:grid;grid-template-columns:repeat(5,minmax(200px,1fr));gap:12px;overflow:auto}article{background:#22272e;border-radius:9px;padding:10px;min-width:200px}h2{font-size:16px;margin:4px 0 12px}img{display:block;width:100%;height:340px;object-fit:contain;background:#111}small{color:#a8b5c5;display:block;line-height:1.4;margin:7px 0;overflow-wrap:anywhere}.refused{height:340px;display:grid;place-items:center;color:#c5c5c5}a{color:#9bc9ff}.row{font-size:19px;margin:20px 0 10px}</style>
<h1>Stessa foto, stessa variazione di esposizione</h1>
<p>Il selettore EV applica lo stesso fattore lineare a tutti i motori, senza correggere automaticamente luminosità o bilanciamento. AHD parte da una luminosità diversa. Le immagini sono PNG sRGB SDR: a EV elevati lo schermo può nascondere le differenze nelle alte luci. A EV negativi si vede la dominante che era presente nei valori lineari.</p>
<header><label>Fotografia <select id="photo"></select></label><label>Esposizione comune <select id="ev"></select></label><label>Vista <select id="zone"><option value="-1">Intera fotografia (ridotta)</option></select></label><span id="meta"></span></header>
<div class="row">Prima della correzione</div><section id="before"></section><div class="row">Dopo la correzione</div><section id="after"></section>
<p>I ritagli contengono 320 × 320 pixel originali; i cinque motori possono avere crop nativi leggermente diversi. Il confronto prima/dopo dello stesso motore usa le medesime coordinate. Il nuovo ripiego neutro tratta il colore indeterminato dei canali saturi: non ricostruisce il colore originale del cielo. I colori lontani dalla saturazione e le ricostruzioni ammesse sono verificati separatamente. La spalla può attenuare luci colorate prossime al clipping. <a href="analysis.json">Dati numerici e limiti</a>.</p>
<script>const DATA=__DATA__;
const photo=document.querySelector('#photo'),ev=document.querySelector('#ev'),zone=document.querySelector('#zone');
for(let i=0;i<DATA.sources.length;i++){const s=DATA.sources[i];photo.add(new Option(s.id+' · '+s.camera,i));if(s.id==='raw-25')photo.value=String(i)}
DATA.ev.forEach((v,i)=>{ev.add(new Option((v>0?'+':'')+v+' EV',i));if(v===-2)ev.value=String(i)});
for(let i=0;i<9;i++)zone.add(new Option('Ritaglio '+(i+1)+' · riga '+(Math.floor(i/3)+1)+', colonna '+(i%3+1),i));
function update(){const s=DATA.sources[Number(photo.value)],e=Number(ev.value),z=Number(zone.value);document.querySelector('#meta').textContent=s.kind==='original'?'RAW originale':'Controllo DNG derivato';
for(const side of ['before','after']){const grid=document.getElementById(side);grid.replaceChildren();DATA.engines.forEach((engine,i)=>{const a=document.createElement('article'),title=document.createElement('h2');title.textContent=DATA.labels[i];a.append(title);const r=s[side][engine];if(r.accepted){const records=z<0?r.exposure_overview:r.exposure_crops[z];const link=document.createElement('a');link.href=DATA[side]+'/'+records[e].png;link.target='_blank';const im=document.createElement('img');im.src=link.href;im.alt=s.id+' '+DATA.labels[i]+' '+DATA.ev[e]+' EV '+side;link.append(im);a.append(link);const note=document.createElement('small');note.textContent=r.recipe;a.append(note)}else{const note=document.createElement('div');note.className='refused';note.textContent='Formato non supportato da questo motore';a.append(note)}grid.append(a)})}}
for(const control of [photo,ev,zone])control.addEventListener('change',update);update();</script></html>'''
    # Keep only metadata needed by the gallery, not the large block statistics.
    for source in gallery:
        for side in ["before", "after"]:
            source[side] = {engine: {k: v for k, v in record.items() if k in
                ["accepted", "recipe", "exposure_overview", "exposure_crops"]}
                for engine, record in source[side].items()}
    (args.output / "index.html").write_text(template.replace("__DATA__", json.dumps(data, ensure_ascii=False).replace("<", "\\u003c")))
    print(json.dumps({k: v for k, v in report.items() if k not in ["records", "refusals"]}, indent=2))


if __name__ == "__main__":
    main()
