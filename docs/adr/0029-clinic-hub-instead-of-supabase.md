# ADR-0029: The clinic's own computer is the sync hub, not a hosted service

- Status: accepted (2026-10-25)
- Deciders: product, engineering
- Amends: ADR-0017 (hosting, accounts, sections 2, 11 and 12), ADR-0028 (where the hospital dashboard lives), ADR-0005 (the hosting question it left open)
- Supersedes: the hosted Supabase project as the default sync service and the website account portal

## Context

ADR-0017 built end-to-end encrypted sync on a Supabase project that ANI.ML Health would host, and ADR-0028 put the hospital dashboard (clinics, invites, usage counts) on the same project and the website portal. Running them for clinics needs the Pro plan before a pilot, custom SMTP for sign-in and invite emails, and an operator. OpenBark is free and open source, and ANI.ML won't pay for or run a service.

What already exists and doesn't depend on Supabase:

- The sync crate's encryption and data model: device keys, the Account Master Key, the Recovery Kit, sealed objects (OBS1) and blobs (OBB1), envelopes, the signed roster, merging and conflict handling, and "audio never syncs" (`engine/os-sync`: keys, seal, guard, map, merge, roster, state, engine, account).
- One seam for the network: `os_sync::supabase::Transport` and `Client::rpc(name, args)`. The server's behaviour is 28 SQL functions in `supabase/migrations/20261020120000_sync.sql` (account_state, register_device, approve_device, push, pull, put_blob, get_blobs, revoke_device, rotate and the rest) plus two Edge Functions.
- The app already runs an HTTP server (`openbark serve`, axum, `engine/os-server`), today bound to 127.0.0.1 only.

## Decision

### 1. No hosted service

ANI.ML runs nothing for sync or the dashboard and pays nothing. Every feature works with the clinic's own hardware, and optionally storage the clinic already pays for. Nothing about a clinic's work reaches ANI.ML.

### 2. The clinic hub

- Any desktop install (Windows, macOS, Linux) can be made the clinic hub: **Settings, Clinic hub, Make this computer the clinic hub**. Off by default; one hub per clinic.
- The hub is two things on one computer: a normal OpenBark device with its own visits, and the sync server for the clinic's other devices. The server stores what the Supabase project stored: ciphertext objects and blobs, wrapped keys, public keys, the signed roster and padded sizes. The server role can't read them (ADR-0017 section 8). **But for any account the hub computer itself belongs to** (its owner's, or a team member who uses that computer), the hub's device keys are on the same machine, so someone who controls the hub computer can read that account's data, as they could on any device of the account. Accounts whose devices don't include the hub stay unreadable to it. The hub computer should be a staff-only machine with full-disk encryption and the team PINs in use.
- The server implements the same calls as the 28 SQL functions, in Rust, in a new module behind the existing `Transport` seam (an `os-hub` crate or an `os-sync` module, whichever keeps the network allow-list simplest). Its store is files in the hub's data folder plus a small index, encrypted at rest under the hub's data key like visits (ADR-0008).
- It listens on the local network only while hub mode is on, on one TCP port (default 47110, changeable), and by default accepts connections only from private address ranges: RFC 1918, link-local, IPv6 ULA, and 100.64.0.0/10 (carrier-grade NAT, which VPNs such as Tailscale use). The allowed ranges are a setting. It never listens otherwise; `openbark serve` stays on 127.0.0.1.

### 3. Identity without email, and who may join

Reaching the hub is not enough to use it: many clinics put clients' guest Wi-Fi on the same network as staff. Every new account and every new device needs something shown on the hub.

- **Hub join codes.** The hub (an admin at the hub computer, or the hub's owner when there's no team) shows a join code: a QR code, or 8 characters plus the hub's address. Single use, valid for 30 minutes on screen (14 days when printed for the dashboard's invites, section 8), stored only as a hash, 10 wrong attempts lock that client address out for an hour.
- **Accounts are per person, as in ADR-0017.** A person creates their account by redeeming a hub join code on their first device. That device makes the account keys and gets the Recovery Kit, as today. The hub computer's own owner does the same on the hub itself.
- **A person's second device** joins their account the ADR-0017 way: it shows 6 words or a QR code, and one of the person's existing devices approves it. It also needs a hub join code, so a stranger on the network can't even reach the approval step.
- **Signed requests.** No email codes, passwords or tokens. Every request is signed with the device's Ed25519 key, with a timestamp and a nonce (5-minute replay window). The hub checks the signature against that account's signed roster. A removed device's requests are refused, and keys rotate as today.
- **Limits.** Rate limits per client address and per account, and a storage quota per account (default 20 GB of sealed objects, a hub setting), so one device can't fill the hub's disk.
- Team members on a shared computer keep their PIN profiles (users.rs). With a team, only visits a person recorded, drafted or signed join their space (ADR-0017 section 3, unchanged).

