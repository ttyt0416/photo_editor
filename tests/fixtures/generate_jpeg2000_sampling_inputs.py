"""Own raw planes, independent FFmpeg native JPEG2000 encoder, C decoded reference."""
from pathlib import Path
import json,os,subprocess
import numpy as np
from jpeg2000_c_reference import component_planes,version
OUT=Path(__file__).resolve().parent/'jpeg2000-sampling';OUT.mkdir(exist_ok=True)
FF='C:/ProgramData/chocolatey/bin/ffmpeg.exe'
def run(args):
    result=subprocess.run([FF,*args],stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=0x08000000 if os.name=='nt' else 0)
    assert result.returncode==0,result.stderr.decode(errors='replace');return result
cases=[]
for w,h in [(7,5),(64,48)]:
 for dy in [1,2]:
  for precision in [8,16]:
   for alpha in [False,True]:
    limit=(1<<precision)-1;shapes=[(h,w),((h+dy-1)//dy,(w+1)//2),((h+dy-1)//dy,(w+1)//2)]+([(h,w)] if alpha else [])
    source=[]
    for c,shape in enumerate(shapes):
        i=np.arange(np.prod(shape),dtype='i8').reshape(shape)
        values=(i*397+c*719+193)%(limit+1);values[i%11==0]=0;values[i%11==1]=limit
        source.append(values.astype('u1' if precision==8 else '<u2'))
    fmt=('yuva' if alpha else 'yuv')+('422p' if dy==1 else '420p')+('16le' if precision==16 else '')
    stem=f'{w}x{h}-{fmt}';raw=OUT/(stem+'.raw');raw.write_bytes(b''.join(p.tobytes() for p in source))
    for lossless in [False,True]:
        name=stem+f'-lossless{lossless}.jp2';path=OUT/name
        run(['-v','error','-y','-f','rawvideo','-pixel_format',fmt,'-video_size',f'{w}x{h}','-i',str(raw),'-frames:v','1','-c:v','jpeg2000','-threads','1','-pred','dwt53' if lossless else 'dwt97int','-prog','pcrl','-tile_width','16','-tile_height','16','-layer_rates','1',str(path)])
        planes=component_planes(path)
        if lossless:
            for actual,expected in zip(planes,source):np.testing.assert_array_equal(actual,expected,err_msg=name)
        cb=np.repeat(np.repeat(planes[1],dy,0),2,1)[:h,:w].astype('f8')-(1<<(precision-1))
        cr=np.repeat(np.repeat(planes[2],dy,0),2,1)[:h,:w].astype('f8')-(1<<(precision-1))
        y=planes[0].astype('f8')
        rgb=np.clip(y[:,:,None]+np.trunc(np.stack([1.402*cr,-(.344*cb+.714*cr),1.772*cb],2)),0,limit).astype('i8')
        a=np.clip(planes[3],0,limit) if alpha else np.full((h,w),limit)
        rgba=np.concatenate([rgb,a[:,:,None]],2).astype('f4')/np.float32(limit)
        reference=name+'.rgba32.le';(OUT/reference).write_bytes(rgba.astype('<f4').tobytes())
        cases.append(dict(file=name,reference=reference,size=[w,h],precision=precision,dy=dy,alpha=alpha,lossless=lossless))
(OUT/'manifest.json').write_text(json.dumps(cases,indent=2))
(OUT/'README.md').write_text(f'Own 7x5/64x48 raw Y/Cb/Cr/alpha planes, independently encoded by FFmpeg native JPEG2000: 4:2:2/4:2:0, 8/16-bit, RGB/alpha, reversible/irreversible, PCRL and16x16 tiles. C OpenJPEG {version} decoded component references plus independent NumPy chroma/color math. Reversible components exactly equal own source. No external media. Complete ISO/other origins/ICC/other color spaces remain separate.\n')
print(f'{len(cases)} independent FFmpeg native JPEG2000 sampling inputs/C component references')
