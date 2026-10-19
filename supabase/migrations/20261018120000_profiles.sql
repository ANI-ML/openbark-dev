-- OpenBark account portal: one small profile per account (ADR-0017).
--
-- The website portal (www.openbark.io/portal) signs people in with an emailed
-- code and keeps a short profile about the person, never about patients:
-- name, clinic, role, country and whether they want update emails. No
-- clinical data is ever stored in this project for the portal.
--
-- Row level security: a signed-in user can read, create, change and delete
-- only their own row. Visitors who aren't signed in (the anon role) get
-- nothing. The row goes with the account: deleting the auth user (the
-- delete-account Edge Function) deletes the profile through the foreign key.
--
-- Apply it with `supabase db push`, or paste it into the Supabase SQL editor
-- and run it once. See supabase/README.md.

create table if not exists public.profiles (
  id uuid primary key references auth.users (id) on delete cascade,
  name text check (name is null or char_length(name) <= 120),
  clinic text check (clinic is null or char_length(clinic) <= 160),
  role text check (role is null or role in ('veterinarian', 'technician', 'other')),
  -- ISO 3166-1 alpha-2, for example 'CA'.
  country text check (country is null or country ~ '^[A-Z]{2}$'),
  updates_opt_in boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now()
);

comment on table public.profiles is
  'OpenBark website portal: one row per account, about the person (never patients). RLS: own row only.';
comment on column public.profiles.updates_opt_in is
  'The person asked for release emails. Off unless they turn it on.';

-- updated_at follows every change.
create or replace function public.profiles_touch_updated_at()
returns trigger
language plpgsql
set search_path = ''
as $$
begin
  new.updated_at := now();
  return new;
end;
$$;

drop trigger if exists profiles_touch_updated_at on public.profiles;
create trigger profiles_touch_updated_at
  before update on public.profiles
  for each row execute function public.profiles_touch_updated_at();

alter table public.profiles enable row level security;

-- Privileges first, policies second. Supabase grants everything on new public
-- tables to anon and authenticated by default; take that back, then give the
-- signed-in role only what the portal needs. The timestamps are the
-- database's to set, and a row's id can't be changed after it is created.
revoke all on table public.profiles from anon, authenticated;
grant select, delete on table public.profiles to authenticated;
grant insert (id, name, clinic, role, country, updates_opt_in) on table public.profiles to authenticated;
grant update (name, clinic, role, country, updates_opt_in) on table public.profiles to authenticated;

drop policy if exists "Read own profile" on public.profiles;
create policy "Read own profile" on public.profiles
  for select to authenticated
  using ((select auth.uid()) = id);

drop policy if exists "Create own profile" on public.profiles;
create policy "Create own profile" on public.profiles
  for insert to authenticated
  with check ((select auth.uid()) = id);

drop policy if exists "Change own profile" on public.profiles;
create policy "Change own profile" on public.profiles
  for update to authenticated
  using ((select auth.uid()) = id)
  with check ((select auth.uid()) = id);

drop policy if exists "Delete own profile" on public.profiles;
create policy "Delete own profile" on public.profiles
  for delete to authenticated
  using ((select auth.uid()) = id);
