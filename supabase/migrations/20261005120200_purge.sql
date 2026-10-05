-- Clock In: daily retention purge (SPEC §9, ARCHITECTURE §6 "Maintenance").

-- - marks and overrides: deleted 30 days after their business date;
-- - removed staff: deleted once no marks reference them (and at least two
--   days after removal, so an in-progress block of a just-removed person
--   is never cut short);
-- - expired admin sessions and alarm-plan items that fired over a day ago.
create function app.purge() returns jsonb
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

    delete from app.staff s
     where s.removed_at < now() - interval '2 days'
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

revoke all on function app.purge() from public, anon, authenticated, service_role;

-- pg_cron, as the Supabase docs describe (https://supabase.com/docs/guides/cron/install).
create extension if not exists pg_cron with schema pg_catalog;
grant usage on schema cron to postgres;
grant all privileges on all tables in schema cron to postgres;

-- Every day at 01:17 UTC (03:17 or 04:17 in Greece). Same name = upsert.
select cron.schedule('clockin-purge', '17 1 * * *', 'select app.purge()');
