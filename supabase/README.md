# Supabase: the OpenBark account portal

The website's account portal (www.openbark.io/portal, ADR-0017) signs people in with Supabase Auth and keeps a short profile about the person. This folder holds everything the Supabase project needs for it:

| Path | What it is |
|---|---|
| `migrations/20261018120000_profiles.sql` | The `public.profiles` table (name, clinic, role, country, update emails), its row level security and its privileges |
| `migrations/20261020120000_sync.sql` | Encrypted sync (ADR-0017, M3 phase 2): the `sync` schema, its row level security, and the functions the app calls (push, pull, devices, keys, rotation, revocation), plus the hourly purge |
| `migrations/20261024120000_clinic.sql` | Clinics (ADR-0028, the hospital dashboard): the `clinic` schema (clinics, members, invites, join codes, daily usage counts), its row level security, the functions the portal calls, and the daily purge |
| `functions/delete-account/` | The Edge Function behind "Delete my account" on the portal and "Delete my sync account" in the app (`index.ts`, with supabase-js pinned in `deno.json`) |
| `functions/revoke-device/` | The Edge Function behind removing a device from the portal |
| `functions/clinic-invite/` | The Edge Function behind "Invite people" on the portal: checks the admin, records the invites and emails people who have no account yet |
| `templates/sign-in-code.html` | The sign-in email: a 6-digit code, no link |
| `templates/clinic-invite.html` | The clinic invite email ("Invite user"): no sign-in link, it points at the account page |
| `config.toml` | A local Supabase for development and the sync tests (`scripts/sync-local-supabase.sh`); not the hosted project's settings |

**No clinical data ever goes to this project for the portal**: no recordings, transcripts, notes or anything about patients. Encrypted sync stores **ciphertext only** in its `sync` schema: transcripts, notes and patient records sealed on the person's devices, with keys the server never has. Recordings never reach it. Clinics (`clinic` schema) hold clinic names, members' display names and roles, pending invite addresses, and **daily counts** a member chooses to share (visits, minutes recorded, notes drafted and signed, calls), each one typed and bounded; nothing about any visit or patient.

Project: `nmecovtmijnfnjuadicp`, URL `https://nmecovtmijnfnjuadicp.supabase.co`. The publishable key in `website/src/portal.js` is public by design; the database's row level security is what protects each account's row. **Never put a secret key or the service role key in this repository, the website or a chat.** The delete-account function gets the service role from Supabase at run time.

## One-time setup (the owner)

Run the commands from the repository root. They use the Supabase CLI through npx, pinned to the version these files were checked with; nothing needs installing. Steps 1 and 2 can also be done in the dashboard alone where noted.

```bash
npx supabase@2.121.0 login                                         # opens the browser once; the token stays on your computer
npx supabase@2.121.0 link --project-ref nmecovtmijnfnjuadicp       # asks for the database password (Project Settings > Database)
```

### 1. Create the profiles table

Either:

- **Dashboard:** SQL Editor > New query. Paste the whole of `supabase/migrations/20261018120000_profiles.sql` and click **Run**. It says "Success. No rows returned".
- **CLI:**
  ```bash
  npx supabase@2.121.0 db push --dry-run    # lists 20261018120000_profiles.sql
  npx supabase@2.121.0 db push
  ```

The migration is safe to run twice. If you used the SQL editor and later switch to `db push`, first record it as applied: `npx supabase@2.121.0 migration repair --status applied 20261018120000`.

Check: Table Editor > `profiles` shows **RLS enabled**, and Authentication > Policies lists four policies on `profiles` (read, create, change and delete own profile).

### 2. Deploy the delete-account function

```bash
npx supabase@2.121.0 functions deploy delete-account --project-ref nmecovtmijnfnjuadicp --no-verify-jwt --use-api
```

- `--no-verify-jwt`: the project signs sign-in tokens with ES256 keys, and the function checks each token itself with Supabase Auth before doing anything. Keep this flag on every redeploy.
- `--use-api` bundles the function on Supabase's side, so Docker isn't needed. If it errors, start Docker Desktop and run the command without it.
- No secrets to set: Supabase gives every function `SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY`. Only if the project's legacy API keys are ever turned off, create a secret key (Project Settings > API Keys) and run `npx supabase@2.121.0 secrets set OPENBARK_SECRET_KEY=<the sb_secret_ key> --project-ref nmecovtmijnfnjuadicp`.
- Browsers may call it only from `https://www.openbark.io` and `https://openbark.io`. To add an origin (for example a local test on port 8099): `npx supabase@2.121.0 secrets set PORTAL_ORIGINS=https://www.openbark.io,https://openbark.io,http://localhost:8099 --project-ref nmecovtmijnfnjuadicp`.

