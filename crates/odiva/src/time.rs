//! Time labels.
//!
//! The protocol carries when a message was said, in UTC milliseconds. Writing
//! that down for a person needs *their* calendar: is it today, was it yesterday,
//! was it last Tuesday. That conversion is the window's job — the core says when,
//! not how to phrase it.
//!
//! Two rules, both from how messengers behave:
//!
//! * a divider appears at the start of the conversation, after a pause long
//!   enough that the next message is a new occasion, and across midnight;
//! * the label says the most recent thing that is still true — the clock for
//!   today, the day for this week, the date after that.

use chrono::{DateTime, Datelike, Local, TimeZone, Timelike, Weekday};

/// How long a silence has to be before the conversation is considered resumed.
///
/// Five minutes: short enough that a real pause gets marked, long enough that
/// answering in two breaths does not put a label between them.
const PAUSE_MS: i64 = 5 * 60 * 1000;

/// Whether a divider belongs above a message sent at `sent`.
pub fn needs_divider(previous: Option<i64>, sent: i64) -> bool {
    let Some(previous) = previous else {
        return true;
    };

    if sent.saturating_sub(previous) >= PAUSE_MS {
        return true;
    }

    local(sent).date_naive() != local(previous).date_naive()
}

/// How a message's time is written, relative to "now".
pub fn label(sent: i64, now: i64) -> String {
    let at = local(sent);
    let day = at.date_naive();
    let today = local(now).date_naive();
    let clock = format!("{:02}:{:02}", at.hour(), at.minute());

    if day == today {
        return format!("今天 {clock}");
    }

    if Some(day) == today.pred_opt() {
        return format!("昨天 {clock}");
    }

    let days_ago = (today - day).num_days();

    if (0..7).contains(&days_ago) {
        return format!("{} {clock}", weekday(at.weekday()));
    }

    if day.year() == today.year() {
        return format!("{}月{}日 {clock}", day.month(), day.day());
    }

    format!(
        "{}年{}月{}日 {clock}",
        day.year(),
        day.month(),
        day.day()
    )
}

fn local(ms: i64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .unwrap_or_else(Local::now)
}

fn weekday(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "周一",
        Weekday::Tue => "周二",
        Weekday::Wed => "周三",
        Weekday::Thu => "周四",
        Weekday::Fri => "周五",
        Weekday::Sat => "周六",
        Weekday::Sun => "周日",
    }
}
