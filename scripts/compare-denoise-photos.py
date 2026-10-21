#!/usr/bin/env python3
"""Private before/after gallery from trusted native RGBA dumps made by the XPC probe.

Requires test-only numpy. Manifest paths and photographs stay in project var/.
Each variant is filtered as a complete native frame before any crop is taken.
"""
import argparse
import hashlib
import html
import json
from pathlib import Path
import struct
import subprocess
import zlib

import numpy as np

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def load(directory, name='input'):
    w, h = json.loads((directory / f'{name}.json').read_text())['size']
    path = directory / f'{name}.rgba32f'
    assert path.stat().st_size == w*h*16
    return np.memmap(path, dtype='<f4', mode='r', shape=(h, w, 4))


def display(rgb):
    # Explicit operations avoid depending on a BLAS implementation for gallery
    # conversion. These are the project's linear Rec.2020 -> linear sRGB values.
    r, g, b = np.moveaxis(np.asarray(rgb, dtype=np.float64), -1, 0)
    s = np.stack((1.660491*r-.5876411*g-.0728499*b,
                  -.1245505*r+1.1328999*g-.0083494*b,
                  -.0181508*r-.1005789*g+1.1187297*b), -1)
    s = np.maximum(s, 0)
    s = np.where(s <= .0031308, 12.92*s, 1.055*s**(1/2.4)-.055)
    return np.round(np.clip(s, 0, 1)*255).astype('uint8')


def png(path, rgb):
    data = display(rgb)
    h, w = data.shape[:2]
    def chunk(kind, payload):
        return struct.pack('>I', len(payload)) + kind + payload + struct.pack('>I', zlib.crc32(kind+payload))
    scan = b''.join(b'\0'+row.tobytes() for row in data)
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
                     + chunk(b'sRGB', b'\0') + chunk(b'IDAT', zlib.compress(scan)) + chunk(b'IEND', b''))


