"""Draw the Windows setup wizard's images from the app icon (DESIGN.md colours).

Usage: python scripts/installer-art.py
Writes apps/desktop/src-tauri/windows/{sidebar,header}.bmp: NSIS wants
164x314 and 150x57 24-bit BMPs. Re-run after changing the icon.
"""
import os

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
TAURI = os.path.join(HERE, "..", "apps", "desktop", "src-tauri")
OUT = os.path.join(TAURI, "windows")
TEAL = (47, 111, 106)  # --accent, Scrub Teal
PAPER = (255, 255, 255)
INK_2 = (99, 99, 105)


def font(size: int, bold: bool = False) -> ImageFont.ImageFont:
    names = ["segoeuib.ttf" if bold else "segoeui.ttf", "arialbd.ttf" if bold else "arial.ttf"]
    for name in names:
        path = os.path.join(os.environ.get("WINDIR", r"C:\Windows"), "Fonts", name)
        if os.path.exists(path):
            return ImageFont.truetype(path, size)
    return ImageFont.load_default()


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    icon = Image.open(os.path.join(TAURI, "icons", "icon.png")).convert("RGBA")

    # Sidebar (welcome and finish pages): teal, the mark, the name.
    side = Image.new("RGB", (164, 314), TEAL)
    mark = icon.resize((76, 76), Image.LANCZOS)
    side.paste(mark, (44, 64), mark)
    d = ImageDraw.Draw(side)
    title = font(22, bold=True)
    w = d.textlength("OpenBark", font=title)
    d.text(((164 - w) / 2, 156), "OpenBark", font=title, fill=PAPER)
    sub = font(12)
    for i, line in enumerate(["Veterinary scribe", "that stays on", "this computer"]):
        lw = d.textlength(line, font=sub)
        d.text(((164 - lw) / 2, 194 + i * 17), line, font=sub, fill=(214, 233, 230))
    side.save(os.path.join(OUT, "sidebar.bmp"))

    # Header (inner pages): white, the mark at the right.
    head = Image.new("RGB", (150, 57), PAPER)
    small = icon.resize((40, 40), Image.LANCZOS)
    head.paste(small, (150 - 40 - 8, 8), small)
    head.save(os.path.join(OUT, "header.bmp"))
    print("wrote", OUT)


if __name__ == "__main__":
    main()
