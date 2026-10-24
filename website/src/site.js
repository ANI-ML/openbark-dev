// OpenBark website: the evidence-link demo, the visitor's platform, and the
// current release (release.json, written by the release workflow).
(function () {
  // ---- Evidence demo: a note line lights up the transcript line it came from.
  var tx = document.getElementById("tx");
  if (tx) {
    var lines = tx.querySelectorAll("li");
    var buttons = document.querySelectorAll(".note button[data-ev]");
    // One ink marker glides to the lit line (transform only: translate to the
    // line, scaleY to its height). Its first placement doesn't animate.
    var marker = document.createElement("span");
    marker.className = "tx-marker";
    marker.setAttribute("aria-hidden", "true");
    tx.parentNode.appendChild(marker);
    var litLine = null;
    var place = function () {
      if (!litLine) return;
      var x = litLine.offsetLeft - 8;
      marker.style.transform = "translate(" + x + "px," + litLine.offsetTop + "px) scaleY(" + litLine.offsetHeight + ")";
      marker.setAttribute("data-on", "");
      if (!marker.hasAttribute("data-glide")) {
        requestAnimationFrame(function () { marker.setAttribute("data-glide", ""); });
      }
    };
    var light = function (id, pressed) {
      lines.forEach(function (li) {
        var on = li.getAttribute("data-id") === id;
        li.classList.toggle("lit", on);
        if (on) litLine = li;
      });
      buttons.forEach(function (b) {
        b.setAttribute("aria-pressed", String(b === pressed));
      });
      place();
    };
    if (window.ResizeObserver) new ResizeObserver(place).observe(tx);
    buttons.forEach(function (b) {
      var id = b.getAttribute("data-ev");
      b.addEventListener("mouseenter", function () { light(id, b); });
      b.addEventListener("focus", function () { light(id, b); });
      b.addEventListener("click", function () { light(id, b); });
    });
    // Start with one line lit, so the page shows the idea before anyone touches it.
    if (buttons[1]) light(buttons[1].getAttribute("data-ev"), buttons[1]);
  }

  // ---- Scroll reveal. Armed by the first scroll: sections still below the
  // fold wait out of sight, then fade up as they come into view. Nothing is
  // hidden before anyone scrolls, and nothing at all with reduced motion.
  var reveals = document.querySelectorAll(".reveal");
  var still = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  if (reveals.length && window.IntersectionObserver && !still) {
    var arm = function () {
      var fold = window.innerHeight;
      var waiting = [];
      // Read every position first, then write: one layout, no thrash.
      reveals.forEach(function (el) { if (el.getBoundingClientRect().top > fold) waiting.push(el); });
      if (!waiting.length) return;
      var seen = new IntersectionObserver(function (entries) {
        entries.forEach(function (en) {
          if (!en.isIntersecting) return;
          en.target.setAttribute("data-reveal", "in");
          seen.unobserve(en.target);
        });
      }, { rootMargin: "0px 0px -8% 0px" });
      waiting.forEach(function (el) {
        el.setAttribute("data-reveal", "wait");
        seen.observe(el);
      });
    };
    window.addEventListener("scroll", arm, { once: true, passive: true });
  }

  // ---- The demo's recording timer ticks, so "Recording" reads as live.
  // A text change only: nothing moves (the pulse is the one expressive animation).
  var rec = document.querySelector(".demo .rec");
  if (rec) {
    var label = rec.lastChild;
    var m = label && label.nodeType === 3 && /(\d+):(\d\d)\s*$/.exec(label.nodeValue);
    if (m) {
      var secs = parseInt(m[1], 10) * 60 + parseInt(m[2], 10);
      var prefix = label.nodeValue.slice(0, m.index);
      var pad = function (n) { return (n < 10 ? "0" : "") + n; };
      setInterval(function () {
        secs = (secs + 1) % 3600;
        label.nodeValue = prefix + pad(Math.floor(secs / 60)) + ":" + pad(secs % 60);
      }, 1000);
    }
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
        soon.textContent = "Download coming soon";
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
    // The visitor's own platform comes first, moved in the DOM so the
    // keyboard order matches what is seen.
    var grid = document.getElementById("downloads");
    var mineCard = grid && grid.querySelector('.dl[data-mine="true"]');
    if (mineCard && grid.firstElementChild !== mineCard) grid.insertBefore(mineCard, grid.firstElementChild);
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
        add("Installers are on their way.");
      }
    }
    var hero = document.getElementById("hero-download");
    if (hero) {
      var mine = platform && files[platform] && files[platform][0];
      var label = hero.querySelector("span") || hero;
      var icon = hero.querySelector("svg");
      // Lucide download once there are installers; Lucide mail (the page's default) until then.
      if (icon) icon.innerHTML = published
        ? '<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>'
        : '<path d="m22 7-8.991 5.727a2 2 0 0 1-2.009 0L2 7"/><rect x="2" y="4" width="20" height="16" rx="2"/>';
      if (published && mine) {
        hero.href = base + "/" + encodeURIComponent(mine.name);
        label.textContent = "Download the beta for " + names[platform];
      } else {
        hero.href = "#download";
        label.textContent = "Download the beta";
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
