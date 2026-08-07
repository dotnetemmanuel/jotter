use jotter_store::{Store, StoreError, TaskState, command, date, query};
use time::macros::date;

#[test]
fn a_task_due_before_today_is_overdue_by_the_right_number_of_days() {
    let due = date!(2026 - 08 - 01);
    let today = date!(2026 - 08 - 04);

    assert!(date::is_overdue(Some(due), false, today));
    assert_eq!(date::days_overdue(Some(due), false, today), Some(3));
}

#[test]
fn a_task_due_today_is_due_today_and_not_overdue() {
    let today = date!(2026 - 08 - 04);

    assert!(!date::is_overdue(Some(today), false, today));
    assert_eq!(date::days_overdue(Some(today), false, today), None);
}

#[test]
fn a_task_with_no_due_date_is_never_overdue() {
    let today = date!(2026 - 08 - 04);

    assert!(!date::is_overdue(None, false, today));
    assert_eq!(date::days_overdue(None, false, today), None);
}

#[test]
fn a_done_task_is_never_overdue_even_if_its_date_has_passed() {
    let due = date!(2026 - 01 - 01);
    let today = date!(2026 - 08 - 04);

    assert!(!date::is_overdue(Some(due), true, today));
    assert_eq!(date::days_overdue(Some(due), true, today), None);
}

// "This week" is picked as the Monday-to-Sunday calendar week containing `today`,
// not a rolling seven days; see the doc comment on `date::is_due_this_week`.
#[test]
fn due_this_week_uses_calendar_days_not_a_rolling_seven() {
    // 2026-08-04 is a Tuesday; this calendar week runs Mon 2026-08-03 to Sun 2026-08-09.
    let today = date!(2026 - 08 - 04);
    let later_this_week = date!(2026 - 08 - 07);
    let next_monday = date!(2026 - 08 - 10);

    assert!(date::is_due_this_week(later_this_week, today));
    // Six days out, well inside a rolling seven-day window, but the next calendar week.
    assert!(!date::is_due_this_week(next_monday, today));
}

#[test]
fn a_project_counts_only_its_own_unfinished_tasks() {
    let store = Store::open_in_memory().unwrap();
    let home = command::create_project(&store, "Home", None).unwrap();
    let trip = command::create_project(&store, "Trip", None).unwrap();

    let paint = command::create_task(&store, "Paint fence", Some(home.id), None, None).unwrap();
    command::create_task(&store, "Mow lawn", Some(home.id), None, None).unwrap();
    command::move_task_to_state(&store, paint.id, TaskState::Done).unwrap();
    command::create_task(&store, "Book flight", Some(trip.id), None, None).unwrap();

    let home_left = query::unfinished_task_count(&store, home.id).unwrap();
    let trip_left = query::unfinished_task_count(&store, trip.id).unwrap();

    assert_eq!(home_left, 1);
    assert_eq!(trip_left, 1);
}

#[test]
fn the_pace_is_tasks_left_over_workdays_left() {
    // Mon through Wed: three workdays, six tasks, so two a day.
    let today = date!(2026 - 08 - 03);
    let due = date!(2026 - 08 - 05);

    let result = date::pace(6, Some(due), today, &date::DEFAULT_WORKDAYS);

    assert_eq!(result, Some(2.0));
}

#[test]
fn a_weekend_between_today_and_the_deadline_does_not_count() {
    // Fri 2026-08-07 to Mon 2026-08-10 spans a weekend: two workdays (Fri, Mon), not four days.
    let today = date!(2026 - 08 - 07);
    let due = date!(2026 - 08 - 10);

    let result = date::pace(4, Some(due), today, &date::DEFAULT_WORKDAYS);

    assert_eq!(result, Some(2.0));
}

#[test]
fn a_project_with_no_due_date_has_no_pace() {
    let store = Store::open_in_memory().unwrap();
    let project = command::create_project(&store, "Someday", None).unwrap();
    command::create_task(&store, "Anything", Some(project.id), None, None).unwrap();

    let pace = query::project_pace(
        &store,
        &project,
        date!(2026 - 08 - 04),
        &date::DEFAULT_WORKDAYS,
    )
    .unwrap();

    assert_eq!(pace, None);
}

