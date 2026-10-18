"""Build the OpenBark website into website/dist (static, served by Vercel).

    python scripts/website-build.py [--preview DIR]

Pages come from website/src (index.html is the landing page's body, site.css,
site.js) and website/content/features.json (the features page, generated
here). website/public is copied as is (release.json, written by the release
workflow, and the icon). The product name lives in PRODUCT below, so a rename
is one line.

--preview DIR also writes a copy for a claude.ai Artifact preview, where the
landing page is body-only (the viewer adds the document wrapper).
"""
import html
import io
import json
import os
import shutil
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
SITE = os.path.join(ROOT, "website")
SRC = os.path.join(SITE, "src")
DIST = os.path.join(SITE, "dist")

PRODUCT = "OpenBark"
TAGLINE = "Open-source veterinary scribe"
CREDIT = "Created by Dr. Christopher Pinard and ANI.ML Health Inc."

FONTS = (
    '<link rel="preconnect" href="https://fonts.googleapis.com">'
    '<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>'
    '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Geist:wght@400..700&family=Geist+Mono:wght@400;500&display=swap">'
)

LOGO = (
    '<svg viewBox="0 0 1024 1024" aria-hidden="true"><rect width="1024" height="1024" rx="224" fill="#F47C20"/>'
    '<g fill="#ffffff"><ellipse cx="512" cy="640" rx="190" ry="160"/>'
    '<ellipse cx="300" cy="430" rx="78" ry="100" transform="rotate(-18 300 430)"/>'
    '<ellipse cx="430" cy="300" rx="80" ry="108" transform="rotate(-6 430 300)"/>'
    '<ellipse cx="594" cy="300" rx="80" ry="108" transform="rotate(6 594 300)"/>'
    '<ellipse cx="724" cy="430" rx="78" ry="100" transform="rotate(18 724 430)"/></g></svg>'
)

e = html.escape


def read(*parts):
    with io.open(os.path.join(*parts), encoding="utf-8") as f:
        return f.read()


def head(title, description, page):
    return (
        f"<title>{e(title)}</title>\n"
        '<meta name="claude:viewport-fit-sides" content="cover">\n'
        f'<meta name="description" content="{e(description)}">\n'
        f'<meta property="og:title" content="{e(title)}">\n'
        f'<meta property="og:description" content="{e(description)}">\n'
        '<meta name="theme-color" content="#F47C20">\n'
        # Marks the page as scripted before first paint, so reveal-on-scroll
        # never hides content when script is off.
        '<script>document.documentElement.classList.add("js")</script>\n'
        '<link rel="icon" href="icon.svg" type="image/svg+xml">\n'
        f"{FONTS}\n"
        '<link rel="stylesheet" href="site.css">\n'
    )


def header(page):
    cur = lambda p: ' aria-current="page"' if p == page else ""
    return f"""<a class="skip" href="#main">Skip to content</a>
<header class="site-head">
  <div class="wrap">
    <a class="brand" href="index.html">{LOGO}<span>{e(PRODUCT)}</span><span class="stage">Pre-alpha</span></a>
    <nav class="site-nav" aria-label="Site">
      <a href="features.html"{cur("features")}>Features</a>
      <a href="index.html#download">Download</a>
      <a class="btn btn-primary" href="index.html#download">Get {e(PRODUCT)}</a>
    </nav>
  </div>
</header>
"""


FOOTER = f"""<footer class="site-foot">
  <div class="wrap">
    <p class="foot-name">{LOGO}<span>{e(PRODUCT)}</span></p>
    <div class="foot-row">
      <p>{e(PRODUCT)}, {e(TAGLINE.lower())}. {e(CREDIT)}<br>
      Drafts are not medical records until a veterinarian reviews and signs them.</p>
      <nav aria-label="Footer">
        <a href="features.html">Features</a>
        <a href="index.html#download">Download</a>
        <a href="features.html#privacy-and-security">Privacy</a>
        <a href="https://www.apache.org/licenses/LICENSE-2.0">Licence: Apache-2.0</a>
      </nav>
    </div>
  </div>
</footer>
"""


