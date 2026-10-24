-- OpenBark hospital dashboard, phase 1 (ADR-0028): the `clinic` schema.
--
-- A clinic is an organisation of accounts on the same Supabase project as the
-- website portal and encrypted sync (ADR-0017). Admins (a practice manager or
-- medical director) invite people by email or with a one-off join code,
-- remove them, change roles, and see daily usage COUNTS that members choose
-- to share. Members see their clinic, their own counts, and can leave.
--
-- No clinical data, ever. The only free text this schema holds is a clinic
-- name, a member's display name (both length-limited, no control characters)
-- and the email address on a pending invite (deleted once it is accepted,
-- declined or revoked, and 30 days after it expires). Usage is one row per
-- member, device and day, holding typed counts with upper bounds: no patient
-- or owner names, no species, reasons, transcripts, notes or recordings, and
-- no time of any single visit.
--
-- Writes go only through the security-definer functions below, each of
-- which checks the caller itself. The signed-in role can SELECT a few tables
-- through row level security (its own membership rows, its clinics' names,
-- its own usage rows) and nothing else; anon gets nothing at all.
--
-- Deleting an account (the delete-account Edge Function) removes the
-- person's memberships and usage rows through `on delete cascade`. If they
-- were a clinic's last admin, the clinic goes too, with its invites, codes
-- and every member's usage rows (the trigger below): an account deletion is
-- never blocked. Removing a member, or a member leaving, keeps their past
-- usage rows; the dashboard shows them as "Former member", without a name.
--
-- Apply with `supabase db push` (hosted) or `supabase db reset` (local). The
-- `clinic` schema must be in the API's exposed schemas (config.toml
-- [api].schemas locally; Project Settings > Data API on a hosted project).

create schema if not exists clinic;
comment on schema clinic is
  'OpenBark clinics (ADR-0028): members, invites and daily usage counts. No clinical data. RLS: own rows; admins through functions.';

-- anon gets no usage: every clinic call needs a signed-in account.
revoke all on schema clinic from public, anon;
grant usage on schema clinic to authenticated, service_role;

-- ---------------------------------------------------------------------------
-- Tables
-- ---------------------------------------------------------------------------

create table if not exists clinic.clinics (
  id uuid primary key default gen_random_uuid(),
  name text not null check (char_length(name) between 1 and 120 and name = btrim(name) and name !~ '[[:cntrl:]]'),
  created_by uuid references auth.users (id) on delete set null,
  -- Invites and join codes made today (UTC), for the daily limit of 50.
  -- Counted here, not from the invites table, so revoking and inviting
  -- again doesn't reset it.
  invite_day date not null default current_date,
  invites_today integer not null default 0 check (invites_today between 0 and 1000),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);
comment on table clinic.clinics is
  'OpenBark clinics (ADR-0028). The name is the only text. RLS: members read their clinics'' names.';

create table if not exists clinic.members (
  clinic_id uuid not null references clinic.clinics (id) on delete cascade,
  user_id uuid not null references auth.users (id) on delete cascade,
  role text not null check (role in ('admin', 'member')),
  -- How the clinic's admins see this person; chosen by the person.
  display_name text not null check (char_length(display_name) between 1 and 80 and display_name = btrim(display_name)
    and display_name !~ '[[:cntrl:]]'),
  -- The person chose "Share usage counts with <clinic>" (off until they
  -- turn it on; ADR-0028 section 5). report_usage refuses while it's off.
  share_usage boolean not null default false,
  joined_at timestamptz not null default now(),
  primary key (clinic_id, user_id)
);
create index if not exists members_user on clinic.members (user_id);
comment on table clinic.members is
  'Who belongs to which clinic, their role and display name, and whether they share usage counts. RLS: own rows.';

-- A pending invite, keyed by the lower-cased email. Accepting, declining or
-- revoking deletes the row; an expired one is purged 30 days later.
create table if not exists clinic.invites (
  id uuid primary key default gen_random_uuid(),
  clinic_id uuid not null references clinic.clinics (id) on delete cascade,
  email text not null check (char_length(email) <= 254 and email = lower(email)
    and email ~ '^[^@[:space:][:cntrl:]]+@[^@[:space:][:cntrl:]]+\.[^@[:space:][:cntrl:]]+$'),
  role text not null check (role in ('admin', 'member')),
  invited_by uuid references auth.users (id) on delete set null,
  sent_count integer not null default 1 check (sent_count between 1 and 1000),
  created_at timestamptz not null default now(),
  last_sent_at timestamptz not null default now(),
  expires_at timestamptz not null default now() + interval '14 days',
  unique (clinic_id, email)
);
create index if not exists invites_email on clinic.invites (email);
comment on table clinic.invites is
  'Pending clinic invites by email (14 days). No table access: functions only.';

-- One-off join codes: 8 Crockford base32 characters (40 random bits), shown
-- once to the admin, stored only as SHA-256. Single use, member role only.
create table if not exists clinic.join_codes (
  id uuid primary key default gen_random_uuid(),
  clinic_id uuid not null references clinic.clinics (id) on delete cascade,
  code_hash bytea not null unique check (octet_length(code_hash) = 32),
  created_by uuid references auth.users (id) on delete set null,
  created_at timestamptz not null default now(),
  expires_at timestamptz not null default now() + interval '14 days'
);
comment on table clinic.join_codes is
  'One-off join codes, SHA-256 only (14 days, single use). No table access: functions only.';

-- Wrong join codes per account: 10 an hour, then a wait.
create table if not exists clinic.code_attempts (
  user_id uuid primary key references auth.users (id) on delete cascade,
  window_start timestamptz not null default now(),
  failures integer not null default 0 check (failures >= 0)
);
comment on table clinic.code_attempts is
  'Failed join-code attempts per account, for the hourly limit. No table access.';

