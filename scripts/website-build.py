"""Build the OpenBark website into website/dist (static, served by Vercel).

    python scripts/website-build.py [--preview DIR]

Pages come from website/src (index.html is the landing page's body,
portal.html the account portal's body, site.css, site.js, portal.js) and
website/content/features.json (the features page, generated here). website/public is copied as is (release.json, written by the release
workflow, the favicon and the app icon). How it looks follows the brand guide,
docs/brand/DESIGN.md. The product name lives in PRODUCT below, so a rename
is one line.

--preview DIR also writes a copy for an online preview, where the
landing page is body-only (the viewer adds the document wrapper).
"""
import html
import io
import json
import os
import re
import shutil
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
SITE = os.path.join(ROOT, "website")
SRC = os.path.join(SITE, "src")
DIST = os.path.join(SITE, "dist")

PRODUCT = "OpenBark"
# The source repository. Until ANI-ML/openbark is public, leave this as None:
# the site then calls OpenBark free (not open source), says the code isn't
# public yet, and shows no Source code links. Once the repository is public,
# set it to "https://github.com/ANI-ML/openbark", rebuild, and re-render
# website/public/og-image.png (SOURCE_PUBLIC=1 node e2e/brand-images.mjs --og).
SOURCE_URL = None
OPEN = "free, open-source" if SOURCE_URL else "free"
TAGLINE = "Open-source veterinary scribe" if SOURCE_URL else "Free veterinary scribe"
CREDIT = "Built by the ANI.ML Health team."
CONTACT = "we.care@animl.health"
COFFEE = "https://buymeacoffee.com/animlhealth"
# The public address: openbark.io redirects to www. Pages are served with
# clean URLs (vercel.json), so features.html is canonically /features.
SITE_URL = "https://www.openbark.io"
OG_IMAGE = SITE_URL + "/og-image.png"  # website/public/og-image.png, from docs/brand/images
PAGES = {"home": "/", "features": "/features", "history": "/history", "portal": "/portal"}

# The account portal (ADR-0017) signs in with Supabase Auth. supabase-js comes
# from jsDelivr, pinned to one version and checked with Subresource Integrity:
# the hash is the sha384 of that exact file (byte-identical to the npm
# tarball's dist/umd/supabase.js). To upgrade, change both lines and the
# script-src entry in website/vercel.json and the root vercel.json (the
# website e2e checks that they agree). Only the portal page loads it.
SUPABASE_JS = "https://cdn.jsdelivr.net/npm/@supabase/supabase-js@2.118.0/dist/umd/supabase.js"
SUPABASE_JS_SRI = "sha384-TcHRA3zBbn+Lexh5n+Oil4guNPOS3rjNDpGRAN46u/s/mB2IZ50ceC6TYDU2yvgK"
# Where supabase-js keeps the portal session in localStorage (sb-<project ref>-auth-token).
SUPABASE_SESSION_KEY = "sb-nmecovtmijnfnjuadicp-auth-token"

# Lucide circle-user-round, for the account link.
ACCOUNT_ICON = (
    '<svg class="lucide" viewBox="0 0 24 24" aria-hidden="true">'
    '<path d="M18 20a6 6 0 0 0-12 0"/><circle cx="12" cy="10" r="4"/><circle cx="12" cy="12" r="10"/></svg>'
)

FONTS = (
    '<link rel="preconnect" href="https://fonts.googleapis.com">'
    '<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>'
    '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Manrope:wght@400;500;600;800&family=JetBrains+Mono:wght@400;500&display=swap">'
)