LOST = f"""<section class="lost">
  <div class="wrap">
    <h1>This page wandered off.</h1>
    <p>The address may be mistyped, or the page moved when the site changed. Everything {e(PRODUCT)} does is on the features page, and the installers are on the home page.</p>
    <div class="hero-actions">
      <a class="btn btn-primary" href="index.html">Go to the home page</a>
      <a class="link-more" href="features.html">See every feature</a>
    </div>
  </div>
</section>
"""


def stat_tiles(stats, limit=8):
    out = []
    for s in stats[:limit]:
        out.append(
            f'<div class="stat"><span class="stat-value">{e(s["value"])}</span>'
            f'<span class="stat-label">{e(s["label"])}</span>'
            f'<span class="stat-source">{e(s.get("source", ""))}</span></div>'
        )
    return "".join(out)


def slug(text):
    return "".join(c.lower() if c.isalnum() else "-" for c in text).strip("-").replace("--", "-")


def features_body(content):
    cats = content["categories"]
    total = sum(len(c["items"]) for c in cats)
    toc = "".join(
        f'<a href="#{e(c.get("id") or slug(c["name"]))}"><span>{e(c["name"])}</span><span class="count">{len(c["items"])}</span></a>'
        for c in cats
    )
    sections = []
    for c in cats:
        cid = c.get("id") or slug(c["name"])
        items = []
        for it in c["items"]:
            status = it.get("status", "shipped")
            badge = "" if status == "shipped" else f'<span class="status" data-s="{e(status)}">{"Being tested" if status == "testing" else "Coming"}</span>'
            items.append(
                f'<li class="fx-item" data-text="{e((it["name"] + " " + it["benefit"] + " " + it.get("detail", "")).lower())}">'
                f'<h3>{e(it["name"])}{badge}</h3>'
                f'<p class="benefit">{e(it["benefit"])}</p>'
                f'<p class="detail">{e(it.get("detail", ""))}</p>'
                f'<p class="evidence">{e(it.get("evidence", ""))}</p></li>'
            )
        sections.append(
            f'<section class="fx-cat" id="{e(cid)}"><h2>{e(c["name"])}</h2><p>{e(c.get("intro", ""))}</p>'
            f'<ul class="fx-items">{"".join(items)}</ul></section>'
        )
    bt = content.get("built_together", {})
    timeline = "".join(
        f'<li><span class="when">{e(m["when"])}</span><span>{e(m["what"])}</span></li>' for m in bt.get("milestones", [])
    )
    practices = "".join(f"<li>{e(p)}</li>" for p in bt.get("practices", []))
    together = f"""<section class="band band-gray" id="built-together">
  <div class="wrap">
    <div class="band-head">
      <h2>Created by Dr. Christopher Pinard and ANI.ML Health Inc.</h2>
      <p>{e(bt.get("intro", ""))}</p>
    </div>
    <div class="stats stats-4">{stat_tiles(bt.get("stats", []), 12)}</div>
    <div class="together">
      <div><h3>Milestones</h3><ol class="timeline">{timeline}</ol></div>
      <div><h3>How the work was done</h3><ul class="practices">{practices}</ul></div>
    </div>
  </div>
</section>"""
    return f"""<section class="fx-hero">
  <div class="wrap">
    <h1>{total} things {e(PRODUCT)} does, and how we know.</h1>
    <p>Each feature lists what it does for your clinic and where it lives in the code or the evaluation, so nothing here is a promise we can't point to. Items marked "Being tested" or "Coming" are built or planned but not yet proven on real devices.</p>
  </div>
</section>
<section class="fx-stats">
  <div class="wrap"><div class="stats stats-4">{stat_tiles(content.get("headline_stats", []))}</div></div>
</section>
<div class="wrap fx-layout" id="fx">
  <nav class="fx-toc" aria-label="Feature categories">
    <div class="fx-filter"><label class="faint" for="fx-q">Find a feature</label><input id="fx-q" type="search" placeholder="Dental, encryption, phone..."></div>
    {toc}
  </nav>
  <div>{"".join(sections)}</div>
</div>
{together}
<script>
(function () {{
  var q = document.getElementById("fx-q");
  if (!q) return;
  q.addEventListener("input", function () {{
    var v = q.value.trim().toLowerCase();
    document.querySelectorAll(".fx-item").forEach(function (li) {{
      li.hidden = v && li.getAttribute("data-text").indexOf(v) < 0;
    }});
    document.querySelectorAll(".fx-cat").forEach(function (s) {{
      s.hidden = v && !s.querySelector(".fx-item:not([hidden])");
    }});
  }});
}})();
</script>
"""