-- Daily usage counts, one row per member, device and day. Phase 2 sends them
-- from the app (Rust, R15 in docs/network.md) once the member turns on
-- sharing; report_usage replaces the day's totals, so a retry is harmless.
-- The foreign keys go to the clinic and the account, never to the
-- membership: removing a member keeps their rows ("Former member").
create table if not exists clinic.usage_daily (
  clinic_id uuid not null references clinic.clinics (id) on delete cascade,
  user_id uuid not null references auth.users (id) on delete cascade,
  -- A random id the app makes for this clinic on this install (not the sync
  -- device id).
  device_id uuid not null,
  day date not null,
  platform text not null check (platform in ('windows', 'macos', 'linux', 'ios', 'web')),
  app_version text not null check (char_length(app_version) <= 40
    and app_version ~ '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$'),
  visits_recorded integer not null default 0 check (visits_recorded between 0 and 1000),
  minutes_recorded numeric(8, 1) not null default 0 check (minutes_recorded between 0 and 1440),
  notes_drafted integer not null default 0 check (notes_drafted between 0 and 5000),
  notes_signed integer not null default 0 check (notes_signed between 0 and 1000),
  calls_recorded integer not null default 0 check (calls_recorded between 0 and 1000),
  updated_at timestamptz not null default now(),
  primary key (clinic_id, user_id, device_id, day)
);
create index if not exists usage_clinic_day on clinic.usage_daily (clinic_id, day);
create index if not exists usage_user on clinic.usage_daily (user_id);
comment on table clinic.usage_daily is
  'Daily usage counts per member, device and day (ADR-0028). Counts only, kept 400 days. RLS: own rows; admins see aggregates through clinic_dashboard.';

-- ---------------------------------------------------------------------------
-- Helpers
-- ---------------------------------------------------------------------------

create or replace function clinic.fail(code text)
returns void
language plpgsql
set search_path = ''
as $$
begin
  raise exception using errcode = 'P0001', message = 'openbark:' || code;
end;
$$;

create or replace function clinic.require_user()
returns uuid
language plpgsql
stable
set search_path = ''
as $$
begin
  if auth.uid() is null then
    perform clinic.fail('not_signed_in');
  end if;
  return auth.uid();
end;
$$;

-- The caller's email, lower-cased, only once Supabase Auth has confirmed it
-- (an emailed code proves it). Invites match on this, never on a claim.
create or replace function clinic.my_email()
returns text
language sql
stable
security definer
set search_path = ''
as $$
  select lower(u.email) from auth.users u where u.id = auth.uid() and u.email_confirmed_at is not null
$$;

create or replace function clinic.role_of(cid uuid, uid uuid)
returns text
language sql
stable
security definer
set search_path = ''
as $$
  select m.role from clinic.members m where m.clinic_id = cid and m.user_id = uid
$$;

create or replace function clinic.require_admin(cid uuid, uid uuid)
returns void
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  if uid is null then
    perform clinic.fail('not_signed_in');
  end if;
  if clinic.role_of(cid, uid) is distinct from 'admin' then
    perform clinic.fail('not_admin');
  end if;
end;
$$;

-- A clinic or display name: trimmed, inner whitespace collapsed, 1 to `max`
-- characters, no control characters.
create or replace function clinic.clean_name(t text, max integer)
returns text
language plpgsql
immutable
set search_path = ''
as $$
declare
  v text := regexp_replace(btrim(coalesce(t, '')), '[[:space:]]+', ' ', 'g');
begin
  if v = '' or char_length(v) > max or v ~ '[[:cntrl:]]' then
    perform clinic.fail('bad_name');
  end if;
  return v;
end;
$$;

-- An email address as typed: lower-cased and trimmed, or null when it
-- doesn't look like one.
create or replace function clinic.norm_email(t text)
returns text
language sql
immutable
set search_path = ''
as $$
  select case
    when v ~ '^[^@[:space:][:cntrl:]]+@[^@[:space:][:cntrl:]]+\.[^@[:space:][:cntrl:]]+$' and char_length(v) <= 254 then v
  end
  from (select lower(btrim(coalesce(t, ''))) as v) s
$$;

create or replace function clinic.admin_count(cid uuid)
returns integer
language sql
stable
security definer
set search_path = ''
as $$
  select count(*)::integer from clinic.members m where m.clinic_id = cid and m.role = 'admin'
$$;

-- Twenty clinics per account at most.
create or replace function clinic.check_membership_limit(uid uuid)
returns void
language plpgsql
stable
security definer
set search_path = ''
as $$
begin
  if (select count(*) from clinic.members m where m.user_id = uid) >= 20 then
    perform clinic.fail('too_many_clinics');
  end if;
end;
$$;

-- Take `n` from the clinic's daily allowance of 50 invites and join codes.
create or replace function clinic.take_invites(cid uuid, n integer)
returns void
language plpgsql
security definer
set search_path = ''
as $$
declare
  c clinic.clinics;
begin
  select * into c from clinic.clinics where id = cid for update;
  if c.invite_day <> current_date then
    c.invites_today := 0;
  end if;
  if c.invites_today + n > 50 then
    perform clinic.fail('rate_limited');
  end if;
  update clinic.clinics set invite_day = current_date, invites_today = c.invites_today + n where id = cid;
end;
$$;

-- ---------------------------------------------------------------------------
-- Every clinic keeps at least one admin
-- ---------------------------------------------------------------------------

-- The functions below refuse to remove or demote the last admin. The one
-- way a last admin can still go is deleting their account (which must never
-- be blocked): the cascade removes their membership, and then the clinic
-- goes too, with its invites, codes and usage rows.
create or replace function clinic.after_member_gone()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  if old.role = 'admin' and not exists (
      select 1 from clinic.members m where m.clinic_id = old.clinic_id and m.role = 'admin') then
    delete from clinic.clinics c where c.id = old.clinic_id;
  end if;
  return null;