# The horizontal lockup from docs/brand/logos/openbark-lockup-horizontal.svg,
# inline so it takes the page's colours (white ink in dark mode) and its
# wordmark renders as live Manrope text. The capsule is the only accent.
LOCKUP = (
    '<svg class="lockup" viewBox="0 0 372 80" role="img" aria-label="{label}">'
    '<g transform="translate(4 4) scale(0.72)" fill="currentColor">'
    '<circle cx="30" cy="20" r="7.5"/><circle cx="50" cy="12" r="7.5"/><circle cx="70" cy="20" r="7.5"/>'
    '<rect class="capsule" x="35" y="28" width="30" height="42" rx="15"/>'
    '<rect class="grille" x="42" y="44" width="16" height="4" rx="2"/>'
    '<rect class="grille" x="42" y="52" width="16" height="4" rx="2"/>'
    '<path d="M24 54 A26 26 0 0 0 76 54" fill="none" stroke="currentColor" stroke-width="7" stroke-linecap="round"/>'
    '<rect x="46.5" y="78" width="7" height="10" rx="3.5"/><rect x="35" y="87" width="30" height="7" rx="3.5"/></g>'
    '<text x="92" y="60" font-family="Manrope, system-ui, -apple-system, \'Segoe UI\', sans-serif" font-size="60" letter-spacing="-1.8">'
    '<tspan font-weight="500">open</tspan><tspan font-weight="800">bark</tspan></text></svg>'
)

# The mono mark (docs/brand/logos/openbark-mark-mono.svg): currentColor,
# grille cut out. Used at 48 px for the empty state.
MARK_MONO = (
    '<svg class="lost-mark" viewBox="0 0 100 100" aria-hidden="true">'
    '<defs><mask id="ob-grille"><rect width="100" height="100" fill="#fff"/>'
    '<rect x="42" y="44" width="16" height="4" rx="2" fill="#000"/><rect x="42" y="52" width="16" height="4" rx="2" fill="#000"/></mask></defs>'
    '<g fill="currentColor"><circle cx="30" cy="20" r="7.5"/><circle cx="50" cy="12" r="7.5"/><circle cx="70" cy="20" r="7.5"/>'
    '<rect x="35" y="28" width="30" height="42" rx="15" mask="url(#ob-grille)"/>'
    '<rect x="46.5" y="78" width="7" height="10" rx="3.5"/><rect x="35" y="87" width="30" height="7" rx="3.5"/></g>'
    '<path d="M24 54 A26 26 0 0 0 76 54" fill="none" stroke="currentColor" stroke-width="7" stroke-linecap="round"/></svg>'
)

e = html.escape


def read(*parts):
    with io.open(os.path.join(*parts), encoding="utf-8") as f:
        return f.read()


def head(title, description, page):
    return (
        f"<title>{e(title)}</title>\n"
        f'<meta name="description" content="{e(description)}">\n'
        f'<meta property="og:title" content="{e(title)}">\n'
        f'<meta property="og:description" content="{e(description)}">\n'
        '<meta property="og:type" content="website">\n'
        f'<meta property="og:site_name" content="{e(PRODUCT)}">\n'
        + (
            f'<link rel="canonical" href="{SITE_URL}{PAGES[page]}">\n'
            f'<meta property="og:url" content="{SITE_URL}{PAGES[page]}">\n'
            if page in PAGES else ""
        )
        + f'<meta property="og:image" content="{OG_IMAGE}">\n'
        '<meta property="og:image:width" content="1200">\n'
        '<meta property="og:image:height" content="630">\n'
        f'<meta property="og:image:alt" content="{e(PRODUCT)}, the {OPEN} AI scribe for veterinary medicine">\n'
        '<meta name="twitter:card" content="summary_large_image">\n'
        f'<meta name="twitter:title" content="{e(title)}">\n'
        f'<meta name="twitter:description" content="{e(description)}">\n'
        f'<meta name="twitter:image" content="{OG_IMAGE}">\n'
        # The browser bar matches the solid header: white, or the dark surface.
        '<meta name="theme-color" content="#FFFFFF" media="(prefers-color-scheme: light)">\n'
        '<meta name="theme-color" content="#0F151D" media="(prefers-color-scheme: dark)">\n'
        # Favicon: docs/brand/logos/openbark-favicon.svg (no grille, for 16 to 32 px).
        '<link rel="icon" href="favicon.svg" type="image/svg+xml">\n'
        '<link rel="apple-touch-icon" href="icon.svg">\n'
        f"{FONTS}\n"
        '<link rel="stylesheet" href="site.css">\n'
    )


SOURCE_NAV = f'      <a href="{SOURCE_URL}">Source code</a>\n' if SOURCE_URL else ""
SOURCE_FOOT = f'\n        <a href="{SOURCE_URL}">Source code</a>' if SOURCE_URL else ""


