#!/usr/bin/env python3
"""Register private linear Rec.2020 patches; differences are not camera accuracy."""
import argparse,array,json,math,sys
from pathlib import Path

def luminance(rgb):
    return [0.2627*rgb[i]+0.6780*rgb[i+1]+0.0593*rgb[i+2] for i in range(0,len(rgb),3)]

def register(a,b,w,h,radius=16,stride=8):
    points=[y*w+x for y in range(radius+2,h-radius-2,stride) for x in range(radius+2,w-radius-2,stride)]
    av=[a[i+1]-a[i-1] for i in points]+[a[i+w]-a[i-w] for i in points]
    an=sum(v*v for v in av)
    if an<1e-12:raise ValueError('Reference patch has insufficient gradient energy')
    scores=[]
    for dy in range(-radius,radius+1):
        for dx in range(-radius,radius+1):
            offset=dy*w+dx
            bv=[b[i+offset+1]-b[i+offset-1] for i in points]+[b[i+offset+w]-b[i+offset-w] for i in points]
            bn=sum(v*v for v in bv)
            score=sum(x*y for x,y in zip(av,bv))/math.sqrt(an*bn) if bn>1e-12 else -1.
            scores.append((score,dx,dy))
    scores.sort(reverse=True);best=scores[0]
    return {'dx':best[1],'dy':best[2],'gradient_cosine':best[0],'runner_up_cosine':scores[1][0], 'at_search_boundary':max(abs(best[1]),abs(best[2]))==radius,'search_radius':radius,'sampling_stride':stride}

def metrics(a,b,w,h,dx,dy):
    margin=20;count=0;absolute=[0.,0.,0.];squares=[0.,0.,0.];means_a=[0.,0.,0.];means_b=[0.,0.,0.];below=[0,0];above=[0,0]
    for y in range(margin,h-margin):
        for x in range(margin,w-margin):
            ia=(y*w+x)*3;ib=((y+dy)*w+x+dx)*3;count+=1
            for c in range(3):
                aa=a[ia+c];bb=b[ib+c];diff=bb-aa
                absolute[c]+=abs(diff);squares[c]+=diff*diff;means_a[c]+=aa;means_b[c]+=bb
                below[0]+=aa<0;below[1]+=bb<0;above[0]+=aa>1;above[1]+=bb>1
    return {'pixels':count,'linear_rgb_mae':[v/count for v in absolute],'linear_rgb_rmse':[math.sqrt(v/count) for v in squares], 'reference_rgb_mean':[v/count for v in means_a],'candidate_rgb_mean':[v/count for v in means_b],'channels_below_zero':below,'channels_above_one':above}

def self_test():
    w=h=96;dx,dy=3,-2
    def signal(x,y):return math.sin(x*.19+y*.03)+math.cos(y*.27-x*.09)+.2*math.sin(x*y*.015)
    a=[signal(x,y) for y in range(h) for x in range(w)]
    b=[2*signal(x-dx,y-dy)+.4 for y in range(h) for x in range(w)]
    r=register(a,b,w,h,6,4);assert (r['dx'],r['dy'])==(dx,dy) and r['gradient_cosine']>1-1e-10
    try:register([0.]*(w*h),b,w,h,6,4)
    except ValueError:pass
    else:raise AssertionError('Constant reference accepted')
    print('Registration: known translation/gain/offset and flat-patch rejection passed')

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('folder',type=Path,nargs='?');p.add_argument('--self-test',action='store_true');args=p.parse_args()
    if args.self_test:self_test();return
    folder=args.folder;rows=[]
    for ref in sorted(folder.glob('aligned-*-Apple.json')):
        index=ref.stem.split('-')[1];metadata=json.loads(ref.read_text());w,h=metadata['size']
        def read(path):
            data=array.array('f');data.frombytes(path.read_bytes())
            if sys.byteorder!='little':data.byteswap()
            assert len(data)==w*h*3 and all(math.isfinite(v) for v in data)
            return data
        a=read(ref.with_suffix('.f32'));la=luminance(a)
        for engine in ['LibRawBilinear','LibRawAhd','TrueRenderer']:
            path=folder/f'aligned-{index}-{engine}.json';other=json.loads(path.read_text());assert other['source_sha256']==metadata['source_sha256'] and other['size']==[w,h]
            b=read(path.with_suffix('.f32'));alignment=register(la,luminance(b),w,h)
            rows.append({'source_index':int(index),'reference':'Apple','candidate':engine,'reference_geometry':metadata,'candidate_geometry':other,'registration':alignment,'metrics':metrics(a,b,w,h,alignment['dx'],alignment['dy']),'alignment_supported':alignment['gradient_cosine']>=.8 and not alignment['at_search_boundary']})
    assert rows,'No private linear patches'
    result={'complete':True,'comparisons':rows,'scope':'Integer translation search of centered private linear Rec.2020 patches, gradient cosine insensitive to a common positive gain. No scaling, lens-warp or subpixel correction; weak/boundary matches remain unqualified. Differences include exposure, WB, active-area and pipeline choices, not just demosaic. No calibrated scene truth or DeltaE00 accuracy claim.'}
    (folder/'aligned-comparison.json').write_text(json.dumps(result,indent=2)+'\n');print('Comparisons:',len(rows),'supported alignments:',sum(r['alignment_supported'] for r in rows))
if __name__=='__main__':main()