def document(inner_head, body):
    return (
        '<!doctype html>\n<html lang="en">\n<head>\n<meta charset="utf-8">\n'
        '<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">\n'
        f"{inner_head}</head>\n<body>\n{body}</body>\n</html>\n"
    )


def main():
    preview = None
    if "--preview" in sys.argv:
        preview = sys.argv[sys.argv.index("--preview") + 1]
    content = json.loads(read(SITE, "content", "features.json"))
    templates = len([d for d in os.listdir(os.path.join(ROOT, "packages", "templates")) if os.path.isdir(os.path.join(ROOT, "packages", "templates", d))])

    landing = (
        read(SRC, "index.html")
        .replace("{{PRODUCT}}", e(PRODUCT))
        .replace("{{TAGLINE}}", e(TAGLINE.lower()))
        .replace("{{TEMPLATES}}", str(templates))
        .replace("{{STATS}}", stat_tiles(content.get("headline_stats", [])))
    )
    landing_title = f"{PRODUCT}"
    landing_desc = f"{PRODUCT} records the visit, transcribes it and drafts the medical record on your clinic's own computer, with open models. {CREDIT}"
    landing_head = head(landing_title, landing_desc, "home")
    landing_body = header("home") + '<main id="main">\n' + landing + "</main>\n" + FOOTER + '<script src="site.js"></script>\n'

    features_title = f"{PRODUCT} Features"
    features_desc = f"Every feature of {PRODUCT}, the open-source veterinary scribe, with where it lives in the code. {CREDIT}"
    features_doc = document(head(features_title, features_desc, "features"), header("features") + '<main id="main">\n' + features_body(content) + "</main>\n" + FOOTER)
    lost_doc = document(
        head(f"Page not found, {PRODUCT}", f"This page isn't part of the {PRODUCT} website.", "lost"),
        header("lost") + '<main id="main">\n' + LOST + "</main>\n" + FOOTER,
    )

    if os.path.isdir(DIST):
        shutil.rmtree(DIST)
    os.makedirs(DIST)
    io.open(os.path.join(DIST, "index.html"), "w", encoding="utf-8", newline="\n").write(document(landing_head, landing_body))
    io.open(os.path.join(DIST, "features.html"), "w", encoding="utf-8", newline="\n").write(features_doc)
    io.open(os.path.join(DIST, "404.html"), "w", encoding="utf-8", newline="\n").write(lost_doc)
    for name in ("site.css", "site.js"):
        shutil.copy(os.path.join(SRC, name), os.path.join(DIST, name))
    pub = os.path.join(SITE, "public")
    for name in os.listdir(pub):
        shutil.copy(os.path.join(pub, name), os.path.join(DIST, name))
    print(f"wrote {DIST}: index.html, features.html ({sum(len(c['items']) for c in content['categories'])} features)")

    if preview:
        os.makedirs(preview, exist_ok=True)
        # Body-only landing page: the Artifact viewer adds the document wrapper.
        io.open(os.path.join(preview, "index.html"), "w", encoding="utf-8", newline="\n").write(landing_head + landing_body)
        for name in os.listdir(DIST):
            if name != "index.html":
                shutil.copy(os.path.join(DIST, name), os.path.join(preview, name))
        print(f"wrote preview {preview}")


if __name__ == "__main__":
    main()
