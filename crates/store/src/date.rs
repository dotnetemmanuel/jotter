//! Calendar-date arithmetic: overdue, due-this-week, and the project pace readout.
//!
//! Every function here takes `today` as an argument and reads no clock. The
//! frontend supplies the real date; tests supply whatever date makes the case
//! interesting. Due dates are calendar dates, `YYYY-MM-DD`, never instants, so
//! nothing in this module or its callers should reach for a timezone type.

use time::{Date, Duration, Weekday};

use crate::StoreError;

const DATE_FORMAT: &[time::format_description::BorrowedFormatItem<'_>] =
    time::macros::format_description!("[year]-[month]-[day]");

/// The default workweek: Monday through Friday.
pub const DEFAULT_WORKDAYS: [Weekday; 5] = [
    Weekday::Monday,
    Weekday::Tuesday,
    Weekday::Wednesday,
    Weekday::Thursday,
    Weekday::Friday,
];

/// Parses a stored due date, `YYYY-MM-DD`, into a calendar [`Date`].
///
/// # Errors
/// Returns [`StoreError::InvalidDueDate`] if `text` is not that shape.
pub fn parse_due_date(text: &str) -> Result<Date, StoreError> {
    Date::parse(text, DATE_FORMAT).map_err(|_| StoreError::InvalidDueDate(text.to_owned()))
}

/// Days a task is overdue as of `today`, or `None` if it is not.
///
/// A task with no due date is never overdue. A task already marked done is never
/// overdue either, however far its date has slipped into the past: a finished task
/// should not keep nagging. A due date of `today` itself is not yet overdue, only
/// one that has already passed.
#[must_use]
pub fn days_overdue(due_date: Option<Date>, done: bool, today: Date) -> Option<i64> {
    if done {
        return None;
    }
    let due = due_date?;
    if due >= today {
        return None;
    }
    Some((today - due).whole_days())
}

/// Whether a task is overdue as of `today`. See [`days_overdue`] for the cases
/// this treats as not overdue: no due date, already done, or a date not yet past.
#[must_use]
pub fn is_overdue(due_date: Option<Date>, done: bool, today: Date) -> bool {
    days_overdue(due_date, done, today).is_some()
}

/// Whether `due_date` falls in the same Monday-to-Sunday calendar week as `today`.
///
/// This is the calendar week containing `today`, not a rolling seven-day window: a
/// task due next Monday does not read as "due this week" just because today
/// happens to be five days out from it. A task due last Friday does not read as
/// "due this week" either, even if today is this Monday, three days later.
#[must_use]
pub fn is_due_this_week(due_date: Date, today: Date) -> bool {
    let days_since_monday = i64::from(today.weekday().number_days_from_monday());
    let week_start = today
        .checked_sub(Duration::days(days_since_monday))
        .unwrap_or(today);
    let week_end = week_start
        .checked_add(Duration::days(6))
        .unwrap_or(week_start);
    due_date >= week_start && due_date <= week_end
}

/// Counts workdays in the closed range `[from, to]`, treating a date as a workday
/// when its weekday appears in `workdays`. Both ends count, so a `from` that is
/// itself a workday contributes one: the day is not yet over, so it is still
/// available to work in. Returns 0 if `from` is after `to`.
#[must_use]
pub fn workdays_between(from: Date, to: Date, workdays: &[Weekday]) -> i64 {
    if from > to {
        return 0;
    }
    let mut count = 0i64;
    let mut day = from;
    loop {
        if workdays.contains(&day.weekday()) {
            count += 1;
        }
        if day == to {
            break;
        }
        let Some(next) = day.next_day() else { break };
        day = next;
    }
    count
}

/// The pace a project needs: tasks left divided by workdays remaining until its
/// due date. Answers "how many would I have to close from here".
///
/// Returns `Some(0.0)` when `tasks_left` is zero, regardless of the due date: a
/// finished project needs no pace at all, which is a different answer from "no
/// pace can be computed" and the two must not be confused. Otherwise returns
/// `None`, rather than a negative or infinite number, when there is no due date,
/// the due date has already passed, or no workday falls between today and the due
/// date (for instance a deadline landing on a weekend with no workday still ahead
/// of it).
#[must_use]
pub fn pace(
    tasks_left: i64,
    due_date: Option<Date>,
    today: Date,
    workdays: &[Weekday],
) -> Option<f64> {
    if tasks_left <= 0 {
        return Some(0.0);
    }
    let due_date = due_date?;
    if due_date < today {
        return None;
    }
    let workdays_remaining = workdays_between(today, due_date, workdays);
    if workdays_remaining == 0 {
        return None;
    }
    // Task and workday counts are small in practice; the precision this loses is not reachable.
    #[allow(clippy::cast_precision_loss)]
    Some(tasks_left as f64 / workdays_remaining as f64)
}
