"""Convert the Windows setup wizard's brand art into the BMPs NSIS needs.

Usage:
  cd apps/desktop && node e2e/brand-images.mjs   (renders the art with the brand fonts)
  python scripts/installer-art.py                 (writes the BMPs)
Writes apps/desktop/src-tauri/windows/{sidebar,header}.bmp: NSIS wants
164x314 and 150x57 24-bit BMPs. The art follows docs/brand/DESIGN.md
(ink panel, the paw-microphone mark, the openbark wordmark).
"""
import os

from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "..")
ART = os.path.join(ROOT, "docs", "brand", "images")
OUT = os.path.join(ROOT, "apps", "desktop", "src-tauri", "windows")


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    for src, dst, size in (("installer-sidebar.png", "sidebar.bmp", (164, 314)), ("installer-header.png", "header.bmp", (150, 57))):
        img = Image.open(os.path.join(ART, src)).convert("RGB")
        assert img.size == size, (src, img.size)
        img.save(os.path.join(OUT, dst))
    print("wrote", OUT)


if __name__ == "__main__":
    main()
