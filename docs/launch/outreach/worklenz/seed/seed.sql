-- Seeds a user, a project and two recurring tasks with Worklenz's own database
-- functions (register_user, create_project, create_quick_task,
-- create_recurring_task_template), the way the app does it.
-- Run in the db container: psql -U postgres -d worklenz_db -f /seed/seed.sql
DO $$
DECLARE
    _user    json;
    _project json;
    _project_id uuid;
    _task json;
    _schedule_id uuid;
    _rec record;
BEGIN
    _user := register_user(json_build_object(
        'email', 'owner@example.com', 'name', 'Test Owner', 'team_name', 'Test Team',
        'password', 'unused', 'timezone', 'UTC'));

    _project := create_project(json_build_object(
        'name', 'Ops', 'key', 'OPS', 'team_id', _user->>'team_id', 'user_id', _user->>'id',
        'color_code', '#70a6f3', 'project_created_log', 'created the project',
        'status_id', (SELECT id FROM sys_project_statuses ORDER BY sort_order NULLS LAST LIMIT 1),
        'health_id', (SELECT id FROM sys_project_healths ORDER BY sort_order NULLS LAST LIMIT 1)));
    _project_id := COALESCE((_project->>'id')::uuid, (SELECT id FROM projects WHERE name = 'Ops' LIMIT 1));

    -- name, weekday (0 = Sunday), week of month, first due date
    FOR _rec IN SELECT * FROM (VALUES
        ('Team sync (1st Monday)', 1, 1, DATE '2026-12-07'),
        ('Release review (2nd Tuesday)', 2, 2, DATE '2026-12-08')
    ) AS v(name, weekday, week_of_month, first_due)
    LOOP
        INSERT INTO task_recurring_schedules (schedule_type, day_of_month, week_of_month, created_by, created_at)
        VALUES ('monthly', _rec.weekday, _rec.week_of_month, (_user->>'id')::uuid, now())
        RETURNING id INTO _schedule_id;

        _task := create_quick_task(json_build_object(
            'name', _rec.name, 'project_id', _project_id, 'reporter_id', _user->>'id',
            'end_date', _rec.first_due, 'schedule_id', _schedule_id));

        UPDATE tasks SET schedule_id = _schedule_id
        WHERE project_id = _project_id AND name = _rec.name AND schedule_id IS NULL;

        PERFORM create_recurring_task_template(
            (SELECT id FROM tasks WHERE project_id = _project_id AND name = _rec.name ORDER BY created_at LIMIT 1),
            _schedule_id);
    END LOOP;
END $$;

SELECT name, end_date FROM tasks ORDER BY name, end_date;
