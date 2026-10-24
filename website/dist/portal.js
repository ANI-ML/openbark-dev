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
// change: their own profile row, nothing else. Clinics (ADR-0028) go through
// the `clinic` schema's functions, which check the caller on the server, and
// the clinic-invite Edge Function; they hold names and daily counts only.
// Deleting the account goes through the delete-account Edge Function
// (supabase/functions), which holds the service role on Supabase's side.
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
    resetClinics();
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
    resetClinics();
    loadProfile(u.id);
    loadClinics();
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

  // ---- Clinics (ADR-0028): invites, the admin dashboard and a member's own
  // counts. Everything goes through the `clinic` schema's functions, which
  // check the caller on the server; inviting by email goes through the
  // clinic-invite Edge Function. Only counts and names ever come back: no
  // clinical data exists in the schema to show.
  var CLINIC_KEY = "openbark-portal-clinic"; // localStorage: the clinic last looked at in this browser
  var clinicArea = $("clinic-area");
  var clinics = []; // my_clinics()
  var invitesForMe = []; // my_invites()
  var current = null; // the clinic shown
  var adminData = null; // clinic_admin(current)
  var dash = null; // clinic_dashboard(current, range)
  var rangeDays = 30;
  var loadSeq = 0; // drops answers that arrive after the person moved on

  var PLATFORM_NAMES = { windows: "Windows", macos: "macOS", linux: "Linux", ios: "iPhone", web: "Web" };
  var CLINIC_WORDS = {
    not_signed_in: "Your sign-in has expired. Sign out, sign in again, then try again.",
    not_admin: "Only this clinic's admins can do that.",
    not_member: "You're not a member of that clinic any more. Reload the page.",
    last_admin: "A clinic needs at least one admin. Make someone else an admin first, or delete the clinic.",
    rate_limited: "This clinic has reached today's limit of 50 invites and join codes. Try again tomorrow.",
    bad_name: "Names need between 1 and 120 characters (80 for a person's name).",
    bad_role: "Choose Member or Admin.",
    expired: "That invite has expired. Ask the clinic's admin to send it again.",
    no_invite: "That invite isn't there any more; it may have been revoked.",
    email_not_confirmed: "Your email address isn't confirmed yet. Sign out, then sign in again with a code.",
    too_many_clinics: "You're in 20 clinics already, the most one account can join.",
    confirmation_mismatch: "The name typed doesn't match the clinic's name, so nothing was deleted.",
    wrong_code: "That code doesn't match one that's still valid. Check it, or ask the clinic's admin for a new one.",
    too_many_attempts: "Too many wrong codes. Wait an hour, then try again.",
    bad_code: "A join code has 8 letters and numbers, like 7KQ2-M9XD.",
    too_many_codes: "This clinic has 50 unused join codes. Revoke some first.",
    too_many: "Up to 50 addresses at a time.",
    no_emails: "Enter at least one email address.",
    no_code: "That join code isn't there any more.",
    bad_range: "That period can't be shown.",
    clinics_off: "Clinics aren't switched on for this site yet. Write to " + CONTACT + " if you'd like to try them.",
    offline: "We couldn't reach the account service. Check your connection and try again."
  };

  function clinicCode(error) {
    if (typeof error === "string") return error;
    var m = (error && error.message) || "";
    if (m.indexOf("openbark:") === 0) return m.slice(9);
    if (error && (error.code === "PGRST106" || error.code === "PGRST202" || error.code === "42883" || error.code === "3F000")) return "clinics_off";
    if (/schema must be one of|invalid schema/i.test(m)) return "clinics_off";
    if ((error && error.name === "FunctionsFetchError") || /failed to fetch|networkerror|load failed/i.test(m)) return "offline";
    return "";
  }
  function clinicProblem(error, fallback, words) {
    var code = clinicCode(error);
    var text = (words && words[code]) || CLINIC_WORDS[code];
    if (text) return text;
    var m = error && error.message ? ": " + error.message : "";
    return fallback + m + ". Try again, or write to " + CONTACT + ".";
  }
  async function crpc(name, args) {
    var res;
    try { res = await sb.schema("clinic").rpc(name, args || {}); } catch (e) { res = { error: e }; }
    if (res.error) throw res.error;
    return res.data;
  }

  // ---- Dates: days are local calendar days, sent as YYYY-MM-DD.
  function pad(n) { return (n < 10 ? "0" : "") + n; }
  function isoDay(d) { return d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()); }
  function addDays(d, n) { return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n); }
  function parseDay(s) { var p = String(s).split("-"); return new Date(+p[0], +p[1] - 1, +p[2]); }
  function makeFormat(opts) {
    try { var f = new Intl.DateTimeFormat(undefined, opts); return function (d) { return f.format(d); }; } catch (e) { return function (d) { return isoDay(d); }; }
  }
  var shortDay = makeFormat({ day: "numeric", month: "short" });
  var longDay = makeFormat({ day: "numeric", month: "short", year: "numeric" });
  function dayText(s) { return s ? longDay(parseDay(s)) : ""; }
  function timeText(ts) { return ts ? longDay(new Date(ts)) : ""; }
  function range() {
    var today = new Date();
    return { from: isoDay(addDays(today, 1 - rangeDays)), to: isoDay(today) };
  }
  function num(n) { return Number(n || 0).toLocaleString(); }
  function hours(minutes) { return (Math.round(Number(minutes || 0) / 6) / 10).toLocaleString(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 }); }
  function plural(n, one, many) { return num(n) + " " + (Number(n) === 1 ? one : many); }
  function profileName() { var n = $("p-name"); return n ? n.value.trim() : ""; }
  function storeClinic(id) { try { store("localStorage").setItem(CLINIC_KEY, id || ""); } catch (e) { /* fine without */ } }
  function storedClinic() { try { return store("localStorage").getItem(CLINIC_KEY) || null; } catch (e) { return null; } }
  function currentClinic() { return clinics.filter(function (c) { return c.id === current; })[0] || null; }

  // ---- Small builders (textContent only; names come from other people).
  function cell(tag, text, cls) { var c = el(tag, cls, text === undefined || text === null ? "" : String(text)); return c; }
  function numCell(text) { return cell("td", text, "num"); }
  function table(id, heads, rows, empty, foot) {
    var t = $(id);
    t.textContent = "";
    var thead = el("thead");
    var hr = el("tr");
    heads.forEach(function (h) {
      var th = cell("th", h.label || h, h.num ? "num" : null);
      th.scope = "col";
      if (h.sr) { th.textContent = ""; th.appendChild(el("span", "sr-only", h.sr)); }
      hr.appendChild(th);
    });
    thead.appendChild(hr);
    t.appendChild(thead);
    var tbody = el("tbody");
    if (!rows.length) {
      var tr = el("tr");
      var td = cell("td", empty, "cl-empty");
      td.colSpan = heads.length;
      tr.appendChild(td);
      tbody.appendChild(tr);
    }
    rows.forEach(function (r) { tbody.appendChild(r); });
    t.appendChild(tbody);
    if (foot) { var tf = el("tfoot"); tf.appendChild(foot); t.appendChild(tf); }
  }
  function rowOf(first, rest) {
    var tr = el("tr");
    var th = cell("th", first);
    th.scope = "row";
    tr.appendChild(th);
    rest.forEach(function (c) { tr.appendChild(c); });
    return tr;
  }
  function stats(id, items) {
    var dl = $(id);
    dl.textContent = "";
    items.forEach(function (it) {
      var d = el("div");
      d.appendChild(el("dt", null, it[0]));
      d.appendChild(el("dd", null, it[1]));
      dl.appendChild(d);
    });
  }

  // ---- Load and draw.
  function resetClinics() {
    loadSeq++;
    clinics = []; invitesForMe = []; current = null; adminData = null; dash = null;
    clinicArea.hidden = true;
    $("clinic-invited").hidden = true;
    $("clinic-dash").hidden = true;
    $("clinics-card").hidden = false;
    ["create-form", "join-form", "leave-form", "clinic-delete-form", "code-shown"].forEach(function (id) { $(id).hidden = true; });
    ["create-open", "join-open", "leave-open"].forEach(function (id) { $(id).hidden = false; $(id).setAttribute("aria-expanded", "false"); });
    $("invite-results").hidden = true;
    ["clinic-msg", "clinic-invited-msg", "create-msg", "join-msg", "invite-msg", "invites-msg", "members-msg", "code-msg",
      "rename-msg", "leave-msg", "clinic-delete-msg", "mine-name-msg"].forEach(function (id) { say($(id), ""); });
    selectTab("tab-usage", false);
  }

  async function loadClinics(prefer) {
    var seq = ++loadSeq;
    var both;
    try {
      both = await Promise.all([crpc("my_clinics"), crpc("my_invites")]);
    } catch (e) {
      if (seq !== loadSeq) return;
      if (clinicCode(e) === "clinics_off") {
        // The hosted project hasn't got the clinic schema yet: say nothing.
        $("clinics-card").hidden = true;
        clinicArea.hidden = true;
        return;
      }
      say($("create-msg"), clinicProblem(e, "Your clinics couldn't be loaded"), "error");
      return;
    }
    if (seq !== loadSeq) return;
    clinics = both[0] || [];
    invitesForMe = both[1] || [];
    var ids = clinics.map(function (c) { return c.id; });
    var want = prefer || current || storedClinic();
    current = ids.indexOf(want) >= 0 ? want : ids[0] || null;
    if (current) storeClinic(current);
    drawInvitesForMe();
    drawClinic();
  }

  function drawInvitesForMe() {
    var box = $("clinic-invite-list");
    box.textContent = "";
    invitesForMe.forEach(function (inv, i) {
      var li = el("li", "cl-invite");
      var what = el("div", "cl-invite-what");
      what.appendChild(el("h3", null, inv.clinic_name));
      var by = inv.invited_by ? " by " + inv.invited_by : "";
      what.appendChild(el("p", null, (inv.member ? "You're already a member; accepting makes you an admin. " : "Invited as " + (inv.role === "admin" ? "an admin" : "a member") + by + ". ") +
        "Open until " + timeText(inv.expires_at) + "."));
      li.appendChild(what);
      var field = el("div", "field");
      var label = el("label", null, "Your name in this clinic");
      label.htmlFor = "inv-name-" + i;
      var input = el("input", "input");
      input.id = "inv-name-" + i;
      input.type = "text";
      input.maxLength = 80;
      input.autocomplete = "name";
      input.value = profileName();
      field.appendChild(label);
      field.appendChild(input);
      li.appendChild(field);
      var actions = el("div", "pt-actions");
      var accept = el("button", "btn btn-primary", "Accept");
      accept.type = "button";
      var decline = el("button", "btn btn-ghost", "Decline");
      decline.type = "button";
      accept.addEventListener("click", async function () {
        var name = input.value.trim();
        if (!name) { say($("clinic-invited-msg"), "Enter the name this clinic will see.", "error"); input.focus(); return; }
        busy(accept, "Joining…");
        try {
          var got = await crpc("accept_invite", { invite_id: inv.id, display_name: name });
          say($("clinic-invited-msg"), "You joined " + inv.clinic_name + ".", "ok");
          await loadClinics(got && got.clinic_id);
        } catch (e) {
          busy(accept);
          say($("clinic-invited-msg"), clinicProblem(e, "The invite couldn't be accepted"), "error");
        }
      });
      decline.addEventListener("click", async function () {
        busy(decline, "Declining…");
        try {
          await crpc("decline_invite", { invite_id: inv.id });
          say($("clinic-invited-msg"), "Invite declined.", "ok");
          await loadClinics();
        } catch (e) {
          busy(decline);
          say($("clinic-invited-msg"), clinicProblem(e, "The invite couldn't be declined"), "error");
        }
      });
      actions.appendChild(accept);
      actions.appendChild(decline);
      li.appendChild(actions);
      box.appendChild(li);
    });
    $("clinic-invited").hidden = !invitesForMe.length;
  }

  function drawClinic() {
    var c = currentClinic();
    clinicArea.hidden = !c && !invitesForMe.length;
    $("clinic-dash").hidden = !c;
    $("clinics-lede").hidden = !!clinics.length;
    $("create-open").textContent = clinics.length ? "Create another clinic" : "Create a clinic";
    if (!c) return;
    var sw = $("clinic-switch");
    $("clinic-switch-wrap").hidden = clinics.length < 2;
    sw.textContent = "";
    clinics.forEach(function (x) {
      var o = el("option", null, x.name);
      o.value = x.id;
      o.selected = x.id === current;
      sw.appendChild(o);
    });
    var admin = c.role === "admin";
    $("clinic-h").textContent = c.name;
    $("clinic-eyebrow").textContent = "Your clinic";
    $("clinic-sub").textContent = admin
      ? "You're an admin. " + plural(c.members, "member", "members") + "."
      : "You're a member. The clinic's admins see your name, your role and, if you share them, your daily usage counts.";
    say($("clinic-msg"), "");
    $("clinic-admin").hidden = !admin;
    $("clinic-delete-open").hidden = !admin;
    $("mine-name").value = c.display_name || "";
    $("mine-sharing").textContent = c.share_usage
      ? "You're sharing daily usage counts with this clinic from OpenBark."
      : "You're not sharing usage counts with this clinic, so nothing is sent. Sharing will be a switch in OpenBark (Settings, Clinic) in a coming version, off until you turn it on.";
    var leaveText = $("leave-text");
    var leaveSubmit = $("leave-form").querySelector('button[type="submit"]');
    if (c.sole_admin) {
      leaveText.textContent = "You're this clinic's only admin. Make someone else an admin first (Members), or delete the clinic.";
      leaveSubmit.disabled = true;
    } else {
      leaveText.textContent = "You'll no longer be part of this clinic. Counts you already shared stay with it, without your name, until they're 400 days old.";
      leaveSubmit.disabled = false;
    }
    loadMine();
    if (admin) { loadAdmin(); loadDashboard(); }
  }

  async function loadMine() {
    var cid = current;
    var r = range();
    var rows;
    try { rows = await crpc("my_usage", { clinic_id: cid, from_day: r.from, to_day: r.to }); } catch (e) {
      if (cid === current) stats("mine-stats", []);
      return;
    }
    if (cid !== current) return;
    var t = { visits: 0, minutes: 0, drafted: 0, signed: 0, calls: 0 };
    (rows || []).forEach(function (d) { Object.keys(t).forEach(function (k) { t[k] += Number(d[k] || 0); }); });
    $("mine-h").textContent = "Your usage, last " + rangeDays + " days";
    stats("mine-stats", [["Visits recorded", num(t.visits)], ["Hours recorded", hours(t.minutes)], ["Notes drafted", num(t.drafted)],
      ["Notes signed", num(t.signed)], ["Calls recorded", num(t.calls)]]);
  }

  async function loadAdmin() {
    var cid = current;
    try {
      var data = await crpc("clinic_admin", { clinic_id: cid });
      if (cid !== current) return;
      adminData = data;
      drawMembers();
      drawPending();
      drawCodes();
      if (document.activeElement !== $("rename-name")) $("rename-name").value = data.clinic.name;
    } catch (e) {
      if (cid === current) say($("clinic-msg"), clinicProblem(e, "The clinic couldn't be loaded"), "error");
    }
  }

  async function loadDashboard() {
    var cid = current;
    var r = range();
    $("usage-summary").textContent = "Loading the counts…";
    try {
      var data = await crpc("clinic_dashboard", { clinic_id: cid, from_day: r.from, to_day: r.to });
      if (cid !== current) return;
      dash = data;
      drawUsage();
    } catch (e) {
      if (cid !== current) return;
      $("usage-summary").textContent = "";
      say($("clinic-msg"), clinicProblem(e, "The usage counts couldn't be loaded"), "error");
    }
  }

  // ---- The usage tab: summary, tiles, a bar chart with a table beside it,
  // and tables by member, platform and app version.
  function filledDays() {
    var byDay = {};
    (dash.days || []).forEach(function (d) { byDay[d.day] = d; });
    var out = [];
    var end = parseDay(dash.to_day);
    for (var d = parseDay(dash.from_day); d <= end; d = addDays(d, 1)) {
      var key = isoDay(d);
      out.push(byDay[key] || { day: key, visits: 0, minutes: 0, drafted: 0, signed: 0, calls: 0 });
    }
    return out;
  }

  function drawUsage() {
    var t = dash.totals || {};
    var days = filledDays();
    var sharing = Number(dash.members_sharing || 0);
    var total = Number(dash.members_total || 0);
    var period = "In the last " + rangeDays + " days";
    var summary;
    if (!sharing && !Number(t.visits) && !Number(t.devices)) {
      summary = "Nobody in this clinic is sharing usage counts yet. Each person decides: sharing will be a switch in OpenBark (Settings, Clinic) in a coming version, off until they turn it on. Until then this page stays empty.";
    } else {
      summary = period + ", " + num(sharing) + " of " + plural(total, "member", "members") + " shared counts from " + plural(t.devices, "device", "devices") + ": " +
        plural(t.visits, "visit", "visits") + " recorded (" + hours(t.minutes) + " hours), " + num(t.drafted) + " notes drafted and " + num(t.signed) + " signed, and " +
        plural(t.calls, "call", "calls") + " recorded.";
    }
    $("usage-summary").textContent = summary;
    stats("usage-stats", [["Visits recorded", num(t.visits)], ["Hours recorded", hours(t.minutes)], ["Notes drafted", num(t.drafted)],
      ["Notes signed", num(t.signed)], ["Calls recorded", num(t.calls)]]);
    drawChart(days);
    table("usage-days", ["Day", { label: "Visits", num: true }, { label: "Hours", num: true }, { label: "Notes drafted", num: true },
      { label: "Notes signed", num: true }, { label: "Calls", num: true }],
      days.slice().reverse().map(function (d) {
        return rowOf(dayText(d.day), [numCell(num(d.visits)), numCell(hours(d.minutes)), numCell(num(d.drafted)), numCell(num(d.signed)), numCell(num(d.calls))]);
      }), "No days in this period.");
    var members = (dash.members || []).map(function (m) {
      var name = m.former ? "Former members" + (m.people > 1 ? " (" + m.people + ")" : "") : m.name + (m.role === "admin" ? " (admin)" : "");
      if (!m.former && !m.share_usage && !Number(m.days_active)) {
        var td = cell("td", "Not sharing", "cl-muted");
        td.colSpan = 6;
        return rowOf(name, [td]);
      }
      return rowOf(name, [numCell(num(m.visits)), numCell(hours(m.minutes)), numCell(num(m.drafted)), numCell(num(m.signed)), numCell(num(m.calls)),
        cell("td", m.last_active ? dayText(m.last_active) : "No counts yet")]);
    });
    var foot = rowOf("All members", [numCell(num(t.visits)), numCell(hours(t.minutes)), numCell(num(t.drafted)), numCell(num(t.signed)), numCell(num(t.calls)), cell("td", "")]);
    table("usage-members", ["Member", { label: "Visits", num: true }, { label: "Hours", num: true }, { label: "Notes drafted", num: true },
      { label: "Notes signed", num: true }, { label: "Calls", num: true }, "Last active"], members, "No members.", foot);
    table("usage-platforms", ["Platform", { label: "Devices", num: true }, { label: "Visits", num: true }, { label: "Hours", num: true }],
      (dash.platforms || []).map(function (p) {
        return rowOf(PLATFORM_NAMES[p.platform] || p.platform, [numCell(num(p.devices)), numCell(num(p.visits)), numCell(hours(p.minutes))]);
      }), "No devices have shared counts in this period.");
    table("usage-versions", ["Version", { label: "Devices", num: true }],
      (dash.versions || []).map(function (v) { return rowOf(v.app_version, [numCell(num(v.devices))]); }),
      "No devices have shared counts in this period.");
  }

  function drawChart(days) {
    var svg = $("usage-svg");
    var NS = "http://www.w3.org/2000/svg";
    var max = days.reduce(function (m, d) { return Math.max(m, Number(d.visits || 0)); }, 0);
    var step = 10, bar = days.length > 45 ? 8 : 7, H = 100;
    while (svg.lastChild && svg.lastChild.nodeName.toLowerCase() !== "title") svg.removeChild(svg.lastChild);
    svg.setAttribute("viewBox", "0 0 " + days.length * step + " " + H);
    days.forEach(function (d, i) {
      var v = Number(d.visits || 0);
      var h = max ? Math.max(v / max * H, v ? 1.5 : 0) : 0;
      var r = document.createElementNS(NS, "rect");
      r.setAttribute("x", String(i * step + (step - bar) / 2));
      r.setAttribute("width", String(bar));
      if (v) {
        r.setAttribute("y", String(H - h));
        r.setAttribute("height", String(h));
      } else {
        r.setAttribute("y", String(H - 1));
        r.setAttribute("height", "1");
        r.setAttribute("class", "cl-zero");
      }
      var title = document.createElementNS(NS, "title");
      title.textContent = shortDay(parseDay(d.day)) + ": " + plural(v, "visit", "visits");
      r.appendChild(title);
      svg.appendChild(r);
    });
    var busiest = days.reduce(function (b, d) { return Number(d.visits) > Number(b.visits) ? d : b; }, days[0] || { visits: 0 });
    var total = days.reduce(function (s, d) { return s + Number(d.visits || 0); }, 0);
    $("usage-svg-label").textContent = "Visits recorded per day, " + dayText(dash.from_day) + " to " + dayText(dash.to_day) + ": " +
      plural(total, "visit", "visits") + (max ? ", the most on " + dayText(busiest.day) + " (" + num(max) + ")" : "") + ". Each day is in the table below.";
    $("usage-chart-max").textContent = max ? num(max) + " a day" : "";
    $("usage-chart-from").textContent = days.length ? shortDay(parseDay(days[0].day)) : "";
    $("usage-chart-to").textContent = days.length ? shortDay(parseDay(days[days.length - 1].day)) : "";
  }

  // ---- The members tab.
  function drawMembers() {
    var me = adminData.me;
    var rows = (adminData.members || []).map(function (m) {
      var you = m.user_id === me;
      var share = el("span", "status", m.share_usage ? "Sharing" : "Not sharing");
      if (m.share_usage) share.setAttribute("data-s", "ok");
      var shareCell = el("td");
      shareCell.appendChild(share);
      var actions = el("td");
      var box = el("div", "cl-actions");
      if (!you) {
        var role = el("button", "btn btn-ghost", m.role === "admin" ? "Make member" : "Make admin");
        role.type = "button";
        role.addEventListener("click", async function () {
          busy(role, "Changing…");
          try {
            await crpc("set_member_role", { clinic_id: current, user_id: m.user_id, role: m.role === "admin" ? "member" : "admin" });
            say($("members-msg"), m.name + " is now " + (m.role === "admin" ? "a member" : "an admin") + ".", "ok");
            await loadClinics(current);
          } catch (e) {
            busy(role);
            say($("members-msg"), clinicProblem(e, "The role couldn't be changed"), "error");
          }
        });
        var remove = el("button", "btn btn-danger", "Remove");
        remove.type = "button";
        remove.addEventListener("click", function () { confirmRemove(box, m); });
        box.appendChild(role);
        box.appendChild(remove);
      }
      actions.appendChild(box);
      return rowOf(m.name + (you ? " (you)" : ""), [
        cell("td", m.role === "admin" ? "Admin" : "Member"),
        cell("td", timeText(m.joined_at)),
        cell("td", m.last_active ? dayText(m.last_active) : (m.share_usage ? "No counts yet" : "Not sharing")),
        shareCell,
        numCell(m.share_usage || m.devices ? num(m.devices) : ""),
        cell("td", (m.app_versions || []).join(", ")),
        actions
      ]);
    });
    table("members-table", ["Name", "Role", "Joined", "Last active", "Sharing", { label: "Devices", num: true }, "App versions", { sr: "Actions" }],
      rows, "No members.");
  }
  function confirmRemove(box, m) {
    box.textContent = "";
    var yes = el("button", "btn btn-danger", "Remove " + m.name);
    yes.type = "button";
    var no = el("button", "btn btn-ghost", "Cancel");
    no.type = "button";
    yes.addEventListener("click", async function () {
      busy(yes, "Removing…");
      try {
        await crpc("remove_member", { clinic_id: current, user_id: m.user_id });
        say($("members-msg"), m.name + " was removed. Counts they shared stay, as a former member without a name.", "ok");
        await loadClinics(current);
      } catch (e) {
        busy(yes);
        say($("members-msg"), clinicProblem(e, "They couldn't be removed"), "error");
      }
    });
    no.addEventListener("click", function () { drawMembers(); });
    box.appendChild(yes);
    box.appendChild(no);
    yes.focus();
  }

  // ---- The invite tab: send, pending, join codes.
  var DELIVERY = {
    emailed: "Invitation emailed",
    sign_in: "Has an account: they'll see the invite when they sign in here",
    saved: "They'll see the invite when they sign in here (invite emails aren't switched on yet)",
    email_failed: "Invite saved, but the email couldn't be sent. They'll see it if they sign in here; try Resend later",
    member: "Already a member",
    invalid: "Not an email address"
  };
  async function sendInvites(emails, role) {
    var res;
    try { res = await sb.functions.invoke("clinic-invite", { body: { clinic_id: current, emails: emails, role: role } }); } catch (e) { res = { error: e }; }
    if (!res.error) return (res.data && res.data.results) || [];
    var status = res.error.context && res.error.context.status;
    var code = "";
    if (res.error.context && typeof res.error.context.json === "function") {
      try { code = ((await res.error.context.json()) || {}).error || ""; } catch (e) { /* not JSON */ }
    }
    if (status === 404 && !code) {
      // The function isn't deployed: record the invites without emails.
      var rows = await crpc("invite_members", { clinic_id: current, emails: emails, role: role });
      return (rows || []).map(function (r) {
        r.delivery = r.status === "invited" || r.status === "renewed" ? "saved" : "none";
        return r;
      });
    }
    throw code || res.error;
  }
  function deliveryText(r) {
    if (r.status === "member" || r.status === "invalid") return DELIVERY[r.status];
    return (DELIVERY[r.delivery] || "Invited") + (r.status === "renewed" ? ", renewed for 14 days" : "");
  }
  $("invite-form").addEventListener("submit", async function (ev) {
    ev.preventDefault();
    var button = this.querySelector('button[type="submit"]');
    var seen = {};
    var emails = $("invite-emails").value.split(/[\s,;]+/).map(function (s) { return s.trim(); }).filter(function (s) {
      var k = s.toLowerCase();
      if (!s || seen[k]) return false;
      seen[k] = true;
      return true;
    });
    var roleInput = document.querySelector('input[name="invite-role"]:checked');
    var role = roleInput ? roleInput.value : "member";
    if (!emails.length) { say($("invite-msg"), CLINIC_WORDS.no_emails, "error"); $("invite-emails").focus(); return; }
    if (emails.length > 50) { say($("invite-msg"), CLINIC_WORDS.too_many, "error"); return; }
    say($("invite-msg"), "");
    busy(button, "Sending…");
    var results;
    try {
      results = await sendInvites(emails, role);
    } catch (e) {
      busy(button);
      say($("invite-msg"), clinicProblem(e, "The invites couldn't be sent"), "error");
      return;
    }
    busy(button);
    var list = $("invite-results");
    list.textContent = "";
    results.forEach(function (r) {
      var li = el("li");
      li.appendChild(el("span", "cl-addr", r.email));
      li.appendChild(el("span", "cl-what", deliveryText(r)));
      list.appendChild(li);
    });
    list.hidden = !results.length;
    var sent = results.filter(function (r) { return r.status === "invited" || r.status === "renewed"; }).length;
    say($("invite-msg"), sent ? "Invited " + plural(sent, "person", "people") + "." : "Nobody new was invited.", sent ? "ok" : null);
    if (sent) $("invite-emails").value = results.filter(function (r) { return r.status === "invalid"; }).map(function (r) { return r.email; }).join("\n");
    loadAdmin();
  });

  function drawPending() {
    var rows = (adminData.invites || []).map(function (inv) {
      var actions = el("td");
      var box = el("div", "cl-actions");
      var resend = el("button", "btn btn-ghost", "Resend");
      resend.type = "button";
      resend.addEventListener("click", async function () {
        busy(resend, "Sending…");
        try {
          var r = (await sendInvites([inv.email], inv.role))[0];
          say($("invites-msg"), inv.email + ": " + (r ? deliveryText(r) : "sent") + ".", "ok");
          loadAdmin();
        } catch (e) {
          busy(resend);
          say($("invites-msg"), clinicProblem(e, "The invite couldn't be sent again"), "error");
        }
      });
      var revoke = el("button", "btn btn-danger", "Revoke");
      revoke.type = "button";
      revoke.addEventListener("click", async function () {
        busy(revoke, "Revoking…");
        try {
          await crpc("revoke_invite", { invite_id: inv.id });
          say($("invites-msg"), "The invite to " + inv.email + " was revoked.", "ok");
          loadAdmin();
        } catch (e) {
          busy(revoke);
          say($("invites-msg"), clinicProblem(e, "The invite couldn't be revoked"), "error");
        }
      });
      box.appendChild(resend);
      box.appendChild(revoke);
      actions.appendChild(box);
      return rowOf(inv.email, [cell("td", inv.role === "admin" ? "Admin" : "Member"), cell("td", timeText(inv.last_sent_at)),
        cell("td", inv.expired ? "Expired " + timeText(inv.expires_at) : timeText(inv.expires_at), inv.expired ? "cl-muted" : null), actions]);
    });
    table("invites-table", ["Email", "Role", "Sent", "Expires", { sr: "Actions" }], rows, "No invites are waiting.");
  }

  function drawCodes() {
    var rows = (adminData.codes || []).map(function (c) {
      var actions = el("td");
      var box = el("div", "cl-actions");
      var revoke = el("button", "btn btn-danger", "Revoke");
      revoke.type = "button";
      revoke.addEventListener("click", async function () {
        busy(revoke, "Revoking…");
        try {
          await crpc("revoke_join_code", { code_id: c.id });
          say($("code-msg"), "The code was revoked.", "ok");
          loadAdmin();
        } catch (e) {
          busy(revoke);
          say($("code-msg"), clinicProblem(e, "The code couldn't be revoked"), "error");
        }
      });
      box.appendChild(revoke);
      actions.appendChild(box);
      return rowOf("Made " + timeText(c.created_at), [cell("td", "Expires " + timeText(c.expires_at)), actions]);
    });
    table("codes-table", ["Unused code", "Expires", { sr: "Actions" }], rows, "No unused join codes.");
  }
  $("code-make").addEventListener("click", async function () {
    var button = this;
    busy(button, "Making a code…");
    say($("code-msg"), "");
    try {
      var c = await crpc("create_join_code", { clinic_id: current });
      $("code-value").textContent = c.code;
      $("code-expires").textContent = timeText(c.expires_at);
      $("code-shown").hidden = false;
      $("code-copy").textContent = "Copy";
      loadAdmin();
    } catch (e) {
      say($("code-msg"), clinicProblem(e, "A code couldn't be made"), "error");
    }
    busy(button);
  });
  $("code-copy").addEventListener("click", function () {
    var text = $("code-value").textContent;
    var done = function () { $("code-copy").textContent = "Copied"; };
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, function () { /* select it instead */ });
  });

  // ---- Clinic settings, and the member's own name, leave and delete.
  $("rename-form").addEventListener("submit", async function (ev) {
    ev.preventDefault();
    var button = this.querySelector('button[type="submit"]');
    busy(button, "Renaming…");
    try {
      var r = await crpc("rename_clinic", { clinic_id: current, name: $("rename-name").value });
      busy(button);
      say($("rename-msg"), "Renamed", "ok");
      clinics.forEach(function (c) { if (c.id === current) c.name = r.name; });
      $("clinic-h").textContent = r.name;
      drawClinicSwitch();
    } catch (e) {
      busy(button);
      say($("rename-msg"), clinicProblem(e, "The clinic couldn't be renamed"), "error");
    }
  });
  function drawClinicSwitch() {
    [].forEach.call($("clinic-switch").options, function (o) {
      var c = clinics.filter(function (x) { return x.id === o.value; })[0];
      if (c) o.textContent = c.name;
    });
  }
  $("mine-name-form").addEventListener("submit", async function (ev) {
    ev.preventDefault();
    var button = this.querySelector('button[type="submit"]');
    busy(button, "Saving…");
    try {
      var r = await crpc("set_display_name", { clinic_id: current, display_name: $("mine-name").value });
      busy(button);
      clinics.forEach(function (c) { if (c.id === current) c.display_name = r.display_name; });
      $("mine-name").value = r.display_name;
      say($("mine-name-msg"), "Saved", "ok");
      if (adminData) loadAdmin();
    } catch (e) {
      busy(button);
      say($("mine-name-msg"), clinicProblem(e, "Your name couldn't be saved"), "error");
    }
  });
  function toggle(openId, formId, show) {
    $(formId).hidden = !show;
    $(openId).hidden = show;
    $(openId).setAttribute("aria-expanded", show ? "true" : "false");
  }
  $("leave-open").addEventListener("click", function () { toggle("leave-open", "leave-form", true); say($("leave-msg"), ""); });
  $("leave-cancel").addEventListener("click", function () { toggle("leave-open", "leave-form", false); $("leave-open").focus(); });
  $("leave-form").addEventListener("submit", async function (ev) {
    ev.preventDefault();
    var button = this.querySelector('button[type="submit"]');
    var name = (currentClinic() || {}).name || "the clinic";
    busy(button, "Leaving…");
    try {
      await crpc("leave_clinic", { clinic_id: current });
      busy(button);
      toggle("leave-open", "leave-form", false);
      current = null;
      storeClinic("");
      await loadClinics();
      say($("create-msg"), "You left " + name + ".", "ok");
    } catch (e) {
      busy(button);
      say($("leave-msg"), clinicProblem(e, "You couldn't leave the clinic"), "error");
    }
  });
  $("clinic-delete-open").addEventListener("click", function () {
    toggle("clinic-delete-open", "clinic-delete-form", true);
    $("clinic-delete-name").value = "";
    $("clinic-delete-submit").disabled = true;
    $("clinic-delete-name").focus();
  });
  $("clinic-delete-cancel").addEventListener("click", function () { toggle("clinic-delete-open", "clinic-delete-form", false); $("clinic-delete-open").focus(); });
  $("clinic-delete-name").addEventListener("input", function () {
    var c = currentClinic();
    $("clinic-delete-submit").disabled = !c || this.value.trim().toLowerCase() !== c.name.toLowerCase();
  });
  $("clinic-delete-form").addEventListener("submit", async function (ev) {
    ev.preventDefault();
    var button = $("clinic-delete-submit");
    if (button.disabled) return;
    var name = (currentClinic() || {}).name || "The clinic";
    busy(button, "Deleting…");
    try {
      await crpc("delete_clinic", { clinic_id: current, confirm_name: $("clinic-delete-name").value });
      busy(button);
      toggle("clinic-delete-open", "clinic-delete-form", false);
      current = null;
      storeClinic("");
      await loadClinics();
      say($("create-msg"), name + " was deleted.", "ok");
    } catch (e) {
      busy(button);
      say($("clinic-delete-msg"), clinicProblem(e, "The clinic couldn't be deleted"), "error");
    }
  });

  // ---- Create a clinic, or join one with a code.
  $("create-open").addEventListener("click", function () {
    toggle("create-open", "create-form", true);
    $("join-form").hidden = true;
    $("join-open").hidden = false;
    if (!$("create-name").value) $("create-name").value = ($("p-clinic") && $("p-clinic").value.trim()) || "";
    if (!$("create-you").value) $("create-you").value = profileName();
    say($("create-msg"), "");
    $("create-name").focus();
  });
  $("create-cancel").addEventListener("click", function () { toggle("create-open", "create-form", false); $("create-open").focus(); });
  $("create-form").addEventListener("submit", async function (ev) {
    ev.preventDefault();
    var button = this.querySelector('button[type="submit"]');
    busy(button, "Creating…");
    try {
      var c = await crpc("create_clinic", { name: $("create-name").value, display_name: $("create-you").value });
      busy(button);
      toggle("create-open", "create-form", false);
      $("create-name").value = "";
      await loadClinics(c.id);
      selectTab("tab-invite", false);
      say($("create-msg"), c.name + " is ready. Invite your team from its page.", "ok");
      $("clinic-h").focus();
    } catch (e) {
      busy(button);
      say($("create-msg"), clinicProblem(e, "The clinic couldn't be created", { rate_limited: "You've created 5 clinics today. Try again tomorrow." }), "error");
    }
  });
  $("join-open").addEventListener("click", function () {
    toggle("join-open", "join-form", true);
    $("create-form").hidden = true;
    $("create-open").hidden = false;
    if (!$("join-you").value) $("join-you").value = profileName();
    say($("join-msg"), "");
    $("join-code").focus();
  });
  $("join-cancel").addEventListener("click", function () { toggle("join-open", "join-form", false); $("join-open").focus(); });
  $("join-form").addEventListener("submit", async function (ev) {
    ev.preventDefault();
    var button = this.querySelector('button[type="submit"]');
    busy(button, "Joining…");
    try {
      var r = await crpc("join_with_code", { code: $("join-code").value, display_name: $("join-you").value });
      busy(button);
      if (r && r.error) {
        var left = r.attempts_left;
        say($("join-msg"), CLINIC_WORDS[r.error] + (typeof left === "number" && left < 5 ? " " + plural(left, "try", "tries") + " left this hour." : ""), "error");
        return;
      }
      toggle("join-open", "join-form", false);
      $("join-code").value = "";
      await loadClinics(r.clinic_id);
      say($("create-msg"), "You joined " + r.name + ".", "ok");
    } catch (e) {
      busy(button);
      say($("join-msg"), clinicProblem(e, "You couldn't join"), "error");
    }
  });

  // ---- Switching clinic, the period and the tabs.
  $("clinic-switch").addEventListener("change", function () {
    current = this.value;
    storeClinic(current);
    adminData = null; dash = null;
    $("code-shown").hidden = true;
    $("invite-results").hidden = true;
    drawClinic();
  });
  document.querySelectorAll('input[name="clinic-range"]').forEach(function (r) {
    r.addEventListener("change", function () {
      rangeDays = Number(r.value) || 30;
      loadDashboard();
      loadMine();
    });
  });
  var TABS = ["tab-usage", "tab-members", "tab-invite", "tab-settings"];
  function selectTab(id, focus) {
    TABS.forEach(function (t) {
      var on = t === id;
      var tab = $(t);
      tab.setAttribute("aria-selected", on ? "true" : "false");
      tab.tabIndex = on ? 0 : -1;
      $(tab.getAttribute("aria-controls")).hidden = !on;
      if (on && focus) tab.focus();
    });
  }
  TABS.forEach(function (t, i) {
    $(t).addEventListener("click", function () { selectTab(t, false); });
    $(t).addEventListener("keydown", function (ev) {
      var next = { ArrowRight: i + 1, ArrowLeft: i - 1, Home: 0, End: TABS.length - 1 }[ev.key];
      if (next === undefined) return;
      ev.preventDefault();
      selectTab(TABS[(next + TABS.length) % TABS.length], true);
    });
  });

  // ---- CSV of what the usage tab shows (counts and names only).
  function csvCell(v) {
    var s = v === null || v === undefined ? "" : String(v);
    if (/^[=+\-@\t\r]/.test(s)) s = "'" + s; // never a formula in a spreadsheet
    return /[",\r\n]/.test(s) ? '"' + s.replace(/"/g, '""') + '"' : s;
  }
  $("usage-csv").addEventListener("click", function () {
    if (!dash) return;
    var lines = [];
    var add = function (cells) { lines.push(cells.map(csvCell).join(",")); };
    add(["OpenBark usage counts", dash.clinic.name]);
    add(["From", dash.from_day, "To", dash.to_day]);
    add([]);
    add(["Day", "Visits recorded", "Minutes recorded", "Notes drafted", "Notes signed", "Calls recorded"]);
    filledDays().forEach(function (d) { add([d.day, d.visits, d.minutes, d.drafted, d.signed, d.calls]); });
    add([]);
    add(["Member", "Role", "Sharing", "Visits recorded", "Minutes recorded", "Notes drafted", "Notes signed", "Calls recorded", "Days active", "Last active"]);
    (dash.members || []).forEach(function (m) {
      add([m.former ? "Former members" : m.name, m.former ? "" : m.role, m.former ? "" : (m.share_usage ? "yes" : "no"), m.visits, m.minutes, m.drafted,
        m.signed, m.calls, m.days_active, m.last_active || ""]);
    });
    add([]);
    add(["Platform", "Devices", "Visits recorded", "Minutes recorded"]);
    (dash.platforms || []).forEach(function (p) { add([PLATFORM_NAMES[p.platform] || p.platform, p.devices, p.visits, p.minutes]); });
    add([]);
    add(["App version", "Devices"]);
    (dash.versions || []).forEach(function (v) { add([v.app_version, v.devices]); });
    var blob = new Blob([lines.join("\r\n") + "\r\n"], { type: "text/csv;charset=utf-8" });
    var a = document.createElement("a");
    var slug = String(dash.clinic.name).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "clinic";
    a.href = URL.createObjectURL(blob);
    a.download = "openbark-usage-" + slug + "-" + dash.from_day + "-to-" + dash.to_day + ".csv";
    document.body.appendChild(a);
    a.click();
    setTimeout(function () { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
  });

  // Before deleting the account: name any clinic it is the only admin of.
  function soleAdminWarning() {
    var box = $("delete-clinic-warn");
    var sole = clinics.filter(function (c) { return c.sole_admin; });
    box.hidden = !sole.length;
    if (!sole.length) return;
    var names = sole.map(function (c) { return c.name + " (" + plural(c.members, "member", "members") + ")"; }).join(", ");
    box.textContent = "You're the only admin of " + names + ". Deleting your account deletes " + (sole.length > 1 ? "those clinics" : "that clinic") +
      " for everyone in " + (sole.length > 1 ? "them" : "it") + ", with their usage counts. To keep " + (sole.length > 1 ? "them" : "it") +
      ", make someone else an admin first.";
  }

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
    soleAdminWarning();
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
