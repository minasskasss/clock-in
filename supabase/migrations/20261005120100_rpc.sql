-- Clock In: RPC functions (ARCHITECTURE §5, §6).
--
-- Every function is SECURITY DEFINER with an empty search_path and fully
-- qualified names. Helpers live in `app` and are never granted. The RPCs live
-- in `public` and are granted to `anon` only (bottom of this file).
--
-- Every RPC returns jsonb: { ok: true, ... } or { ok: false, error: <code>, ... }.
--
-- ⚠️ A failed passphrase check RETURNS an error code and never RAISEs:
-- raising would roll back the transaction and undo the lockout counter.

-- ---------------------------------------------------------------------------
-- Helpers (schema app, never granted)
-- ---------------------------------------------------------------------------

create function app.ok(extra jsonb default '{}') returns jsonb
language sql immutable set search_path = ''
as $$ select jsonb_build_object('ok', true) || extra $$;

create function app.fail(code text, extra jsonb default '{}') returns jsonb
language sql immutable set search_path = ''
as $$ select jsonb_build_object('ok', false, 'error', code) || extra $$;

create function app.invalid(details text) returns jsonb
language sql immutable set search_path = ''
as $$ select app.fail('invalid_input', jsonb_build_object('details', details)) $$;

-- The business date that contains "now" (SPEC §5), in Greek local time.
-- Used only for cheap range checks; the exact logic lives in clockin-core.
create function app.business_today() returns date
language sql stable set search_path = ''
as $$
    select ((now() at time zone 'Europe/Athens') - s.rollover::interval)::date
      from app.settings s
     where s.id
$$;

-- Same normalisation as clockin-core: NFC, lowercase, whitespace collapsed, trimmed.
create function app.normalize_passphrase(p text) returns text
language sql immutable set search_path = ''
as $$
    select btrim(regexp_replace(lower(normalize(coalesce(p, ''), NFC)), '\s+', ' ', 'g'))
$$;