### 4. Transport security

- The local network is untrusted. Traffic is TLS (rustls) to a self-signed hub certificate made when hub mode is turned on. Its SHA-256 fingerprint travels in the pairing QR code and the 6 words' check, and devices pin it: a different certificate is refused, with a message to pair again.
- Objects stay sealed end to end inside the TLS connection, so a compromised hub or a copied hub folder still reveals only what ADR-0017 section 8 lists.

### 5. Finding the hub

- mDNS / DNS-SD on the local network: the hub advertises `_openbark-hub._tcp` with its port and a short fingerprint prefix. Devices browse for it only while sync is on.
- A manual address (IP or name, and port) for networks that block multicast.
- On iPhone: the Local Network permission (`NSLocalNetworkUsageDescription`, `NSBonjourServices` = `_openbark-hub._tcp`), asked for only when the person turns sync on.

### 6. When devices sync

- Desktops: every 30 seconds while open and on the hub's network, as today.
- iPhone: when the app is open or comes to the foreground on the hub's network, and on Stop and save. iOS doesn't guarantee background network time, so a visit recorded away from the clinic syncs when the phone is back on the clinic's Wi-Fi and the app is opened. The phone keeps working offline meanwhile; nothing is lost.
- The hub computer must be on for others to sync. Devices show "Hub not reachable: syncs when you're back on the clinic network".

### 7. Away from the clinic (optional, later)

