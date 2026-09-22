#!/usr/bin/env python3
"""Private, generated large-image traces and bounded renderer-pressure injection."""
import argparse, ctypes, hashlib, importlib.util, json, shutil, struct, subprocess, tempfile, threading, time, zlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('navigation',ROOT/'scripts/test-navigation-surface.py')
nav=importlib.util.module_from_spec(spec);spec.loader.exec_module(nav)

def chunk(kind,data):
    return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)

def generate(path,w,h,variant):
    compressor=zlib.compressobj(3);parts=[]
    ramp=bytes(round(x*255/(w-1)) for x in range(w))
    stripes=bytes(224 if ((x//7+variant)%2) else 32 for x in range(w))
    for y in range(h):
        row=bytearray(w*3);row[0::3]=ramp
        row[1::3]=bytes([round(y*255/(h-1))])*w
        row[2::3]=stripes if y%128<64 else ramp
        parts.append(compressor.compress(b'\0'+row))
    parts.append(compressor.flush())
    path.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',w,h,8,2,0,0,0))+chunk(b'sRGB',b'\0')+chunk(b'IDAT',b''.join(parts))+chunk(b'IEND',b''))

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundle',type=Path,required=True)
    parser.add_argument('--sizes',default='12,24,45')
    parser.add_argument('--qualities',default='Standard,Full')
    parser.add_argument('--modes',default='Cpu,Gpu')
    parser.add_argument('--memory-mib',type=int,default=4096)
    parser.add_argument('--pressure',action='store_true')
    parser.add_argument('--raw-folder',type=Path)
    parser.add_argument('--engines',default='Apple')
    args=parser.parse_args()
    work=Path(tempfile.mkdtemp(prefix='large-navigation-',dir=ROOT/'var'));print('Evidence:',work,flush=True)
    bundle=work/'TrueRenderer.app';shutil.copytree(args.bundle,bundle,symlinks=True)
    subprocess.run(['codesign','--verify','--deep','--strict',str(bundle)],check=True)
    library=work/'memory-probe.dylib'
    subprocess.run(['xcrun','clang','-dynamiclib','-O2','-Wall','-Wextra','-Werror',str(ROOT/'scripts/preview-memory-probe.c'),'-o',str(library)],check=True)
    probe=ctypes.CDLL(str(library)).tr_preview_usage
    probe.argtypes=[ctypes.c_char_p]+[ctypes.POINTER(ctypes.c_uint64)]*4
    report={'passed':False,'complete':False,'cases':[],'configuration':{k:str(v) if isinstance(v,Path) else v for k,v in vars(args).items()},'binary_sha256':{str(p.relative_to(bundle)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [bundle/'Contents/MacOS/TrueRenderer',bundle/'Contents/XPCServices/Decoder0.xpc/Contents/MacOS/Decoder',bundle/'Contents/XPCServices/Decoder1.xpc/Contents/MacOS/Decoder']},'scope':'Generated large PNG or explicitly authorized RAW. Renderer pressure is injected; no physical OS-pressure claim. Memory sums sampled host and XPC, not all driver allocations or shorter peaks. Endpoints compare the selected provider to CPU, not independent camera colour truth.'}
    destination=work/'report.json'
    def save():destination.write_text(json.dumps(report,indent=2)+'\n')
    save()
    try:
        sizes=['raw'] if args.raw_folder else args.sizes.split(',')
        for size in sizes:
            for engine in args.engines.split(','):
                for quality in args.qualities.split(','):
                    for mode in args.modes.split(','):
                        case_root=work/f'{size}-{engine}-{quality}-{mode}';case_root.mkdir();(case_root/'reports').mkdir()
                        folder=case_root/'inputs';folder.mkdir()
                        if args.raw_folder:
                            sources=sorted(p for p in args.raw_folder.iterdir() if p.suffix.lower() in ('.nef','.dng'))[:2]
                            assert len(sources)==2
                            for i,p in enumerate(sources):shutil.copyfile(p,folder/(str(i)+p.suffix.lower()))
                            targets=[p.name for p in sorted(folder.iterdir())]
                        else:
                            w,h={'12':(4000,3000),'24':(6000,4000),'45':(8256,5504)}[size]
                            targets=['04_Frequenze_radiali.png','02_Paesaggio_analitico.png']
                            for i,name in enumerate(targets):generate(folder/name,w,h,i)
                        hashes={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.iterdir()}
                        for phase in (['cold'] if args.pressure or args.raw_folder else ['cold','ssd']):
                            row={'size':size,'engine':engine,'quality':quality,'mode':mode,'phase':phase,'passed':False};report['cases'].append(row);save()
                            samples=[];stop=threading.Event()
                            def sample():
                                while not stop.is_set():
                                    rss,foot,count=(ctypes.c_uint64() for _ in range(3));details=(ctypes.c_uint64*(128*4))()
                                    status=probe(str(bundle).encode(),ctypes.byref(rss),ctypes.byref(foot),ctypes.byref(count),details)
                                    processes=[{'pid':details[i*4], 'rss_bytes':details[i*4+1],
                                                'footprint_bytes':details[i*4+2],
                                                'role':['host','decoder0','decoder1'][details[i*4+3]]}
                                               for i in range(min(count.value,128))]
                                    samples.append([time.monotonic(),rss.value,foot.value,count.value,status,processes]);stop.wait(.025)
                            thread=threading.Thread(target=sample);thread.start()
                            try:
                                result=nav.run_probe(bundle,case_root,mode,quality,phase,True,False,args.memory_mib,folder,targets,args.pressure,engine,True,True)
                                row['result']=result
                                row['first_access'] = 'decode' if result['samples'][0]['decode_jobs'] else 'cache-hit'
                                if args.pressure:
                                    assert result['pressure_wide_before']>0 and result['pressure_wide_after']==0,'Optional reserve not exercised and evicted'
                                    assert result['samples'][5]['presenter']['wide_frames']==0
                                if quality=='Standard':
                                    assert result['samples'][0]['base_level']>0,'Reduced representation not exercised'
                                assert result['samples'][8]['base_level']==0,'1:1 did not obtain level zero'
                                row['passed']=True
                            except Exception as error:
                                row['error']=str(error)
                                path=case_root/'reports/navigation-surface.json'
                                if path.exists():row['result']=json.loads(path.read_text())
                                raise
                            finally:
                                stop.set();thread.join()
                                active=[s for s in samples if s[3]>0]
                                row['memory']={'sample_count':len(active),'peak_rss':max((s[1] for s in active),default=0),'peak_footprint':max((s[2] for s in active),default=0),'maximum_processes':max((s[3] for s in active),default=0),'incomplete_samples':sum(s[4]!=0 for s in active),'maximum_gap_seconds':max((b[0]-a[0] for a,b in zip(samples,samples[1:])),default=0)}
                                for metric,index in [('rss',1),('footprint',2)]:
                                    peak=max(active,key=lambda s:s[index],default=None)
                                    row['memory']['processes_at_peak_'+metric]=peak[5] if peak else []
                                timeline=case_root/'reports'/f'{phase}-memory.json'
                                timeline.write_text(json.dumps({'columns':['monotonic_seconds','rss_bytes','footprint_bytes','process_count','status','processes'],'samples':samples})+'\n')
                                save()
                            assert row['memory']['sample_count'] > 0 and row['memory']['maximum_processes'] >= 2 and row['memory']['incomplete_samples'] == 0, 'Memory sampling incomplete'
                            row['memory']['within_requested_budget'] = max(row['memory']['peak_rss'], row['memory']['peak_footprint']) <= args.memory_mib * 1024**2
                            assert all(hashlib.sha256((folder/name).read_bytes()).hexdigest()==digest for name,digest in hashes.items())
                            row['inputs_unchanged']=True;save();print(size,engine,quality,mode,phase,'passed',flush=True)
        report.update(passed=all(c['passed'] for c in report['cases']),complete=True)
    finally:save();print('Report:',destination,flush=True)
if __name__=='__main__':main()
