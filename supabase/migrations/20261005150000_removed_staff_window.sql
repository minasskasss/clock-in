-- Clock In: one window for removed staff, shared by the snapshot and the purge.
--
-- `get_snapshot` keeps a removed person for 3 days after removal, so a block
-- that was running when they were removed (or that ends after the rollover)
-- still shows and still rings. The purge must never delete someone who is
-- still inside that window: it now deletes removed staff only when their
-- removal is older than the same window and no mark references them.
-- (Before, the purge waited 2 days and the snapshot 3.)

create function app.removed_staff_window() returns interval
language sql immutable set search_path = ''
as $$ select interval '3 days' $$;

revoke all on function app.removed_staff_window() from public, anon, authenticated, service_role;

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

create or replace function app.purge() returns jsonb
language plpgsql volatile set search_path = ''
as $$
declare
    v_cutoff date := app.business_today() - 30;
    v_marks integer;
    v_overrides integer;
    v_staff integer;
    v_sessions integer;
    v_plan integer;
begin
    delete from app.marks where business_date <= v_cutoff;
    get diagnostics v_marks = row_count;

    delete from app.overrides where business_date <= v_cutoff;
    get diagnostics v_overrides = row_count;

    -- Only once outside the snapshot window, and only with no marks at all
    -- (voided ones included) referencing them.
    delete from app.staff s
     where s.removed_at < now() - app.removed_staff_window()
       and not exists (select 1 from app.marks m where m.staff_id = s.id);
    get diagnostics v_staff = row_count;

    delete from app.admin_sessions where expires_at <= now();
    get diagnostics v_sessions = row_count;

    delete from app.alarm_plan where fires_at < now() - interval '1 day';
    get diagnostics v_plan = row_count;

    return jsonb_build_object(
        'marks', v_marks, 'overrides', v_overrides, 'staff', v_staff,
        'sessions', v_sessions, 'plan_items', v_plan);
end;
$$;

-- `create or replace` keeps the existing grants; state them again anyway.
revoke all on function app.purge() from public, anon, authenticated, service_role;
revoke all on function public.get_snapshot(text) from public, anon, authenticated, service_role;
grant execute on function public.get_snapshot(text) to anon;

notify pgrst, 'reload schema';
