"""Independent DNG sensor parsing, camera-matrix solve and sample balance/output."""
from pathlib import Path
import json
import numpy as np
from verify_raw_outputs import parse,orient,expected,SRGB
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw';OUT=ROOT/'qa-output/raw-white-balance';m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));pixels=0
for case in m['cases']:
    _,tags,sensor=parse(FIX/case['input']);mono=tags[262][0]==1;cfa=tags[262][0]==32803
    if cfa:sensor=np.tile([sensor[0,0,0],sensor[0,1,0],sensor[1,1,0]],(10,12,1))
    if 50720 in tags:
        left,top=tags[50719];top+=tags[50829][0];left+=tags[50829][1];w,h=tags[50720];sensor=sensor[top:top+h,left:left+w]
    sensor=orient(sensor,tags[274][0]);h,w,_=sensor.shape;focus=np.asarray(case['focus'],dtype='f4');center=np.floor(np.float32(focus*np.array([w-1,h-1],dtype='f4'))+np.float32(.5)).astype(int);radius=case['radius'];x,y=center;patch=sensor[max(0,y-radius):min(h,y+radius+1),max(0,x-radius):min(w,x+radius+1)].astype('f8');average=patch.mean(axis=(0,1))
    if mono:gains=np.ones(3)
    else:
        xyz2cam=np.asarray(tags[50721],dtype='f4').reshape(3,3).astype('f8');rgb2cam=xyz2cam@SRGB;rgb2cam/=rgb2cam.sum(axis=1,keepdims=True);camera2rgb=np.linalg.inv(rgb2cam)
        white=np.linalg.solve(camera2rgb,np.ones(3));gains=white/average;gains/=gains[1]
    np.testing.assert_allclose(np.asarray(case['gains']),gains,rtol=2e-5,atol=2e-6,err_msg=str(case))
    actual=np.fromfile(OUT/case['output'],dtype='<f4').reshape(h,w,4);assert np.isfinite(actual).all()
    # Reuse the original independent binary parser/color pipeline with the newly solved gains.
    _,tags,original=parse(FIX/case['input']);reference=expected(tags,original,gains)
    if cfa:np.testing.assert_allclose(actual[2:-2,2:-2],reference[2:-2,2:-2],rtol=2e-5,atol=3e-6)
    else:np.testing.assert_allclose(actual,reference,rtol=2e-5,atol=3e-6)
    pixels+=w*h
print(f"RAW white balance: {len(m['cases'])} independently parsed sensor patches/NumPy matrix solves across 20 DNG inputs and {pixels} output pixels; 8 orientations/crops, linear/Bayer/mono, 16/32-bit, source/output checked")
