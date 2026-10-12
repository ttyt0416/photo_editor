"""Collect the exact Windows non-dev Cargo closure and available source notices, offline."""
from pathlib import Path
import hashlib
import json
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-pc-windows-msvc"


def collect(metadata):
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root_id = metadata["resolve"]["root"]
    seen, pending = set(), [root_id]
    while pending:
        ident = pending.pop()
        if ident in seen:
            continue
        seen.add(ident)
        pending.extend(d["pkg"] for d in nodes[ident]["deps"]
                       if any(k["kind"] != "dev" for k in d["dep_kinds"]))
    result = []
    for ident in sorted(seen - {root_id}, key=lambda i: (packages[i]["name"], packages[i]["version"])):
        package = packages[ident]
        folder = Path(package["manifest_path"]).parent
        candidates = set()
        for path in folder.rglob("*"):
            if (path.is_file() and not path.is_symlink()
                and path.name.lower().startswith(("license", "licence", "copying", "notice", "copyright", "ofl", "ufl"))
                and path.suffix.lower() in ("", ".txt", ".md", ".rst", ".html", ".license")):
                candidates.add(path)
        if package.get("license_file"):
            path = (folder / package["license_file"]).resolve()
            if not path.is_relative_to(folder.resolve()) or not path.is_file():
                raise ValueError(f"Missing or external license_file: {package['name']}")
            candidates.add(path)
        notices = []
        for path in sorted(candidates):
            raw = path.read_bytes()
            try:
                text, encoding = raw.decode("utf-8"), "utf-8"
            except UnicodeDecodeError:
                text, encoding = raw.decode("latin-1"), "latin-1"
            notices.append({"path": path.relative_to(folder).as_posix(), "encoding": encoding,
                            "sha256": hashlib.sha256(raw).hexdigest(), "text": text})
        try:
            source = folder.relative_to(ROOT).as_posix()
        except ValueError:
            source = package["source"]
        result.append({"name": package["name"], "version": package["version"],
                       "license": package.get("license") or "See license_file",
                       "repository": package.get("repository"), "source": source,
                       "notices": notices})
    return {"version": 1, "target": TARGET, "packages": result}


def main():
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--offline", "--locked", "--format-version", "1", "--filter-platform", TARGET],
        cwd=ROOT))
    result = collect(metadata)
    destination = ROOT / "licenses/windows-dependencies.json"
    destination.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    count = sum(len(p["notices"]) for p in result["packages"])
    missing = [p["name"] + " " + p["version"] for p in result["packages"] if not p["notices"]]
    print(f"{len(result['packages'])} Windows dependencies, {count} exact source notice files; {len(missing)} packages with declaration only")
    print("Declaration only: " + ", ".join(missing))


if __name__ == "__main__":
    main()
