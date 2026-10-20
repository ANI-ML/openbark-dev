-- OpenBark encrypted sync (ADR-0017, M3 phase 2): the `sync` schema.
--
-- The server holds ciphertext only. Every clinical byte arrives already
-- sealed on the device (OBS1 objects, OBB1 blob parts, OBE1 key envelopes,
-- OBW1 wrapped keys); this schema stores those bytes, the public keys, and
-- the few plaintext facts ADR-0017 section 8 lists (kinds, versions, padded
-- sizes, times). It never holds a key that opens clinical data.
--
-- The cloud can lock a device out but can never let one in:
-- - signing in proves who you are; it gives no keys;
-- - only an approved device (or the Recovery Kit) can wrap the Account
--   Master Key to a new device, and the server can't open what it stores;
-- - every row is behind row level security, and every policy calls
--   sync.device_ok(): the request's auth session must belong to an approved
--   device of the same account. Revoking a device refuses it at once, not
--   when its access token expires.
--
-- Writes go only through the security-definer functions below, each of
-- which checks the caller itself. The signed-in role can SELECT (through
-- RLS) but never insert, update or delete a table directly. Binary values
-- travel as base64 text, both ways.
--
-- Every table references auth.users with `on delete cascade` (directly or
-- through sync.spaces), so the delete-account Edge Function removes every
-- row and key envelope with the user.
--
-- Apply with `supabase db push` (hosted) or `supabase db reset` (local).
-- The `sync` schema must be in the API's exposed schemas (config.toml
-- [api].schemas locally; Project Settings > Data API on a hosted project).

create schema if not exists sync;
comment on schema sync is
  'OpenBark encrypted sync (ADR-0017). Ciphertext, public keys and padded sizes only. RLS: own account, approved devices.';

grant usage on schema sync to anon, authenticated, service_role;

-- ---------------------------------------------------------------------------
-- Tables
-- ---------------------------------------------------------------------------

