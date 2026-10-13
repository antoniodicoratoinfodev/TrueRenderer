#!/usr/bin/env python3
"""Private, local inspection gallery for a completed RAW color campaign (Pillow)."""
import argparse
import array
import hashlib
import html
import json
import os
from pathlib import Path
import struct

from PIL import Image


def render_linear(path, size, gain):
    """Presentation only. Metrics always use the original linear block statistics."""
    pixels = array.array("B")
    for r, g, b in struct.iter_unpack("<fff", path.read_bytes()):
        rgb = (1.660491*r-.5876411*g-.0728499*b,
               -.1245505*r+1.1328999*g-.0083494*b,
               -.0181508*r-.1005789*g+1.1187297*b)
        for v in rgb:
            v *= gain
            v = 12.92*v if v <= .0031308 else 1.055*v**(1/2.4)-.055
            pixels.append(int(min(1., max(0., v))*255.+.5))
    return Image.frombytes("RGB", tuple(size), pixels.tobytes())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("campaign", type=Path)
    parser.add_argument("analysis", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    campaign = args.campaign.resolve()
    analysis = json.loads(args.analysis.read_bytes())
    if analysis["campaign_manifest_sha256"] != hashlib.sha256((campaign / "manifest.json").read_bytes()).hexdigest():
        raise ValueError("analysis belongs to another campaign")
    args.output.mkdir()
    records, checks = [], []
    for source in analysis["files"]:
        raw = json.loads((campaign / (source["id"] + ".json")).read_bytes())
        metrics = {p["engine"]: p for p in source["pairs"]}
        engines = []
        for engine in raw["engines"]:
            entry = {"engine": engine["engine"], "accepted": engine["accepted"]}
            if engine["accepted"]:
                overview = engine["overview"]
                original = campaign / overview["png"]
                # Relative local links keep private photographs out of source control.
                entry.update({"original": os.path.relpath(original, args.output),
                              "crops": [os.path.relpath(campaign / f, args.output) for f in engine["crop_pngs"]],
                              "zones": [z["registration"] for z in engine["zones"]],
                              "size": engine["size"], "recipe": engine["recipe"],
                              "input_color": engine["info"]["input_color"]})
                pair = metrics.get(engine["engine"])
                entry["metrics"] = pair
                gain = pair["comparison_exposure_gain"] if pair and pair["sufficient_for_exposure_estimate"] else 1.
                if pair and pair["sufficient_for_exposure_estimate"]:
                    filename = f'{source["id"]}-{engine["engine"]}-normalized.png'
                    render_linear(campaign / overview["linear"], overview["size"], gain).save(args.output / filename)
                    entry["normalized"] = filename
                else:
                    entry["normalized"] = entry["original"]
                if source["id"] in ("raw-00", "raw-30", "raw-39"):
                    reconstructed = render_linear(campaign / overview["linear"], overview["size"], 1.)
                    reference = Image.open(original).convert("RGB")
                    maximum = max(abs(a-b) for a, b in zip(reconstructed.tobytes(), reference.tobytes()))
                    checks.append({"id": source["id"], "engine": engine["engine"], "maximum_display_code_difference": maximum})
                    if maximum > 1:
                        raise ValueError("gallery display differs from native sRGB conversion")
            else:
                entry["error"] = engine["error"]
            engines.append(entry)
        records.append({"id": source["id"], "camera": source["camera"], "kind": source["kind"], "engines": engines})
    data = json.dumps(records, ensure_ascii=False, allow_nan=False).replace("<", "\\u003c")
    page = r'''<!doctype html><html lang="it"><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>TrueRenderer · confronto colore RAW</title><style>
body{background:#202124;color:#eee;font:15px system-ui;margin:28px}h1{font-size:24px}p{max-width:1100px;line-height:1.5}
select,input{margin:8px;padding:6px;font:inherit}#grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:18px}
article{background:#2b2c2e;padding:12px;min-width:0}h2{font-size:18px;margin:0 0 12px}img{display:block;width:100%;height:auto}
.crop{position:relative;width:256px;height:256px;overflow:hidden;max-width:100%}.crop img{position:absolute;width:320px;max-width:none}
small{display:block;line-height:1.5;color:#c5c5c5;margin-top:8px}.weak{color:#ffc18a}.metric{font-variant-numeric:tabular-nums}
details{margin-top:12px;overflow-wrap:anywhere}table{border-collapse:collapse;margin:20px 0}td,th{text-align:left;padding:8px 14px;border-bottom:1px solid #555}
</style><h1>TrueRenderer · confronto colore RAW</h1>
<p>Experimental è l'ancora delle differenze, non un riferimento misurato. Il confronto di esposizione applica un solo fattore a tutti i canali del motore; non corregge il WB né il profilo. ΔE00 descrive differenze fra rese: un valore minore non dimostra maggiore fedeltà alla scena.</p>
<label>File <select id="file"></select></label><label>Vista <select id="zone"><option value="-1">Intera fotografia</option></select></label>
<label><input type="checkbox" id="exposure">Esposizione comune nelle viste d'insieme</label>
<p id="caption"></p><div id="grid"></div>
<details><summary>Metodo e lettura</summary><p>Misure su medie lineari di blocchi 16×16 in nove zone, prima di sRGB e clipping. Solo zone registrate e blocchi nel dominio comune poco variabile. Per i crop viene mostrato il quadrato interno 256×256, traslato verso Experimental; le zone deboli restano segnalate. Le statistiche sono riferite all'intero file. Le anteprime sono sRGB SDR: il display può limitare colori e alte luci. Le immagini non sono acquisite sotto illuminanti o target noti. I controlli DNG conservano i campioni/profilo dell'autonomo e cambiano contenitore/metadati.</p></details>
<h2 style="margin-top:28px">Mediane per fotografia, poi mediana per gruppo</h2>TABLE
<script>const records=DATA;
const labels={TrueRendererExperimental:"TrueRenderer autonomo",Apple:"Apple RAW",LibRawAhd:"LibRaw AHD",LibRawBilinear:"LibRaw bilineare",TrueRenderer:"TrueRenderer storico"};
const files=document.querySelector('#file'),zone=document.querySelector('#zone'),exposure=document.querySelector('#exposure'),grid=document.querySelector('#grid');
records.forEach((r,i)=>files.add(new Option(r.camera+' · '+r.id+(r.kind==='original'?'':' · controllo DNG'),i)));
for(let i=0;i<9;i++)zone.add(new Option('Zona '+(i+1)+' · riga '+(Math.floor(i/3)+1)+', colonna '+(i%3+1),i));
function node(tag,text){const e=document.createElement(tag);if(text!==undefined)e.textContent=text;return e;}
function fmt(v){return v===null||v===undefined?'insufficiente':v.toFixed(3);}
function draw(){grid.replaceChildren();const r=records[+files.value],z=+zone.value;
exposure.disabled=z>=0;document.querySelector('#caption').textContent=r.camera+' · '+r.id+' · '+(z>=0?'Crop allineati, ricette originali.':'Viste d’insieme: '+(exposure.checked?'esposizione comune.':'ricette originali.'));
r.engines.forEach(e=>{const card=node('article');card.append(node('h2',labels[e.engine]));grid.append(card);if(!e.accepted){card.append(node('p','Rifiuto previsto'),node('small',e.error));return;}
const img=node('img');img.alt=labels[e.engine]+' '+r.id;
if(z<0){img.src=exposure.checked?e.normalized:e.original;card.append(img);}else{img.src=e.crops[z];const crop=node('div');crop.className='crop';let reg=e.zones[z];img.style.left=-(32+(reg?reg.dx:0))+'px';img.style.top=-(32+(reg?reg.dy:0))+'px';crop.append(img);card.append(crop);if(reg){let note=node('small',(reg.supported?'Registrazione ammessa':'Registrazione debole, esclusa')+' · Δxy pixel '+reg.dx+', '+reg.dy+' · coseno '+fmt(reg.gradient_cosine));if(!reg.supported)note.className='weak';card.append(note);}}
const p=e.metrics;if(p){if(z<0&&exposure.checked&&!p.sufficient_for_exposure_estimate)card.append(node('small','Vista originale: dati insufficienti per stimare l’esposizione comune.'));const s=node('small','ΔE00 mediano: originale '+fmt(p.raw.delta_e00.median)+' · esposizione comune '+fmt(p.exposure_normalized.delta_e00.median)+'\nEsposizione '+fmt(p.comparison_exposure_ev)+' EV · zone '+p.registered_zones+'/9 · blocchi '+p.exposure_normalized.delta_e00.count);s.className='metric';card.append(s);}else card.append(node('small','Ancora delle coordinate e delle differenze'));
const detail=node('details');detail.append(node('summary','Ricetta e metadati'),node('small',e.recipe+' · '+e.size.join('×')),node('small',e.input_color));card.append(detail);});}
files.onchange=zone.onchange=exposure.onchange=draw;draw();</script></html>'''
    rows = []
    for group in analysis["aggregates"]:
        def median(key):
            v = group[key]["median"]
            return 'insufficiente' if v is None else f'{v:.3f}'
        rows.append('<tr>' + ''.join(f'<td>{html.escape(str(v))}</td>' for v in
                    [group["camera"], group["kind"], group["engine"], group["files_with_normalized_comparison"],
                     median("matched_raw_file_medians_delta_e00"), median("normalized_file_medians_delta_e00"), median("exposure_ev")]) + '</tr>')
    table = '<table><tr><th>Fotocamera</th><th>Dati</th><th>Motore</th><th>File</th><th>ΔE originale*</th><th>ΔE esposizione comune</th><th>EV</th></tr>' + ''.join(rows) + '</table><p>*Stessi blocchi del confronto normalizzato.</p>'
    (args.output / 'index.html').write_text(page.replace('TABLE', table).replace('DATA', data), encoding='utf-8')
    (args.output / 'display-checks.json').write_text(json.dumps(checks, indent=2) + '\n')
    print(args.output / 'index.html')


if __name__ == "__main__":
    main()