end;
$$;

drop trigger if exists members_last_admin on clinic.members;
create trigger members_last_admin
  after delete on clinic.members
  for each row execute function clinic.after_member_gone();

-- A role change that would leave no admin is refused outright.
create or replace function clinic.after_role_change()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  if old.role = 'admin' and new.role <> 'admin' and not exists (
      select 1 from clinic.members m where m.clinic_id = new.clinic_id and m.role = 'admin') then
    perform clinic.fail('last_admin');
  end if;
  return null;
end;
$$;

drop trigger if exists members_keep_admin on clinic.members;
create trigger members_keep_admin
  after update of role on clinic.members
  for each row execute function clinic.after_role_change();

create or replace function clinic.touch_updated_at()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  new.updated_at := now();
  return new;
end;
$$;

drop trigger if exists clinics_touch_updated_at on clinic.clinics;
create trigger clinics_touch_updated_at
  before update of name on clinic.clinics
  for each row execute function clinic.touch_updated_at();

-- ---------------------------------------------------------------------------
-- Row level security
-- ---------------------------------------------------------------------------

alter table clinic.clinics enable row level security;
alter table clinic.members enable row level security;
alter table clinic.invites enable row level security;
alter table clinic.join_codes enable row level security;
alter table clinic.code_attempts enable row level security;
alter table clinic.usage_daily enable row level security;

-- Privileges first, policies second. Nothing for anon; the signed-in role
-- may read its own membership rows, the names of its clinics and its own
-- usage rows, and write nothing directly. Invites, join codes and attempts
-- have no table access at all.
revoke all on all tables in schema clinic from public, anon, authenticated;
grant select (id, name, created_at) on clinic.clinics to authenticated;
grant select on clinic.members, clinic.usage_daily to authenticated;
grant all on all tables in schema clinic to service_role;

drop policy if exists "Members read their clinics" on clinic.clinics;
create policy "Members read their clinics" on clinic.clinics
  for select to authenticated
  using (exists (select 1 from clinic.members m where m.clinic_id = clinics.id and m.user_id = (select auth.uid())));

drop policy if exists "Own memberships" on clinic.members;
create policy "Own memberships" on clinic.members
  for select to authenticated
  using (user_id = (select auth.uid()));

drop policy if exists "Own usage" on clinic.usage_daily;
create policy "Own usage" on clinic.usage_daily
  for select to authenticated
  using (user_id = (select auth.uid()));

-- ---------------------------------------------------------------------------
-- Clinics and members
-- ---------------------------------------------------------------------------

-- The caller's clinics: name, role, display name, sharing, and enough to
-- warn a sole admin before they delete their account.
create or replace function clinic.my_clinics()
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
begin
  return coalesce((
    select jsonb_agg(jsonb_build_object(
      'id', c.id, 'name', c.name, 'role', m.role, 'display_name', m.display_name,
      'share_usage', m.share_usage, 'joined_at', m.joined_at,
      'members', (select count(*) from clinic.members x where x.clinic_id = c.id),
      'sole_admin', m.role = 'admin' and clinic.admin_count(c.id) = 1) order by m.joined_at)
    from clinic.members m join clinic.clinics c on c.id = m.clinic_id
    where m.user_id = uid), '[]'::jsonb);
end;
$$;

