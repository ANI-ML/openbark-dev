# Clinic hub: authorization, call by call

ADR-0029 replaces the hosted Supabase project with a hub the clinic runs on its own computer (`engine/os-hub`). The server's behaviour was 28 SQL functions in `supabase/migrations/20261020120000_sync.sql`, guarded by `auth.uid()`, the sign-in session and row level security, plus two Edge Functions. This table maps each of them to the hub's check, so the security review can compare them one by one. It is part of the review's scope (ADR-0029 section 11).

Code: `engine/os-hub/src/lib.rs` (`Hub::handle`, the checks every request passes), `engine/os-hub/src/calls.rs` (one Rust function per SQL function), `engine/os-hub/src/listener.rs` (the TLS listener), `engine/os-sync/src/hub.rs` (the device's side: pinned TLS, signing). Tests: `engine/os-hub/src/tests.rs` (each call: unsigned, bad signature, replayed nonce, old request, revoked device, another account's data, no join code, used and expired codes, the lockout, the quota, the rate limits, a refused client address), `engine/os-hub/tests/flow.rs` (the sync client against the hub) and `.github/workflows/hub-e2e.yml` (three servers and an attack client on one runner).

## Who is asking

Supabase knew the person from the sign-in (`auth.uid()`, an email account) and the device from the session (`session_id` bound to a device row). The hub has no email, passwords, tokens or sessions:

- **The device is who signs.** Every request carries `x-openbark-device` (the device id), `x-openbark-time`, `x-openbark-nonce` and `x-openbark-signature`: Ed25519 over `OBH1 \n method \n path \n SHA-256(body) \n time \n nonce \n device` (`os_sync::hub::canonical`). The hub checks it against the key it registered for that device.
- **The account is the device's account.** A device belongs to one account (set when it created the account, or when a device of the account, or the Recovery Kit, approved it). `auth.uid()` becomes "the calling device's account".
- **Reaching the hub is not enough.** Creating an account and registering any new device need a **hub join code**: shown on the hub's screen (8 characters, 40 bits, Crockford base32), single use, 30 minutes, kept only as an HMAC under a per-hub key, compared against every live code without stopping early. Ten wrong codes from one client address (an IPv6 /64 counts as one) lock it out for an hour.
- **The hub can't read the roster.** "A device in the account's signed roster" is enforced in two places: the hub keeps its own device table (pending, approved, revoked) and checks requests against it; the devices check every object's signature against the roster, sealed under the AMK the hub never has, and set aside what a device the roster doesn't list signed (ADR-0017 section 5, unchanged). A device the hub approved but the roster doesn't list can't get anything opened on another device.

## Checks every request passes (`Hub::handle`), before any call

| # | Check | Refusal |
|---|---|---|
| 1 | The client address is in the allowed ranges (default: loopback, RFC 1918, link-local, IPv6 ULA, 100.64.0.0/10). The listener closes other connections before the TLS handshake; `handle` checks again | connection closed / 403 `address_not_allowed` |
| 2 | TLS 1.3 with the hub's certificate; devices pin its SHA-256 (a different certificate is refused on the device, with a message to pair again) | handshake fails |
| 3 | Rate limit per client address (default 3000 requests a minute, bursts of the same), before anything else is read | 429 `rate_limited` |
| 4 | `POST /rpc/<name>`, name `[a-z_]{1,32}`; body at most 12 MB (the listener refuses a larger Content-Length or body; 10 s for headers, 60 s for a body) | 405, 404 `unknown_call`, 413 `too_large` |
| 5 | The four signature headers are present and well formed | 401 `unsigned` |
| 6 | The time is within 5 minutes of the hub's clock, and not before the hub process started (the nonces it remembers went with the last run) | 401 `stale_request` |
| 7 | The device is known and the signature verifies with its registered key. A device the hub doesn't know may only call `create_account` or `register_device`, signed by the key in the body, for the device id in the body | 401 `unknown_device`, `bad_signature` |
| 8 | The nonce is new for this device (kept for the window, in memory) | 401 `replayed` |
| 9 | Rate limit per account (or per device not in an account yet; default 2400 a minute) | 429 `rate_limited` |
| 10 | A removed device may only call `device_status` and `rebind_device` (to learn it was removed) | 403 `revoked` |
| 11 | The body is a JSON object | 400 `bad_request` |

