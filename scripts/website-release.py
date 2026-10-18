"""Point the website's downloads at a published release.

    python scripts/website-release.py v0.1.0 [--repo ANI-ML/openbark-releases]

Reads the release's assets from the public releases repo (with the `gh` CLI,
which needs GH_TOKEN) and writes website/public/release.json: version, date,
and for each platform the files with their sizes. Run
`python scripts/website-build.py` afterwards to rebuild website/dist.
Refuses a release that's still a draft, so the site never links to files
visitors can't download.
"""
import json
import os
import subprocess
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
OUT = os.path.join(ROOT, "website", "public", "release.json")


def size(n):
    return f"{n / 1e6:.0f} MB" if n >= 1e6 else f"{n / 1e3:.0f} KB"


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    tag = sys.argv[1]
    repo = sys.argv[sys.argv.index("--repo") + 1] if "--repo" in sys.argv else "ANI-ML/openbark-releases"
    raw = subprocess.run(
        ["gh", "release", "view", tag, "--repo", repo, "--json", "tagName,isDraft,publishedAt,url,assets"],
        capture_output=True, text=True, check=True,
    ).stdout
    rel = json.loads(raw)
    if rel["isDraft"]:
        sys.exit(f"{tag} is still a draft in {repo}: publish it on GitHub first.")
    version = tag.lstrip("v")
    files = {"windows": [], "mac": [], "linux": []}
    for a in rel["assets"]:
        name, n = a["name"], a["size"]
        low = name.lower()
        if low.endswith("-setup.exe"):
            files["windows"].append({"label": "Windows setup (.exe)", "name": name, "size": size(n)})
        elif low.endswith(".dmg"):
            files["mac"].append({"label": "Disk image (.dmg)", "name": name, "size": size(n)})
        elif low.endswith(".deb"):
            files["linux"].append({"label": "Ubuntu, Debian (.deb)", "name": name, "size": size(n)})
        elif low.endswith(".rpm"):
            files["linux"].append({"label": "Fedora (.rpm)", "name": name, "size": size(n)})
        elif low.endswith(".appimage"):
            files["linux"].append({"label": "Any distribution (AppImage)", "name": name, "size": size(n)})
    previous = {}
    if os.path.exists(OUT):
        with open(OUT, encoding="utf-8") as f:
            previous = json.load(f)
    out = {
        "product": previous.get("product", "OpenBark"),
        "version": version,
        "published": True,
        "date": (rel.get("publishedAt") or "")[:10] or None,
        "download_base": f"https://github.com/{repo}/releases/download/{tag}",
        "all": rel["url"],
        "notes": rel["url"],
        "app_store": previous.get("app_store"),
        "files": files,
    }
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        json.dump(out, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"release.json: {version}, " + ", ".join(f"{k} {len(v)}" for k, v in files.items()))


if __name__ == "__main__":
    main()
