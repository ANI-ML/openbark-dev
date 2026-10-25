# Clinic hub: authorization, call by call

ADR-0029 replaces the hosted Supabase project with a hub the clinic runs on its own computer (`engine/os-hub`). The server's behaviour was 28 SQL functions in `supabase/migrations/20261020120000_sync.sql`, guarded by `auth.uid()`, the sign-in session and row level security, plus two Edge Functions. This table maps each of them to the hub's check, so the security review can compare them one by one. It is part of the review's scope (ADR-0029 section 11).

Code: `engine/os-hub/src/lib.rs` (`Hub::handle`, the checks every request passes), `engine/os-hub/src/calls.rs` (one Rust function per SQL function), `engine/os-hub/src/listener.rs` (the TLS listener), `engine/os-sync/src/hub.rs` (the device's side: pinned TLS, signing). Tests: `engine/os-hub/src/tests.rs` (each call: unsigned, bad signature, replayed nonce, old request, revoked device, another account's data, no join code, used and expired codes, the lockout, the quota, the rate limits, a refused client address), `engine/os-hub/tests/flow.rs` (the sync client against the hub) and `.github/workflows/hub-e2e.yml` (three servers and an attack client on one runner).

## Who is asking

Supabase knew the person from the sign-in (`auth.uid()`, an email account) and the device from the session (`session_id` bound to a device row). The hub has no email, passwords, tokens or sessions:

- **The device is who signs.** Every request carries `x-openbark-device` (the device id), `x-openbark-time`, `x-openbark-nonce` and `x-openbark-signature`: Ed25519 over `OBH1 \n method \n path \n SHA-256(body) \n time \n nonce \n device` (`os_sync::hub::canonical`). The hub checks it against the key it registered for that device.
- **The account is the device's account.** A device belongs to one account (set when it created the account, or when a device of the account, or the Recovery Kit, approved it). `auth.uid()` becomes "the calling device's account".
- **Reaching the hub is not enough.** Creating an account and registering any new device need a **hub join code**: shown on the hub's screen (8 characters, 40 bits, Crockford base32), single use, 30 minutes, kept only as an HMAC under a per-hub key (a damaged key is an error, never a default), compared against every live code without stopping early. Making a new code retires every older unused one: only the code on screen works. Ten wrong codes from one client address (an IPv6 /64 counts as one) lock it out for an hour; twenty wrong codes from anywhere stop every code until the hub's screen makes a new one. Within the allowed ranges the join code is the only gate, so the defaults are the clinic's own private ranges; 100.64.0.0/10 (Tailscale and similar) is opt-in.
- **A new device confirms which account it joined.** Any approved device of any account on the hub that sees a person's 6 words (or pairing QR code) could approve that person's new device into its own account. So after a hub approval the new device is in a `confirm` phase: it shows the devices of the account it was put in and syncs nothing (nothing pulled, sealed or pushed) until the person types the code the approving device shows. That code is a MAC under the pairing key over the account id and the AMK version the approval was for (`os_sync::keys::pair_confirm`, 8 characters), so only the device that approved into *that* account can show the matching code, and the person sees it on their own device. A wrong code is refused; **Start over** makes the device leave the account it was put in (it revokes itself) and take a new id. Turning sync off doesn't skip the check.
- **The certificate check is keyed by the join code.** A code typed by hand comes with a 10-character check: 50 bits of HMAC-SHA256 of the certificate's fingerprint keyed by that join code (`os_sync::hub::check_code`). The fingerprint is public (it travels in every handshake), but a forger would also need the code, which only the hub's screen shows and which is sent only after the check passes; so there is nothing to grind offline. The QR code carries the full fingerprint. The device also checks that the certificate is validly self-signed and within its dates, not only that its bytes hash right.
- **The hub can't read the roster.** "A device in the account's signed roster" is enforced in two places: the hub keeps its own device table (pending, approved, revoked) and checks requests against it; the devices check every object's signature against the roster, sealed under the AMK the hub never has, and set aside what a device the roster doesn't list signed (ADR-0017 section 5, unchanged). A device the hub approved but the roster doesn't list can't get anything opened on another device.

## Checks every request passes, before any call

