# Supabase: the OpenBark account portal

The website's account portal (www.openbark.io/portal, ADR-0017) signs people in with Supabase Auth and keeps a short profile about the person. This folder holds everything the Supabase project needs for it:

| Path | What it is |
|---|---|
| `migrations/20261018120000_profiles.sql` | The `public.profiles` table (name, clinic, role, country, update emails), its row level security and its privileges |
| `functions/delete-account/` | The Edge Function behind "Delete my account" (`index.ts`, with supabase-js pinned in `deno.json`) |
| `templates/sign-in-code.html` | The sign-in email: a 6-digit code, no link |

**No clinical data ever goes to this project for the portal**: no recordings, transcripts, notes or anything about patients. Encrypted sync (ADR-0017, later in M3) will add its own `sync` schema here.

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

## How it is protected

- **Row level security** on `profiles`: a signed-in person can read, create, change and delete only the row whose `id` is their own user id. Visitors who aren't signed in get nothing. The timestamps are the database's to set, and a row's `id` can't be changed.
- **Delete my account** asks Supabase Auth who the token belongs to, requires the account's email address typed as confirmation, then deletes that user with the service role. The profile row goes with it (`on delete cascade`). When the `sync` schema arrives, its tables must also cascade from `auth.users`, or the function must delete them first. Local data on the person's computers is never touched.
- **The website's CSP** (`website/vercel.json`) allows connections to this one project and loads supabase-js from one pinned jsDelivr URL with Subresource Integrity. No realtime connection is opened.
- **Region:** still to check (ADR-0017 section 11). It can't be changed later.

## How it was tested

- The migration was run twice on Postgres 16 with stand-ins for Supabase's roles and `auth.uid()`, with checks that one user can't see, change or delete another's row, that the anon role is refused, that the timestamps and `id` can't be set, and that deleting the auth user deletes the profile.
- The function was type-checked and linted with Deno and run against a mock Supabase Auth: preflight, no token, a forged token, a wrong confirmation, a disallowed origin, the wrong method, and a real deletion.
- `apps/desktop/e2e/website.mjs` checks the portal under the production CSP, signed out (no call to Supabase), and through a mocked sign-in; it never reaches this project.
- The whole flow was also run against a local Supabase stack (`supabase start` in a scratch copy): a real emailed code with this template, the profile through RLS, the delete function and the cascade.