def switch(text):
    """Keep the <!--if source-public--> or <!--if source-private--> blocks that apply, drop the others."""
    keep, drop = ("public", "private") if SOURCE_URL else ("private", "public")
    text = re.sub(rf"<!--if source-{drop}-->.*?<!--end-->", "", text, flags=re.S)
    return re.sub(rf"<!--if source-{keep}-->(.*?)<!--end-->", r"\1", text, flags=re.S)


def header(page):
    cur = lambda p: ' aria-current="page"' if p == page else ""
    return f"""<a class="skip" href="#main">Skip to content</a>
<header class="site-head">
  <div class="wrap">
    <a class="brand" href="index.html">{LOCKUP.format(label=e(PRODUCT) + ", home")}<span class="stage">Beta</span></a>
    <nav class="site-nav" aria-label="Site">
      <a href="features.html"{cur("features")}>Features</a>
      <a href="history.html"{cur("history")}>Version history</a>
      <a href="index.html#contact">Contact</a>
{SOURCE_NAV}      <a class="nav-account" href="portal.html"{cur("portal")}>{ACCOUNT_ICON}<span data-account-label="Account">Sign in</span></a>
      <a class="btn btn-primary" href="index.html#download">Get {e(PRODUCT)}</a>
    </nav>
  </div>
</header>
"""


FOOTER = f"""<footer class="site-foot">
  <div class="wrap">
    <p class="foot-name">{LOCKUP.format(label=e(PRODUCT))}</p>
    <div class="foot-row">
      <p>{e(PRODUCT)}, {e(TAGLINE.lower())}. Built by the <a href="https://www.animl.health">ANI.ML Health</a> team.<br>
      Drafts are not medical records until a veterinarian reviews and signs them.</p>
      <nav aria-label="Footer">
        <a href="features.html">Features</a>
        <a href="history.html">Version history</a>
        <a href="index.html#download">Download</a>
        <a href="portal.html" data-account-label="Your account">Sign in</a>
        <a href="mailto:{CONTACT}">{CONTACT}</a>
        <a href="{COFFEE}" rel="noopener">Buy us a coffee</a>
        <a href="features.html#privacy">Privacy</a>
        <a href="https://www.apache.org/licenses/LICENSE-2.0">Licence: Apache-2.0</a>{SOURCE_FOOT}
      </nav>
    </div>
  </div>
</footer>
<script>
// "Sign in" reads "Account" when this browser holds a portal sign-in. Read locally; nothing is sent.
try {{
  if (localStorage.getItem("{SUPABASE_SESSION_KEY}")) document.querySelectorAll("[data-account-label]").forEach(function (el) {{
    el.setAttribute("data-out-label", el.textContent);
    el.textContent = el.getAttribute("data-account-label");
  }});
}} catch (err) {{}}
</script>
"""


LOST = f"""<section class="lost">
  <div class="wrap">
    {MARK_MONO}
    <h1>This page wandered off.</h1>
    <p>The address may be mistyped, or the page moved when the site changed. Everything {e(PRODUCT)} does is on the features page, and the installers are on the home page.</p>
    <div class="hero-actions">
      <a class="btn btn-primary" href="index.html">Go to the home page</a>
      <a class="link-more" href="features.html">See every feature</a>
    </div>
  </div>
</section>
"""


def stat_block(stats, limit=8):
    """Value and plain label per tile; where each number comes from sits in one closed list below."""
    shown = stats[:limit]
    tiles = "".join(
        f'<div class="stat"><span class="stat-value">{e(s["value"])}</span>'
        f'<span class="stat-label">{e(s["label"])}</span></div>'
        for s in shown
    )
    sources = "".join(
        f'<li><span class="how-what">{e(s["value"])}, {e(s["label"])}</span><span class="evidence">{e(s["source"])}</span></li>'
        for s in shown if s.get("source")
    )
    how = f'<details class="how how-stats"><summary>Where these numbers come from</summary><ul>{sources}</ul></details>' if sources else ""
    return f'<div class="stats stats-4">{tiles}</div>{how}'


