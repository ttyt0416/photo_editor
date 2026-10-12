"""Independent JSON check of actual migrated workspace settings."""
from pathlib import Path
import json
root=Path(__file__).resolve().parents[2]
m=json.loads((root/'qa-output/menu-shortcuts/workspace109.json').read_text(encoding='utf-8'))
b=m['shortcuts']['bindings'];assert len(b)==109
assert len({json.dumps(i['action'],sort_keys=True) for i in b})==109
slots={i['action']['menu']:i for i in b if isinstance(i['action'],dict) and 'menu' in i['action']}
assert list(slots)==list(range(71))
assert slots[2]['key']=='F8' and slots[2]['alt'] and slots[16]['key']=='F10'
assert next(i for i in b if i['action']=='command_palette')['key']=='F9'
assert next(i for i in b if i['action']=={'tool':'brush'})['key']=='F6'
print('Independent workspace JSON: 109 unique commands, stable 71 menu IDs, historical custom keys and RAW/NewLayer custom shortcuts preserved')