-- At least 5 words and 20 characters; at most 72 bytes (bcrypt's limit).
-- The client also checks every word against the EFF list.
create function app.passphrase_acceptable(normalized text) returns boolean
language sql immutable set search_path = ''
as $$
    select char_length(normalized) >= 20
       and octet_length(normalized) <= 72
       and cardinality(string_to_array(normalized, ' ')) >= 5
$$;

create function app.hash_token(token text) returns bytea
language sql immutable set search_path = ''
as $$ select sha256(convert_to(coalesce(token, ''), 'UTF8')) $$;

-- 32 random bytes, base64url without padding.
create function app.new_token() returns text
language sql volatile set search_path = ''
as $$ select translate(encode(extensions.gen_random_bytes(32), 'base64'), '+/=', '-_') $$;

create function app.device_input_problem(p_name text, p_platform text) returns text
language sql immutable set search_path = ''
as $$
    select case
        when p_name is null or char_length(btrim(p_name)) not between 1 and 60 then 'device_name'
        when p_platform is null or p_platform not in ('windows', 'android') then 'platform'
    end
$$;

create function app.register_device(p_name text, p_platform text) returns jsonb
language plpgsql volatile set search_path = ''
as $$
declare
    v_secret text := app.new_token();
    v_id uuid;
begin
    insert into app.devices (name, platform, secret_hash)
    values (btrim(p_name), p_platform, app.hash_token(v_secret))
    returning id into v_id;
    return app.ok(jsonb_build_object('device_id', v_id, 'device_secret', v_secret));
end;
$$;

-- Checks a passphrase against the stored hash, with the shared lockout
-- (ARCHITECTURE §5.4). Never raises.
create function app.verify_passphrase(p_passphrase text) returns jsonb
language plpgsql volatile set search_path = ''
as $$
declare
    st app.auth_state;
begin
    select * into st from app.auth_state where id for update;

    if st.passphrase_hash is null then
        return app.fail('not_initialized');
    end if;

    if st.locked_until is not null and st.locked_until > now() then
        return app.fail('locked', jsonb_build_object(
            'retry_after_s', ceil(extract(epoch from st.locked_until - now()))::integer));
    end if;

    if extensions.crypt(app.normalize_passphrase(p_passphrase), st.passphrase_hash)
       = st.passphrase_hash then
        update app.auth_state set failed_count = 0, locked_until = null where id;
        return app.ok();
    end if;

    -- 5th failure: 1 minute; each further failure doubles it, up to 60.
    update app.auth_state
       set failed_count = failed_count + 1,
           locked_until = case
               when failed_count + 1 >= 5 then
                   now() + make_interval(mins => least(power(2, least(failed_count + 1 - 5, 6))::integer, 60))
           end
     where id
    returning * into st;

    if st.locked_until is not null then
        return app.fail('bad_passphrase', jsonb_build_object(
            'retry_after_s', ceil(extract(epoch from st.locked_until - now()))::integer));
    end if;
    return app.fail('bad_passphrase');
end;
$$;

-- Device authentication: the secret must match a non-revoked device.
-- Updates `last_seen_at` at most once a minute.
create function app.auth_device(p_secret text, out dev uuid, out err text)
language plpgsql volatile set search_path = ''
as $$
declare
    d app.devices;
begin
    if p_secret is null or char_length(p_secret) not between 1 and 200 then
        err := 'bad_secret';
        return;
    end if;
    select * into d from app.devices where secret_hash = app.hash_token(p_secret);
    if not found then
        err := 'bad_secret';
        return;
    end if;
    if d.revoked_at is not null then
        err := 'revoked';
        return;
    end if;
    dev := d.id;
    update app.devices
       set last_seen_at = now()
     where id = d.id
       and (last_seen_at is null or last_seen_at < now() - interval '1 minute');
end;
$$;

-- Admin authentication: a valid device plus a live session of that device.
-- Each successful call slides the session's expiry to 10 minutes from now.
create function app.auth_admin(p_secret text, p_session text, out dev uuid, out err text)
language plpgsql volatile set search_path = ''
as $$
declare
    a record;
begin
    select * into a from app.auth_device(p_secret);
    if a.err is not null then
        err := a.err;
        return;
    end if;
    update app.admin_sessions
       set expires_at = now() + interval '10 minutes'
     where token_hash = app.hash_token(p_session)
       and device_id = a.dev
       and expires_at > now();
    if not found then
        err := 'no_session';
        return;
    end if;
    dev := a.dev;
end;
$$;

-- Replaces a person's weekly template. Blocks may carry the `id` of an
-- existing block; blocks without one are matched to an unchanged existing
-- block. Either way the id is kept, so marks stay attached when a block is
-- edited mid-shift (DECISIONS 2026-10-04). Returns a problem or null.
create function app.apply_week(p_staff_id uuid, p_blocks jsonb) returns text
language plpgsql volatile set search_path = ''
as $$
declare
    r record;
    v_id uuid;
    v_keep uuid[] := '{}';
begin
    if p_blocks is null or jsonb_typeof(p_blocks) <> 'array' then
        return 'blocks';
    end if;
    if jsonb_array_length(p_blocks) not between 1 and 100 then
        return 'blocks_count';
    end if;

    for r in
        select x.id, x.weekday, x.start, x."end"
          from jsonb_to_recordset(p_blocks) as x(id uuid, weekday smallint, start time, "end" time)
    loop
        if r.weekday is null or r.start is null or r."end" is null then
            return 'block_fields';
        end if;
        v_id := null;
        if r.id is not null then
            select b.id into v_id from app.schedule_blocks b
             where b.id = r.id and b.staff_id = p_staff_id and b.id <> all (v_keep);
        end if;
        if v_id is null then
            select b.id into v_id from app.schedule_blocks b
             where b.staff_id = p_staff_id and b.weekday = r.weekday
               and b.start_time = r.start and b.end_time = r."end"
               and b.id <> all (v_keep)
             limit 1;
        end if;
        if v_id is null then
            insert into app.schedule_blocks (staff_id, weekday, start_time, end_time)
            values (p_staff_id, r.weekday, r.start, r."end")
            returning id into v_id;
        else
            update app.schedule_blocks
               set weekday = r.weekday, start_time = r.start, end_time = r."end"
             where id = v_id
               and (weekday, start_time, end_time) is distinct from (r.weekday, r.start, r."end");
        end if;
        v_keep := v_keep || v_id;
    end loop;

    delete from app.schedule_blocks where staff_id = p_staff_id and id <> all (v_keep);
    return null;
end;
$$;

-- Same as apply_week, for the blocks of one override. An empty array removes
-- all blocks (day off).
create function app.apply_override_blocks(p_override_id uuid, p_blocks jsonb) returns text
language plpgsql volatile set search_path = ''
as $$
declare
    r record;
    v_id uuid;
    v_keep uuid[] := '{}';
begin
    if p_blocks is null or jsonb_typeof(p_blocks) <> 'array' then
        return 'blocks';
    end if;
    if jsonb_array_length(p_blocks) > 20 then
        return 'blocks_count';
    end if;

    for r in
        select x.id, x.start, x."end"
          from jsonb_to_recordset(p_blocks) as x(id uuid, start time, "end" time)
    loop
        if r.start is null or r."end" is null then
            return 'block_fields';
        end if;
        v_id := null;
        if r.id is not null then
            select b.id into v_id from app.override_blocks b
             where b.id = r.id and b.override_id = p_override_id and b.id <> all (v_keep);
        end if;
        if v_id is null then
            select b.id into v_id from app.override_blocks b
             where b.override_id = p_override_id
               and b.start_time = r.start and b.end_time = r."end"
               and b.id <> all (v_keep)
             limit 1;
        end if;
        if v_id is null then
            insert into app.override_blocks (override_id, start_time, end_time)
            values (p_override_id, r.start, r."end")
            returning id into v_id;
        else
            update app.override_blocks
               set start_time = r.start, end_time = r."end"
             where id = v_id
               and (start_time, end_time) is distinct from (r.start, r."end");
        end if;
        v_keep := v_keep || v_id;
    end loop;

    delete from app.override_blocks where override_id = p_override_id and id <> all (v_keep);
    return null;
end;
$$;

create function app.name_problem(p_first text, p_last text) returns text
language sql immutable set search_path = ''
as $$
    select case
        when p_first is null or char_length(btrim(p_first)) not between 1 and 40 then 'first_name'
        when p_last is null or char_length(btrim(p_last)) not between 1 and 40 then 'last_name'
    end
$$;

-- Turns a data or constraint error into an invalid_input answer.
create function app.invalid_from_error(p_state text, p_constraint text) returns jsonb
language sql immutable set search_path = ''
as $$
    select app.fail('invalid_input', jsonb_build_object(
        'details', coalesce(nullif(p_constraint, ''), p_state)))
$$;

-- ---------------------------------------------------------------------------
-- Public RPCs (no device secret)
-- ---------------------------------------------------------------------------

-- Works only while no passphrase exists. Sets the passphrase and quit code
-- and pairs the calling device.
create function public.admin_initialize(
    p_passphrase text, p_quit_code text, p_device_name text, p_platform text
) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    st app.auth_state;
    v_norm text := app.normalize_passphrase(p_passphrase);
    v_problem text := app.device_input_problem(p_device_name, p_platform);
begin
    select * into st from app.auth_state where id for update;
    if st.passphrase_hash is not null then
        return app.fail('already_initialized');
    end if;
    if not app.passphrase_acceptable(v_norm) then
        return app.invalid('passphrase');
    end if;
    if p_quit_code is null or p_quit_code !~ '^[0-9]{4}$' then
        return app.invalid('quit_code');
    end if;
    if v_problem is not null then
        return app.invalid(v_problem);
    end if;

    update app.auth_state
       set passphrase_hash = extensions.crypt(v_norm, extensions.gen_salt('bf', 12)),
           failed_count = 0,
           locked_until = null
     where id;
    update app.settings set quit_code = p_quit_code where id;
    return app.register_device(p_device_name, p_platform);
end;
$$;

-- Pairs a new device. Lockout-protected.
create function public.pair_device(p_passphrase text, p_device_name text, p_platform text)
returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    v_problem text := app.device_input_problem(p_device_name, p_platform);
    v_check jsonb;
begin
    if v_problem is not null then
        return app.invalid(v_problem);
    end if;
    v_check := app.verify_passphrase(p_passphrase);
    if not (v_check ->> 'ok')::boolean then
        return v_check;
    end if;
    return app.register_device(p_device_name, p_platform);
end;
$$;

-- ---------------------------------------------------------------------------
-- Device RPCs (first argument: the device secret)
-- ---------------------------------------------------------------------------

create function public.get_version(p_secret text) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    m app.meta;
begin
    select * into a from app.auth_device(p_secret);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    select * into m from app.meta where id;
    return app.ok(jsonb_build_object(
        'config_version', m.config_version,
        'data_version', m.data_version,
        'plan_config_version', m.plan_config_version,
        'plan_horizon_end', m.plan_horizon_end,
        'server_now', now()));
end;
$$;

-- Everything a device needs to show Today, ring alarms and run Settings.
create function public.get_snapshot(p_secret text) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    m app.meta;
    s app.settings;
    v_from date;
    v_ids uuid[];
    v_staff jsonb;
    v_blocks jsonb;
    v_overrides jsonb;
    v_marks jsonb;
    v_devices jsonb;
begin
    select * into a from app.auth_device(p_secret);
    if a.err is not null then
        return app.fail(a.err);
    end if;

    -- Versions first: data read afterwards is never older than them, so a
    -- concurrent change only causes one extra refetch.
    select * into m from app.meta where id;
    select * into s from app.settings where id;
    -- Two business days back: the core looks that far back for blocks
    -- (overnight blocks, missed alarms, the marks of the last 2 days).
    v_from := app.business_today() - 2;

    -- Active staff, staff removed recently (an in-progress block stays until
    -- it ends), and removed staff that still have marks in the window.
    v_ids := array(
        select st.id from app.staff st
         where st.removed_at is null
            or st.removed_at > now() - interval '3 days'
            or exists (select 1 from app.marks mk
                        where mk.staff_id = st.id and mk.voided_at is null
                          and mk.business_date >= v_from));

    select coalesce(jsonb_agg(jsonb_build_object(
               'id', st.id, 'first_name', st.first_name, 'last_name', st.last_name,
               'removed_at', st.removed_at) order by st.last_name, st.first_name, st.id), '[]')
      into v_staff
      from app.staff st
     where st.id = any (v_ids);

    select coalesce(jsonb_agg(jsonb_build_object(
               'id', b.id, 'staff_id', b.staff_id, 'weekday', b.weekday,
               'start', b.start_time, 'end', b.end_time)
               order by b.staff_id, b.weekday, b.start_time), '[]')
      into v_blocks
      from app.schedule_blocks b
     where b.staff_id = any (v_ids);

    select coalesce(jsonb_agg(jsonb_build_object(
               'id', o.id, 'staff_id', o.staff_id, 'business_date', o.business_date,
               'kind', o.kind,
               'blocks', (select coalesce(jsonb_agg(jsonb_build_object(
                                  'id', ob.id, 'start', ob.start_time, 'end', ob.end_time)
                                  order by ob.start_time), '[]')
                            from app.override_blocks ob where ob.override_id = o.id))
               order by o.business_date, o.staff_id), '[]')
      into v_overrides
      from app.overrides o
     where o.staff_id = any (v_ids) and o.business_date >= v_from;

    select coalesce(jsonb_agg(jsonb_build_object(
               'id', mk.id, 'staff_id', mk.staff_id, 'source_block_id', mk.source_block_id,
               'business_date', mk.business_date, 'kind', mk.kind,
               'marked_at', mk.marked_at, 'device_id', mk.device_id)
               order by mk.marked_at, mk.id), '[]')
      into v_marks
      from app.marks mk
     where mk.voided_at is null and mk.business_date >= v_from;

    select coalesce(jsonb_agg(jsonb_build_object(
               'id', d.id, 'name', d.name, 'platform', d.platform,
               'created_at', d.created_at, 'last_seen_at', d.last_seen_at)
               order by d.created_at, d.id), '[]')
      into v_devices
      from app.devices d
     where d.revoked_at is null;

    return app.ok(jsonb_build_object(
        'staff', v_staff,
        'blocks', v_blocks,
        'overrides', v_overrides,
        'settings', jsonb_build_object(
            'checkin_offset_min', s.checkin_offset_min,
            'checkout_offset_min', s.checkout_offset_min,
            'rollover', s.rollover,
            'autostart', s.autostart,
            'quit_code', s.quit_code),
        'marks', v_marks,
        'devices', v_devices,
        'this_device_id', a.dev,
        'config_version', m.config_version,
        'data_version', m.data_version,
        'plan_config_version', m.plan_config_version,
        'plan_horizon_end', m.plan_horizon_end,
        'server_now', now()));
end;
$$;

-- Records a check-in or check-out. Idempotent on `p_client_id`; if another
-- device already marked the same block occurrence, returns that mark.
create function public.mark(
    p_secret text, p_client_id uuid, p_staff_id uuid, p_source_block_id uuid,
    p_business_date date, p_kind text
) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    v_today date;
    mk app.marks;
begin
    select * into a from app.auth_device(p_secret);
    if a.err is not null then
        return app.fail(a.err);
    end if;

    if p_client_id is null or p_staff_id is null or p_source_block_id is null
       or p_business_date is null then
        return app.invalid('fields');
    end if;
    if p_kind is null or p_kind not in ('in', 'out') then
        return app.invalid('kind');
    end if;
    -- Offline marks may arrive late, so allow the last two business days.
    v_today := app.business_today();
    if p_business_date not between v_today - 2 and v_today + 1 then
        return app.invalid('business_date');
    end if;
    if not exists (select 1 from app.staff where id = p_staff_id) then
        return app.invalid('staff_id');
    end if;

    insert into app.marks (id, staff_id, source_block_id, business_date, kind, device_id)
    values (p_client_id, p_staff_id, p_source_block_id, p_business_date, p_kind, a.dev)
    on conflict do nothing
    returning * into mk;

    if mk.id is null then
        select * into mk from app.marks where id = p_client_id;
    end if;
    if mk.id is null then
        select * into mk from app.marks
         where staff_id = p_staff_id and source_block_id = p_source_block_id
           and business_date = p_business_date and kind = p_kind and voided_at is null;
    end if;

    return app.ok(jsonb_build_object(
        'mark_id', mk.id, 'marked_at', mk.marked_at, 'voided', mk.voided_at is not null));
end;
$$;

-- Replaces the alarm plan atomically, unless the configuration changed since
-- the device computed it.
create function public.upload_plan(
    p_secret text, p_base_config_version bigint, p_horizon_end timestamptz, p_items jsonb
) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    m app.meta;
    v_count integer;
    v_state text;
    v_constraint text;
begin
    select * into a from app.auth_device(p_secret);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    if p_base_config_version is null or p_horizon_end is null then
        return app.invalid('fields');
    end if;
    if p_items is null or jsonb_typeof(p_items) <> 'array' or jsonb_array_length(p_items) > 20000 then
        return app.invalid('items');
    end if;

    select * into m from app.meta where id for update;
    if m.config_version <> p_base_config_version then
        return app.fail('version_conflict', jsonb_build_object('config_version', m.config_version));
    end if;

    begin
        delete from app.alarm_plan where true;
        insert into app.alarm_plan
              (item_id, fires_at, kind, staff_id, display_name, business_date, source_block_id)
        select x.item_id, x.fires_at, x.kind, x.staff_id, x.display_name, x.business_date,
               x.source_block_id
          from jsonb_to_recordset(p_items) as x(
                item_id text, fires_at timestamptz, kind text, staff_id uuid,
                display_name text, business_date date, source_block_id uuid);
        get diagnostics v_count = row_count;
        update app.meta
           set plan_config_version = p_base_config_version,
               plan_horizon_end = p_horizon_end
         where id;
    exception when data_exception or integrity_constraint_violation then
        get stacked diagnostics v_state = returned_sqlstate, v_constraint = constraint_name;
        return app.invalid_from_error(v_state, v_constraint);
    end;

    return app.ok(jsonb_build_object('items', v_count));
end;
$$;

-- The plan from 15 minutes ago onward.
create function public.get_plan(p_secret text) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    m app.meta;
    v_items jsonb;
begin
    select * into a from app.auth_device(p_secret);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    select * into m from app.meta where id;
    select coalesce(jsonb_agg(jsonb_build_object(
               'item_id', p.item_id, 'fires_at', p.fires_at, 'kind', p.kind,
               'staff_id', p.staff_id, 'display_name', p.display_name,
               'business_date', p.business_date, 'source_block_id', p.source_block_id)
               order by p.fires_at, p.kind, p.item_id), '[]')
      into v_items
      from app.alarm_plan p
     where p.fires_at >= now() - interval '15 minutes';
    return app.ok(jsonb_build_object(
        'plan_config_version', m.plan_config_version,
        'horizon_end', m.plan_horizon_end,
        'config_version', m.config_version,
        'items', v_items));
end;
$$;

-- Pre-alarm check (Android). `p_items` is [{item_id, fires_at}]. An item is
-- due only if the plan still has that id at the same time and no live mark
-- suppresses it (DECISIONS 2026-10-04).
create function public.check_alarm(p_secret text, p_items jsonb) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    v_items jsonb;
    v_state text;
    v_constraint text;
begin
    select * into a from app.auth_device(p_secret);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    if p_items is null or jsonb_typeof(p_items) <> 'array' or jsonb_array_length(p_items) > 1000 then
        return app.invalid('items');
    end if;

    begin
        select coalesce(jsonb_agg(jsonb_build_object(
                   'item_id', e.v ->> 'item_id',
                   'due', exists (
                       select 1 from app.alarm_plan p
                        where p.item_id = e.v ->> 'item_id'
                          and p.fires_at = (e.v ->> 'fires_at')::timestamptz
                          and not exists (
                              select 1 from app.marks mk
                               where mk.voided_at is null and mk.staff_id = p.staff_id
                                 and mk.source_block_id = p.source_block_id
                                 and mk.business_date = p.business_date and mk.kind = p.kind)))
                   order by e.ord), '[]')
          into v_items
          from jsonb_array_elements(p_items) with ordinality as e(v, ord);
    exception when data_exception then
        get stacked diagnostics v_state = returned_sqlstate, v_constraint = constraint_name;
        return app.invalid_from_error(v_state, v_constraint);
    end;

    return app.ok(jsonb_build_object(
        'items', v_items,
        'config_version', (select config_version from app.meta where id)));
end;
$$;

-- Starts an admin session (10 minutes, sliding). Lockout-protected.
create function public.admin_login(p_secret text, p_passphrase text) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    v_check jsonb;
    v_token text;
begin
    select * into a from app.auth_device(p_secret);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    v_check := app.verify_passphrase(p_passphrase);
    if not (v_check ->> 'ok')::boolean then
        return v_check;
    end if;

    delete from app.admin_sessions where expires_at <= now();
    v_token := app.new_token();
    insert into app.admin_sessions (token_hash, device_id, expires_at)
    values (app.hash_token(v_token), a.dev, now() + interval '10 minutes');
    return app.ok(jsonb_build_object('session_token', v_token, 'expires_in_s', 600));
end;
$$;

-- Ends this device's admin session. Needs only the device secret, so an
-- already-expired session can still be cleared.
create function public.admin_logout(p_secret text, p_session text) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
begin
    select * into a from app.auth_device(p_secret);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    delete from app.admin_sessions
     where token_hash = app.hash_token(p_session) and device_id = a.dev;
    return app.ok();
end;
$$;

-- ---------------------------------------------------------------------------
-- Admin RPCs (device secret + admin session)
-- ---------------------------------------------------------------------------

create function public.staff_create(
    p_secret text, p_session text, p_first_name text, p_last_name text, p_blocks jsonb
) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    v_problem text := app.name_problem(p_first_name, p_last_name);
    v_id uuid;
    v_state text;
    v_constraint text;
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    if v_problem is not null then
        return app.invalid(v_problem);
    end if;

    begin
        insert into app.staff (first_name, last_name)
        values (btrim(p_first_name), btrim(p_last_name))
        returning id into v_id;
        v_problem := app.apply_week(v_id, p_blocks);
        if v_problem is not null then
            raise exception using errcode = 'P0001', message = v_problem;
        end if;
    exception
        when data_exception or integrity_constraint_violation then
            get stacked diagnostics v_state = returned_sqlstate, v_constraint = constraint_name;
            return app.invalid_from_error(v_state, v_constraint);
        when raise_exception then
            return app.invalid(sqlerrm);
    end;

    return app.ok(jsonb_build_object('staff_id', v_id));
end;
$$;

create function public.staff_update(
    p_secret text, p_session text, p_staff_id uuid, p_first_name text, p_last_name text
) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    v_problem text := app.name_problem(p_first_name, p_last_name);
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    if v_problem is not null then
        return app.invalid(v_problem);
    end if;
    update app.staff
       set first_name = btrim(p_first_name), last_name = btrim(p_last_name), updated_at = now()
     where id = p_staff_id and removed_at is null;
    if not found then
        return app.invalid('staff_id');
    end if;
    return app.ok();
end;
$$;

-- Removes a person from the next block onward (SPEC §4.1). Their blocks and
-- marks stay until the retention purge.
create function public.staff_remove(p_secret text, p_session text, p_staff_id uuid)
returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    update app.staff set removed_at = now(), updated_at = now()
     where id = p_staff_id and removed_at is null;
    if not found then
        return app.invalid('staff_id');
    end if;
    return app.ok();
end;
$$;

-- Replaces a person's whole weekly template atomically.
create function public.schedule_set(p_secret text, p_session text, p_staff_id uuid, p_blocks jsonb)
returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    v_problem text;
    v_state text;
    v_constraint text;
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    if not exists (select 1 from app.staff where id = p_staff_id and removed_at is null) then
        return app.invalid('staff_id');
    end if;

    begin
        v_problem := app.apply_week(p_staff_id, p_blocks);
        if v_problem is not null then
            raise exception using errcode = 'P0001', message = v_problem;
        end if;
    exception
        when data_exception or integrity_constraint_violation then
            get stacked diagnostics v_state = returned_sqlstate, v_constraint = constraint_name;
            return app.invalid_from_error(v_state, v_constraint);
        when raise_exception then
            return app.invalid(sqlerrm);
    end;
    return app.ok();
end;
$$;

-- Creates or replaces the override of one person on one business date
-- (today or later). `off` takes no blocks; `replace` takes at least one.
create function public.override_set(
    p_secret text, p_session text, p_staff_id uuid, p_business_date date, p_kind text,
    p_blocks jsonb
) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    v_id uuid;
    v_problem text;
    v_state text;
    v_constraint text;
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    if not exists (select 1 from app.staff where id = p_staff_id and removed_at is null) then
        return app.invalid('staff_id');
    end if;
    if p_business_date is null or p_business_date < app.business_today() then
        return app.invalid('business_date');
    end if;
    if p_kind is null or p_kind not in ('off', 'replace') then
        return app.invalid('kind');
    end if;
    if p_blocks is null or jsonb_typeof(p_blocks) <> 'array' then
        return app.invalid('blocks');
    end if;
    if p_kind = 'off' and jsonb_array_length(p_blocks) <> 0 then
        return app.invalid('blocks');
    end if;
    if p_kind = 'replace' and jsonb_array_length(p_blocks) = 0 then
        return app.invalid('blocks_count');
    end if;

    begin
        insert into app.overrides (staff_id, business_date, kind)
        values (p_staff_id, p_business_date, p_kind)
        on conflict (staff_id, business_date) do update set kind = excluded.kind
        returning id into v_id;
        v_problem := app.apply_override_blocks(v_id, p_blocks);
        if v_problem is not null then
            raise exception using errcode = 'P0001', message = v_problem;
        end if;
    exception
        when data_exception or integrity_constraint_violation then
            get stacked diagnostics v_state = returned_sqlstate, v_constraint = constraint_name;
            return app.invalid_from_error(v_state, v_constraint);
        when raise_exception then
            return app.invalid(sqlerrm);
    end;
    return app.ok(jsonb_build_object('override_id', v_id));
end;
$$;

create function public.override_delete(p_secret text, p_session text, p_override_id uuid)
returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    delete from app.overrides where id = p_override_id;
    return app.ok(jsonb_build_object('deleted', found));
end;
$$;

create function public.settings_update(
    p_secret text, p_session text, p_checkin_offset_min integer, p_checkout_offset_min integer,
    p_rollover time, p_autostart boolean, p_quit_code text
) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    if p_checkin_offset_min is null or p_checkin_offset_min not between -60 and 30 then
        return app.invalid('checkin_offset_min');
    end if;
    if p_checkout_offset_min is null or p_checkout_offset_min not between -60 and 30 then
        return app.invalid('checkout_offset_min');
    end if;
    if p_rollover is null or p_rollover not between '00:00' and '08:00'
       or date_trunc('minute', p_rollover) <> p_rollover then
        return app.invalid('rollover');
    end if;
    if p_autostart is null then
        return app.invalid('autostart');
    end if;
    if p_quit_code is null or p_quit_code !~ '^[0-9]{4}$' then
        return app.invalid('quit_code');
    end if;

    update app.settings
       set checkin_offset_min = p_checkin_offset_min,
           checkout_offset_min = p_checkout_offset_min,
           rollover = p_rollover,
           autostart = p_autostart,
           quit_code = p_quit_code
     where id;
    return app.ok();
end;
$$;

-- Corrects a mistaken mark (SPEC §4.5).
create function public.mark_void(p_secret text, p_session text, p_mark_id uuid) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    update app.marks set voided_at = now() where id = p_mark_id and voided_at is null;
    return app.ok(jsonb_build_object('voided', found));
end;
$$;

create function public.device_revoke(p_secret text, p_session text, p_device_id uuid)
returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    update app.devices set revoked_at = now() where id = p_device_id and revoked_at is null;
    if not found then
        return app.invalid('device_id');
    end if;
    delete from app.admin_sessions where device_id = p_device_id;
    return app.ok();
end;
$$;

-- Needs the current passphrase (lockout-protected) as well as a session.
-- Signs out every admin session; paired devices stay paired.
create function public.change_passphrase(
    p_secret text, p_session text, p_old_passphrase text, p_new_passphrase text
) returns jsonb
language plpgsql volatile security definer set search_path = ''
as $$
declare
    a record;
    v_new text := app.normalize_passphrase(p_new_passphrase);
    v_check jsonb;
begin
    select * into a from app.auth_admin(p_secret, p_session);
    if a.err is not null then
        return app.fail(a.err);
    end if;
    if not app.passphrase_acceptable(v_new) then
        return app.invalid('passphrase');
    end if;
    v_check := app.verify_passphrase(p_old_passphrase);
    if not (v_check ->> 'ok')::boolean then
        return v_check;
    end if;

    update app.auth_state
       set passphrase_hash = extensions.crypt(v_new, extensions.gen_salt('bf', 12)),
           failed_count = 0,
           locked_until = null
     where id;
    delete from app.admin_sessions where true;
    return app.ok();
end;
$$;

-- ---------------------------------------------------------------------------
-- Grants: helpers are never granted; the RPCs above go to `anon` only.
-- ---------------------------------------------------------------------------

revoke all on all functions in schema app from public, anon, authenticated, service_role;

do $$
declare
    f regprocedure;
    v_count integer := 0;
begin
    for f in
        select p.oid::regprocedure
          from pg_catalog.pg_proc p
          join pg_catalog.pg_namespace n on n.oid = p.pronamespace
         where n.nspname = 'public'
           and p.proname in (
               'admin_initialize', 'pair_device',
               'get_version', 'get_snapshot', 'mark', 'upload_plan', 'get_plan',
               'check_alarm', 'admin_login', 'admin_logout',
               'staff_create', 'staff_update', 'staff_remove', 'schedule_set',
               'override_set', 'override_delete', 'settings_update', 'mark_void',
               'device_revoke', 'change_passphrase')
    loop
        execute format('revoke all on function %s from public, anon, authenticated, service_role', f);
        execute format('grant execute on function %s to anon', f);
        v_count := v_count + 1;
    end loop;
    if v_count <> 20 then
        raise exception 'expected 20 RPC functions, found %', v_count;
    end if;
end;
$$;

notify pgrst, 'reload schema';
