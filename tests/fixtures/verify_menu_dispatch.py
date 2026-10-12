"""Independent exported menu/action/shortcut/current-state inspection."""
from pathlib import Path
import json
root=Path(__file__).resolve().parents[2]
entries=json.loads((root/'qa-output/menu-dispatch/catalog.json').read_text(encoding='utf-8'))
assert len(entries)==89
assert {e['category'] for e in entries}=={'파일','편집','이미지','레이어','선택','필터','보기','창','도움말'}
assert len({json.dumps(e['action'],sort_keys=True) for e in entries})==89
slots={e['action']['menu']:e for e in entries if isinstance(e['action'],dict) and 'menu' in e['action']}
assert set(slots)==set(range(71))
assert slots[16]['label']=='새 레이어' and slots[16]['shortcut']=='F10'
assert slots[69]['label']=='프로그램 정보…' and slots[70]['label']=='오픈소스 라이선스…'
assert slots[19]['disabled'] and slots[27]['checked'] is True
assert next(e for e in entries if e['action']=='command_palette')['shortcut']=='Ctrl+F'
assert next(e for e in entries if e['action']=='redo')['shortcut']=='Ctrl+Shift+Z / Ctrl+Y'
assert next(e for e in entries if e['action']=='transform')['disabled']
print('Menu dispatch: nine categories/89 unique actions/71 stable IDs, current custom keys/alternate redo/checked grid/locked delete and transform')