Check (safe, deletes nothing):

```bash
curl -i -X OPTIONS https://nmecovtmijnfnjuadicp.supabase.co/functions/v1/delete-account -H "Origin: https://www.openbark.io"
# 204, with Access-Control-Allow-Origin: https://www.openbark.io
curl -i -X POST https://nmecovtmijnfnjuadicp.supabase.co/functions/v1/delete-account
# 401 {"error":"not_signed_in"}
```

Until this step is done, "Delete my account" tells the person nothing was deleted and to write to we.care@animl.health.

### 3. Site URL and redirect URLs

Authentication > URL Configuration:

- **Site URL:** `https://www.openbark.io`
- **Redirect URLs:** add `https://www.openbark.io/portal`

Sign-in is by code, so these only matter if an email ever carries a link; then it brings the person back to the portal.

### 4. Put the code in the emails

Authentication > Emails > Templates. Supabase sends **Confirm signup** the first time an address signs in and **Magic Link** every time after, so change both:

- **Subject:** `Your OpenBark sign-in code`
- **Body:** paste the whole of `supabase/templates/sign-in-code.html`. It shows `{{ .Token }}`, the 6-digit code, and no link.

Then Authentication > Sign In / Providers > Email:

- **Email** provider and **Confirm email**: on (they are).
- **Email OTP Length:** 6.
- **Email OTP Expiration:** 3600 seconds. The portal and the email say the code "expires after an hour"; if you change this, change those two sentences (`website/src/portal.html`, `supabase/templates/sign-in-code.html`).
- **Allow new users to sign up:** on. The same code creates the account.

### 5. Send the emails through SendGrid

Until this is done, Supabase's built-in sender only delivers to members of the Supabase team, and only a few emails an hour. Everyone else sees "Sign-in emails can't go to this address yet."

In SendGrid:

1. Settings > Sender Authentication > **Authenticate Your Domain** for the domain the emails will come from (for example `openbark.io`, at Namecheap). Add the DNS records SendGrid shows and wait until it says verified.
2. Settings > API Keys > **Create API Key**: restricted access, with only **Mail Send** set to full access. Copy the key; SendGrid shows it once.
3. Optional, matching the site's no-tracking stance: Settings > Tracking, turn **Open Tracking** and **Click Tracking** off.

In Supabase, Authentication > Emails > **SMTP Settings**, turn on custom SMTP:

| Field | Value |
|---|---|
| Sender email | an address on the authenticated domain, for example `no-reply@openbark.io` |
| Sender name | `OpenBark` |
| Host | `smtp.sendgrid.net` |
| Port | `587` |
| Username | `apikey` (that exact word) |
| Password | the SendGrid API key from step 2 |
| Minimum interval between emails | `60` seconds (the portal's "Send a new code" countdown assumes 60) |

Save. The key lives only in Supabase. Then check Authentication > Rate Limits: **emails sent per hour** applies to the whole project; raise it if many people sign in at once.

### 6. Try it

1. Open https://www.openbark.io/portal with an address outside the Supabase team.
2. Ask for a code; the email arrives within a minute, with the code and no link.
3. Type the code: the account page shows your address. Save a profile, reload: it's still there.
4. Sign out, sign in again (this time the Magic Link template sends the code).
5. Delete the account with a throwaway address: the page signs out and says the account was deleted, and Authentication > Users no longer lists it.

### 7. Show Sign in on the site (engineering)

Until this point the portal page exists but nothing links to it. Once step 6 works, set `PORTAL_LINKED = True` in `scripts/website-build.py`, rebuild (`python scripts/website-build.py`), run the website e2e and push. The header and footer then show Sign in on every page.

## Encrypted sync (M3 phase 2, Preview; test data only until the security review)

Only when the owner decides to try sync on the hosted project. Every step is safe to repeat.

1. **Region check.** Project Settings > General shows the region. Real clinic data needs `ca-central-1` on the Pro plan (ADR-0017 section 11); until then, test data only.
2. **Apply the migration:** `npx supabase@2.121.0 db push` (lists `20261020120000_sync.sql`), or paste the file into the SQL editor and run it.
3. **Expose the schema:** Project Settings > Data API > Exposed schemas: add `sync`. Without it the app gets "schema must be one of public, graphql_public".
4. **Sessions:** Authentication > Sessions (or JWT settings): access token expiry **600** seconds; refresh token rotation on, reuse interval 10 s.
5. **Deploy the device-removal function:** `npx supabase@2.121.0 functions deploy revoke-device --project-ref nmecovtmijnfnjuadicp --no-verify-jwt --use-api`. Redeploy `delete-account` too (its comment changed; the behaviour didn't).
6. **pg_cron:** Database > Extensions: pg_cron on. The migration schedules `openbark-sync-purge` hourly when pg_cron is available; if it was off when the migration ran, run `select cron.schedule('openbark-sync-purge', '17 * * * *', 'select sync.purge()');` once.
7. **Revocation check:** in the SQL editor, `select has_table_privilege('postgres', 'auth.sessions', 'DELETE');` should be `true`. If it isn't, a removed device is still refused at once by row level security, but its refresh token keeps working until it tries; tell engineering.
8. **Try it** with two test computers and a test address: Settings > Encrypted sync on each (the guide: `docs/guide/sync.md`).

Check (as the service role, in the SQL editor): `select kind, count(*), min(padded_size), max(padded_size) from sync.objects group by kind;` shows only kinds and sizes; `select substring(body from 1 for 4) from sync.objects limit 5;` shows `OBS1`.

The sign-in email says "Type this code on the OpenBark account page"; the app uses the same email.

## Clinics (the hospital dashboard, ADR-0028)

Clinics let a practice manager invite their team from the account page, and see daily usage counts that members choose to share. Phase 1 is the portal only: the app doesn't call any of this yet. Until these steps are done, the portal hides its Clinics card (it notices the schema isn't there) and nothing else changes. Every step is safe to repeat.

1. **Apply the migration:** `npx supabase@2.121.0 db push` (lists `20261024120000_clinic.sql`), or paste the file into the SQL editor and run it. It needs no extension; it schedules its purge with pg_cron if pg_cron is on.
2. **Expose the schema:** Project Settings > Data API > Exposed schemas: add `clinic` (beside `sync` if that's there). Without it the portal hides Clinics.
3. **Deploy the invite function:**
   ```bash
   npx supabase@2.121.0 functions deploy clinic-invite --project-ref nmecovtmijnfnjuadicp --no-verify-jwt --use-api
   ```
   Same flags and the same allowed origins (`PORTAL_ORIGINS`) as delete-account. Optional: `PORTAL_URL` (default `https://www.openbark.io/portal`), where an invite would land if a template ever carried a link. Until it's deployed, "Invite people" still records invites (they show at the person's next sign-in) and says no email was sent.
4. **The invite email:** Authentication > Emails > Templates > **Invite user**:
   - **Subject:** `You're invited to a clinic on OpenBark`
   - **Body:** paste the whole of `supabase/templates/clinic-invite.html`. It has no sign-in link and no token: it sends people to the account page, where they sign in with a code and accept. `{{ .SiteURL }}` must be `https://www.openbark.io` (step 3 of the one-time setup).
   Invite emails go through the same SendGrid SMTP and count against the same Authentication > Rate Limits as sign-in codes; raise "emails sent per hour" before inviting a whole hospital.
5. **pg_cron:** with pg_cron on, the migration schedules `openbark-clinic-purge` daily at 03:41 UTC. If it was off when the migration ran: `select cron.schedule('openbark-clinic-purge', '41 3 * * *', 'select clinic.purge()');`
6. **Keep "Confirm email" on** (Authentication > Sign In / Providers > Email). Invites are matched on the account's confirmed address.
7. **Try it** with two test addresses: on the portal, Create a clinic, invite the second address, sign in with it in a private window and accept; make a join code and join with a third. Then delete one test account and check its membership is gone.

Check (as the service role, in the SQL editor): `select table_name, column_name, data_type from information_schema.columns where table_schema = 'clinic' and data_type in ('text', 'bytea');` lists only `clinics.name`, `members.display_name`, `members.role`, `invites.email`, `invites.role`, `join_codes.code_hash`, `usage_daily.platform` and `usage_daily.app_version`.

### What clinics can and can't do

- A person signs in with the same emailed code. Whoever creates a clinic is its first admin; admins invite (by email, up to 50 invites and join codes per clinic per day), remove people, change roles, rename or delete the clinic, and see the dashboard. A clinic always keeps one admin.
- Members' email addresses are never shown to admins; each person chooses the name their clinic sees.
- Usage counts arrive only from people who turned on sharing (phase 2, in the app). The dashboard shows per-day clinic totals and per-person totals for 7, 30 or 90 days, devices by platform and app version, and a CSV of the same.
- Removing someone keeps their past counts as "Former member", without a name. Deleting an account removes the person's memberships and counts; if they were a clinic's last admin, the clinic is deleted too (the portal warns first).
- Counts older than 400 days are deleted daily. If that wording should mention the app, change `templates/sign-in-code.html` and paste it again (step 4 above).

### Testing locally

`scripts/sync-local-supabase.sh start` runs this folder in a local Supabase (Docker; on Windows inside WSL), then `eval "$(bash scripts/sync-local-supabase.sh env)"` and `cargo test -p os-sync --test supabase`. `scripts/sync-e2e.py` drives two OpenBark servers through it.

## How it is protected

- **Row level security** on `profiles`: a signed-in person can read, create, change and delete only the row whose `id` is their own user id. Visitors who aren't signed in get nothing. The timestamps are the database's to set, and a row's `id` can't be changed.
- **Delete my account** asks Supabase Auth who the token belongs to, requires the account's email address typed as confirmation, then deletes that user with the service role. The profile row goes with it (`on delete cascade`), and so does every row of the `sync` schema (each table cascades from `auth.users`, directly or through `sync.spaces`), and the person's clinic memberships and usage counts (a clinic whose last admin this was goes too). Local data on the person's computers is never touched.
- **Clinics:** every table in the `clinic` schema has row level security; the anon role has no access to the schema at all. A signed-in person can read their own memberships, their clinics' names and their own usage rows, and write nothing directly: every change goes through a function that checks the caller (admin, member, sharing on, bounds, limits). `invite_members_for`, which takes a user id, can be called only with the service role (the clinic-invite function).
- **The website's CSP** (`website/vercel.json`) allows connections to this one project and loads supabase-js from one pinned jsDelivr URL with Subresource Integrity. No realtime connection is opened.
- **Region:** still to check (ADR-0017 section 11). It can't be changed later.

## How it was tested

- **Clinics:** `scripts/clinic-e2e.py` runs against a local Supabase on CI (`.github/workflows/clinic-e2e.yml`, commits tagged `[clinic-e2e]`): five accounts signing in with codes from Mailpit, invites by email through the real `clinic-invite` function (an existing account and a new one, which then signs in with `/otp` and `/verify` as the portal does), join codes, expired and revoked invites, usage reports accepted and refused, what each person and anon can read, the dashboard's sums, the last-admin rule, removing a member, account deletion through the real `delete-account` function (including a last admin, whose clinic goes), the purge, and the list of text columns. `apps/desktop/e2e/portal-clinic.mjs` then drives the published portal page in Chromium against the same local stack, and `apps/desktop/e2e/website.mjs` checks it with Supabase mocked.

- The migration was run twice on Postgres 16 with stand-ins for Supabase's roles and `auth.uid()`, with checks that one user can't see, change or delete another's row, that the anon role is refused, that the timestamps and `id` can't be set, and that deleting the auth user deletes the profile.
- The function was type-checked and linted with Deno and run against a mock Supabase Auth: preflight, no token, a forged token, a wrong confirmation, a disallowed origin, the wrong method, and a real deletion.
- `apps/desktop/e2e/website.mjs` checks the portal under the production CSP, signed out (no call to Supabase), and through a mocked sign-in; it never reaches this project.
- The whole flow was also run against a local Supabase stack (`supabase start` in a scratch copy): a real emailed code with this template, the profile through RLS, the delete function and the cascade.