-- One row per account that has turned on sync.
create table if not exists sync.accounts (
  user_id uuid primary key references auth.users (id) on delete cascade,
  -- The current Account Master Key version. Rotation (after a device is
  -- removed) adds one.
  amk_version integer not null default 1 check (amk_version >= 1),
  -- The Recovery Kit's X25519 public key: any device can wrap a rotated AMK
  -- to it without knowing the kit.
  kit_x25519_pub bytea not null check (octet_length(kit_x25519_pub) = 32),
  -- SHA-256 of a token derived from the kit (HKDF, its own label). Lets a new
  -- device that holds the kit approve itself; reveals nothing about the kit's
  -- private key.
  kit_verifier bytea not null check (octet_length(kit_verifier) = 32),
  -- The device roster: names, platforms and public keys, sealed and signed
  -- (OBS1) under a key derived from the AMK. Clients accept object signatures
  -- only from keys in it.
  roster bytea not null check (substring(roster from 1 for 4) = '\x4f425331'::bytea and octet_length(roster) <= 65536),
  roster_version integer not null default 1,
  -- Set when a device was removed and the keys haven't been rotated yet; the
  -- next approved device to come online rotates them.
  rotate_needed boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

create table if not exists sync.devices (
  -- The device's own id (a UUIDv7 it made); not secret.
  id uuid primary key,
  user_id uuid not null references auth.users (id) on delete cascade,
  -- The auth session this device signs in with (the access token's
  -- session_id claim). device_ok() matches on it.
  session_id uuid,
  status text not null check (status in ('pending', 'approved', 'revoked')),
  platform text not null check (platform in ('windows', 'macos', 'linux', 'ios', 'android', 'web', 'other')),
  x25519_pub bytea not null check (octet_length(x25519_pub) = 32),
  ed25519_pub bytea not null check (octet_length(ed25519_pub) = 32),
  -- SHA-256 of a random secret only the device knows: it re-binds the row to
  -- a new auth session after signing in again, and asks whether it was
  -- revoked without a session.
  rebind_verifier bytea not null check (octet_length(rebind_verifier) = 32),
  -- Pairing (pending devices only): an HMAC over the device's id and public
  -- keys, and its name sealed under a key derived (Argon2id) from the 6-word
  -- pairing code shown on its screen. The approving device checks the HMAC
  -- before it wraps anything, so the server can't swap the keys.
  pair_mac bytea check (pair_mac is null or octet_length(pair_mac) = 32),
  pair_blob bytea check (pair_blob is null or octet_length(pair_blob) <= 1024),
  pending_expires_at timestamptz,
  created_at timestamptz not null default now(),
  approved_at timestamptz,
  revoked_at timestamptz,
  last_seen_at timestamptz
);
create index if not exists devices_user on sync.devices (user_id);
create unique index if not exists devices_session on sync.devices (session_id) where status <> 'revoked';

-- The AMK, wrapped (OBE1: an ephemeral X25519 key, HKDF-SHA256,
-- XChaCha20-Poly1305) to one approved device or to the Recovery Kit.
create table if not exists sync.key_envelopes (
  user_id uuid not null references auth.users (id) on delete cascade,
  -- A device id as text, or 'kit'.
  recipient text not null check (recipient = 'kit' or recipient ~ '^[0-9a-f-]{36}$'),
  device_id uuid references sync.devices (id) on delete cascade,
  amk_version integer not null,
  envelope bytea not null check (substring(envelope from 1 for 4) = '\x4f424531'::bytea and octet_length(envelope) <= 512),
  created_at timestamptz not null default now(),
  primary key (user_id, recipient, amk_version)
);

-- M3: one personal space per account. Clinic spaces come after M3.
create table if not exists sync.spaces (
  id uuid primary key,
  owner uuid not null references auth.users (id) on delete cascade,
  kind text not null default 'personal' check (kind in ('personal')),
  -- The current space key version (rotation adds one).
  key_version integer not null default 1 check (key_version >= 1),
  -- The pull cursor: every committed change takes the next number, under the
  -- space's row lock, so seq order is commit order.
  seq bigint not null default 0,
  created_at timestamptz not null default now()
);
create unique index if not exists spaces_personal on sync.spaces (owner) where kind = 'personal';

create table if not exists sync.space_members (
  space_id uuid not null references sync.spaces (id) on delete cascade,
  user_id uuid not null references auth.users (id) on delete cascade,
  role text not null default 'owner' check (role in ('owner')),
  primary key (space_id, user_id)
);

-- The space key and the naming key, sealed (OBW1) under the AMK.
create table if not exists sync.space_keys (
  space_id uuid not null references sync.spaces (id) on delete cascade,
  version integer not null,
  amk_version integer not null,
  wrapped bytea not null check (substring(wrapped from 1 for 4) = '\x4f425731'::bytea and octet_length(wrapped) <= 512),
  created_at timestamptz not null default now(),
  primary key (space_id, version)
);

-- Sync objects: OBS1 ciphertext, padded to 1, 4 or 8 KiB (or whole 8 KiB
-- steps). `id` is a random-looking id derived on the device; nothing here
-- names a visit, a patient or a folder.
create table if not exists sync.objects (
  space_id uuid not null references sync.spaces (id) on delete cascade,
  id uuid not null,
  kind text not null check (kind in ('visit', 'run', 'event', 'checks', 'transcript', 'patient', 'tombstone')),
  version integer not null check (version >= 1),
  seq bigint not null,
  body bytea check (body is null or (substring(body from 1 for 4) = '\x4f425331'::bytea and octet_length(body) <= 73728)),
  -- The object's key (DEK), wrapped (OBW1) under a space key version.
  -- Rotation re-wraps it; the body is never re-encrypted.
  wrapped_dek bytea check (wrapped_dek is null or (substring(wrapped_dek from 1 for 4) = '\x4f425731'::bytea and octet_length(wrapped_dek) <= 256)),
  key_version integer,
  padded_size integer,
  deleted boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz,
  primary key (space_id, id),
  check ((deleted and body is null and wrapped_dek is null) or (not deleted and body is not null and wrapped_dek is not null))
);
create index if not exists objects_seq on sync.objects (space_id, seq);

-- Blob parts (transcripts and note runs; never audio): OBB1 ciphertext, the
-- OBA1 chunk layout under its own magic, padded to whole 64 KiB chunks. Each
-- belongs to one version of one object, whose sealed manifest lists the
-- parts and their SHA-256. The OBA1 magic (an encrypted recording on disk)
-- is refused here as well as on the device.
create table if not exists sync.blobs (
  space_id uuid not null references sync.spaces (id) on delete cascade,
  object_id uuid not null,
  object_version integer not null check (object_version >= 1),
  part integer not null check (part >= 0 and part < 64),
  body bytea not null check (substring(body from 1 for 4) = '\x4f424231'::bytea and octet_length(body) <= 1100000),
  created_at timestamptz not null default now(),
  primary key (space_id, object_id, object_version, part)
);

-- ---------------------------------------------------------------------------
-- Who is asking
-- ---------------------------------------------------------------------------

-- The auth session of this request (the access token's session_id claim).
create or replace function sync.my_session()
returns uuid
language sql
stable
set search_path = ''
as $$
  select nullif(auth.jwt() ->> 'session_id', '')::uuid
$$;

-- The device bound to this request's session (pending or approved).
create or replace function sync.my_device()
returns uuid
language sql
stable
security definer
set search_path = ''
as $$
  select d.id from sync.devices d
  where d.user_id = auth.uid() and d.session_id = sync.my_session() and d.status <> 'revoked'
$$;

-- True only when this request's session belongs to an approved device of the
-- signed-in account. Every policy and every write calls it.
create or replace function sync.device_ok()
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select exists (
    select 1 from sync.devices d
    where d.user_id = auth.uid() and d.session_id = sync.my_session() and d.status = 'approved'
  )
$$;

create or replace function sync.fail(code text)
returns void
language plpgsql
set search_path = ''
as $$
begin
  raise exception using errcode = 'P0001', message = 'openbark:' || code;
end;
$$;

-- base64 text to bytes, with a size limit.
create or replace function sync.b64(t text, max_len integer)
returns bytea
language plpgsql
immutable
set search_path = ''
as $$
declare
  out bytea;
begin
  if t is null then
    return null;
  end if;
  begin
    out := decode(t, 'base64');
  exception when others then
    perform sync.fail('bad_base64');
  end;
  if octet_length(out) > max_len then
    perform sync.fail('too_large');
  end if;
  return out;
end;
$$;

-- bytes to base64 text without line breaks.
create or replace function sync.enc(b bytea)
returns text
language sql
immutable
set search_path = ''
as $$
  select replace(encode(b, 'base64'), E'\n', '')
$$;

create or replace function sync.require_device()
returns uuid
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  dev uuid;
begin
  if auth.uid() is null then
    perform sync.fail('not_signed_in');
  end if;
  select d.id into dev from sync.devices d
  where d.user_id = auth.uid() and d.session_id = sync.my_session() and d.status = 'approved';
  if dev is null then
    perform sync.fail('not_approved');
  end if;
  return dev;
end;
$$;

-- ---------------------------------------------------------------------------
-- Row level security: own account, approved devices
-- ---------------------------------------------------------------------------

alter table sync.accounts enable row level security;
alter table sync.devices enable row level security;
alter table sync.key_envelopes enable row level security;
alter table sync.spaces enable row level security;
alter table sync.space_members enable row level security;
alter table sync.space_keys enable row level security;
alter table sync.objects enable row level security;
alter table sync.blobs enable row level security;

revoke all on all tables in schema sync from public, anon, authenticated;
grant select on sync.accounts, sync.key_envelopes, sync.spaces, sync.space_members, sync.space_keys,
  sync.objects, sync.blobs to authenticated;
-- Not the session id or the rebind verifier.
grant select (id, user_id, status, platform, x25519_pub, ed25519_pub, pair_mac, pair_blob, pending_expires_at,
  created_at, approved_at, revoked_at, last_seen_at) on sync.devices to authenticated;
grant all on all tables in schema sync to service_role;

drop policy if exists "Own account, approved device" on sync.accounts;
create policy "Own account, approved device" on sync.accounts
  for select to authenticated
  using (user_id = (select auth.uid()) and (select sync.device_ok()));

-- A pending device sees only its own row.
drop policy if exists "Own devices" on sync.devices;
create policy "Own devices" on sync.devices
  for select to authenticated
  using (user_id = (select auth.uid()) and ((select sync.device_ok()) or id = (select sync.my_device())));

-- A device sees only the envelopes addressed to it, plus the kit's (which
-- only the Recovery Kit opens; a new device recovering with the kit needs it).
drop policy if exists "Own envelopes" on sync.key_envelopes;
create policy "Own envelopes" on sync.key_envelopes
  for select to authenticated
  using (user_id = (select auth.uid()) and (device_id = (select sync.my_device()) or recipient = 'kit'));

drop policy if exists "Member, approved device" on sync.spaces;
create policy "Member, approved device" on sync.spaces
  for select to authenticated
  using (owner = (select auth.uid()) and (select sync.device_ok()));

drop policy if exists "Member, approved device" on sync.space_members;
create policy "Member, approved device" on sync.space_members
  for select to authenticated
  using (user_id = (select auth.uid()) and (select sync.device_ok()));

drop policy if exists "Member, approved device" on sync.space_keys;
create policy "Member, approved device" on sync.space_keys
  for select to authenticated
  using ((select sync.device_ok()) and exists (
    select 1 from sync.space_members m where m.space_id = space_keys.space_id and m.user_id = (select auth.uid())));

drop policy if exists "Member, approved device" on sync.objects;
create policy "Member, approved device" on sync.objects
  for select to authenticated
  using ((select sync.device_ok()) and exists (
    select 1 from sync.space_members m where m.space_id = objects.space_id and m.user_id = (select auth.uid())));

drop policy if exists "Member, approved device" on sync.blobs;
create policy "Member, approved device" on sync.blobs
  for select to authenticated
  using ((select sync.device_ok()) and exists (
    select 1 from sync.space_members m where m.space_id = blobs.space_id and m.user_id = (select auth.uid())));

-- ---------------------------------------------------------------------------
-- Account and devices
-- ---------------------------------------------------------------------------

-- What this session is: whether the account has sync, and this device's
-- status. Touches last_seen_at for an approved device.
create or replace function sync.account_state()
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  acct sync.accounts;
  dev sync.devices;
  sp sync.spaces;
begin
  if auth.uid() is null then
    perform sync.fail('not_signed_in');
  end if;
  select * into acct from sync.accounts where user_id = auth.uid();
  select * into dev from sync.devices
    where user_id = auth.uid() and session_id = sync.my_session() and status <> 'revoked';
  if dev.id is not null and dev.status = 'approved' then
    update sync.devices set last_seen_at = now() where id = dev.id;
    select * into sp from sync.spaces where owner = auth.uid() and kind = 'personal';
  end if;
  return jsonb_build_object(
    'account', acct.user_id is not null,
    'device', case when dev.id is null then null else jsonb_build_object('id', dev.id, 'status', dev.status) end,
    'amk_version', case when dev.status = 'approved' then acct.amk_version end,
    'roster_version', case when dev.status = 'approved' then acct.roster_version end,
    'rotate_needed', case when dev.status = 'approved' then acct.rotate_needed end,
    'space', case when sp.id is null then null else jsonb_build_object('id', sp.id, 'key_version', sp.key_version, 'seq', sp.seq) end
  );
end;
$$;

-- The first device turns on sync: the account, its space, the first space
-- key, this device (approved), and the AMK wrapped to this device and to the
-- Recovery Kit. Everything arrives sealed.
create or replace function sync.create_account(
  device_id uuid, platform text, x25519_pub text, ed25519_pub text, rebind_verifier text,
  kit_pub text, kit_verifier text, space_id uuid, device_envelope text, kit_envelope text,
  space_key text, roster text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  sess uuid := sync.my_session();
begin
  if uid is null or sess is null then
    perform sync.fail('not_signed_in');
  end if;
  if exists (select 1 from sync.accounts where user_id = uid) then
    perform sync.fail('account_exists');
  end if;
  if exists (select 1 from sync.devices d where d.id = create_account.device_id) then
    perform sync.fail('device_exists');
  end if;
  insert into sync.accounts (user_id, kit_x25519_pub, kit_verifier, roster)
    values (uid, sync.b64(kit_pub, 32), sync.b64(kit_verifier, 32), sync.b64(roster, 65536));
  insert into sync.devices (id, user_id, session_id, status, platform, x25519_pub, ed25519_pub, rebind_verifier,
      approved_at, last_seen_at)
    values (create_account.device_id, uid, sess, 'approved', create_account.platform, sync.b64(x25519_pub, 32),
      sync.b64(ed25519_pub, 32), sync.b64(rebind_verifier, 32), now(), now());
  insert into sync.key_envelopes (user_id, recipient, device_id, amk_version, envelope)
    values (uid, create_account.device_id::text, create_account.device_id, 1, sync.b64(device_envelope, 512)),
           (uid, 'kit', null, 1, sync.b64(kit_envelope, 512));
  insert into sync.spaces (id, owner) values (create_account.space_id, uid);
  insert into sync.space_members (space_id, user_id) values (create_account.space_id, uid);
  insert into sync.space_keys (space_id, version, amk_version, wrapped)
    values (create_account.space_id, 1, 1, sync.b64(space_key, 512));
  return sync.account_state();
end;
$$;

-- A new device asks to join: pending for 10 minutes, bound to this session.
create or replace function sync.register_device(
  device_id uuid, platform text, x25519_pub text, ed25519_pub text, rebind_verifier text,
  pair_mac text, pair_blob text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  sess uuid := sync.my_session();
  existing sync.devices;
begin
  if uid is null or sess is null then
    perform sync.fail('not_signed_in');
  end if;
  if not exists (select 1 from sync.accounts where user_id = uid) then
    perform sync.fail('no_account');
  end if;
  select * into existing from sync.devices d where d.id = register_device.device_id;
  -- A removed device may ask again (it gets new keys and needs a new
  -- approval, like any new device); an approved one rebinds instead.
  if existing.id is not null and (existing.user_id <> uid or existing.status = 'approved') then
    perform sync.fail('device_exists');
  end if;
  if (select count(*) from sync.devices d
      where d.user_id = uid and d.status = 'pending' and d.pending_expires_at > now()
        and d.id <> register_device.device_id) >= 5 then
    perform sync.fail('too_many_pending');
  end if;
  -- One device per session: an older pending row on this session goes.
  delete from sync.devices d where d.session_id = sess and d.status = 'pending' and d.id <> register_device.device_id;
  insert into sync.devices (id, user_id, session_id, status, platform, x25519_pub, ed25519_pub, rebind_verifier,
      pair_mac, pair_blob, pending_expires_at)
    values (register_device.device_id, uid, sess, 'pending', register_device.platform, sync.b64(x25519_pub, 32),
      sync.b64(ed25519_pub, 32), sync.b64(rebind_verifier, 32), sync.b64(pair_mac, 32), sync.b64(pair_blob, 1024),
      now() + interval '10 minutes')
  on conflict (id) do update set
    status = 'pending', approved_at = null, revoked_at = null,
    session_id = excluded.session_id, platform = excluded.platform, x25519_pub = excluded.x25519_pub,
    ed25519_pub = excluded.ed25519_pub, rebind_verifier = excluded.rebind_verifier, pair_mac = excluded.pair_mac,
    pair_blob = excluded.pair_blob, pending_expires_at = excluded.pending_expires_at;
  return sync.account_state();
end;
$$;

-- An approved device signed in again (a new auth session): bind its row to
-- the new session, proven by the device's own secret. A revoked device gets
-- 'revoked' and nothing else.
create or replace function sync.rebind_device(device_id uuid, rebind_secret text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  sess uuid := sync.my_session();
  dev sync.devices;
begin
  if uid is null or sess is null then
    perform sync.fail('not_signed_in');
  end if;
  select * into dev from sync.devices d where d.id = rebind_device.device_id and d.user_id = uid;
  if dev.id is null or dev.rebind_verifier <> sha256(sync.b64(rebind_secret, 64)) then
    perform sync.fail('unknown_device');
  end if;
  if dev.status = 'revoked' then
    return jsonb_build_object('device', jsonb_build_object('id', dev.id, 'status', 'revoked'));
  end if;
  update sync.devices set session_id = null where session_id = sess and id <> dev.id and status <> 'revoked';
  update sync.devices set session_id = sess, last_seen_at = now() where id = dev.id;
  return sync.account_state();
end;
$$;

-- Without a session (its refresh token may be gone): was this device
-- revoked? Proven by the device's own secret; says nothing else.
create or replace function sync.device_status(device_id uuid, rebind_secret text)
returns text
language plpgsql
security definer
set search_path = ''
as $$
declare
  dev sync.devices;
begin
  select * into dev from sync.devices d where d.id = device_status.device_id;
  if dev.id is null or dev.rebind_verifier <> sha256(sync.b64(rebind_secret, 64)) then
    return 'unknown';
  end if;
  return dev.status;
end;
$$;

-- An approved device approves a pending one: the AMK wrapped to it, and the
-- roster with it added (both made on the approving device).
create or replace function sync.approve_device(
  device_id uuid, envelope text, amk_version integer, roster text, roster_version integer)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  target sync.devices;
  acct sync.accounts;
begin
  perform sync.require_device();
  select * into acct from sync.accounts where user_id = uid for update;
  select * into target from sync.devices d where d.id = approve_device.device_id and d.user_id = uid for update;
  if target.id is null or target.status <> 'pending' then
    perform sync.fail('not_pending');
  end if;
  if target.pending_expires_at < now() then
    perform sync.fail('expired');
  end if;
  if approve_device.amk_version <> acct.amk_version then
    perform sync.fail('stale_amk');
  end if;
  if approve_device.roster_version <> acct.roster_version + 1 then
    perform sync.fail('stale_roster');
  end if;
  insert into sync.key_envelopes (user_id, recipient, device_id, amk_version, envelope)
    values (uid, target.id::text, target.id, acct.amk_version, sync.b64(envelope, 512))
  on conflict on constraint key_envelopes_pkey do update set envelope = excluded.envelope;
  update sync.devices set status = 'approved', approved_at = now(), pair_mac = null, pair_blob = null,
    pending_expires_at = null where id = target.id;
  update sync.accounts set roster = sync.b64(approve_device.roster, 65536),
    roster_version = approve_device.roster_version, updated_at = now() where user_id = uid;
  return jsonb_build_object('approved', target.id);
end;
$$;

-- A pending device that holds the Recovery Kit approves itself: the kit
-- token proves it (its SHA-256 is the account's kit_verifier). The device
-- opened the kit's envelope itself and wrapped the AMK to its own key. It
-- can't read the roster while pending, so it adds itself with set_roster
-- straight after, before it seals anything.
create or replace function sync.approve_with_kit(kit_token text, envelope text, amk_version integer)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  me sync.devices;
  acct sync.accounts;
begin
  if uid is null then
    perform sync.fail('not_signed_in');
  end if;
  select * into acct from sync.accounts where user_id = uid for update;
  select * into me from sync.devices d
    where d.user_id = uid and d.session_id = sync.my_session() and d.status = 'pending' for update;
  if me.id is null then
    perform sync.fail('not_pending');
  end if;
  if acct.kit_verifier <> sha256(sync.b64(kit_token, 64)) then
    perform sync.fail('wrong_kit');
  end if;
  if approve_with_kit.amk_version <> acct.amk_version then
    perform sync.fail('stale_amk');
  end if;
  insert into sync.key_envelopes (user_id, recipient, device_id, amk_version, envelope)
    values (uid, me.id::text, me.id, acct.amk_version, sync.b64(envelope, 512))
  on conflict on constraint key_envelopes_pkey do update set envelope = excluded.envelope;
  update sync.devices set status = 'approved', approved_at = now(), pair_mac = null, pair_blob = null,
    pending_expires_at = null, last_seen_at = now() where id = me.id;
  return sync.account_state();
end;
$$;

-- The shared part of removing a device: revoked at once (device_ok() turns
-- false), its envelopes gone, its auth session deleted so its refresh token
-- stops working, and the keys marked for rotation.
create or replace function sync.revoke(uid uuid, target uuid)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  dev sync.devices;
begin
  select * into dev from sync.devices d where d.id = target and d.user_id = uid for update;
  if dev.id is null then
    perform sync.fail('unknown_device');
  end if;
  if dev.status = 'revoked' then
    return jsonb_build_object('revoked', dev.id);
  end if;
  update sync.devices set status = 'revoked', revoked_at = now(), pair_mac = null, pair_blob = null
    where id = dev.id;
  delete from sync.key_envelopes e where e.user_id = uid and e.device_id = dev.id;
  if dev.status = 'approved' then
    update sync.accounts set rotate_needed = true, updated_at = now() where user_id = uid;
  end if;
  if dev.session_id is not null then
    begin
      delete from auth.sessions s where s.id = dev.session_id;
    exception when insufficient_privilege then
      -- device_ok() already refuses it; its refresh token lasts until it
      -- next tries and is refused.
      raise warning 'openbark: could not delete the auth session of a revoked device';
    end;
  end if;
  return jsonb_build_object('revoked', dev.id);
end;
$$;

-- From an approved device (it may remove itself).
create or replace function sync.revoke_device(device_id uuid)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
begin
  perform sync.require_device();
  return sync.revoke(auth.uid(), revoke_device.device_id);
end;
$$;

-- From the website portal, through the revoke-device Edge Function, which
-- checks the person's token itself and calls this with the service role.
create or replace function sync.revoke_device_for(user_id uuid, device_id uuid)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
begin
  return sync.revoke(revoke_device_for.user_id, revoke_device_for.device_id);
end;
$$;

-- Replace the roster (a removed device marked, a name changed).
create or replace function sync.set_roster(roster text, roster_version integer)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  acct sync.accounts;
begin
  perform sync.require_device();
  select * into acct from sync.accounts where user_id = auth.uid() for update;
  if set_roster.roster_version <> acct.roster_version + 1 then
    perform sync.fail('stale_roster');
  end if;
  update sync.accounts set roster = sync.b64(set_roster.roster, 65536), roster_version = set_roster.roster_version,
    updated_at = now() where user_id = auth.uid();
  return jsonb_build_object('roster_version', set_roster.roster_version);
end;
$$;

-- The roster and this account's kit public key (approved devices).
create or replace function sync.get_account()
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  acct sync.accounts;
begin
  perform sync.require_device();
  select * into acct from sync.accounts where user_id = auth.uid();
  return jsonb_build_object('amk_version', acct.amk_version, 'kit_pub', sync.enc(acct.kit_x25519_pub),
    'roster', sync.enc(acct.roster), 'roster_version', acct.roster_version, 'rotate_needed', acct.rotate_needed);
end;
$$;

-- Key rotation after a device was removed (ADR-0017 section 3): a new AMK
-- wrapped to every remaining approved device and to the kit, a new space
-- key version, every object's DEK re-wrapped (no object is re-encrypted),
-- and the roster. All or nothing; refused if the devices or objects changed
-- meanwhile, so nothing is left behind under the old keys.
create or replace function sync.rotate(
  amk_version integer, envelopes jsonb, kit_envelope text, space_id uuid, key_version integer,
  space_key text, roster text, roster_version integer, rewrapped jsonb)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  acct sync.accounts;
  sp sync.spaces;
  approved uuid[];
  given uuid[];
  live_count integer;
  given_count integer;
begin
  perform sync.require_device();
  select * into acct from sync.accounts where user_id = uid for update;
  select * into sp from sync.spaces s where s.id = rotate.space_id and s.owner = uid for update;
  if sp.id is null then
    perform sync.fail('unknown_space');
  end if;
  if rotate.amk_version <> acct.amk_version + 1 or rotate.key_version <> sp.key_version + 1 then
    perform sync.fail('stale_amk');
  end if;
  if rotate.roster_version <> acct.roster_version + 1 then
    perform sync.fail('stale_roster');
  end if;
  select coalesce(array_agg(d.id order by d.id), '{}') into approved
    from sync.devices d where d.user_id = uid and d.status = 'approved';
  select coalesce(array_agg((e ->> 'device_id')::uuid order by (e ->> 'device_id')::uuid), '{}') into given
    from jsonb_array_elements(rotate.envelopes) e;
  if approved <> given then
    perform sync.fail('devices_changed');
  end if;
  select count(*) into live_count from sync.objects o where o.space_id = sp.id and not o.deleted;
  select count(*) into given_count from jsonb_array_elements(rotate.rewrapped) r
    join sync.objects o on o.space_id = sp.id and o.id = (r ->> 'id')::uuid and not o.deleted
      and o.version = (r ->> 'version')::integer;
  if live_count <> given_count or given_count <> jsonb_array_length(rotate.rewrapped) then
    perform sync.fail('objects_changed');
  end if;

  insert into sync.key_envelopes (user_id, recipient, device_id, amk_version, envelope)
    select uid, e ->> 'device_id', (e ->> 'device_id')::uuid, rotate.amk_version, sync.b64(e ->> 'envelope', 512)
    from jsonb_array_elements(rotate.envelopes) e;
  insert into sync.key_envelopes (user_id, recipient, device_id, amk_version, envelope)
    values (uid, 'kit', null, rotate.amk_version, sync.b64(kit_envelope, 512));
  delete from sync.key_envelopes e where e.user_id = uid and e.amk_version < rotate.amk_version;

  insert into sync.space_keys (space_id, version, amk_version, wrapped)
    values (sp.id, rotate.key_version, rotate.amk_version, sync.b64(space_key, 512));
  delete from sync.space_keys k where k.space_id = sp.id and k.version < rotate.key_version;

  update sync.objects o set wrapped_dek = sync.b64(r ->> 'wrapped_dek', 256), key_version = rotate.key_version
    from jsonb_array_elements(rotate.rewrapped) r
    where o.space_id = sp.id and o.id = (r ->> 'id')::uuid;

  update sync.spaces s set key_version = rotate.key_version where s.id = sp.id;
  update sync.accounts set amk_version = rotate.amk_version, roster = sync.b64(rotate.roster, 65536),
    roster_version = rotate.roster_version, rotate_needed = false, updated_at = now() where user_id = uid;
  return sync.account_state();
end;
$$;

-- Every live object's id, version and wrapped DEK (for rotation).
create or replace function sync.dek_list(space_id uuid)
returns jsonb
language sql
stable
security invoker
set search_path = ''
as $$
  select coalesce(jsonb_agg(jsonb_build_object('id', o.id, 'version', o.version,
    'wrapped_dek', sync.enc(o.wrapped_dek), 'key_version', o.key_version) order by o.id), '[]'::jsonb)
  from sync.objects o where o.space_id = dek_list.space_id and not o.deleted
$$;

-- ---------------------------------------------------------------------------
-- Push and pull
-- ---------------------------------------------------------------------------

-- Commit sealed objects. Takes the space's row lock, so `seq` order is
-- commit order and a late commit is never skipped by a pull. Each item:
--   {"op": "put"|"delete", "id", "kind", "base_version" (null when new),
--    "body", "wrapped_dek", "key_version", "padded_size"}
-- Results, in order: ok (with version and seq), conflict (with the current
-- row), exists (an immutable object already there), stale_key (sealed under
-- an old space key version), deleted.
create or replace function sync.push(space_id uuid, items jsonb)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
  sp sync.spaces;
  item jsonb;
  cur sync.objects;
  results jsonb := '[]'::jsonb;
  op text;
  base integer;
  next_seq bigint;
begin
  perform sync.require_device();
  if jsonb_typeof(items) <> 'array' or jsonb_array_length(items) > 100 then
    perform sync.fail('bad_items');
  end if;
  select * into sp from sync.spaces s
    where s.id = push.space_id
      and exists (select 1 from sync.space_members m where m.space_id = s.id and m.user_id = uid)
    for update;
  if sp.id is null then
    perform sync.fail('unknown_space');
  end if;
  next_seq := sp.seq;
  for item in select * from jsonb_array_elements(items) loop
    op := coalesce(item ->> 'op', 'put');
    base := (item ->> 'base_version')::integer;
    select * into cur from sync.objects o where o.space_id = sp.id and o.id = (item ->> 'id')::uuid for update;
    if op = 'delete' then
      if cur.id is null or cur.deleted then
        results := results || jsonb_build_object('id', item ->> 'id', 'status', 'ok');
        continue;
      end if;
      next_seq := next_seq + 1;
      update sync.objects o set deleted = true, body = null, wrapped_dek = null, key_version = null,
        padded_size = null, version = cur.version + 1, seq = next_seq, updated_at = now(), deleted_at = now()
        where o.space_id = sp.id and o.id = cur.id;
      delete from sync.blobs b where b.space_id = sp.id and b.object_id = cur.id;
      results := results || jsonb_build_object('id', cur.id, 'status', 'ok', 'version', cur.version + 1, 'seq', next_seq);
      continue;
    elsif op <> 'put' then
      perform sync.fail('bad_items');
    end if;
    if (item ->> 'key_version')::integer is distinct from sp.key_version then
      results := results || jsonb_build_object('id', item ->> 'id', 'status', 'stale_key', 'key_version', sp.key_version);
      continue;
    end if;
    if cur.id is not null then
      if cur.deleted then
        results := results || jsonb_build_object('id', cur.id, 'status', 'deleted', 'version', cur.version);
        continue;
      end if;
      if cur.kind in ('run', 'event') then
        results := results || jsonb_build_object('id', cur.id, 'status', 'exists', 'version', cur.version, 'seq', cur.seq);
        continue;
      end if;
      if base is distinct from cur.version then
        results := results || jsonb_build_object('id', cur.id, 'status', 'conflict', 'current', jsonb_build_object(
          'id', cur.id, 'kind', cur.kind, 'version', cur.version, 'seq', cur.seq, 'body', sync.enc(cur.body),
          'wrapped_dek', sync.enc(cur.wrapped_dek), 'key_version', cur.key_version, 'deleted', cur.deleted));
        continue;
      end if;
      if cur.kind <> item ->> 'kind' then
        perform sync.fail('kind_changed');
      end if;
    elsif base is not null then
      results := results || jsonb_build_object('id', item ->> 'id', 'status', 'conflict', 'current', null);
      continue;
    end if;
    next_seq := next_seq + 1;
    insert into sync.objects (space_id, id, kind, version, seq, body, wrapped_dek, key_version, padded_size)
      values (sp.id, (item ->> 'id')::uuid, item ->> 'kind', coalesce(cur.version, 0) + 1, next_seq,
        sync.b64(item ->> 'body', 73728), sync.b64(item ->> 'wrapped_dek', 256), sp.key_version,
        (item ->> 'padded_size')::integer)
    on conflict on constraint objects_pkey do update set
      version = excluded.version, seq = excluded.seq, body = excluded.body, wrapped_dek = excluded.wrapped_dek,
      key_version = excluded.key_version, padded_size = excluded.padded_size, updated_at = now();
    -- Parts of older versions go with them.
    delete from sync.blobs b where b.space_id = sp.id and b.object_id = (item ->> 'id')::uuid
      and b.object_version < coalesce(cur.version, 0) + 1;
    results := results || jsonb_build_object('id', item ->> 'id', 'status', 'ok',
      'version', coalesce(cur.version, 0) + 1, 'seq', next_seq);
  end loop;
  update sync.spaces s set seq = next_seq where s.id = sp.id;
  update sync.devices d set last_seen_at = now() where d.user_id = uid and d.session_id = sync.my_session();
  return results;
end;
$$;

-- Upload one blob part for an object version that isn't committed yet. A
-- committed version's parts never change.
create or replace function sync.put_blob(space_id uuid, object_id uuid, object_version integer, part integer, body text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := auth.uid();
begin
  perform sync.require_device();
  if not exists (select 1 from sync.space_members m where m.space_id = put_blob.space_id and m.user_id = uid) then
    perform sync.fail('unknown_space');
  end if;
  if exists (select 1 from sync.objects o where o.space_id = put_blob.space_id and o.id = put_blob.object_id
      and o.version >= put_blob.object_version) then
    perform sync.fail('committed');
  end if;
  insert into sync.blobs (space_id, object_id, object_version, part, body)
    values (put_blob.space_id, put_blob.object_id, put_blob.object_version, put_blob.part, sync.b64(body, 1100000))
  on conflict on constraint blobs_pkey do update set body = excluded.body, created_at = now();
  return jsonb_build_object('ok', true);
end;
$$;

-- Rows after a cursor, oldest first. Security invoker: row level security
-- decides what comes back (nothing for a pending or revoked device).
create or replace function sync.pull(space_id uuid, after bigint, max_rows integer)
returns jsonb
language sql
stable
security invoker
set search_path = ''
as $$
  select jsonb_build_object(
    'head', (select s.seq from sync.spaces s where s.id = pull.space_id),
    'rows', coalesce((select jsonb_agg(jsonb_build_object(
        'id', o.id, 'kind', o.kind, 'version', o.version, 'seq', o.seq, 'body', sync.enc(o.body),
        'wrapped_dek', sync.enc(o.wrapped_dek), 'key_version', o.key_version, 'deleted', o.deleted) order by o.seq)
      from (select * from sync.objects o2 where o2.space_id = pull.space_id and o2.seq > pull.after
            order by o2.seq limit least(greatest(pull.max_rows, 1), 500)) o), '[]'::jsonb))
$$;

-- One object version's blob parts, in order (security invoker, as pull).
create or replace function sync.get_blobs(space_id uuid, object_id uuid, object_version integer)
returns jsonb
language sql
stable
security invoker
set search_path = ''
as $$
  select coalesce(jsonb_agg(jsonb_build_object('part', b.part, 'body', sync.enc(b.body)) order by b.part), '[]'::jsonb)
  from sync.blobs b
  where b.space_id = get_blobs.space_id and b.object_id = get_blobs.object_id
    and b.object_version = get_blobs.object_version
$$;

-- This device's AMK envelope(s) and the space keys (approved devices, or
-- a pending device collecting its own envelope).
create or replace function sync.my_keys()
returns jsonb
language sql
stable
security invoker
set search_path = ''
as $$
  select jsonb_build_object(
    'envelopes', coalesce((select jsonb_agg(jsonb_build_object('recipient', e.recipient, 'amk_version', e.amk_version,
        'envelope', sync.enc(e.envelope)) order by e.amk_version) from sync.key_envelopes e), '[]'::jsonb),
    'space_keys', coalesce((select jsonb_agg(jsonb_build_object('space_id', k.space_id, 'version', k.version,
        'amk_version', k.amk_version, 'wrapped', sync.enc(k.wrapped)) order by k.version) from sync.space_keys k), '[]'::jsonb))
$$;

-- The account's devices, as the server knows them: ids, status, platform,
-- public keys, times, and (pending only) the pairing values. Names are in
-- the sealed roster, not here.
create or replace function sync.list_devices()
returns jsonb
language sql
stable
security invoker
set search_path = ''
as $$
  select coalesce(jsonb_agg(jsonb_build_object('id', d.id, 'status', d.status, 'platform', d.platform,
    'x25519_pub', sync.enc(d.x25519_pub), 'ed25519_pub', sync.enc(d.ed25519_pub), 'pair_mac', sync.enc(d.pair_mac),
    'pair_blob', sync.enc(d.pair_blob), 'pending_expires_at', d.pending_expires_at, 'created_at', d.created_at,
    'approved_at', d.approved_at, 'revoked_at', d.revoked_at, 'last_seen_at', d.last_seen_at) order by d.created_at), '[]'::jsonb)
  from sync.devices d
$$;

-- ---------------------------------------------------------------------------
-- Housekeeping
-- ---------------------------------------------------------------------------

-- Remove what's no longer needed: deleted objects after a day (their
-- bodies went at once), blob parts no committed version uses, pending
-- devices that expired a day ago. Run hourly by pg_cron where available.
create or replace function sync.purge()
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  n_objects integer;
  n_blobs integer;
  n_devices integer;
begin
  delete from sync.objects o where o.deleted and o.deleted_at < now() - interval '1 day';
  get diagnostics n_objects = row_count;
  delete from sync.blobs b where b.created_at < now() - interval '1 day' and not exists (
    select 1 from sync.objects o where o.space_id = b.space_id and o.id = b.object_id
      and o.version = b.object_version and not o.deleted);
  get diagnostics n_blobs = row_count;
  delete from sync.devices d where d.status = 'pending' and d.pending_expires_at < now() - interval '1 day';
  get diagnostics n_devices = row_count;
  return jsonb_build_object('objects', n_objects, 'blobs', n_blobs, 'devices', n_devices);
end;
$$;

-- ---------------------------------------------------------------------------
-- Function privileges: nothing for the public; the signed-in role gets the
-- functions above; the service role (Edge Functions) gets the portal's.
-- ---------------------------------------------------------------------------

revoke all on all functions in schema sync from public, anon, authenticated;
grant execute on function
  sync.my_session(), sync.my_device(), sync.device_ok(),
  sync.fail(text), sync.b64(text, integer), sync.enc(bytea), sync.require_device(),
  sync.account_state(),
  sync.create_account(uuid, text, text, text, text, text, text, uuid, text, text, text, text),
  sync.register_device(uuid, text, text, text, text, text, text),
  sync.rebind_device(uuid, text),
  sync.approve_device(uuid, text, integer, text, integer),
  sync.approve_with_kit(text, text, integer),
  sync.revoke_device(uuid),
  sync.set_roster(text, integer),
  sync.get_account(),
  sync.rotate(integer, jsonb, text, uuid, integer, text, text, integer, jsonb),
  sync.dek_list(uuid),
  sync.push(uuid, jsonb),
  sync.put_blob(uuid, uuid, integer, integer, text),
  sync.pull(uuid, bigint, integer),
  sync.get_blobs(uuid, uuid, integer),
  sync.my_keys(),
  sync.list_devices()
to authenticated;
grant execute on function sync.device_status(uuid, text), sync.fail(text), sync.b64(text, integer) to anon, authenticated;
grant execute on all functions in schema sync to service_role;

-- Hourly purge, where pg_cron is available (Supabase has it).
do $$
begin
  if exists (select 1 from pg_available_extensions where name = 'pg_cron') then
    create extension if not exists pg_cron;
    perform cron.unschedule(jobid) from cron.job where jobname = 'openbark-sync-purge';
    perform cron.schedule('openbark-sync-purge', '17 * * * *', 'select sync.purge()');
  end if;
exception when others then
  raise notice 'openbark: pg_cron not scheduled (%); run select sync.purge() from a scheduler', sqlerrm;
end;
$$;
