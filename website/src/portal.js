// OpenBark account portal (/portal).
//
// Sign-in is an emailed one-time code through Supabase Auth: no passwords and
// no social logins. supabase-js keeps the session in this browser's
// localStorage, as it does by default. The profile is about the person (name,
// clinic, role, country, update emails), never about patients: no clinical
// data ever comes to this page (ADR-0017).
//
// The project URL and publishable key below are public by design. Row level
// security (supabase/migrations) decides what a signed-in person can read and
// change: their own profile row, nothing else. Deleting the account goes
// through the delete-account Edge Function (supabase/functions), which holds
// the service role on Supabase's side.
(function () {
  "use strict";

  var SUPABASE_URL = "https://nmecovtmijnfnjuadicp.supabase.co";
  var SUPABASE_KEY = "sb_publishable_f80ns1rDRfliZVnPi_LNBg_HnypbsgZ";
  var STORAGE_KEY = "sb-nmecovtmijnfnjuadicp-auth-token"; // supabase-js's default key for this project
  var PENDING_KEY = "openbark-portal-pending"; // sessionStorage: where the last code went, and when
  var CONTACT = "we.care@animl.health";
  var RESEND_SECONDS = 60; // Supabase's default gap between codes to one address
  var PENDING_MINUTES = 60; // the code's default lifetime

  var $ = function (id) { return document.getElementById(id); };
  var root = $("portal");
  if (!root) return;

  var views = { loading: $("view-loading"), out: $("view-signed-out"), in: $("view-signed-in") };
  var emailForm = $("email-form");
  var emailInput = $("email");
  var codeForm = $("code-form");
  var codeInput = $("code");
  var codeEmail = $("code-email");
  var resendButton = $("resend");
  var signinMsg = $("signin-msg");
  var profileForm = $("profile-form");
  var profileMsg = $("profile-msg");
  var deleteOpen = $("delete-open");
  var deleteForm = $("delete-form");
  var deleteInput = $("delete-email");
  var deleteSubmit = $("delete-submit");
  var deleteMsg = $("delete-msg");

  // ---- Small helpers.
  function show(name) {
    Object.keys(views).forEach(function (k) { if (views[k]) views[k].hidden = k !== name; });
    root.setAttribute("data-state", name);
  }
  function say(el, text, tone) {
    el.textContent = text || "";
    if (tone) el.setAttribute("data-tone", tone); else el.removeAttribute("data-tone");
  }
  function busy(button, label) {
    if (label) {
      button.setAttribute("data-idle", button.textContent);
      button.textContent = label;
      button.disabled = true;
      button.setAttribute("aria-busy", "true");
    } else {
      if (button.hasAttribute("data-idle")) button.textContent = button.getAttribute("data-idle");
      button.disabled = false;
      button.removeAttribute("aria-busy");
    }
  }
  function store(kind) {
    try { return window[kind]; } catch (e) { return null; }
  }
  function readPending() {
    try {
      var p = JSON.parse(store("sessionStorage").getItem(PENDING_KEY) || "null");
      if (p && p.email && Date.now() - p.at < PENDING_MINUTES * 60000) return p;
    } catch (e) { /* private mode or bad JSON: no pending code */ }
    return null;
  }
  function writePending(email, at) {
    try { store("sessionStorage").setItem(PENDING_KEY, JSON.stringify({ email: email, at: at })); } catch (e) { /* fine without */ }
  }
  function clearPending() {
    try { store("sessionStorage").removeItem(PENDING_KEY); } catch (e) { /* fine without */ }
  }
  // "Sign in" in the header and footer reads "Account" while signed in.
  function labelAccount(signedIn) {
    document.querySelectorAll("[data-account-label]").forEach(function (el) {
      if (!el.hasAttribute("data-out-label")) el.setAttribute("data-out-label", el.textContent);
      el.textContent = signedIn ? el.getAttribute("data-account-label") : el.getAttribute("data-out-label");
    });
  }

  // Plain words for what went wrong (brand guide 8: honest, no "Oops").
  function explain(error, step) {
    var code = (error && error.code) || "";
    var status = error && error.status;
    var message = (error && error.message) || "";
    var lower = message.toLowerCase();
    if ((error && error.name === "AuthRetryableFetchError") || /failed to fetch|networkerror|load failed/.test(lower)) {
      return "We couldn't reach the sign-in service. Check your connection and try again.";
    }
    if (code === "over_email_send_rate_limit" || code === "over_request_rate_limit" || status === 429) {
      return "Too many codes were asked for. Wait a minute, then send a new one.";
    }
    if (step === "verify" && (code === "otp_expired" || /expired|invalid/.test(lower))) {
      return "That code is wrong or has expired. Check the latest email, or send a new code.";
    }
    if (code === "email_address_invalid" || /email.*invalid|invalid.*email/.test(lower)) {
      return "That email address doesn't look right. Check it and try again.";
    }
    if (code === "email_address_not_authorized" || /not authorized/.test(lower)) {
      return "Sign-in emails can't go to this address yet. Write to " + CONTACT + " and we'll set you up.";
    }
    if (code === "signup_disabled") return "New accounts are paused for now. Write to " + CONTACT + " to join the beta.";
    return (step === "verify" ? "You couldn't be signed in" : "The code couldn't be sent") + (message ? ": " + message : "") + ". Try again, or write to " + CONTACT + ".";
  }

  // ---- Countries: ISO 3166-1 alpha-2 codes (inhabited), named in the visitor's language.
  var COUNTRIES = "AD AE AF AG AI AL AM AO AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GT GU GW GY HK HN HR HT HU ID IE IL IM IN IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TG TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG US UY UZ VA VC VE VG VI VN VU WF WS XK YE YT ZA ZM ZW".split(" ");
  (function fillCountries() {
    var select = $("p-country");
    if (!select) return;
    var names = null;
    try { names = new Intl.DisplayNames([navigator.language || "en", "en"], { type: "region" }); } catch (e) { /* codes only */ }
    COUNTRIES
      .map(function (c) { return { code: c, name: (names && names.of(c)) || c }; })
      .sort(function (a, b) { return a.name.localeCompare(b.name); })
      .forEach(function (c) {
        var o = document.createElement("option");
        o.value = c.code;
        o.textContent = c.name;
        select.appendChild(o);
      });
  })();

  // ---- Downloads, from release.json (written by the release workflow).
  var PLATFORMS = {
    windows: { name: "Windows", req: "Windows 10 or 11. 16 GB memory." },
    mac: { name: "macOS", req: "macOS 13 or later, Apple silicon. 16 GB memory." },
    linux: { name: "Linux", req: "Ubuntu, Debian, Fedora and more. 16 GB memory." }
  };
  var ua = navigator.userAgent || "";
  var platform = /iPhone|iPad|iPod/.test(ua) ? "ios" : /Mac/.test(ua) ? "mac" : /Windows/.test(ua) ? "windows" : /Linux|X11/.test(ua) ? "linux" : null;

  function el(tag, cls, text) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text) n.textContent = text;
    return n;
  }
  function drawDownloads(rel) {
    var box = $("downloads");
    var line = $("downloads-line");
    if (!box || !line) return;
    var published = !!(rel && rel.published && rel.version);
    var base = rel && rel.download_base ? String(rel.download_base).replace(/\/$/, "") : "";
    var files = (rel && rel.files) || {};
    line.textContent = "";
    if (published) {
      line.appendChild(el("span", null, "Version " + rel.version + (rel.date ? ", " + rel.date : "")));
      if (rel.notes) { var n = el("a", null, "What's new"); n.href = rel.notes; line.appendChild(n); }
      if (rel.all) { var a = el("a", null, "All downloads and checksums"); a.href = rel.all; line.appendChild(a); }
    } else {
      line.textContent = "The beta installers aren't published yet. Turn on update emails in your profile and we'll write when they are.";
    }
    var order = ["windows", "mac", "linux"].sort(function (x, y) { return (y === platform) - (x === platform); });
    box.textContent = "";
    order.forEach(function (os) {
      var row = el("div", "pt-dl-row");
      row.setAttribute("data-os", os);
      if (os === platform) row.setAttribute("data-mine", "true");
      var what = el("div", "pt-dl-what");
      var h = el("h3", null, PLATFORMS[os].name);
      if (os === platform) h.appendChild(el("span", "status pt-mine", "This computer"));
      what.appendChild(h);
      what.appendChild(el("p", "pt-dl-req", PLATFORMS[os].req));
      row.appendChild(what);
      var actions = el("div", "pt-dl-files");
      var list = files[os] || [];
      if (published && list.length) {
        list.forEach(function (f, i) {
          var link = el("a", "btn " + (os === platform && i === 0 ? "btn-primary" : "btn-ghost"), f.label);
          link.href = base + "/" + encodeURIComponent(f.name);
          actions.appendChild(link);
        });
      } else {
        actions.appendChild(el("span", "status", "Coming soon"));
      }
      row.appendChild(actions);
      box.appendChild(row);
    });
    if (platform === "ios") {
      box.appendChild(el("p", "pt-dl-note", "The iPhone app isn't built yet. Open this page on your computer to download OpenBark."));
    }
  }
  function loadDownloads() {
    drawDownloads(null);
    if (!window.fetch) return;
    fetch("release.json", { cache: "no-store" })
      .then(function (r) { return r.ok ? r.json() : null; })
      .then(function (rel) { if (rel) drawDownloads(rel); })
      .catch(function () { /* stays on "not published yet" */ });
  }

  // ---- No sign-in library: the CDN script was blocked or didn't load.
  if (!window.supabase || typeof window.supabase.createClient !== "function") {
    show("out");
    emailForm.querySelectorAll("input, button").forEach(function (n) { n.disabled = true; });
    say(signinMsg, "Sign-in couldn't load. Check your connection, then reload the page. If it keeps happening, write to " + CONTACT + ".", "error");
    return;
  }

  var sb = window.supabase.createClient(SUPABASE_URL, SUPABASE_KEY, {
    auth: { persistSession: true, autoRefreshToken: true, detectSessionInUrl: true }
  });

  var user = null; // the signed-in user being shown
  var profileExists = false;
  var notice = null; // shown once on the sign-in card: "You're signed out." and the like
  var cooldown = null;

  // A deletion that had to fall back to a reload leaves ?deleted=1 behind.
  if (/[?&]deleted=1\b/.test(location.search)) {
    notice = "Your account and profile were deleted. OpenBark on your computers isn't affected.";
    try { history.replaceState(null, "", location.pathname); } catch (e) { /* keeps the query */ }
  }

  // ---- Signed out: the email step, then the code step.
  function toEmailStep(focus) {
    clearInterval(cooldown);
    clearPending();
    codeForm.hidden = true;
    emailForm.hidden = false;
    if (focus) emailInput.focus();
  }
  function toCodeStep(email, sentAt, focus) {
    codeEmail.textContent = email;
    codeInput.value = "";
    emailForm.hidden = true;
    codeForm.hidden = false;
    startCooldown(sentAt);
    if (focus) codeInput.focus();
  }
  function startCooldown(sentAt) {
    clearInterval(cooldown);
    var tick = function () {
      var left = Math.ceil(RESEND_SECONDS - (Date.now() - sentAt) / 1000);
      if (left > 0) {
        resendButton.disabled = true;
        resendButton.textContent = "Send a new code in " + left + " s";
      } else {
        clearInterval(cooldown);
        resendButton.disabled = false;
        resendButton.textContent = "Send a new code";
      }
    };
    tick();
    cooldown = setInterval(tick, 1000);
  }
  function enterSignedOut() {
    user = null;
    labelAccount(false);
    show("out");
    var pending = readPending();
    if (pending) toCodeStep(pending.email, pending.at, false);
    else toEmailStep(false);
    if (notice) { say(signinMsg, notice, "ok"); notice = null; }
  }

  async function sendCode(email, button) {
    say(signinMsg, "");
    busy(button, "Sending…");
    var res;
    try {
      res = await sb.auth.signInWithOtp({
        email: email,
        // The email shows a code. If a template still carries a link, it
        // brings the visitor back here, where the session is picked up.
        options: { shouldCreateUser: true, emailRedirectTo: location.origin + location.pathname }
      });
    } catch (e) {
      res = { error: e };
    }
    busy(button);
    if (res.error) {
      say(signinMsg, explain(res.error, "send"), "error");
      return false;
    }
    var at = Date.now();
    writePending(email, at);
    toCodeStep(email, at, true);
    say(signinMsg, "Code sent. It can take a minute to arrive; check your spam folder if it doesn't.", "ok");
    return true;
  }

  emailForm.addEventListener("submit", function (ev) {
    ev.preventDefault();
    var email = emailInput.value.trim();
    if (!emailInput.checkValidity() || !email) {
      say(signinMsg, "Enter the email address you'd like to sign in with.", "error");
      emailInput.focus();
      return;
    }
    sendCode(email, emailForm.querySelector('button[type="submit"]'));
  });

  codeInput.addEventListener("input", function () {
    var digits = codeInput.value.replace(/\D/g, "").slice(0, 10);
    if (digits !== codeInput.value) codeInput.value = digits;
  });

  codeForm.addEventListener("submit", async function (ev) {
    ev.preventDefault();
    var token = codeInput.value.replace(/\D/g, "");
    if (token.length < 6) {
      say(signinMsg, "Enter the 6-digit code from the email.", "error");
      codeInput.focus();
      return;
    }
    var button = codeForm.querySelector('button[type="submit"]');
    say(signinMsg, "");
    busy(button, "Signing in…");
    var res;
    try {
      res = await sb.auth.verifyOtp({ email: codeEmail.textContent, token: token, type: "email" });
    } catch (e) {
      res = { error: e };
    }
    busy(button);
    if (res.error) {
      say(signinMsg, explain(res.error, "verify"), "error");
      codeInput.select();
      return;
    }
    clearPending();
    clearInterval(cooldown);
    // onAuthStateChange (SIGNED_IN) shows the account.
  });

  resendButton.addEventListener("click", function () {
    sendCode(codeEmail.textContent, resendButton);
  });
  $("change-email").addEventListener("click", function () {
    say(signinMsg, "");
    toEmailStep(true);
  });

  // ---- Signed in.
  function enterAccount(u, focus) {
    user = u;
    labelAccount(true);
    show("in");
    $("account-email").textContent = u.email || "";
    resetDelete();
    loadProfile(u.id);
    loadDownloads();
    if (focus) $("account-h").focus();
  }

  function setRole(value) {
    profileForm.querySelectorAll('input[name="role"]').forEach(function (r) { r.checked = r.value === value; });
  }
  function getRole() {
    var r = profileForm.querySelector('input[name="role"]:checked');
    return r ? r.value : null;
  }
  function clean(value, max) {
    var v = String(value || "").trim().slice(0, max);
    return v || null;
  }

  async function loadProfile(uid) {
    profileExists = false;
    profileForm.reset();
    say(profileMsg, "Loading your profile…");
    var res;
    try {
      res = await sb.from("profiles").select("name, clinic, role, country, updates_opt_in").eq("id", uid).maybeSingle();
    } catch (e) {
      res = { error: e };
    }
    if (!user || user.id !== uid) return;
    if (res.error) {
      say(profileMsg, "Your profile couldn't be loaded" + (res.error.message ? ": " + res.error.message : "") + ". Reload the page to try again.", "error");
      return;
    }
    say(profileMsg, "");
    var p = res.data;
    if (!p) return; // new account: an empty form
    profileExists = true;
    $("p-name").value = p.name || "";
    $("p-clinic").value = p.clinic || "";
    setRole(p.role);
    $("p-country").value = p.country || "";
    $("p-updates").checked = !!p.updates_opt_in;
  }

  profileForm.addEventListener("submit", async function (ev) {
    ev.preventDefault();
    if (!user) return;
    var uid = user.id;
    var button = profileForm.querySelector('button[type="submit"]');
    var row = {
      name: clean($("p-name").value, 120),
      clinic: clean($("p-clinic").value, 160),
      role: getRole(),
      country: $("p-country").value || null,
      updates_opt_in: $("p-updates").checked
    };
    say(profileMsg, "");
    busy(button, "Saving…");
    var res;
    try {
      if (profileExists) {
        res = await sb.from("profiles").update(row).eq("id", uid);
      } else {
        res = await sb.from("profiles").insert(Object.assign({ id: uid }, row));
        // Saved from another tab in the meantime: change that row instead.
        if (res.error && res.error.code === "23505") res = await sb.from("profiles").update(row).eq("id", uid);
      }
    } catch (e) {
      res = { error: e };
    }
    busy(button);
    if (res.error) {
      say(profileMsg, "Your profile wasn't saved" + (res.error.message ? ": " + res.error.message : "") + ". Try again.", "error");
      return;
    }
    profileExists = true;
    say(profileMsg, "Saved", "ok");
  });
  profileForm.addEventListener("input", function () {
    if (profileMsg.getAttribute("data-tone") === "ok") say(profileMsg, "");
  });

  // Signing out here signs out this browser only; OpenBark on other devices
  // keeps its own sign-in.
  function dropLocalSession() {
    try { store("localStorage").removeItem(STORAGE_KEY); } catch (e) { /* nothing stored */ }
  }
  $("sign-out").addEventListener("click", async function () {
    var button = $("sign-out");
    busy(button, "Signing out…");
    notice = "You're signed out.";
    var res;
    try { res = await sb.auth.signOut({ scope: "local" }); } catch (e) { res = { error: e }; }
    if (res && res.error) {
      // Offline or the server refused: forget the session here anyway.
      dropLocalSession();
      location.reload();
      return;
    }
    busy(button);
  });

  // ---- Delete my account.
  function resetDelete() {
    deleteForm.hidden = true;
    deleteOpen.hidden = false;
    deleteOpen.setAttribute("aria-expanded", "false");
    deleteInput.value = "";
    deleteSubmit.disabled = true;
    say(deleteMsg, "");
  }
  deleteOpen.addEventListener("click", function () {
    deleteForm.hidden = false;
    deleteOpen.hidden = true;
    deleteOpen.setAttribute("aria-expanded", "true");
    deleteInput.focus();
  });
  $("delete-cancel").addEventListener("click", function () {
    resetDelete();
    deleteOpen.focus();
  });
  deleteInput.addEventListener("input", function () {
    var want = ((user && user.email) || "").toLowerCase();
    deleteSubmit.disabled = !want || deleteInput.value.trim().toLowerCase() !== want;
  });

  async function deleteProblem(error) {
    var status = error && error.context && error.context.status;
    var code = "";
    if (error && error.context && typeof error.context.json === "function") {
      try { code = ((await error.context.json()) || {}).error || ""; } catch (e) { /* not JSON */ }
    }
    if (error && error.name === "FunctionsFetchError") {
      return "We couldn't reach the account service, so nothing was deleted. Try again later, or write to " + CONTACT + " and we'll delete it for you.";
    }
    if (status === 404) return "Account deletion isn't switched on yet, so nothing was deleted. Write to " + CONTACT + " and we'll delete your account for you.";
    if (code === "confirmation_mismatch") return "That email address doesn't match this account, so nothing was deleted.";
    if (status === 401) return "Your sign-in has expired, so nothing was deleted. Sign out, sign in again, then try again.";
    return "Your account couldn't be deleted" + (code ? " (" + code + ")" : "") + ". Try again, or write to " + CONTACT + ".";
  }

  deleteForm.addEventListener("submit", async function (ev) {
    ev.preventDefault();
    if (!user || deleteSubmit.disabled) return;
    say(deleteMsg, "");
    busy(deleteSubmit, "Deleting…");
    $("delete-cancel").disabled = true;
    var res;
    try {
      res = await sb.functions.invoke("delete-account", { body: { confirm: deleteInput.value.trim() } });
    } catch (e) {
      res = { error: e };
    }
    if (res.error) {
      say(deleteMsg, await deleteProblem(res.error), "error");
      busy(deleteSubmit);
      $("delete-cancel").disabled = false;
      return;
    }
    notice = "Your account and profile were deleted. OpenBark on your computers isn't affected.";
    clearPending();
    var out;
    try { out = await sb.auth.signOut({ scope: "local" }); } catch (e) { out = { error: e }; }
    $("delete-cancel").disabled = false;
    busy(deleteSubmit);
    if (out && out.error) {
      dropLocalSession();
      location.replace(location.pathname + "?deleted=1");
    }
  });

  // ---- One place decides what shows: the session supabase-js holds.
  // INITIAL_SESSION fires once with the stored session (or none), then
  // SIGNED_IN, SIGNED_OUT and TOKEN_REFRESHED as things change.
  sb.auth.onAuthStateChange(function (event, session) {
    // Deferred: supabase-js asks that the callback not call back into the client directly.
    setTimeout(function () {
      var u = session && session.user;
      if (u) {
        if (!user || user.id !== u.id) enterAccount(u, event === "SIGNED_IN");
      } else if (user || event === "INITIAL_SESSION" || root.getAttribute("data-state") === "loading") {
        var wasIn = !!user;
        enterSignedOut();
        if (wasIn && event === "SIGNED_OUT") emailInput.focus();
      }
    }, 0);
  });
})();
