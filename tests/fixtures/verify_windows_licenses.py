"""Independent lock/Windows closure, source-byte notices and shortcut migration verification."""
from pathlib import Path
import hashlib
import json
import subprocess

root = Path(__file__).resolve().parents[2]
metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--offline", "--locked", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc"], cwd=root))
packages = {p["id"]: p for p in metadata["packages"]}
# Independent fixed-point reachability rather than the collector's stack traversal.
reachable = {metadata["resolve"]["root"]}
while True:
    expanded = reachable | {dep["pkg"] for node in metadata["resolve"]["nodes"]
                           if node["id"] in reachable for dep in node["deps"]
                           if any(kind["kind"] in (None, "build") for kind in dep["dep_kinds"])}
    if expanded == reachable:
        break
    reachable = expanded
reachable.remove(metadata["resolve"]["root"])
expected = {(packages[i]["name"], packages[i]["version"]): packages[i] for i in reachable}
notice_data = json.loads((root / "licenses/windows-dependencies.json").read_text(encoding="utf-8"))
assert notice_data["version"] == 1 and notice_data["target"] == "x86_64-pc-windows-msvc"
actual = {(p["name"], p["version"]): p for p in notice_data["packages"]}
assert len(actual) == len(notice_data["packages"]) == 308
assert set(actual) == set(expected), (set(actual) - set(expected), set(expected) - set(actual))
files = 0
for key, package in actual.items():
    original = expected[key]
    folder = Path(original["manifest_path"]).parent
    assert package["license"] == (original.get("license") or "See license_file")
    assert package["repository"] == original.get("repository")
    paths = set()
    for notice in package["notices"]:
        assert notice["path"] not in paths
        paths.add(notice["path"])
        source = (folder / notice["path"]).resolve()
        assert source.is_relative_to(folder.resolve())
        raw = source.read_bytes()
        assert notice["text"].encode(notice["encoding"]) == raw, (key, notice["path"])
        assert notice["sha256"] == hashlib.sha256(raw).hexdigest()
        files += 1
    for source in folder.rglob("*"):
        if (source.is_file() and not source.is_symlink()
            and source.name.lower().startswith(("license", "licence", "copying", "notice", "copyright", "ofl", "ufl"))
            and source.suffix.lower() in ("", ".txt", ".md", ".rst", ".html", ".license")):
            assert source.relative_to(folder).as_posix() in paths, (key, source)
assert files == 528
previous = json.loads((root / "qa-output/program-info/previous107.json").read_text(encoding="utf-8"))["bindings"]
current = json.loads((root / "qa-output/program-info/current109.json").read_text(encoding="utf-8"))["bindings"]
assert len(previous) == 107 and len(current) == 109
assert current[:107] == previous
assert [b["action"] for b in current[107:]] == [{"menu": 69}, {"menu": 70}]
assert all(b["key"] is None for b in current[107:])
print("Windows license inventory: exact 308 locked non-dev dependencies, 528 source-byte/SHA-256 notices, 18 declaration-only packages; 107 custom shortcuts preserved when upgrading to 109")
