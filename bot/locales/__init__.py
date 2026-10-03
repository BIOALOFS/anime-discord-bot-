import json
from pathlib import Path
_ROOT = Path(__file__).parent
_DATA = {lang: json.loads((_ROOT / f"{lang}.json").read_text(encoding="utf8")) for lang in ("ar", "en")}
def t(language: str, key: str, **values): return _DATA.get(language, _DATA["ar"]).get(key, key).format(**values)
def value(language: str,key: str):return _DATA.get(language,_DATA["ar"]).get(key,_DATA["ar"].get(key))