Calls that change something write the index (sealed, temp file then rename) before answering. One lock serializes every call, so sequence order is commit order (what the SQL's row locks gave).

## The 28 SQL functions

| SQL function | Supabase: who may, and what it checked | Hub (`calls.rs`) |
|---|---|---|
| `my_session()` | The access token's `session_id` claim | No sessions. The signed request's device id (checks 5 to 8) |
| `my_device()` | The device row bound to this session and user, not revoked | The device row for the signing device; a revoked one is limited by check 10 |
| `device_ok()` | True when this session's device is approved and belongs to `auth.uid()`; every policy called it | `Ctx::require_device()`: the device is approved and in an account |
| `fail(code)` | Raises `openbark:<code>` | `Refusal { status, code }`, answered as `{"message": "openbark:<code>"}` |
| `b64(t, max)` | base64 to bytes, `bad_base64`, `too_large` | `calls::bytes()` with the same limits and codes |
| `enc(b)` | bytes to base64 | Stored and returned as base64 |
| `require_device()` | `not_signed_in` without a user; `not_approved` unless this session's device is approved | 401 from checks 5 to 7; `require_device()` gives 403 `not_approved` |
| `account_state()` | Signed in; this session's device (pending or approved); account versions and space only for an approved device; touches `last_seen_at` | The signing device (pending or approved); `account_id`, versions and space only when approved; touches `last_seen_at` |
| `create_account(...)` | Signed in with a session; the user has no account (`account_exists`); the device id is new (`device_exists`); table checks | A device the hub doesn't know (check 7, signed by `ed25519_pub` in the body); **a valid hub join code**, used up only after every other check passes; `account_id` (made on the device) new (`account_exists`); device and space ids new (`device_exists`, `space_exists`); the table checks below |
| `register_device(...)` | Signed in; the user has an account (`no_account`); the id isn't another user's or revoked (`revoked`, `device_exists`); at most 5 pending per account (`too_many_pending`); one pending device per session; 10 minutes | A device the hub doesn't know needs **a valid hub join code** and waits in the hub's lobby (no account yet: the hub can't tell whose it is), filed under `pair_tag` (Argon2id of its 6 words); its own pending row may be renewed without a code, signed by its registered key, and can't change its keys (`keys_changed`); a revoked or approved id is refused (`revoked`, `device_exists`); at most 50 waiting in the lobby (`too_many_pending`); 10 minutes |
| `rebind_device(device_id, rebind_secret)` | Signed in; the device is the user's and `sha256(secret)` matches (`unknown_device`); revoked: only `revoked`; binds the row to the new session | The device id is the signing device's and `sha256(secret)` matches (constant time; `unknown_device`); revoked: only `revoked`. Nothing to bind: every request is signed |
| `device_status(device_id, rebind_secret)` | Anyone (anon): the status only with the right secret, else `unknown` | Any signed, known device (revoked included); the status only with the right secret, else `unknown`. No longer anonymous |
| `approve_device(...)` | Approved device; the target is the user's pending device (`not_pending`), unexpired (`expired`); `amk_version` current (`stale_amk`); `roster_version` + 1 (`stale_roster`) | Approved device; the target is pending, in the lobby (or already the caller's account), **and its `pair_tag` matches** the one the approver derived from the words (constant time; `not_pending` otherwise, so the tag isn't confirmed), unexpired (`expired`); `stale_amk`, `stale_roster` as before; the target joins the caller's account |
| `find_pending(pair_tag)` (hub only) | (Supabase listed the account's pending devices through `list_devices` and RLS) | Approved device; returns only lobby devices that are pending, unexpired and filed under exactly this tag. Nobody can list waiting devices without the words |
| `approve_with_kit(kit_token, envelope, amk_version)` | Signed in; this session's device is pending (`not_pending`); `sha256(token)` is the account's kit verifier (`wrong_kit`); `stale_amk` | The signing device is pending (`not_pending`); the account is the one whose kit verifier is `sha256(token)`, compared with every account without stopping early (`wrong_kit`); a device already in another account is refused (`wrong_kit`); `stale_amk` |
| `revoke(uid, target)` | Internal: the target is the user's (`unknown_device`); revoked at once; its envelopes deleted; `rotate_needed` if it was approved; its auth session deleted | Internal: the target is in the account (`unknown_device`); revoked at once (check 10 refuses its next request); its envelopes deleted; `rotate_needed` if it was approved. No session to delete |
| `revoke_device(device_id)` | `require_device()`, then `revoke` in the user's account (a device may remove itself) | `require_device()`, then `revoke` in the caller's account |
| `revoke_device_for(user_id, device_id)` | Service role only (the website's revoke-device Edge Function, which checked the person's token) | Not a network call: the hub's own screen (Settings, Clinic hub, Remove), on the hub computer, in the app or web mode on 127.0.0.1; with a team, a veterinarian who manages it. Works for lobby devices too |
| `set_roster(roster, roster_version)` | `require_device()`; version + 1 (`stale_roster`) | Same; OBS1, at most 64 KiB |
| `get_account()` | `require_device()`; the user's account | Same; the caller's account |
| `rotate(...)` | `require_device()`; the user's space (`unknown_space`); AMK and key versions + 1 (`stale_amk`); roster + 1 (`stale_roster`); the envelopes name exactly the approved devices (`devices_changed`); every live object re-wrapped at its version (`objects_changed`); all or nothing | Same, and stricter: each device and each object may appear once (the SQL counted joined rows, so one object given twice could stand in for one left out); validated in full before anything changes |
| `dek_list(space_id)` | Security invoker: RLS returned rows only for an approved device of a member | `require_device()` and the caller's own space; another space is refused (`unknown_space`) rather than answered with nothing |
| `push(space_id, items)` | `require_device()`; member (`unknown_space`); at most 100 items (`bad_items`); per item `ok`, `conflict`, `exists`, `stale_key`, `deleted`; `kind_changed` and a bad op refuse the whole push; table checks | Same results and refusals, worked out on a copy and applied only if the whole push is good; **the account's quota** (default 20 GB, a hub setting: 507 `quota_exceeded`); kinds and formats checked |
| `put_blob(...)` | `require_device()`; member (`unknown_space`); the version isn't committed (`committed`); table checks | Same; part 0 to 63, version at least 1; OBB1 at most 1,100,000 bytes; the quota |
| `pull(space_id, after, max_rows)` | Security invoker: RLS, so nothing for a pending or revoked device or another account; at most 500 rows | `require_device()` and the caller's own space (`unknown_space` otherwise); 1 to 500 rows |
| `get_blobs(...)` | Security invoker, as `pull` | As `pull` |
| `my_keys()` | Security invoker: the envelopes addressed to this device and the account's kit envelopes; space keys only for an approved device | The same for an account's devices. A pending lobby device sees its own envelopes; recovering with the kit it gives the kit token and sees **only that account's kit envelopes**, which only the kit opens (`wrong_kit` for a wrong token) |
| `list_devices()` | Security invoker: an approved device sees the account's devices; a pending one only itself | Same |
| `purge()` | pg_cron, hourly | The hub, hourly and when it opens; not a network call |

## The Edge Functions

| Edge Function | Supabase | Hub |
|---|---|---|
| `delete-account` | The person's token; `confirm` must be their email; deletes the auth user, and every sync row with it | The `delete_account` call: `require_device()`; deletes the caller's account, its devices, envelopes, spaces, objects and blob parts. The app asks the person to type "delete" first; local visits stay |
| `revoke-device` | The person's token, then `revoke_device_for` with the service role | The hub's own screen (see `revoke_device_for` above) |

## Table checks carried over

The SQL's column checks are enforced in `calls.rs` before anything is stored: public keys, verifiers, the pairing MAC and the pairing tag exactly 32 bytes; envelopes `OBE1` and at most 512 bytes; space keys `OBW1` and at most 512; wrapped object keys `OBW1` and at most 256; the roster `OBS1` and at most 65,536; object bodies `OBS1` and at most 73,728; blob parts `OBB1` and at most 1,100,000; the pairing blob at most 1,024; platform in (windows, macos, linux, ios, android, web, other); kind in (visit, run, event, checks, transcript, patient, tombstone); ids are UUIDs (kept in canonical lower-case form). The hub checks formats and sizes, never contents: it can't open them.

## What changed from the SQL, and why

- **A lobby for new devices.** Without email, a new device can't say which account it wants. It waits unassigned, filed under a tag only someone given its 6 words can derive (Argon2id, 19 MiB, 2 passes, as the existing pairing key), and joins the account of the device that approves it. The tag adds no offline guessing power over what the SQL already stored (the pairing MAC under the same derivation).
- **Account ids are made on the device**, as UUIDs (the roster is sealed to the account id before the hub sees it). The hub refuses one it has.
- **Another account's space is refused**, not answered with nothing, in `pull`, `get_blobs` and `dek_list`.
- **`rotate` refuses duplicates.**
- **`device_status` needs a signature** from a device the hub knows.
- **Nonces live in memory**, for twice the window; requests timed before the hub started are refused instead of remembered.

## For the reviewers

- The hub computer itself: an account any of whose devices is the hub computer is readable by whoever controls that computer, as on any device of the account (ADR-0029 section 2). Accounts without a device there stay unreadable to it.
- Manual joining trusts a 50-bit certificate check typed from the hub's screen; the QR code carries the full fingerprint. An attacker on the clinic network would have to make a certificate whose fingerprint matches 50 bits between the code being shown and typed.
- A join code is 40 bits, single use, 30 minutes, and every wrong guess counts toward the address's lockout.
- mDNS answers are hints only; nothing is trusted from them.