def how_we_know(detail, evidence):
    """The technical detail and the evidence, closed by default (brand guide 8: lead with what the clinician gets)."""
    if not detail and not evidence:
        return ""
    body = (f'<p class="detail">{e(detail)}</p>' if detail else "") + (f'<p class="evidence">{e(evidence)}</p>' if evidence else "")
    return f'<details class="how"><summary>How we know</summary>{body}</details>'


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
                f'{how_we_know(it.get("detail", ""), it.get("evidence", ""))}</li>'
            )
        sections.append(
            f'<section class="fx-cat" id="{e(cid)}"><h2>{e(c["name"])}</h2><p>{e(c.get("intro", ""))}</p>'
            f'<ul class="fx-items">{"".join(items)}</ul></section>'
        )
    bt = content.get("built_together", {})
    practices = "".join(f"<li>{e(p)}</li>" for p in bt.get("practices", []))
    together = f"""<section class="band band-gray" id="built-together">
  <div class="wrap">
    <div class="band-head">
      <h2>Built by the ANI.ML Health team.</h2>
      <p>{e(bt.get("intro", ""))}</p>
    </div>
    <div class="together">
      <div><h3>How we build it</h3><ul class="practices">{practices}</ul></div>
    </div>
  </div>
</section>"""
    return f"""<section class="fx-hero">
  <div class="wrap">
    <h1>{total} things {e(PRODUCT)} does for your clinic.</h1>
    <p>Each feature says what it does for you. Open "How we know" under any of them to see the test or measurement behind it, so nothing here is a promise we can't point to. Items marked "Being tested" or "Coming" are built or planned but not yet proven on real devices.</p>
  </div>
</section>
<section class="fx-stats">
  <div class="wrap">{stat_block(content.get("headline_stats", []))}</div>
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


def history_entries(history):
    for m in history["milestones"]:
        for en in m["entries"]:
            yield m, en


def history_body(history):
    total = sum(len(en["items"]) for _, en in history_entries(history))
    days = sorted({en["date"] for _, en in history_entries(history)})
    span = days[0] if len(days) == 1 else f"{days[0]} to {days[-1]}"
    nxt = "".join(f'<li><p>{e(i["text"])}</p>{how_we_know("", i.get("evidence", ""))}</li>' for i in history["next"])
    toc = "".join(f'<a href="#{e(m["id"])}"><span>{e(m["name"])}</span><span class="count">{len(m["entries"])}</span></a>' for m in history["milestones"])
    groups = []
    for m in history["milestones"]:
        entries = []
        for en in m["entries"]:
            items = "".join(
                f'<li><p>{e(i["text"])}'
                + (' <span class="status" data-s="testing">Being tested</span>' if i.get("status") == "testing" else "")
                + f'</p>{how_we_know("", i.get("evidence", ""))}</li>'
                for i in en["items"]
            )
            entries.append(
                f'<article class="hx-entry"><p class="hx-date"><time datetime="{e(en["date"])}">{e(en["date"])}</time></p>'
                f'<div><h3>{e(en["title"])}</h3><p class="hx-summary">{e(en["summary"])}</p><ul class="hx-items">{items}</ul></div></article>'
            )
        groups.append(
            f'<section class="hx-group" id="{e(m["id"])}"><h2>{e(m["name"])}<span>{e(m["tagline"])}</span></h2>{"".join(entries)}</section>'
        )
    return f"""<section class="fx-hero">
  <div class="wrap">
    <h1>Version history</h1>
    <p>Version {e(history["version"])}, {e(history["stage"].lower())}. The major changes in each milestone, newest first. Changes marked "Being tested" are built but not yet proven on real devices.</p>
  </div>
</section>
<section class="fx-stats">
  <div class="wrap">
    <div class="hx-next">
      <h2>Coming next</h2>
      <p>Planned or being tested, not finished. No dates are promised.</p>
      <ul>{nxt}</ul>
    </div>
  </div>
</section>
<div class="wrap fx-layout" id="hx">
  <nav class="fx-toc" aria-label="Milestones">{toc}</nav>
  <div>{"".join(groups)}</div>
