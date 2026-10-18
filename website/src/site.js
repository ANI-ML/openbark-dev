// OpenBark website: the evidence-link demo, the visitor's platform, and the
// current release (release.json, written by the release workflow).
(function () {
  // ---- Evidence demo: a note line lights up the transcript line it came from.
  var tx = document.getElementById("tx");
  if (tx) {
    var lines = tx.querySelectorAll("li");
    var buttons = document.querySelectorAll(".note button[data-ev]");
    var light = function (id, pressed) {
      lines.forEach(function (li) {
        li.classList.toggle("lit", li.getAttribute("data-id") === id);
      });
      buttons.forEach(function (b) {
        b.setAttribute("aria-pressed", String(b === pressed));
      });
    };
    buttons.forEach(function (b) {
      var id = b.getAttribute("data-ev");
      b.addEventListener("mouseenter", function () { light(id, b); });
      b.addEventListener("focus", function () { light(id, b); });
      b.addEventListener("click", function () { light(id, b); });
    });
    // Start with one line lit, so the page shows the idea before anyone touches it.
    if (buttons[1]) light(buttons[1].getAttribute("data-ev"), buttons[1]);
  }

  // ---- Platform and release.
  var ua = navigator.userAgent || "";
  var platform = /iPhone|iPad|iPod/.test(ua) ? "ios" : /Mac/.test(ua) ? "mac" : /Windows/.test(ua) ? "windows" : /Linux|X11/.test(ua) ? "linux" : null;
  var names = { windows: "Windows", mac: "macOS", linux: "Linux", ios: "iPhone" };

  var render = function (rel) {
    var published = rel && rel.published && rel.version;
    var base = rel && rel.download_base ? rel.download_base.replace(/\/$/, "") : "";
    var files = (rel && rel.files) || {};
    ["windows", "mac", "linux", "ios"].forEach(function (os) {
      var box = document.querySelector('[data-files="' + os + '"]');
      if (!box) return;
      box.textContent = "";
      var list = files[os] || [];
      if (os === "ios") {
        var a = document.createElement("a");
        a.className = "btn btn-ghost";
        if (rel && rel.app_store) {
          a.href = rel.app_store;
          a.textContent = "App Store";
        } else {
          a.setAttribute("aria-disabled", "true");
          a.textContent = "App Store: coming soon";
        }
        box.appendChild(a);
        return;
      }
      if (!published || !list.length) {
        var soon = document.createElement("a");
        soon.className = "btn btn-ghost";
        soon.setAttribute("aria-disabled", "true");
        soon.textContent = "Coming with the public beta";
        box.appendChild(soon);
        return;
      }
      list.forEach(function (f, i) {
        var row = document.createElement("div");
        row.className = "file";
        var link = document.createElement("a");
        link.className = i === 0 ? "btn btn-primary" : "btn btn-ghost";
        link.href = base + "/" + encodeURIComponent(f.name);
        link.textContent = f.label;
        var tag = document.createElement("span");
        tag.className = "tag mono";
        tag.textContent = f.size || "";
        row.appendChild(link);
        row.appendChild(tag);
        box.appendChild(row);
      });
    });
    document.querySelectorAll(".dl").forEach(function (el) {
      el.setAttribute("data-mine", String(el.getAttribute("data-os") === platform));
    });
    var line = document.getElementById("release-line");
    if (line) {
      line.textContent = "";
      var add = function (text, href) {
        var el = document.createElement(href ? "a" : "span");
        el.textContent = text;
        if (href) el.href = href;
        line.appendChild(el);
      };
      if (published) {
        add("Version " + rel.version + (rel.date ? ", " + rel.date : ""));
        if (rel.notes) add("What's new", rel.notes);
        if (rel.all) add("All downloads and checksums", rel.all);
      } else {
        add("Current version: " + ((rel && rel.version) || "pre-alpha") + " (pre-alpha, not yet published)");
      }
    }
    var hero = document.getElementById("hero-download");
    if (hero) {
      var mine = platform && files[platform] && files[platform][0];
      if (published && mine) {
        hero.href = base + "/" + encodeURIComponent(mine.name);
        hero.textContent = "Download for " + names[platform];
      } else {
        hero.href = "#download";
        hero.textContent = "See downloads";
      }
    }
  };

  // The page works without release.json (it shows "coming soon").
  render(window.__RELEASE__ || null);
  if (window.fetch) {
    fetch("release.json", { cache: "no-store" })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (rel) { if (rel) render(rel); })
      .catch(function () {});
  }
})();