def metrics(actual, reference):
    delta = np.asarray(actual-reference, dtype=np.float64)
    y = delta[..., 0]*.2627+delta[..., 1]*.678+delta[..., 2]*.0593
    return {'rgb_rmse': float(np.sqrt(np.mean(delta**2))),
            'luma_rmse': float(np.sqrt(np.mean(y**2))),
            'chroma_rmse': float(np.sqrt(np.mean((delta-y[..., None])**2)))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--baseline', required=True, type=Path)
    parser.add_argument('--candidate', required=True, type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    assert ROOT/'var' in root.parents and not root.exists(), 'Choose a new private campaign directory'
    root.mkdir(parents=True)
    probes = {'before': args.baseline.resolve(strict=True), 'after': args.candidate.resolve(strict=True)}
    spec = json.loads(args.manifest.read_text())
    report = {'passed': False, 'scope': spec['scope'], 'probe_sha256': {k: digest(v) for k, v in probes.items()}, 'cases': []}
    sections = []
    for index, case in enumerate(spec['cases']):
        directory = Path(case['input']).resolve(strict=True)
        source = load(directory)
        folder = root / f'case-{index+1}'
        folder.mkdir()
        row = {'label': case['label'], 'source_sha256': json.loads((directory/'input.json').read_text()).get('source_sha256'),
               'input_sha256': digest(directory/'input.rgba32f'), 'size': list(source.shape[1::-1]), 'variants': {}, 'crops': []}
        outputs = {'input': source}
        for variant, method in [('before', 'spectral'), ('after', 'multiscale')]:
            output = folder/variant
            command = [str(probes[variant]), str(directory), str(output), '80', '80', method, '4']
            with output.with_suffix('.log').open('w') as log:
                subprocess.run(command, check=True, stdout=log, stderr=log, timeout=120)
            outputs[variant] = load(folder, variant)
            row['variants'][variant] = json.loads(output.with_suffix('.json').read_text())
            row['variants'][variant]['rgba32f_sha256'] = digest(output.with_suffix('.rgba32f'))
        for crop_index, crop in enumerate(case['crops']):
            x, y, w, h = crop['rect']
            assert x >= 0 and y >= 0 and x+w <= source.shape[1] and y+h <= source.shape[0]
            samples = {k: v[y:y+h, x:x+w, :3] for k, v in outputs.items()}
            if 'reference' in crop:
                ref = crop['reference']
                dx, dy = ref['offset']
                samples['reference'] = load(Path(ref['input']))[y+dy:y+dy+h, x+dx:x+dx+w, :3] * ref['gain']
            item = {'label': crop['label'], 'rect': crop['rect'], 'reference': crop.get('reference'), 'metrics': {}}
            if 'reference' in samples:
                item['metrics'] = {k: metrics(v, samples['reference']) for k, v in samples.items() if k != 'reference'}
            row['crops'].append(item)
            title = html.escape(case['label']+' · '+crop['label'])
            figures = []
            paths = {}
            labels = {'input': 'Foto senza riduzione rumore', 'before': 'Filtro precedente · 80/80', 'after': 'Filtro aggiornato · 80/80', 'reference': 'ISO 100 · riferimento imperfetto'}
            for key, values in samples.items():
                filename = f'crop-{crop_index+1}-{key}.png'
                png(folder/filename, values)
                paths[key] = f'{folder.name}/{filename}'
                figures.append(f'<figure><figcaption>{labels[key]}</figcaption><img src="{paths[key]}" width="{w}" height="{h}"></figure>')
            sections.append(f'<section><h2>{title}</h2><p>Pixel nativi, stessa area e conversione sRGB. Luminanza 80 · cromia 80 · soglia 0.</p>'
                            f'<button onclick="this.textContent=this.nextElementSibling.classList.toggle(\'after\')?\'Mostra filtro precedente\':\'Mostra filtro aggiornato\'">Mostra filtro aggiornato</button>'
                            f'<div class="blink" style="width:{w}px;height:{h}px"><img src="{paths["before"]}"><img src="{paths["after"]}"><span class="old">Filtro precedente</span><span class="new">Filtro aggiornato</span></div>'
                            + '<div class="row">'+''.join(figures)+'</div></section>')
        report['cases'].append(row)
        (root/'comparison.json').write_text(json.dumps(report, indent=2, ensure_ascii=False)+'\n')
        print(case['label'], {k: v['seconds'] for k, v in row['variants'].items()}, flush=True)
    report['passed'] = True
    (root/'comparison.json').write_text(json.dumps(report, indent=2, ensure_ascii=False)+'\n')
    (root/'index.html').write_text('''<!doctype html><html lang="it"><meta charset="utf-8"><title>TrueRenderer · Riduzione rumore</title>
<style>body{background:#151719;color:#eee;font:15px system-ui;margin:32px}h1{font-size:28px}p{color:#bfc6ce;max-width:1000px;line-height:1.6}section{border-top:1px solid #45494e;margin-top:36px;padding-top:16px}.row{display:flex;overflow-x:auto;gap:16px;padding:16px 0}figure{margin:0;flex-shrink:0}figcaption{padding:8px 0;color:#c8d2dd}img{display:block}button{padding:8px 16px;background:#344b63;color:white;border:0;border-radius:6px;cursor:pointer;margin:8px 0}.blink{position:relative;max-width:none}.blink img{position:absolute;top:0;left:0}.blink img:nth-child(2),.blink .new{display:none}.blink.after img:nth-child(2),.blink.after .new{display:block}.blink.after .old{display:none}.blink span{position:absolute;top:8px;left:8px;background:#101214cc;padding:4px 8px}</style>
<h1>TrueRenderer · Confronto riduzione rumore</h1><p>Il pulsante confronta <strong>Filtro precedente</strong> (la riduzione rumore che avevi prima delle modifiche) e <strong>Filtro aggiornato</strong> (la nuova riduzione rumore). L’etichetta sulla foto indica quale risultato stai guardando. Nella fila sotto, <strong>Foto senza riduzione rumore</strong> mostra invece la foto con il filtro disattivato. I due filtri sono confrontati con gli stessi valori: luminanza 80, cromia 80 e soglia 0. Elaborazione dell’intero fotogramma nativo prima del ritaglio; nessun ridimensionamento o ritocco dei confronti. I JPEG sono copie compresse dello stesso sviluppo RAW e vengono riaperti tramite i servizi XPC dell’app. Il riferimento ISO 100 ha esposizione e allineamento fissati sull’ingresso e non costituisce una verità assoluta.</p>'''
                                     + ''.join(sections) + '</html>')


if __name__ == '__main__':
    main()