</div>
"""


def changelog_md(history):
    out = [
        "# Changelog",
        "",
        f"All notable changes to {PRODUCT}, newest first. Generated from `website/content/history.json` by `scripts/website-build.py`; edit that file, not this one. The same history is on the website's Version history page.",
        "",
        f"## {history['version']} ({history['stage'].lower()})",
        "",
    ]
    for m in history["milestones"]:
        out += [f"### {m['name']}: {m['tagline']}", ""]
        for en in m["entries"]:
            out += [f"#### {en['date']}: {en['title']}", "", en["summary"], ""]
            for i in en["items"]:
                tag = " (being tested)" if i.get("status") == "testing" else ""
                out.append(f"- {i['text']}{tag}")
            out.append("")
    out += ["## Coming next", "", "Planned or being tested, not finished. No dates are promised.", ""]
    out += [f"- {i['text']}" for i in history["next"]]
    return "\n".join(out) + "\n"


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
    history = json.loads(read(SITE, "content", "history.json"))
    templates = len([d for d in os.listdir(os.path.join(ROOT, "packages", "templates")) if os.path.isdir(os.path.join(ROOT, "packages", "templates", d))])

    landing = (
        switch(read(SRC, "index.html"))
        .replace("{{PRODUCT}}", e(PRODUCT))
        .replace("{{TAGLINE}}", e(TAGLINE.lower()))
        .replace("{{TEMPLATES}}", str(templates))
        .replace("{{CONTACT}}", e(CONTACT))
        .replace("{{COFFEE}}", e(COFFEE))
        .replace("{{SOURCE}}", e(SOURCE_URL or ""))
        .replace("{{FEATURES}}", str(sum(len(c["items"]) for c in content["categories"])))
    )
    landing_title = f"{PRODUCT}, the {OPEN} AI scribe for veterinary medicine"
    licence = "Free and open source (Apache-2.0)." if SOURCE_URL else "Free to use."
    landing_desc = f"{PRODUCT} listens to the visit and drafts the record on your clinic's own computer. Your recordings and notes stay on your computer. No cloud. No subscription. {licence} {CREDIT}"
    landing_head = head(landing_title, landing_desc, "home")
    landing_body = header("home") + '<main id="main">\n' + landing + "</main>\n" + FOOTER + '<script src="site.js"></script>\n'

    features_title = f"{PRODUCT} Features"
    features_desc = f"Every feature of {PRODUCT}, the {OPEN} veterinary scribe, and how we know it works. {CREDIT}"
    features_doc = document(head(features_title, features_desc, "features"), header("features") + '<main id="main">\n' + features_body(content) + "</main>\n" + FOOTER)
    history_doc = document(
        head(f"{PRODUCT} Version history", f"Everything built into {PRODUCT} so far, newest first. {CREDIT}", "history"),
        header("history") + '<main id="main">\n' + history_body(history) + "</main>\n" + FOOTER,
    )
    portal = read(SRC, "portal.html").replace("{{PRODUCT}}", e(PRODUCT)).replace("{{CONTACT}}", e(CONTACT))
    portal_doc = document(
        head(
            f"Your {PRODUCT} account",
            f"Sign in to your {PRODUCT} account with a code we email you: downloads, update emails, and deleting the account. "
            f"No clinical data is ever stored here. {CREDIT}",
            "portal",
        ),
        header("portal") + '<main id="main">\n' + portal + "</main>\n" + FOOTER
        + f'<script src="{SUPABASE_JS}" integrity="{SUPABASE_JS_SRI}" crossorigin="anonymous" referrerpolicy="no-referrer"></script>\n'
        + '<script src="portal.js"></script>\n',
    )
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
    io.open(os.path.join(DIST, "history.html"), "w", encoding="utf-8", newline="\n").write(history_doc)
    io.open(os.path.join(DIST, "portal.html"), "w", encoding="utf-8", newline="\n").write(portal_doc)
    io.open(os.path.join(ROOT, "CHANGELOG.md"), "w", encoding="utf-8", newline="\n").write(changelog_md(history))
    for name in ("site.css", "site.js", "portal.js"):
        shutil.copy(os.path.join(SRC, name), os.path.join(DIST, name))
    pub = os.path.join(SITE, "public")
    for name in os.listdir(pub):
        shutil.copy(os.path.join(pub, name), os.path.join(DIST, name))
    print(f"wrote {DIST}: index.html, features.html ({sum(len(c['items']) for c in content['categories'])} features), history.html, portal.html")

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
