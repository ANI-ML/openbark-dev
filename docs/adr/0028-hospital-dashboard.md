# ADR-0028: Hospital dashboard: clinics, invites and usage counts

- Status: accepted (phase 1 built 2026-10-24 on `feat/clinic-dashboard`; phase 2 proposed)
- Date: 2026-10-24
- Deciders: product owner (asked on 2026-10-24: "Add a hospital dashboard so they can monitor dashboard of usage, or send out invites across the hospital however you see fit architecturally"), engineering
- Relations:
  - Builds on ADR-0017: the same Supabase project, the same accounts and the same emailed 6-digit sign-in code (no passwords). It adds a `clinic` schema beside `sync`; it reads nothing in `sync` and adds nothing to it.
  - Keeps ADR-0005 and ADR-0017's promise: no clinical data reaches the cloud through this feature, not even encrypted. Only daily counts.
  - Not the Clinic Hub (M4, LAN). An optional LAN-only dashboard on the Hub is listed under Later.

## Context

A practice manager or medical director wants to bring the whole hospital onto OpenBark and see that it is being used: who has joined, on which devices and versions, and roughly how much it is used. Today every install is on its own. Accounts exist only for the website portal and for encrypted sync, and each account is one person.

What can't change:
- **Local-first.** Recordings, transcripts, notes and patient details stay on the clinic's devices (ADR-0005, ADR-0013, ADR-0017). A dashboard that needed visit details in the cloud would break that, so it shows counts and nothing else.
- **Every network call is listed** in `docs/network.md`, off by default where that makes sense, and made from Rust in the app (no Supabase SDK in the app).
- **Account deletion is never blocked** (App Store guideline 5.1.1(v), and the portal's own promise). The app's "Delete my sync account" calls the same `delete-account` Edge Function, so whatever the clinic schema does must keep that call working.

## Decision

### 1. A clinic is an organisation of accounts

A clinic lives in the same Supabase project as the website portal and encrypted sync (`nmecovtmijnfnjuadicp` today). People sign in on the portal with the existing emailed code. A person may belong to several clinics (relief vets do); the limit is 20 per account, and an account may create 5 clinics a day.

Schema `clinic`, exposed to the REST API like `sync` (`supabase/config.toml` locally; Project Settings > Data API on the hosted project), not `public`. The portal's profile table sits in `public` because it is one table with plain row level security; the clinic tables are reached mostly through functions, as sync's are, and a schema of their own keeps their privileges in one place (`revoke all ... in schema clinic`) and keeps them out of the default `public` grants Supabase adds to new tables.

| Table | What it holds |
|---|---|
| `clinics` | id, name (1 to 120 characters, no control characters), who created it (set to null when that account goes), today's invite count |
| `members` | clinic, account, role (`admin` or `member`), display name (1 to 80 characters), `share_usage` (off by default), joined |
| `invites` | clinic, the invitee's email (lower-cased), role, who sent it, how many times, expiry (14 days) |
| `join_codes` | clinic, SHA-256 of the code, who made it, expiry (14 days) |
| `code_attempts` | wrong join codes per account in the last hour |
| `usage_daily` | clinic, account, device, day, platform, app version, five counts (section 5) |

### 2. Roles

- **admin**: a practice manager or medical director. Invites and removes people, changes roles, makes join codes, renames or deletes the clinic, and sees the dashboard.
- **member**: sees their clinic, their own counts and their sharing choice, and can leave.

Whoever creates a clinic is its first admin. **A clinic always has at least one admin**: the functions refuse to remove, demote or let leave the last admin (`last_admin`), and a trigger refuses any role change that would leave none. An admin who wants to go hands over first (makes someone else an admin), or deletes the clinic (typing its name to confirm).

### 3. Invites by email, and join codes

**By email.** An admin types one or more addresses (up to 50 at a time) and a role. Each becomes one invite row keyed by the lower-cased address, valid for 14 days. Inviting a pending address again renews it (14 more days); that is how "Resend" works. Admins can revoke a pending invite. Accepting, declining or revoking deletes the row; an expired row is deleted 30 days after it expired.

The portal calls the **`clinic-invite` Edge Function** with the admin's token. The function asks Supabase Auth who the token belongs to, then calls `clinic.invite_members_for` with the service role. That SQL function checks that the person is an admin of that clinic (on the server, never trusting the browser), applies the daily limit and records the invites. Then, for each address:

| The address | What happens | The portal tells the admin |
|---|---|---|
| Has no account yet, or was invited before and never signed in | The function calls Supabase Auth's `inviteUserByEmail`, which creates an unconfirmed account and sends the **Invite user** email (`supabase/templates/clinic-invite.html`) | "Invitation emailed" |
| Already has an account | No email; the invite is waiting at their next portal sign-in | "They'll see it when they sign in" |
| Already a member | Nothing | "Already a member" |
| Doesn't look like an address | Nothing stored | "Not an email address" |

The invite email carries **no sign-in link and no token**: it points at the account page, where the person signs in with an emailed code like everyone else. It doesn't name the clinic, so the clinic's name is never written into the new account's metadata.

The invitee **accepts on the portal**. After signing in, the portal lists invites whose address matches the account's email **as confirmed by Supabase Auth** (`auth.users.email`, `email_confirmed_at` set), read in a security-definer function, never from a token claim. The person chooses the display name the clinic will see (the portal suggests the name in their profile) and accepts or declines. This relies on the hosted project keeping **Confirm email** on (it is; ADR-0017): an account whose address was never confirmed sees no invites.

**Join codes.** An admin can make a one-off join code for someone who will join from the app later (phase 2), or who prefers not to give an email address in advance: 8 characters of Crockford base32 (40 random bits, from `gen_random_uuid()`, so no extension is needed), shown once as `XXXX-XXXX`, stored only as its SHA-256, valid for 14 days, used once, member role only. Typing is forgiving: lower case, spaces and hyphens are fine, and I, L and O read as 1, 1 and 0. An account may get 10 wrong codes an hour; after that even the right code is refused until the hour is up. A clinic may have 50 live codes.

**Limits.** Invites, renewals and join codes together: 50 per clinic per day (UTC), counted on the clinic row so revoking and inviting again doesn't reset it. Supabase Auth's own email limits apply on top (Authentication > Rate Limits).

**Known limits.**
- People who already have an account get no email in phase 1. Supabase Auth can only email an existing account a sign-in code, and a code nobody asked for would confuse more than it helps. The portal tells the admin so they can let the person know. Sending a plain "you're invited" email to existing accounts needs an email sender of our own (SendGrid's API, not Supabase's SMTP) and is left for later.
- The admin learns whether an address already has an OpenBark account (from the delivery column above). Only clinic admins see it, within the daily limit; the direct database call (`invite_members`) leaves it out.

### 4. Removing members, leaving, and deleting accounts

- **Removing a member or leaving** ends the membership. That person's past usage rows stay with the clinic, and the dashboard sums them into one **"Former member"** row with no name and no account id. They age out after 400 days like every row. They can still read their own past rows (row level security), and they disappear at once if the person deletes their account.
- **Deleting an account** (the existing `delete-account` Edge Function, from the portal or the app) removes the person's memberships and every one of their usage rows, through `on delete cascade` on `auth.users`. Invites they sent stay, with no sender. Join codes they made stay. Nothing about them remains.
- **If they were the last admin of a clinic, the clinic is deleted too**, with its invites, codes and every member's usage rows (a trigger after a membership row is deleted). The alternative, refusing the deletion until ownership is handed over, was rejected: an account deletion must never be blocked, and the app's "Delete my sync account" calls the same function and couldn't explain the refusal. The portal **warns before deleting the account**: "You're the only admin of <clinic>; deleting your account deletes the clinic for its N members. Make someone else an admin first to keep it."

### 5. Usage counts, not clinical data

Phase 2 sends them from the app; phase 1 builds the table, the function and the dashboard.

`clinic.usage_daily`: one row per clinic, member, device and day.

| Column | Type and bounds |
|---|---|
| `clinic_id`, `user_id` | references to the clinic and the account (never to the membership, so removing a member keeps their rows) |
| `device_id` | a random id the app makes for this clinic on this install, not the sync device id, so the two can't be joined |
| `day` | a date. No time of day exists anywhere in the table except `updated_at`, which admins never see |
| `platform` | `windows`, `macos`, `linux`, `ios` or `web` |
| `app_version` | a version number, `^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$`, 40 characters at most |
| `visits_recorded` | 0 to 1000 |
| `minutes_recorded` | 0 to 1440, one decimal |
| `notes_drafted` | 0 to 5000 |
| `notes_signed` | 0 to 1000 |
| `calls_recorded` | 0 to 1000 |

**What never goes there, and can't:** patient or owner names, species, reasons for visits, transcripts, notes, templates, recordings or any audio, phone numbers, the time of any single visit, and anything else free-form. The schema enforces it: every column is typed and bounded, and the only free text in the whole `clinic` schema is a clinic name, a member's display name and the email address on a pending invite. The end-to-end test reads the column list from the database and fails if anything else appears.

**`report_usage(clinic_id, day, device_id, platform, app_version, visits_recorded, minutes_recorded, notes_drafted, notes_signed, calls_recorded)`**:
- writes only the caller's own row (there is no user parameter), in a clinic they belong to, and only while their `share_usage` is on;
- **replaces** that device's totals for that day (an upsert), so the app sends the day's running totals and a retry is harmless;
- accepts a day from 7 days ago to tomorrow in UTC (UTC + 1 day, because clinics east of UTC start their day before UTC does); later is `future_day`, earlier `too_old`;
- refuses a count out of bounds, an unknown platform or a malformed version before the table's own checks would;
- allows 10 devices per person per clinic in 30 days.

**Sharing is the member's choice, off by default.** `members.share_usage` starts off. Only the person can turn it on (`set_usage_sharing`), and `report_usage` refuses while it's off, so the server enforces the opt-in rather than trusting the app. The dashboard shows "Not sharing" for anyone who hasn't turned it on. Turning it off stops new counts; rows already sent stay until they age out, the person deletes their account or the clinic is deleted.

**Who reads what.**
- A member reads their own rows (row level security, and `my_usage`).
- Admins read the clinic's figures only through two functions:
  - `clinic_admin(clinic_id)`: members (display name, role, joined, sharing, last day with counts, devices, platforms and app versions in the last 30 days), pending invites and live join codes. **Members' email addresses are never returned.**
  - `clinic_dashboard(clinic_id, from_day, to_day)`, at most 400 days: per-day clinic totals; per-member totals for the period, with former members summed into one row; devices and visits per platform; devices per app version (each device's latest).
- Admins see a person's totals for the period, not that person's day-by-day timeline. CSV export holds the same figures.
- Nobody reads `invites`, `join_codes` or `code_attempts` directly, and the anon role gets nothing from the schema at all.

**Retention.** `clinic.purge()` deletes usage rows older than 400 days (a year and a month, so a year-on-year comparison works), invites 30 days past expiry, expired codes and stale attempt counters. pg_cron runs it daily as `openbark-clinic-purge` where pg_cron is available, guarded the same way as the sync purge.

### 6. Network

- **The website** (the visitor's browser) makes the phase 1 calls, with the supabase-js the portal already loads: the `clinic` functions over `/rest/v1/rpc/` and `/functions/v1/clinic-invite`. They join row W2 in `docs/network.md`.
- **The app makes none in phase 1.** Phase 2 adds **R15** (planned in `docs/network.md`): the app calls `/rest/v1/rpc/report_usage` once a day and when it closes, only after the person has joined a clinic **and** turned on "Share usage counts with <clinic>", off by default, sending the counts in section 5 and nothing else.

### 7. Phase 2 (proposed, not built)

- **Settings > Clinic** in the app: join with a code or by signing in (the app already signs in for encrypted sync, R12), see the clinic's name and your role, and **Leave clinic**.
- An explicit switch, **"Share usage counts with <clinic>"**, off until the person turns it on; it calls `set_usage_sharing` and starts the reporter. The settings page lists exactly what is sent (section 5) and shows the last report.
- **The reporter in Rust**: in `os-sync` or a small new crate behind the allow-listed HTTP client (`scripts/ci/check-network.sh`), counting from the local visit list (visits recorded, minutes of audio, notes drafted and signed, calls recorded) per local day, sending the last 7 days' totals once a day. A guard like `os_sync::guard` refuses any body with a field other than the count columns.
- Joining from the app needs the account's sign-in only; it doesn't need encrypted sync turned on.

### 8. Later

- An optional **LAN-only dashboard on the Clinic Hub** (M4), for clinics that want the same figures without any cloud: the Hub would collect the counts over the clinic network.
- "You're invited" emails to people who already have an account (section 3, Known limits).
- Two-factor sign-in required for clinic admins once TOTP is in the portal (ADR-0017 section 2).

## Consequences

- **The promise holds.** No transcript, note, recording, patient detail or visit time reaches the cloud through clinics. What the server learns is a person's daily counts in a clinic they joined and chose to share with, their platform and app version, and the names in section 1. `docs/network.md` and the portal say so.
- **Workplace monitoring.** Per-member counts are monitoring, however coarse. That is why sharing is each member's own switch, off by default, why admins see totals for a period rather than a person's daily timeline, and why removing someone leaves no name on their past counts.
- **Account deletion stays one step.** It can delete a clinic whose last admin leaves this way; the portal warns first.
- **A third kind of email.** The owner pastes the invite template into the hosted project (Authentication > Emails > Invite user), and invite emails count against the same SendGrid and Supabase limits as sign-in codes.
- **Owner steps** before the hosted portal can use any of this: apply the migration, add `clinic` to the exposed schemas, deploy `clinic-invite`, and paste the invite template (`supabase/README.md`, "Clinics").
- **Tests.** `scripts/clinic-e2e.py` (CI: `.github/workflows/clinic-e2e.yml`, a commit tagged `[clinic-e2e]`) runs every rule above against a local Supabase, including account deletion through the real `delete-account` function.

## Alternatives considered

| Option | Why not |
|---|---|
| Send per-visit events (one row per visit with a timestamp) | Visit times are clinical metadata (working hours, consult lengths, which vet saw how many patients when). Daily totals answer the manager's question without them |
| Admins see each member's day-by-day counts | Finer monitoring than "is the hospital using it" needs. Clinic-wide per-day totals plus per-person totals for a period are enough, and the table can serve more later if a clinic asks and its members agree |
| Tables in `public`, written directly through row level security | Writes need checks across rows (last admin, limits, sharing, bounds with clear errors). Functions do that in one place; a separate schema keeps Supabase's default `public` grants away |
| Refuse to delete the last admin's account until ownership is handed over | Account deletion must never be blocked, and the app's "Delete my sync account" (same function) couldn't explain it. The clinic goes, after a warning on the portal |
| Make the most senior member admin automatically when the last admin goes | Gives someone admin rights they never accepted |
| Invite links with a token in the email | A link that signs someone in is what the code-only sign-in avoids (ADR-0017). The email points at the account page; the code proves the address |
| A reusable join code for the whole clinic | Anyone who saw it could join, indefinitely. One-off codes for 14 days, and email invites for everyone else |
| Show members' email addresses to admins | Not needed to run a clinic once someone has joined; the display name is enough, and the admin typed the address in the first place |
| Count usage from encrypted sync's metadata | Sync is optional and per person, and its metadata isn't meant to be read as usage. Counts must come from an explicit choice to share them |