-- Create a clinic; the caller is its first admin. Five a day per account.
create or replace function clinic.create_clinic(name text, display_name text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  n text := clinic.clean_name(create_clinic.name, 120);
  dn text := clinic.clean_name(create_clinic.display_name, 80);
  cid uuid;
begin
  perform clinic.check_membership_limit(uid);
  if (select count(*) from clinic.clinics c where c.created_by = uid and c.created_at > now() - interval '1 day') >= 5 then
    perform clinic.fail('rate_limited');
  end if;
  insert into clinic.clinics (name, created_by) values (n, uid) returning id into cid;
  insert into clinic.members (clinic_id, user_id, role, display_name) values (cid, uid, 'admin', dn);
  return jsonb_build_object('id', cid, 'name', n, 'role', 'admin');
end;
$$;

create or replace function clinic.rename_clinic(clinic_id uuid, name text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  n text := clinic.clean_name(rename_clinic.name, 120);
begin
  perform clinic.require_admin(rename_clinic.clinic_id, auth.uid());
  update clinic.clinics c set name = n where c.id = rename_clinic.clinic_id;
  return jsonb_build_object('id', rename_clinic.clinic_id, 'name', n);
end;
$$;

-- Delete a clinic: its members, invites, codes and usage rows. The admin
-- types the clinic's name to confirm.
create or replace function clinic.delete_clinic(clinic_id uuid, confirm_name text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  c clinic.clinics;
begin
  perform clinic.require_admin(delete_clinic.clinic_id, auth.uid());
  select * into c from clinic.clinics x where x.id = delete_clinic.clinic_id;
  if lower(btrim(coalesce(confirm_name, ''))) <> lower(c.name) then
    perform clinic.fail('confirmation_mismatch');
  end if;
  delete from clinic.clinics x where x.id = c.id;
  return jsonb_build_object('deleted', c.id);
end;
$$;

create or replace function clinic.set_display_name(clinic_id uuid, display_name text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  dn text := clinic.clean_name(set_display_name.display_name, 80);
begin
  update clinic.members m set display_name = dn where m.clinic_id = set_display_name.clinic_id and m.user_id = uid;
  if not found then
    perform clinic.fail('not_member');
  end if;
  return jsonb_build_object('display_name', dn);
end;
$$;

-- "Share usage counts with <clinic>": the person's own choice, off until
-- they turn it on. Turning it off stops new counts; rows already sent stay
-- until they age out (400 days), the person deletes their account, or the
-- clinic is deleted.
create or replace function clinic.set_usage_sharing(clinic_id uuid, share boolean)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
begin
  update clinic.members m set share_usage = coalesce(set_usage_sharing.share, false)
    where m.clinic_id = set_usage_sharing.clinic_id and m.user_id = uid;
  if not found then
    perform clinic.fail('not_member');
  end if;
  return jsonb_build_object('share_usage', coalesce(set_usage_sharing.share, false));
end;
$$;

create or replace function clinic.leave_clinic(clinic_id uuid)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  r text;
begin
  -- The clinic row's lock serialises every change to its admins, so two
  -- admins leaving or demoting each other at once can't leave none.
  perform 1 from clinic.clinics c where c.id = leave_clinic.clinic_id for update;
  r := clinic.role_of(leave_clinic.clinic_id, uid);
  if r is null then
    perform clinic.fail('not_member');
  end if;
  if r = 'admin' and clinic.admin_count(leave_clinic.clinic_id) = 1 then
    perform clinic.fail('last_admin');
  end if;
  delete from clinic.members m where m.clinic_id = leave_clinic.clinic_id and m.user_id = uid;
  return jsonb_build_object('left', leave_clinic.clinic_id);
end;
$$;

create or replace function clinic.remove_member(clinic_id uuid, user_id uuid)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  r text;
begin
  perform 1 from clinic.clinics c where c.id = remove_member.clinic_id for update;
  perform clinic.require_admin(remove_member.clinic_id, uid);
  r := clinic.role_of(remove_member.clinic_id, remove_member.user_id);
  if r is null then
    perform clinic.fail('not_member');
  end if;
  if r = 'admin' and clinic.admin_count(remove_member.clinic_id) = 1 then
    perform clinic.fail('last_admin');
  end if;
  delete from clinic.members m where m.clinic_id = remove_member.clinic_id and m.user_id = remove_member.user_id;
  return jsonb_build_object('removed', remove_member.user_id);
end;
$$;

create or replace function clinic.set_member_role(clinic_id uuid, user_id uuid, role text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  r text;
begin
  perform 1 from clinic.clinics c where c.id = set_member_role.clinic_id for update;
  perform clinic.require_admin(set_member_role.clinic_id, uid);
  if set_member_role.role is null or set_member_role.role not in ('admin', 'member') then
    perform clinic.fail('bad_role');
  end if;
  r := clinic.role_of(set_member_role.clinic_id, set_member_role.user_id);
  if r is null then
    perform clinic.fail('not_member');
  end if;
  if r = 'admin' and set_member_role.role = 'member' and clinic.admin_count(set_member_role.clinic_id) = 1 then
    perform clinic.fail('last_admin');
  end if;
  update clinic.members m set role = set_member_role.role
    where m.clinic_id = set_member_role.clinic_id and m.user_id = set_member_role.user_id;
  return jsonb_build_object('user_id', set_member_role.user_id, 'role', set_member_role.role);
end;
$$;

-- ---------------------------------------------------------------------------
-- Invites by email
-- ---------------------------------------------------------------------------

-- The shared part of inviting: the admin check, the daily limit, one row
-- per address (an invite already pending is renewed: 14 more days). Each
-- result: {email, status: invited | renewed | member | invalid, account:
-- active | invited | none}. `account` says whether Supabase Auth already
-- knows the address, so the clinic-invite Edge Function knows whom to email;
-- the portal's direct call (invite_members) leaves it out.
create or replace function clinic.invite(uid uuid, cid uuid, emails text[], role text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  e text;
  raw text;
  seen text[] := '{}';
  todo text[] := '{}';
  results jsonb := '[]'::jsonb;
  existed boolean;
  acct text;
begin
  perform clinic.require_admin(cid, uid);
  if invite.role is null or invite.role not in ('admin', 'member') then
    perform clinic.fail('bad_role');
  end if;
  if emails is null or cardinality(emails) = 0 then
    perform clinic.fail('no_emails');
  end if;
  if cardinality(emails) > 50 then
    perform clinic.fail('too_many');
  end if;
  foreach raw in array emails loop
    e := clinic.norm_email(raw);
    if e is null then
      results := results || jsonb_build_object('email', left(btrim(coalesce(raw, '')), 254), 'status', 'invalid');
      continue;
    end if;
    if e = any(seen) then
      continue;
    end if;
    seen := seen || e;
    if exists (select 1 from clinic.members m join auth.users u on u.id = m.user_id
               where m.clinic_id = cid and lower(u.email) = e) then
      results := results || jsonb_build_object('email', e, 'status', 'member');
      continue;
    end if;
    todo := todo || e;
  end loop;
  if cardinality(todo) > 0 then
    perform clinic.take_invites(cid, cardinality(todo));
  end if;
  foreach e in array todo loop
    existed := exists (select 1 from clinic.invites i where i.clinic_id = cid and i.email = e);
    insert into clinic.invites as i (clinic_id, email, role, invited_by)
      values (cid, e, invite.role, uid)
    on conflict on constraint invites_clinic_id_email_key do update set
      role = excluded.role, invited_by = excluded.invited_by, sent_count = least(i.sent_count + 1, 1000),
      last_sent_at = now(), expires_at = now() + interval '14 days';
    select case when u.email_confirmed_at is not null then 'active' else 'invited' end into acct
      from auth.users u where lower(u.email) = e order by u.created_at limit 1;
    results := results || jsonb_build_object('email', e, 'status', case when existed then 'renewed' else 'invited' end,
      'account', coalesce(acct, 'none'));
    acct := null;
  end loop;
  return results;
end;
$$;

-- From the portal, without sending any email: people see the invite when
-- they next sign in to their account page. (The portal uses this only when
-- the clinic-invite Edge Function isn't deployed.)
create or replace function clinic.invite_members(clinic_id uuid, emails text[], role text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
begin
  return coalesce((select jsonb_agg(r.value - 'account' order by r.ordinality) from jsonb_array_elements(
    clinic.invite(clinic.require_user(), invite_members.clinic_id, invite_members.emails, invite_members.role))
    with ordinality r), '[]'::jsonb);
end;
$$;

-- From the clinic-invite Edge Function, which checks the person's token
-- itself and calls this with the service role.
create or replace function clinic.invite_members_for(user_id uuid, clinic_id uuid, emails text[], role text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
begin
  return clinic.invite(invite_members_for.user_id, invite_members_for.clinic_id, invite_members_for.emails,
    invite_members_for.role);
end;
$$;

create or replace function clinic.revoke_invite(invite_id uuid)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  cid uuid;
begin
  select i.clinic_id into cid from clinic.invites i where i.id = revoke_invite.invite_id;
  if cid is null then
    perform clinic.fail('no_invite');
  end if;
  perform clinic.require_admin(cid, auth.uid());
  delete from clinic.invites i where i.id = revoke_invite.invite_id;
  return jsonb_build_object('revoked', revoke_invite.invite_id);
end;
$$;

-- Invites waiting for the caller's confirmed email address.
create or replace function clinic.my_invites()
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  e text := clinic.my_email();
begin
  if e is null then
    return '[]'::jsonb;
  end if;
  return coalesce((
    select jsonb_agg(jsonb_build_object(
      'id', i.id, 'clinic_id', c.id, 'clinic_name', c.name, 'role', i.role, 'expires_at', i.expires_at,
      'invited_by', (select m.display_name from clinic.members m where m.clinic_id = c.id and m.user_id = i.invited_by),
      'member', exists (select 1 from clinic.members m where m.clinic_id = c.id and m.user_id = uid))
      order by i.created_at)
    from clinic.invites i join clinic.clinics c on c.id = i.clinic_id
    where i.email = e and i.expires_at > now()), '[]'::jsonb);
end;
$$;

create or replace function clinic.accept_invite(invite_id uuid, display_name text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  e text := clinic.my_email();
  dn text := clinic.clean_name(accept_invite.display_name, 80);
  inv clinic.invites;
begin
  if e is null then
    perform clinic.fail('email_not_confirmed');
  end if;
  select * into inv from clinic.invites i where i.id = accept_invite.invite_id and i.email = e for update;
  if inv.id is null then
    perform clinic.fail('no_invite');
  end if;
  if inv.expires_at <= now() then
    perform clinic.fail('expired');
  end if;
  if clinic.role_of(inv.clinic_id, uid) is null then
    perform clinic.check_membership_limit(uid);
  end if;
  -- Already a member: an admin invite promotes; a member invite changes nothing.
  insert into clinic.members as m (clinic_id, user_id, role, display_name)
    values (inv.clinic_id, uid, inv.role, dn)
  on conflict on constraint members_pkey do update set
    role = case when excluded.role = 'admin' then 'admin' else m.role end;
  delete from clinic.invites i where i.id = inv.id;
  return jsonb_build_object('clinic_id', inv.clinic_id, 'role', clinic.role_of(inv.clinic_id, uid));
end;
$$;

create or replace function clinic.decline_invite(invite_id uuid)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  e text := clinic.my_email();
begin
  perform clinic.require_user();
  delete from clinic.invites i where i.id = decline_invite.invite_id and i.email = e;
  if not found then
    perform clinic.fail('no_invite');
  end if;
  return jsonb_build_object('declined', decline_invite.invite_id);
end;
$$;

-- ---------------------------------------------------------------------------
-- Join codes
-- ---------------------------------------------------------------------------

-- Crockford base32 without the check symbol. I, L, O and U are left out.
create or replace function clinic.norm_code(t text)
returns text
language sql
immutable
set search_path = ''
as $$
  select case when v ~ '^[0-9A-HJKMNP-TV-Z]{8}$' then v end
  from (select translate(regexp_replace(upper(coalesce(t, '')), '[[:space:]-]', '', 'g'), 'ILO', '110') as v) s
$$;

-- A new one-off code, returned once (as XXXX-XXXX) and stored as SHA-256.
-- Counts against the daily invite allowance; 50 live codes per clinic.
create or replace function clinic.create_join_code(clinic_id uuid)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  alphabet constant text := '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
  raw bytea;
  bits bigint;
  code text;
  row_id uuid;
  exp timestamptz;
begin
  perform clinic.require_admin(create_join_code.clinic_id, uid);
  delete from clinic.join_codes j where j.clinic_id = create_join_code.clinic_id and j.expires_at <= now();
  if (select count(*) from clinic.join_codes j where j.clinic_id = create_join_code.clinic_id) >= 50 then
    perform clinic.fail('too_many_codes');
  end if;
  perform clinic.take_invites(create_join_code.clinic_id, 1);
  for attempt in 1..5 loop
    -- 40 bits from the random part of a version 4 UUID (pg_strong_random).
    raw := substring(uuid_send(gen_random_uuid()) from 1 for 5);
    bits := (get_byte(raw, 0)::bigint << 32) | (get_byte(raw, 1)::bigint << 24) | (get_byte(raw, 2)::bigint << 16)
      | (get_byte(raw, 3)::bigint << 8) | get_byte(raw, 4)::bigint;
    code := '';
    for i in reverse 7..0 loop
      code := code || substr(alphabet, ((bits >> (i * 5)) & 31)::integer + 1, 1);
    end loop;
    begin
      insert into clinic.join_codes (clinic_id, code_hash, created_by)
        values (create_join_code.clinic_id, sha256(convert_to(code, 'UTF8')), uid)
        returning id, expires_at into row_id, exp;
      return jsonb_build_object('id', row_id, 'code', substr(code, 1, 4) || '-' || substr(code, 5, 4), 'expires_at', exp);
    exception when unique_violation then
      -- Another live code has these 40 bits; draw again.
      null;
    end;
  end loop;
  perform clinic.fail('try_again');
end;
$$;

create or replace function clinic.revoke_join_code(code_id uuid)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  cid uuid;
begin
  select j.clinic_id into cid from clinic.join_codes j where j.id = revoke_join_code.code_id;
  if cid is null then
    perform clinic.fail('no_code');
  end if;
  perform clinic.require_admin(cid, auth.uid());
  delete from clinic.join_codes j where j.id = revoke_join_code.code_id;
  return jsonb_build_object('revoked', revoke_join_code.code_id);
end;
$$;

-- Join with a code: single use, member role. A wrong or expired code is
-- answered {"error": "wrong_code"} rather than raised, so the failure count
-- is kept (a raised error would roll it back); 10 wrong codes an hour.
create or replace function clinic.join_with_code(code text, display_name text)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  dn text := clinic.clean_name(join_with_code.display_name, 80);
  v text := clinic.norm_code(join_with_code.code);
  att clinic.code_attempts;
  jc clinic.join_codes;
begin
  if v is null then
    perform clinic.fail('bad_code');
  end if;
  insert into clinic.code_attempts (user_id) values (uid) on conflict on constraint code_attempts_pkey do nothing;
  select * into att from clinic.code_attempts a where a.user_id = uid for update;
  if att.window_start < now() - interval '1 hour' then
    update clinic.code_attempts a set window_start = now(), failures = 0 where a.user_id = uid;
    att.failures := 0;
  end if;
  if att.failures >= 10 then
    perform clinic.fail('too_many_attempts');
  end if;
  select * into jc from clinic.join_codes j where j.code_hash = sha256(convert_to(v, 'UTF8')) for update;
  if jc.id is null or jc.expires_at <= now() then
    update clinic.code_attempts a set failures = a.failures + 1 where a.user_id = uid;
    return jsonb_build_object('error', 'wrong_code', 'attempts_left', greatest(0, 9 - att.failures));
  end if;
  -- Someone who is already a member doesn't use up the code.
  if clinic.role_of(jc.clinic_id, uid) is null then
    perform clinic.check_membership_limit(uid);
    insert into clinic.members (clinic_id, user_id, role, display_name) values (jc.clinic_id, uid, 'member', dn);
    delete from clinic.join_codes j where j.id = jc.id;
  end if;
  return jsonb_build_object('clinic_id', jc.clinic_id, 'name', (select c.name from clinic.clinics c where c.id = jc.clinic_id),
    'role', clinic.role_of(jc.clinic_id, uid));
end;
$$;

-- ---------------------------------------------------------------------------
-- Usage counts
-- ---------------------------------------------------------------------------

-- The app's daily report (phase 2, R15): this device's totals for one day,
-- replacing what it sent before for that day. Only the caller's own row, in
-- a clinic they belong to with sharing on; the day within the last 7 days
-- and not in the future (UTC today + 1, for clinics east of UTC); every
-- count within its bounds; at most 10 devices per person per clinic in 30
-- days.
create or replace function clinic.report_usage(
  clinic_id uuid, day date, device_id uuid, platform text, app_version text,
  visits_recorded integer, minutes_recorded numeric, notes_drafted integer, notes_signed integer, calls_recorded integer)
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  m clinic.members;
  mins numeric(8, 1);
begin
  select * into m from clinic.members x where x.clinic_id = report_usage.clinic_id and x.user_id = uid;
  if m.user_id is null then
    perform clinic.fail('not_member');
  end if;
  if not m.share_usage then
    perform clinic.fail('not_sharing');
  end if;
  if report_usage.day is null or report_usage.device_id is null then
    perform clinic.fail('bad_request');
  end if;
  if report_usage.day > current_date + 1 then
    perform clinic.fail('future_day');
  end if;
  if report_usage.day < current_date - 7 then
    perform clinic.fail('too_old');
  end if;
  if report_usage.platform is null or report_usage.platform not in ('windows', 'macos', 'linux', 'ios', 'web') then
    perform clinic.fail('bad_platform');
  end if;
  if report_usage.app_version is null or char_length(report_usage.app_version) > 40
      or report_usage.app_version !~ '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$' then
    perform clinic.fail('bad_version');
  end if;
  mins := round(report_usage.minutes_recorded, 1);
  if report_usage.visits_recorded is null or report_usage.visits_recorded not between 0 and 1000
      or mins is null or mins not between 0 and 1440
      or report_usage.notes_drafted is null or report_usage.notes_drafted not between 0 and 5000
      or report_usage.notes_signed is null or report_usage.notes_signed not between 0 and 1000
      or report_usage.calls_recorded is null or report_usage.calls_recorded not between 0 and 1000 then
    perform clinic.fail('out_of_range');
  end if;
  if not exists (select 1 from clinic.usage_daily u where u.clinic_id = report_usage.clinic_id and u.user_id = uid
                   and u.device_id = report_usage.device_id)
     and (select count(distinct u.device_id) from clinic.usage_daily u where u.clinic_id = report_usage.clinic_id
            and u.user_id = uid and u.day > current_date - 30) >= 10 then
    perform clinic.fail('too_many_devices');
  end if;
  insert into clinic.usage_daily (clinic_id, user_id, device_id, day, platform, app_version, visits_recorded,
      minutes_recorded, notes_drafted, notes_signed, calls_recorded)
    values (report_usage.clinic_id, uid, report_usage.device_id, report_usage.day, report_usage.platform,
      report_usage.app_version, report_usage.visits_recorded, mins, report_usage.notes_drafted,
      report_usage.notes_signed, report_usage.calls_recorded)
  on conflict on constraint usage_daily_pkey do update set
    platform = excluded.platform, app_version = excluded.app_version, visits_recorded = excluded.visits_recorded,
    minutes_recorded = excluded.minutes_recorded, notes_drafted = excluded.notes_drafted,
    notes_signed = excluded.notes_signed, calls_recorded = excluded.calls_recorded, updated_at = now();
  return jsonb_build_object('ok', true);
end;
$$;

-- The caller's own counts per day (all their devices), in one clinic.
create or replace function clinic.my_usage(clinic_id uuid, from_day date, to_day date)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
begin
  if from_day is null or to_day is null or from_day > to_day or to_day - from_day > 400 then
    perform clinic.fail('bad_range');
  end if;
  return coalesce((
    select jsonb_agg(jsonb_build_object('day', d.day, 'visits', d.visits, 'minutes', d.minutes, 'drafted', d.drafted,
      'signed', d.signed, 'calls', d.calls) order by d.day)
    from (select u.day, sum(u.visits_recorded) as visits, sum(u.minutes_recorded) as minutes,
            sum(u.notes_drafted) as drafted, sum(u.notes_signed) as signed, sum(u.calls_recorded) as calls
          from clinic.usage_daily u
          where u.clinic_id = my_usage.clinic_id and u.user_id = uid and u.day between from_day and to_day
          group by u.day) d), '[]'::jsonb);
end;
$$;

-- What an admin manages: the clinic, its members (display name, role,
-- joined, sharing, last day with counts, and the devices, platforms and app
-- versions seen in the last 30 days), pending invites and live join codes.
-- Members' email addresses are never returned.
create or replace function clinic.clinic_admin(clinic_id uuid)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  c clinic.clinics;
begin
  perform clinic.require_admin(clinic_admin.clinic_id, uid);
  select * into c from clinic.clinics x where x.id = clinic_admin.clinic_id;
  return jsonb_build_object(
    'clinic', jsonb_build_object('id', c.id, 'name', c.name, 'created_at', c.created_at),
    'me', uid,
    'invites_left_today', case when c.invite_day = current_date then 50 - c.invites_today else 50 end,
    'members', coalesce((
      select jsonb_agg(jsonb_build_object(
        'user_id', m.user_id, 'name', m.display_name, 'role', m.role, 'joined_at', m.joined_at,
        'share_usage', m.share_usage,
        'last_active', (select max(u.day) from clinic.usage_daily u where u.clinic_id = c.id and u.user_id = m.user_id),
        'devices', (select count(distinct u.device_id) from clinic.usage_daily u
                    where u.clinic_id = c.id and u.user_id = m.user_id and u.day > current_date - 30),
        'platforms', coalesce((select jsonb_agg(distinct u.platform) from clinic.usage_daily u
                    where u.clinic_id = c.id and u.user_id = m.user_id and u.day > current_date - 30), '[]'::jsonb),
        'app_versions', coalesce((select jsonb_agg(distinct u.app_version) from clinic.usage_daily u
                    where u.clinic_id = c.id and u.user_id = m.user_id and u.day > current_date - 30), '[]'::jsonb))
        order by m.role, lower(m.display_name))
      from clinic.members m where m.clinic_id = c.id), '[]'::jsonb),
    'invites', coalesce((
      select jsonb_agg(jsonb_build_object('id', i.id, 'email', i.email, 'role', i.role, 'created_at', i.created_at,
        'last_sent_at', i.last_sent_at, 'expires_at', i.expires_at, 'expired', i.expires_at <= now(),
        'sent_count', i.sent_count) order by i.created_at desc)
      from clinic.invites i where i.clinic_id = c.id), '[]'::jsonb),
    'codes', coalesce((
      select jsonb_agg(jsonb_build_object('id', j.id, 'created_at', j.created_at, 'expires_at', j.expires_at)
        order by j.created_at desc)
      from clinic.join_codes j where j.clinic_id = c.id and j.expires_at > now()), '[]'::jsonb));
end;
$$;

-- The dashboard: usage between two days (at most 400 days apart). Per-day
-- clinic totals; per-member totals for the period (people who are no longer
-- members are summed into one "former" row, with no name or id); devices
-- and visits per platform; devices per app version (each device's latest
-- version in the period). Admins only. No row ever names a patient, and no
-- time finer than a day exists to return.
create or replace function clinic.clinic_dashboard(clinic_id uuid, from_day date, to_day date)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  uid uuid := clinic.require_user();
  cid uuid := clinic_dashboard.clinic_id;
begin
  perform clinic.require_admin(cid, uid);
  if from_day is null or to_day is null or from_day > to_day or to_day - from_day > 400 then
    perform clinic.fail('bad_range');
  end if;
  return (
    with u as (
      select x.*, (m.user_id is not null) as current_member
      from clinic.usage_daily x
      left join clinic.members m on m.clinic_id = x.clinic_id and m.user_id = x.user_id
      where x.clinic_id = cid and x.day between from_day and to_day
    ),
    latest as (
      select distinct on (u.device_id, u.user_id) u.device_id, u.user_id, u.app_version, u.platform
      from u order by u.device_id, u.user_id, u.day desc
    )
    select jsonb_build_object(
      'clinic', (select jsonb_build_object('id', c.id, 'name', c.name) from clinic.clinics c where c.id = cid),
      'from_day', from_day, 'to_day', to_day,
      'totals', (select jsonb_build_object(
          'visits', coalesce(sum(u.visits_recorded), 0), 'minutes', coalesce(sum(u.minutes_recorded), 0),
          'drafted', coalesce(sum(u.notes_drafted), 0), 'signed', coalesce(sum(u.notes_signed), 0),
          'calls', coalesce(sum(u.calls_recorded), 0),
          'devices', count(distinct (u.user_id, u.device_id)),
          'reporting', count(distinct u.user_id) filter (where u.current_member)) from u),
      'members_total', (select count(*) from clinic.members m where m.clinic_id = cid),
      'members_sharing', (select count(*) from clinic.members m where m.clinic_id = cid and m.share_usage),
      'days', coalesce((select jsonb_agg(jsonb_build_object('day', d.day, 'visits', d.visits, 'minutes', d.minutes,
            'drafted', d.drafted, 'signed', d.signed, 'calls', d.calls) order by d.day)
          from (select u.day, sum(u.visits_recorded) as visits, sum(u.minutes_recorded) as minutes,
                  sum(u.notes_drafted) as drafted, sum(u.notes_signed) as signed, sum(u.calls_recorded) as calls
                from u group by u.day) d), '[]'::jsonb),
      'members', coalesce((select jsonb_agg(r order by (r ->> 'former')::boolean, r ->> 'name') from (
          select jsonb_build_object(
            'user_id', m.user_id, 'name', m.display_name, 'role', m.role, 'former', false,
            'share_usage', m.share_usage,
            'visits', coalesce(sum(u.visits_recorded), 0), 'minutes', coalesce(sum(u.minutes_recorded), 0),
            'drafted', coalesce(sum(u.notes_drafted), 0), 'signed', coalesce(sum(u.notes_signed), 0),
            'calls', coalesce(sum(u.calls_recorded), 0), 'days_active', count(distinct u.day),
            'last_active', max(u.day)) as r
          from clinic.members m left join u on u.user_id = m.user_id
          where m.clinic_id = cid
          group by m.user_id, m.display_name, m.role, m.share_usage
          union all
          select jsonb_build_object(
            'user_id', null, 'name', null, 'role', null, 'former', true, 'share_usage', null,
            'people', count(distinct u.user_id),
            'visits', sum(u.visits_recorded), 'minutes', sum(u.minutes_recorded), 'drafted', sum(u.notes_drafted),
            'signed', sum(u.notes_signed), 'calls', sum(u.calls_recorded), 'days_active', count(distinct u.day),
            'last_active', max(u.day))
          from u where not u.current_member
          having count(*) > 0) s), '[]'::jsonb),
      'platforms', coalesce((select jsonb_agg(jsonb_build_object('platform', p.platform, 'devices', p.devices,
            'visits', p.visits, 'minutes', p.minutes) order by p.visits desc, p.platform)
          from (select u.platform, count(distinct (u.user_id, u.device_id)) as devices,
                  sum(u.visits_recorded) as visits, sum(u.minutes_recorded) as minutes
                from u group by u.platform) p), '[]'::jsonb),
      'versions', coalesce((select jsonb_agg(jsonb_build_object('app_version', v.app_version, 'devices', v.devices)
            order by v.devices desc, v.app_version desc)
          from (select l.app_version, count(*) as devices from latest l group by l.app_version) v), '[]'::jsonb)
    ));
end;
$$;

-- ---------------------------------------------------------------------------
-- Housekeeping
-- ---------------------------------------------------------------------------

-- Usage rows older than 400 days, invites 30 days past expiry, expired join
-- codes, and stale attempt counters. Run daily by pg_cron where available.
create or replace function clinic.purge()
returns jsonb
language plpgsql
security definer
set search_path = ''
as $$
declare
  n_usage integer;
  n_invites integer;
  n_codes integer;
begin
  delete from clinic.usage_daily u where u.day < current_date - 400;
  get diagnostics n_usage = row_count;
  delete from clinic.invites i where i.expires_at < now() - interval '30 days';
  get diagnostics n_invites = row_count;
  delete from clinic.join_codes j where j.expires_at < now();
  get diagnostics n_codes = row_count;
  delete from clinic.code_attempts a where a.window_start < now() - interval '1 day';
  return jsonb_build_object('usage', n_usage, 'invites', n_invites, 'codes', n_codes);
end;
$$;

-- ---------------------------------------------------------------------------
-- Function privileges: nothing for the public or anon; the signed-in role
-- gets the API below; the service role (Edge Functions) gets everything,
-- including invite_members_for, which takes a user id and so must never be
-- callable by a signed-in person.
-- ---------------------------------------------------------------------------

revoke all on all functions in schema clinic from public, anon, authenticated;
grant execute on function
  clinic.my_clinics(),
  clinic.create_clinic(text, text),
  clinic.rename_clinic(uuid, text),
  clinic.delete_clinic(uuid, text),
  clinic.set_display_name(uuid, text),
  clinic.set_usage_sharing(uuid, boolean),
  clinic.leave_clinic(uuid),
  clinic.remove_member(uuid, uuid),
  clinic.set_member_role(uuid, uuid, text),
  clinic.invite_members(uuid, text[], text),
  clinic.revoke_invite(uuid),
  clinic.my_invites(),
  clinic.accept_invite(uuid, text),
  clinic.decline_invite(uuid),
  clinic.create_join_code(uuid),
  clinic.revoke_join_code(uuid),
  clinic.join_with_code(text, text),
  clinic.report_usage(uuid, date, uuid, text, text, integer, numeric, integer, integer, integer),
  clinic.my_usage(uuid, date, date),
  clinic.clinic_admin(uuid),
  clinic.clinic_dashboard(uuid, date, date)
to authenticated;
grant execute on all functions in schema clinic to service_role;

-- Daily purge, where pg_cron is available (Supabase has it).
do $$
begin
  if exists (select 1 from pg_available_extensions where name = 'pg_cron') then
    create extension if not exists pg_cron;
    perform cron.unschedule(jobid) from cron.job where jobname = 'openbark-clinic-purge';
    perform cron.schedule('openbark-clinic-purge', '41 3 * * *', 'select clinic.purge()');
  end if;
exception when others then
  raise notice 'openbark: pg_cron not scheduled (%); run select clinic.purge() from a scheduler', sqlerrm;
end;
$$;