A **sync folder** in storage the clinic already has: a OneDrive, Dropbox, Google Drive or iCloud Drive folder synced by its own desktop client, or a network drive. The hub mirrors its encrypted object store to the folder, and a remote computer (a vet's laptop at home) syncs through that folder with the same formats and the same keys. The provider holds only sealed objects. ANI.ML runs nothing. On iPhone this waits for a later decision (the Files app and iCloud Drive need their own design); the phone syncs through the hub.

### 8. Hospital dashboard (amends ADR-0028)

- The clinic lives on the hub: roles (admin, member), the last-admin rule, join codes shown as a QR code or 8 characters (single use, 14 days), and daily usage counts per member and device. Counts only, and "Share usage counts" stays off until each person turns it on.
- Devices report counts to the hub over the same signed channel. Admins see the dashboard in the hub's desktop app (Settings, Clinic, Dashboard) and in web mode on the hub computer: totals, per day, per member, per platform and app version, CSV.
- No invite emails: an admin shows or prints a hub join code (section 3), and the person redeems it on their device. One mechanism serves joining the hub and joining the clinic.

### 9. What is retired

Once the hub passes the end-to-end suite (section 11):

- The Supabase transport in `os-sync`, the migrations (profiles, sync, clinic), the Edge Functions, the email templates and `scripts/sync-local-supabase.sh`.
- The website account portal (`/portal`, `portal.js`, supabase-js, the CSP entries), the clinic end-to-end workflow and `portal-clinic.mjs`. The website is static again: downloads, features, version history, privacy.
- SendGrid as a dependency.

They remain in git history. A clinic that wants a server reachable from anywhere can run the hub on a machine it exposes through its own VPN; that's the clinic's choice and needs no code from us.

### 10. docs/network.md

The internet rows for sync (R12, R13, R14) and the planned R15 go. New local-network rows, all off by default:

- L1: the hub's TLS listener (hub mode on only, private addresses only).
- L2: mDNS advertising (hub) and browsing (devices with sync on).
- L3: device to hub calls (sync and usage counts), signed, over TLS, private addresses only.
- L4 (later): the sync folder, which is file access, not network access by OpenBark.

The privacy page and the App Store privacy label become: no data collected, no data leaves the clinic's own devices and storage.

### 11. Testing

The sync end-to-end suite runs without Docker or Supabase: a hub (desktop profile), a phone (iPhone profile) and a second computer on one runner, joined by manual address (mDNS where the runner allows multicast). It keeps every step of today's suite (setup and pairing, a visit from the phone arriving transcribed without its audio, the signed note coming back, offline edits on both sides, conflicting offline drafts, removing a device, recovering from the kit, and the hub holding ciphertext only), and adds: a wrong hub certificate is refused, a request with a bad signature or a replayed nonce is refused, a removed device is refused, creating an account or registering a device without a valid hub join code is refused, a used or expired code is refused, the storage quota and rate limits hold, and the hub refuses a client address outside the allowed ranges.

The server's authorization is reviewed call by call: `docs/security/hub-authorization.md` maps each of the 28 SQL functions and its `auth.uid()` and row-level-security checks to the hub's equivalent (signature, roster membership, account ownership). That table is part of the security review's scope.

## Consequences

- Cost to ANI.ML: none. No accounts, no email, no operator.
- Clinical data never reaches a third party unless the clinic chooses a sync folder in its own storage, and then only sealed objects.
- Sync needs the hub on and devices on its network. Remote work needs the sync folder (section 7) or the clinic's own VPN.
- The phone syncs in the foreground only; the Visits screen shows which visits are waiting to sync.
- Real networks get in the way, and the guide must say how to check each:
  - Wi-Fi client isolation (common on guest networks, sometimes on staff ones) stops the phone reaching the hub at all, even by address.
  - Staff Wi-Fi is often a different subnet from the wired desktops; mDNS doesn't cross subnets, so the manual address is needed.
  - Windows Firewall asks for permission when hub mode turns on.
  - The macOS firewall may ask again after each update while the app is unsigned (ADR-0020).
  The first real-iPhone test should be on a real clinic network, not only at home.
- The iPhone app no longer waits for a hosted project or its review to test sync, only for TestFlight.
- The independent security review stays a gate before real clinic data syncs; its scope is now the hub listener, the pairing, the signed requests and TLS pinning, on top of the cryptography.
- About two thirds of `os-sync` carries over unchanged; the server logic moves from SQL to Rust.

## Alternatives considered

- **Keep Supabase on the free plan.** $0 at first, but free projects pause after a week without traffic and have hard limits; not dependable for clinical work, and it still needs email sending and an operator.
- **Self-hosted Supabase as the default.** Docker, Postgres and an email service per clinic: too much for most clinics. Left possible only through the clinic's own effort.
- **A relay ANI.ML runs.** Cheaper than Supabase but still a service to pay for and operate.
- **Phone to computer without a hub (each computer syncs with each phone).** Simpler for one vet and one laptop, but every device pair needs its own link and merges multiply. The hub is the same code with one server, and a single-vet clinic simply makes their own computer the hub.
- **CloudKit for Apple devices.** Free for users, but Apple-only; clinics run Windows.

## Implementation notes (phase A, 2026-10-25)

How phase A built sections 2 to 6, 10 and 11, where the text above left a choice open:

- **Crates.** `os-hub` (desktop only, os-app's `hub` feature) owns the listener, the store, the certificate and mDNS advertising. `os-sync` owns the device's side: the hub transport (pinned TLS 1.3 with rustls and ring, one request per connection), request signing, join codes and mDNS browsing. Phones link `os-sync` (os-app's `sync` feature) and never `os-hub`. `scripts/ci/check-network.sh` names which crate may have which network capability.
- **A person's next device** can't name its account without email, so it waits in the hub's lobby, filed under an Argon2id tag of its 6 words; the approving device finds it with the same tag (`find_pending`) and the device joins the approver's account. Recovery with the kit finds the account by the kit verifier.
- **Typing a join code** (no QR code) also needs the hub's 10-character certificate check, shown under the code: the device compares it with the certificate it sees before pinning the full fingerprint.
- **Loopback** is in the default allowed ranges, so the hub computer's own device can join its hub (Settings, Clinic hub, Use this computer with its hub).
- **The run-time preview signal** is `OPENBARK_SYNC_PREVIEW=1` (it was `OPENBARK_SYNC_URL`); hub mode sits behind the same gate.
- docs/security/hub-authorization.md maps every SQL function to the hub's check and lists where the hub is stricter.