#[test]
fn a_deadline_already_past_has_no_pace_rather_than_a_negative_one() {
    let store = Store::open_in_memory().unwrap();
    let project = command::create_project(&store, "Overdue project", Some("2026-01-01")).unwrap();
    command::create_task(&store, "Anything", Some(project.id), None, None).unwrap();

    let pace = query::project_pace(
        &store,
        &project,
        date!(2026 - 08 - 04),
        &date::DEFAULT_WORKDAYS,
    )
    .unwrap();

    assert_eq!(pace, None);
}

#[test]
fn a_deadline_with_no_workdays_before_it_has_no_pace_rather_than_a_division_by_zero() {
    let store = Store::open_in_memory().unwrap();
    // Sat 2026-08-08, due Sun 2026-08-09: the deadline is this weekend, no workday in between.
    let project = command::create_project(&store, "Weekend project", Some("2026-08-09")).unwrap();
    command::create_task(&store, "Anything", Some(project.id), None, None).unwrap();

    let pace = query::project_pace(
        &store,
        &project,
        date!(2026 - 08 - 08),
        &date::DEFAULT_WORKDAYS,
    )
    .unwrap();

    assert_eq!(pace, None);
}

#[test]
fn a_project_with_nothing_left_has_a_pace_of_zero_rather_than_no_pace() {
    let store = Store::open_in_memory().unwrap();
    let project = command::create_project(&store, "Done project", Some("2026-01-01")).unwrap();
    let task = command::create_task(&store, "Last thing", Some(project.id), None, None).unwrap();
    command::move_task_to_state(&store, task.id, TaskState::Done).unwrap();

    let pace = query::project_pace(
        &store,
        &project,
        date!(2026 - 08 - 04),
        &date::DEFAULT_WORKDAYS,
    )
    .unwrap();

    assert_eq!(pace, Some(0.0));
}

#[test]
fn overdue_tasks_lists_only_undone_tasks_past_their_date() {
    let store = Store::open_in_memory().unwrap();
    let today = date!(2026 - 08 - 04);
    let overdue = command::create_task(&store, "Overdue", None, Some("2026-08-01"), None).unwrap();
    command::create_task(&store, "Not yet due", None, Some("2026-08-10"), None).unwrap();
    let done_but_late =
        command::create_task(&store, "Done but late", None, Some("2026-01-01"), None).unwrap();
    command::move_task_to_state(&store, done_but_late.id, TaskState::Done).unwrap();

    let ids: Vec<i64> = query::overdue_tasks(&store, today)
        .unwrap()
        .iter()
        .map(|task| task.id)
        .collect();

    assert_eq!(ids, vec![overdue.id]);
}

#[test]
fn tasks_due_this_week_lists_only_undone_tasks_in_the_current_calendar_week() {
    let store = Store::open_in_memory().unwrap();
    // 2026-08-04 is a Tuesday; this calendar week runs Mon 2026-08-03 to Sun 2026-08-09.
    let today = date!(2026 - 08 - 04);
    let this_week =
        command::create_task(&store, "This week", None, Some("2026-08-07"), None).unwrap();
    command::create_task(&store, "Next week", None, Some("2026-08-10"), None).unwrap();
    let done_this_week = command::create_task(
        &store,
        "Done, but this week",
        None,
        Some("2026-08-07"),
        None,
    )
    .unwrap();
    command::move_task_to_state(&store, done_this_week.id, TaskState::Done).unwrap();

    let ids: Vec<i64> = query::tasks_due_this_week(&store, today)
        .unwrap()
        .iter()
        .map(|task| task.id)
        .collect();

    assert_eq!(ids, vec![this_week.id]);
}

#[test]
fn a_malformed_due_date_is_rejected_when_creating_a_task() {
    let store = Store::open_in_memory().unwrap();

    let err = command::create_task(&store, "Bad date", None, Some("tomorrow"), None).unwrap_err();

    assert!(matches!(err, StoreError::InvalidDueDate(text) if text == "tomorrow"));
}

#[test]
fn a_malformed_due_date_is_rejected_when_creating_a_project() {
    let store = Store::open_in_memory().unwrap();

    let err = command::create_project(&store, "Bad project", Some("not-a-date")).unwrap_err();

    assert!(matches!(err, StoreError::InvalidDueDate(text) if text == "not-a-date"));
}

#[test]
fn a_malformed_due_date_never_reaches_the_row() {
    let store = Store::open_in_memory().unwrap();

    let _ = command::create_task(&store, "Bad date", None, Some("tomorrow"), None);

    assert!(
        query::tasks_in_state(&store, TaskState::NotStarted)
            .unwrap()
            .is_empty()
    );
}
