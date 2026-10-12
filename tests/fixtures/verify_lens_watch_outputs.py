"""Independent XML/profile comparison for background catalog replacement."""
from pathlib import Path
import json,xml.etree.ElementTree as ET
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/lens-library/watch'
before=json.loads((OUT/'before.json').read_text());after=json.loads((OUT/'changed.json').read_text())
assert len(before)==len(after)==13
changed=[]
for i,(old,new) in enumerate(zip(before,after)):
    assert old['name']==new['name'] and old['model']==new['model'] and old['focal']==new['focal']
    for p in [old,new]:
        lens=ET.fromstring(p['xml']);assert lens.tag=='lens' and lens.findtext('model')==p['name']
        match=[n for n in lens.iter('distortion') if n.get('model')==p['model'] and float(n.get('focal'))==p['focal']]
        if match:
            keys={'poly3':['k1'],'poly5':['k1','k2'],'ptlens':['a','b','c'],'acm':['k1','k2','k3','k4','k5']}[p['model']]
            values=[float(match[0].get(k,'0')) for k in keys]+[0]*(5-len(keys))
            assert p['coefficients']==values
    a={k:v for k,v in old.items() if k!='xml'};b={k:v for k,v in new.items() if k!='xml'}
    if a!=b:
        changed.append(i);assert old['coefficients'][0]==.02 and new['coefficients'][0]==.08
        a['coefficients']=b['coefficients'];assert a==b
assert changed==[0]
assert json.loads((OUT/'settings.json').read_text())['paths']==[]
settings=json.loads((ROOT/'qa-output/lens-library/watch-ui/settings.json').read_text());assert settings['watch'] is False and settings['paths']==[]
print('Lensfun file watch: 13 source-preserving profiles independently parsed; one intended coefficient change, remaining camera/model/geometry/calibration settings retained, repair/clear and preference outputs verified')