The listener (`listener.rs`) closes a connection from outside the allowed ranges before the TLS handshake, keeps at most 256 connections and 16 per address (an IPv6 /64 counts as one), and allows 5 seconds for the handshake and for headers, 20 for a body and 2 minutes for a connection. Then, **before any body is read** (`Hub::admit`, checks 1 to 9), and only after (`Hub::finish`, checks 10 to 14):

| # | Check | Refusal |
|---|---|---|
| 1 | The client address is in the allowed ranges (default: loopback, RFC 1918, link-local, IPv6 ULA; 100.64.0.0/10 only if added) | connection closed / 403 `address_not_allowed` |
| 2 | TLS 1.3 with the hub's certificate; devices pin its SHA-256 and check its self-signature (a different certificate is refused on the device, with a message to pair again) | handshake fails |
| 3 | Rate limit per client address (default 3000 requests a minute, bursts of the same) | 429 `rate_limited` |
| 4 | `POST /rpc/<name>`, name `[a-z_]{1,32}` | 405, 404 `unknown_call` |
| 5 | The four signature headers are present and well formed | 401 `unsigned` |
| 6 | The time is within 5 minutes of the hub's clock (compared without overflow), not before the hub process started, and while the hub has run for less than 5 minutes not more than 30 seconds ahead (the nonces it remembers went with the last run) | 401 `stale_request` |
| 7 | The device is known, or the call creates one (`create_account`, `register_device`) | 401 `unknown_device` |
| 8 | A removed device may only call `device_status` and `rebind_device` (to learn it was removed) | 403 `revoked` |
| 9 | The declared body fits the call: 128 KB (and always for a device the hub doesn't know), 2 MB for `put_blob`, 12 MB for `push` and `rotate`; at most 4 bodies over 128 KB are read at once | 413 `too_large`, 503 `busy` |
| 10 | The body read fits the same limit | 413 `too_large` |
| 11 | The signature verifies over the raw body's hash, before any JSON is parsed, with the device's registered key (for a device the hub doesn't know, the key in its small body, for the device id in the body) | 401 `bad_signature`, `unknown_device` |
| 12 | The nonce is new for this device (kept for twice the window, in memory, with room for everything the rate limit allows) | 401 `replayed` |
| 13 | Rate limit per account (or per device not in an account yet; default 2400 a minute), counted only for signed requests | 429 `rate_limited` |
| 14 | The body is a JSON object; at most 16 calls run at once (more wait up to 10 seconds) | 400 `bad_request`, 503 `busy` |

Calls that change something write the index before answering: sealed, temp file then rename, flushed to the disk with its folder, the previous index kept as `index.json.bak` (used if `index.json` is damaged). Superseded object and blob files are removed only after the index that no longer names them is saved. One lock serializes every call, so sequence order is commit order (what the SQL's row locks gave). Objects count toward the quota with 512 bytes of metadata each, and an account holds at most 250,000.

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
| `register_device(...)` | Signed in; the user has an account (`no_account`); the id isn't another user's or revoked (`revoked`, `device_exists`); at most 5 pending per account (`too_many_pending`); one pending device per session; 10 minutes | A device the hub doesn't know needs **a valid hub join code** and waits in the hub's lobby (no account yet: the hub can't tell whose it is), filed under `pair_tag` (Argon2id of its 6 words); its own pending row may be renewed without a code while it hasn't lapsed (a lapsed one needs a new code), signed by its registered key, and can't change its keys (`keys_changed`); a revoked or approved id is refused (`revoked`, `device_exists`); at most 5 waiting from one client address and 200 in all (`too_many_pending`); 10 minutes |
| `rebind_device(device_id, rebind_secret)` | Signed in; the device is the user's and `sha256(secret)` matches (`unknown_device`); revoked: only `revoked`; binds the row to the new session | The device id is the signing device's and `sha256(secret)` matches (constant time; `unknown_device`); revoked: only `revoked`. Nothing to bind: every request is signed |
| `device_status(device_id, rebind_secret)` | Anyone (anon): the status only with the right secret, else `unknown` | Any signed, known device (revoked included); the status only with the right secret, else `unknown`. No longer anonymous |
| `approve_device(...)` | Approved device; the target is the user's pending device (`not_pending`), unexpired (`expired`); `amk_version` current (`stale_amk`); `roster_version` + 1 (`stale_roster`) | Approved device; the target is pending, in the lobby (or already the caller's account), **and its `pair_tag` matches** the one the approver derived from the words (constant time; `not_pending` otherwise, so the tag isn't confirmed), unexpired (`expired`); `stale_amk`, `stale_roster` as before; the target joins the caller's account. The hub can't tell a colleague's device from the person's own: the new device's confirmation code (see "Who is asking") is what stops another account taking it |
| `find_pending(pair_tag)` (hub only) | (Supabase listed the account's pending devices through `list_devices` and RLS) | Approved device; returns only lobby devices that are pending, unexpired and filed under exactly this tag. Nobody can list waiting devices without the words |
| `approve_with_kit(kit_token, envelope, amk_version)` | Signed in; this session's device is pending (`not_pending`); `sha256(token)` is the account's kit verifier (`wrong_kit`); `stale_amk` | The signing device is pending (`not_pending`); the account is the one whose kit verifier is `sha256(token)`, compared with every account without stopping early (`wrong_kit`); a device already in another account is refused (`wrong_kit`); `stale_amk` |
| `revoke(uid, target)` | Internal: the target is the user's (`unknown_device`); revoked at once; its envelopes deleted; `rotate_needed` if it was approved; its auth session deleted | Internal: the target is in the account (`unknown_device`); revoked at once (check 8 refuses its next request); its envelopes deleted; `rotate_needed` if it was approved. No session to delete |
| `revoke_device(device_id)` | `require_device()`, then `revoke` in the user's account (a device may remove itself) | `require_device()`, then `revoke` in the caller's account |
| `revoke_device_for(user_id, device_id)` | Service role only (the website's revoke-device Edge Function, which checked the person's token) | Not a network call: the hub's own screen (Settings, Clinic hub, Remove), on the hub computer, in the app or web mode on 127.0.0.1; with a team, a veterinarian who manages it. Works for lobby devices too |
| `set_roster(roster, roster_version)` | `require_device()`; version + 1 (`stale_roster`) | Same; OBS1, at most 64 KiB |
| `get_account()` | `require_device()`; the user's account | Same; the caller's account |
| `rotate(...)` | `require_device()`; the user's space (`unknown_space`); AMK and key versions + 1 (`stale_amk`); roster + 1 (`stale_roster`); the envelopes name exactly the approved devices (`devices_changed`); every live object re-wrapped at its version (`objects_changed`); all or nothing | Same, and stricter: each device and each object may appear once (the SQL counted joined rows, so one object given twice could stand in for one left out); validated in full before anything changes |
| `dek_list(space_id)` | Security invoker: RLS returned rows only for an approved device of a member | `require_device()` and the caller's own space; another space is refused (`unknown_space`) rather than answered with nothing |
| `push(space_id, items)` | `require_device()`; member (`unknown_space`); at most 100 items (`bad_items`); per item `ok`, `conflict`, `exists`, `stale_key`, `deleted`; `kind_changed` and a bad op refuse the whole push; table checks | Same results and refusals, worked out on a copy and applied only if the whole push is good; **the account's quota** (default 20 GB, a hub setting, each object's metadata counted: 507 `quota_exceeded`) and at most 250,000 objects (507 `too_many_objects`); kinds and formats checked |
| `put_blob(...)` | `require_device()`; member (`unknown_space`); the version isn't committed (`committed`); table checks | Same; part 0 to 63, version at least 1; OBB1 at most 1,100,000 bytes; the quota |
| `pull(space_id, after, max_rows)` | Security invoker: RLS, so nothing for a pending or revoked device or another account; at most 500 rows | `require_device()` and the caller's own space (`unknown_space` otherwise); 1 to 500 rows; a body the hub can't read is flagged (`missing`) instead of failing the page |
| `get_blobs(...)` | Security invoker, as `pull` | As `pull`; in pages of about 16 MB from `from_part`, so a large version stays under the device's 64 MB answer limit |
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
- **Nonces live in memory**, for twice the window; requests timed before the hub started are refused instead of remembered, and while the hub has run for less than one window, requests more than 30 seconds in the future too. A request captured before a restart can be replayed only if its device's clock was ahead by less than 30 seconds and it arrives within that time.
- **A new device must confirm the account** it was approved into (hub only; with Supabase the email account already named it).

## The security review of 2026-10-25 (PR #18), and what changed

| Finding | Fix | Tests |
|---|---|---|
| 1 (major). Any approved device of any account that saw a person's 6 words could approve their new device into its own account, which then uploaded its visits | The `confirm` phase: no sync until the code the approving device shows (a MAC under the pairing key over the account and AMK version) is typed; the device lists the account's devices; Start over leaves the account | `tests/flow.rs` `another_account_with_the_words_cant_take_a_new_device`; hub-e2e `attacks` (the hub's own device as the other account, a colleague's laptop) |
| 2 (major). The typed 50-bit check was the fingerprint's own bits, which are public, so a matching certificate could be ground offline once for the hub's life | The check is an HMAC of the fingerprint keyed by the join code (new with every code, sent only after it passes); the device checks the certificate's self-signature | `os-sync` `the_check_is_keyed_by_the_join_code`; `os-hub` `over_tls_the_pinned_certificate_decides`; hub-e2e `setup` |
| 3 (major). Bodies of up to 12 MB were buffered, JSON parsed and the store locked before the signature was checked; the connection cap was per full IPv6 address | Checks 1 to 9 before reading; small bodies unless the call needs more; signature before JSON; 16 calls and 4 large bodies at once; per /64; shorter timeouts | `bodies_are_bounded_before_they_are_read`, `the_signature_is_checked_before_any_json_is_parsed`, `connections_count_per_address_and_per_ipv6_64`; hub-e2e `attacks` (flood) |
| 4. Large blob versions could exceed the device's answer limit; objects were uncapped | Paged `get_blobs`; object metadata counted; 250,000 objects | `large_blobs_come_back_in_pages` |
| 5. Durability | Flushed saves with a backup; deletes after the save; a missing body flagged | `the_index_survives_damage_and_a_missing_body_is_flagged` |
| 6. Replays after a restart | The 30-second rule while young | `requests_from_the_future_are_refused_while_the_hub_is_young` |
| 7. Old codes stayed live; no hub-wide guess budget | A new code retires older ones; 20 wrong codes stop all | `a_new_code_retires_the_old_ones_and_too_many_wrong_codes_stop_them_all`; hub-e2e `setup` |
| 8. 100.64.0.0/10 on by default | Opt-in; the settings say the join code is the only gate | `private_ranges_are_allowed_by_default_and_nothing_else` |
| 9. A run-time variable could switch on the preview in a release build | Only the build-time `OPENBARK_SYNC_PREVIEW` counts | the e2e workflows build with it |
| Nits | Strict Ed25519 verification; overflow-free time checks; the live join code only for whoever manages the hub (`by` on `GET /api/clinic-hub`); joining the hub computer's own device needs that role too; a damaged code key is an error; a lapsed lobby request needs a new code; nonce room follows the rate limit; the lobby is bounded per address | `the_lobby_is_bounded_per_address_and_a_lapsed_request_needs_a_new_code`, `a_nonce_is_used_once` |

## For the reviewers

- The hub computer itself: an account any of whose devices is the hub computer is readable by whoever controls that computer, as on any device of the account (ADR-0029 section 2). Accounts without a device there stay unreadable to it.
- Manual joining: the 10-character check is keyed by the join code, so it can only be forged by someone who has seen the code on the hub's screen (and the code is single use, live for 30 minutes, and retired by the next one). An attacker who has seen the screen could equally type the code themselves; the check doesn't defend against that, the hub's physical placement does. The QR code carries the full fingerprint.
- The confirmation code is 40 bits, typed by the person on their own new device; it can't be guessed against the hub (it never reaches the hub). An attacker device that approved first knows its own matching code and could try to talk the person into typing it; the new device shows the attacker's device names to make that visible.
- A join code is 40 bits, single use, 30 minutes; every wrong guess counts toward the address's lockout and the hub-wide budget.
- mDNS answers are hints only; nothing is trusted from them (the advertisement carries 16 hex characters of the fingerprint, which is public anyway).
