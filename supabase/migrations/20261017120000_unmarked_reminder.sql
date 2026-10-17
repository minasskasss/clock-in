-- Clock In 1.1.0: the «Λείπει σήμανση» reminder setting (SPEC §4.4, §7.6).
--
-- Compatible with 1.0.0 clients, which keep working against this schema:
--
-- - The new column has a default, so nothing that inserts or updates
--   `app.settings` without it changes behaviour.
-- - `settings_update` (1.0.0's call) is left exactly as it is. It sets only
--   its own five columns, so a 1.0.0 device saving «Ειδοποιήσεις» never
--   resets the new value. 1.1.0 calls `settings_update_v2`, a separate name
--   rather than an extra defaulted argument, so PostgREST never has to pick
--   between overloads and 1.0.0's request is untouched.
-- - `get_snapshot` and `get_plan` only gain a key; 1.0.0 ignores unknown keys
--   (serde without `deny_unknown_fields`; Kotlin `org.json` with `opt…`).
--   `create or replace` keeps their grants.

alter table app.settings
    add column unmarked_reminder_min integer not null default 10
        check (unmarked_reminder_min between 0 and 60);

comment on column app.settings.unmarked_reminder_min is
    'Minutes after «Σταμάτημα» before the «Λείπει σήμανση» reminder; 0 = off.';

create or replace function public.get_snapshot(p_secret text) returns jsonb
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

    -- Active staff, staff removed within the window (an in-progress block
    -- stays until it ends), and removed staff that still have marks in it.
    v_ids := array(
        select st.id from app.staff st
         where st.removed_at is null
            or st.removed_at >= now() - app.removed_staff_window()
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
            'quit_code', s.quit_code,
            'unmarked_reminder_min', s.unmarked_reminder_min),
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

create or replace function public.get_plan(p_secret text) returns jsonb
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
        'unmarked_reminder_min', (select st.unmarked_reminder_min from app.settings st where st.id),
        'items', v_items));
end;
$$;

-- Every setting, including the reminder (1.1.0). Like `settings_update`, all
-- values are sent, so the caller re-sends the current ones it doesn't change.
create function public.settings_update_v2(
    p_secret text, p_session text, p_checkin_offset_min integer, p_checkout_offset_min integer,
    p_rollover time, p_autostart boolean, p_quit_code text,
    p_unmarked_reminder_min integer
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
    if p_unmarked_reminder_min is null or p_unmarked_reminder_min not between 0 and 60 then
        return app.invalid('unmarked_reminder_min');
    end if;

    update app.settings
       set checkin_offset_min = p_checkin_offset_min,
           checkout_offset_min = p_checkout_offset_min,
           rollover = p_rollover,
           autostart = p_autostart,
           quit_code = p_quit_code,
           unmarked_reminder_min = p_unmarked_reminder_min
     where id;
    return app.ok();
end;
$$;

revoke all on function public.settings_update_v2(text, text, integer, integer, time, boolean, text, integer)
    from public, anon, authenticated, service_role;
grant execute on function public.settings_update_v2(text, text, integer, integer, time, boolean, text, integer)
    to anon;

-- Unchanged grants, restated (`create or replace` keeps them anyway).
revoke all on function public.get_snapshot(text) from public, anon, authenticated, service_role;
grant execute on function public.get_snapshot(text) to anon;
revoke all on function public.get_plan(text) from public, anon, authenticated, service_role;
grant execute on function public.get_plan(text) to anon;

notify pgrst, 'reload schema';
