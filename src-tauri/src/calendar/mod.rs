//! The calendar and reminders: events, and things to be reminded about, saved in mouce's
//! database. A note in plain language becomes a draft (`understand`) that the user checks
//! before anything is saved, and a gentle reminder appears beside the mouce when one is
//! due (`alerts`). Either can repeat (`date::Repeat`).
//!
//! Dates and times are kept as the user wrote them, on their own clock ("2026-10-30",
//! "15:00"). When to remind is a moment (`alert_at`, Unix milliseconds) worked out here with
//! the computer's time zone (`date`).
//!
//! A repeating event is saved once, on the day it starts, and shown on every day it falls
//! on; it reminds before each one. A repeating reminder is about one day at a time
//! (`current`): ticking it off moves it on, and so does its next time coming round.

pub mod alerts;
pub mod date;
pub mod understand;

use crate::store::{AppStore, StoreError};
use date::{Day, Repeat};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

/// The longest title kept, in characters.
pub const MAX_TITLE: usize = 200;
/// The furthest ahead of its event a reminder can be, in minutes (a week).
const MAX_REMIND_BEFORE: u32 = 7 * 24 * 60;
/// When an all-day event reminds: 9:00 on the day (or the days before), in minutes.
pub const ALL_DAY_REMINDER: u32 = 9 * 60;
/// How many days of a repeating event are looked through for its next reminder. A daily
/// event reminding a week ahead needs eight; the rest is room for a computer that slept.
const SEARCH_LIMIT: usize = 1000;
/// The most days a repeating event is shown on in one range (a month's grid is 42).
const SHOWN_LIMIT: usize = 62;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Something happening on a day, perhaps at a time.
    Event,
    /// Something to do, with a time to be reminded.
    Reminder,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Event => "event",
            Kind::Reminder => "reminder",
        }
    }

    fn parse(s: &str) -> Option<Kind> {
        match s {
            "event" => Some(Kind::Event),
            "reminder" => Some(Kind::Reminder),
            _ => None,
        }
    }
}

/// A saved event or reminder, as shown on one day.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: i64,
    pub kind: Kind,
    pub title: String,
    /// The day it is shown on, "YYYY-MM-DD": one day of a repeating event, or the day a
    /// repeating reminder is about now.
    pub date: String,
    /// The day it was saved for (where a repeating one starts); changing it uses this.
    pub start_date: String,
    /// "HH:MM", 24-hour; `None` is an all-day event.
    pub time: Option<String>,
    /// Events only: minutes before the start to remind; `None` is no reminder.
    pub remind_before: Option<u32>,
    pub repeat: Option<Repeat>,
    /// When it next reminds, in Unix milliseconds; `None` is never.
    pub alert_at: Option<i64>,
    /// Reminders only: it has been done.
    pub done: bool,
}

/// An event or reminder as the page sends it, to add or to change.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub kind: Kind,
    pub title: String,
    pub date: String,
    pub time: Option<String>,
    pub remind_before: Option<u32>,
    #[serde(default)]
    pub repeat: Option<Repeat>,
}

/// Whether `s` is a real day written "YYYY-MM-DD".
pub fn valid_date(s: &str) -> bool {
    Day::parse(s).is_some()
}

/// `s` as "HH:MM" on the 24-hour clock, if it is a time. "9:05" becomes "09:05".
pub fn normal_time(s: &str) -> Option<String> {
    let (h, m) = s.trim().split_once(':')?;
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit());
    if !digits(h) || h.len() > 2 || m.len() != 2 || !digits(m) {
        return None;
    }
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then(|| format!("{h:02}:{m:02}"))
}

/// Checks what the page sent and tidies it: a trimmed title, a normal time, and a weekday
/// series that starts on a weekday.
pub fn check(input: Input) -> Result<Input, String> {
    let title = input.title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return Err("Give it a title".to_string());
    }
    if title.chars().count() > MAX_TITLE {
        return Err(format!("Keep the title under {MAX_TITLE} characters"));
    }
    let day = Day::parse(&input.date).ok_or("Pick a date")?;
    let time = match input.time.as_deref() {
        None => None,
        Some(t) => Some(normal_time(t).ok_or("Pick a time")?),
    };
    if input.kind == Kind::Reminder && time.is_none() {
        return Err("Pick a time to be reminded".to_string());
    }
    if input.remind_before.is_some_and(|m| m > MAX_REMIND_BEFORE) {
        return Err("A reminder can be at most a week before".to_string());
    }
    let remind_before = if input.kind == Kind::Event { input.remind_before } else { None };
    let date = date::first_day(day, input.repeat).to_string();
    Ok(Input { title, date, time, remind_before, ..input })
}

/// When day `day` of something at `time` (all day when `None`) reminds, `before` minutes
/// ahead, in Unix milliseconds.
fn alert_for(day: Day, time: Option<&str>, before: u32) -> Option<i64> {
    let minute = match time {
        Some(t) => date::minutes(t)?,
        None => ALL_DAY_REMINDER,
    };
    Some(date::local_ms(day, minute)? - i64::from(before) * 60_000)
}

/// The days of a series in order, with when each reminds, from the first that reminds at
/// `after` or later. A series that does not repeat has its one day, however long ago.
fn reminders_from(
    start: Day,
    repeat: Option<Repeat>,
    time: Option<&str>,
    before: u32,
    after: i64,
) -> impl Iterator<Item = (Day, i64)> + '_ {
    // A reminder can be up to a week before its day, so begin looking a little earlier.
    let from = repeat
        .and_then(|_| date::local_day(after))
        .map(|(today, _)| today.add_days(-i64::from(before / (24 * 60)) - 1))
        .unwrap_or(start);
    let mut next = date::next_on_or_after(start, repeat, from);
    std::iter::from_fn(move || {
        let day = next?;
        next = repeat.and_then(|_| date::next_on_or_after(start, repeat, day.add_days(1)));
        Some((day, alert_for(day, time, before)?))
    })
    .take(SEARCH_LIMIT)
    .skip_while(move |(_, at)| repeat.is_some() && *at < after)
}

/// The latest day of a series that has reminded by `now`, after the one at `since`.
fn latest_due(
    start: Day,
    repeat: Option<Repeat>,
    time: Option<&str>,
    since: i64,
    now: i64,
) -> Option<(Day, i64)> {
    reminders_from(start, repeat, time, 0, since + 1).take_while(|(_, at)| *at <= now).last()
}

/// The row as saved.
struct Saved {
    id: i64,
    kind: Kind,
    title: String,
    date: String,
    time: Option<String>,
    remind_before: Option<u32>,
    repeat: Option<Repeat>,
    /// Repeating reminders: the day it is about now.
    current: Option<String>,
    alert_at: Option<i64>,
    done: bool,
}

impl Saved {
    fn start(&self) -> Option<Day> {
        Day::parse(&self.date)
    }

    fn current_day(&self) -> Option<Day> {
        self.current.as_deref().and_then(Day::parse).or_else(|| self.start())
    }

    /// As shown on `day`.
    fn on(&self, day: String) -> Item {
        Item {
            id: self.id,
            kind: self.kind,
            title: self.title.clone(),
            date: day,
            start_date: self.date.clone(),
            time: self.time.clone(),
            remind_before: self.remind_before,
            repeat: self.repeat,
            alert_at: self.alert_at,
            done: self.done,
        }
    }

    fn item(&self) -> Item {
        self.on(self.current.clone().unwrap_or_else(|| self.date.clone()))
    }
}

const COLUMNS: &str = "id, kind, title, date, time, remind_before, repeat, current, alert_at, done";

fn saved_from(row: &Row<'_>) -> rusqlite::Result<Saved> {
    let kind: String = row.get(1)?;
    let repeat: Option<String> = row.get(6)?;
    Ok(Saved {
        id: row.get(0)?,
        kind: Kind::parse(&kind).unwrap_or(Kind::Event),
        title: row.get(2)?,
        date: row.get(3)?,
        time: row.get(4)?,
        remind_before: row.get(5)?,
        repeat: repeat.as_deref().and_then(Repeat::parse),
        current: row.get(7)?,
        alert_at: row.get(8)?,
        done: row.get(9)?,
    })
}

fn query(
    conn: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<Saved>, StoreError> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(args, saved_from)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn get_saved(conn: &Connection, id: i64) -> Result<Option<Saved>, StoreError> {
    let sql = format!("SELECT {COLUMNS} FROM calendar_items WHERE id = ?1");
    Ok(conn.query_row(&sql, params![id], saved_from).optional()?)
}

/// Adds `column` to the table when a database from before it existed lacks it.
fn add_column(conn: &Connection, column: &str, kind: &str) -> Result<(), StoreError> {
    let mut stmt = conn.prepare("SELECT name FROM pragma_table_info('calendar_items')")?;
    let names = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    if !names.iter().any(|n| n == column) {
        conn.execute_batch(&format!("ALTER TABLE calendar_items ADD COLUMN {column} {kind}"))?;
    }
    Ok(())
}

/// Creates the table if it is not there yet, and adds what newer versions keep.
pub fn init(store: &AppStore) -> Result<(), StoreError> {
    let conn = store.connection();
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS calendar_items (
            id INTEGER PRIMARY KEY,
            kind TEXT NOT NULL,
            title TEXT NOT NULL,
            date TEXT NOT NULL,
            time TEXT,
            remind_before INTEGER,
            alert_at INTEGER,
            alerted INTEGER NOT NULL DEFAULT 0,
            done INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS calendar_items_date ON calendar_items (date);
        CREATE INDEX IF NOT EXISTS calendar_items_alert ON calendar_items (alert_at)
            WHERE alerted = 0 AND done = 0;",
    )?;
    add_column(&conn, "repeat", "TEXT")?;
    add_column(&conn, "current", "TEXT")?;
    Ok(())
}

pub fn get(store: &AppStore, id: i64) -> Result<Option<Item>, StoreError> {
    Ok(get_saved(&store.connection(), id)?.map(|s| s.item()))
}

/// When a newly saved (or changed) item first reminds, and for a repeating reminder the
/// day that is: its first time from `now` on.
fn first_reminder(input: &Input, now: i64) -> (Option<String>, Option<i64>) {
    let Some(start) = Day::parse(&input.date) else { return (None, None) };
    let before = match input.kind {
        Kind::Reminder => 0,
        Kind::Event => match input.remind_before {
            Some(b) => b,
            None => return (None, None),
        },
    };
    let first = reminders_from(start, input.repeat, input.time.as_deref(), before, now).next();
    let current = match (input.kind, input.repeat, first) {
        (Kind::Reminder, Some(_), Some((day, _))) => Some(day.to_string()),
        _ => None,
    };
    (current, first.map(|(_, at)| at))
}

/// Saves a new event or reminder (already `check`ed) at `now`.
pub fn add(store: &AppStore, input: &Input, now: i64) -> Result<Item, StoreError> {
    let (current, alert_at) = first_reminder(input, now);
    let conn = store.connection();
    conn.execute(
        "INSERT INTO calendar_items
            (kind, title, date, time, remind_before, repeat, current, alert_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            input.kind.as_str(),
            input.title,
            input.date,
            input.time,
            input.remind_before,
            input.repeat.map(Repeat::as_str),
            current,
            alert_at
        ],
    )?;
    let id = conn.last_insert_rowid();
    let saved = get_saved(&conn, id)?;
    saved.map(|s| s.item()).ok_or_else(|| StoreError::Db("the new item was not saved".into()))
}

/// Changes event or reminder `id` (already `check`ed) at `now`. A new reminder time means
/// it will remind again. Returns `None` when there is no such item.
pub fn update(
    store: &AppStore,
    id: i64,
    input: &Input,
    now: i64,
) -> Result<Option<Item>, StoreError> {
    let (current, alert_at) = first_reminder(input, now);
    let conn = store.connection();
    let changed = conn.execute(
        "UPDATE calendar_items SET
            alerted = CASE WHEN alert_at IS ?8 THEN alerted ELSE 0 END,
            kind = ?1, title = ?2, date = ?3, time = ?4, remind_before = ?5, repeat = ?6,
            current = ?7, alert_at = ?8
         WHERE id = ?9",
        params![
            input.kind.as_str(),
            input.title,
            input.date,
            input.time,
            input.remind_before,
            input.repeat.map(Repeat::as_str),
            current,
            alert_at,
            id
        ],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    Ok(get_saved(&conn, id)?.map(|s| s.item()))
}

pub fn delete(store: &AppStore, id: i64) -> Result<(), StoreError> {
    store.connection().execute("DELETE FROM calendar_items WHERE id = ?1", params![id])?;
    Ok(())
}

/// Removes every reminder that has been done.
pub fn clear_done(store: &AppStore) -> Result<(), StoreError> {
    store
        .connection()
        .execute("DELETE FROM calendar_items WHERE kind = 'reminder' AND done = 1", [])?;
    Ok(())
}

/// Marks reminder `id` done or not done at `now`. A repeating one is never done: it moves
/// on to its next time from now. Returns `None` when there is no such reminder.
pub fn set_done(
    store: &AppStore,
    id: i64,
    done: bool,
    now: i64,
) -> Result<Option<Item>, StoreError> {
    let conn = store.connection();
    let Some(saved) = get_saved(&conn, id)?.filter(|s| s.kind == Kind::Reminder) else {
        return Ok(None);
    };
    match (saved.repeat, saved.start()) {
        (Some(repeat), Some(start)) if done => {
            let time = saved.time.as_deref();
            let since = saved.current_day().and_then(|d| alert_for(d, time, 0)).unwrap_or(now);
            let next = reminders_from(start, Some(repeat), time, 0, since.max(now - 1) + 1).next();
            conn.execute(
                "UPDATE calendar_items SET current = ?1, alert_at = ?2, alerted = 0 WHERE id = ?3",
                params![next.map(|(d, _)| d.to_string()), next.map(|(_, at)| at), id],
            )?;
        }
        _ => {
            conn.execute("UPDATE calendar_items SET done = ?1 WHERE id = ?2", params![done, id])?;
        }
    }
    Ok(get_saved(&conn, id)?.map(|s| s.item()))
}

fn sort(items: &mut [Item]) {
    items.sort_by(|a, b| {
        (&a.date, a.time.is_some(), &a.time, a.id).cmp(&(&b.date, b.time.is_some(), &b.time, b.id))
    });
}

/// Events and reminders on the days `from` to `to` ("YYYY-MM-DD", both included): a
/// repeating event on each of its days, a repeating reminder on the day it is about now.
pub fn between(store: &AppStore, from: &str, to: &str) -> Result<Vec<Item>, StoreError> {
    let (Some(first), Some(last)) = (Day::parse(from), Day::parse(to)) else {
        return Ok(Vec::new());
    };
    let sql = format!(
        "SELECT {COLUMNS} FROM calendar_items
         WHERE (repeat IS NULL AND date >= ?1 AND date <= ?2)
            OR (repeat IS NOT NULL AND date <= ?2)"
    );
    let rows = query(&store.connection(), &sql, params![from, to])?;
    let mut items = Vec::new();
    for row in &rows {
        match (row.kind, row.repeat, row.start()) {
            (Kind::Event, Some(repeat), Some(start)) => {
                let days = date::days_between(start, Some(repeat), first, last, SHOWN_LIMIT);
                items.extend(days.into_iter().map(|d| row.on(d.to_string())));
            }
            (Kind::Reminder, Some(_), _) => {
                if row.current_day().is_some_and(|d| d >= first && d <= last) {
                    items.push(row.item());
                }
            }
            _ => items.push(row.item()),
        }
    }
    sort(&mut items);
    Ok(items)
}

/// Every reminder: the ones still to do first, soonest first.
pub fn reminders(store: &AppStore) -> Result<Vec<Item>, StoreError> {
    let sql = format!("SELECT {COLUMNS} FROM calendar_items WHERE kind = 'reminder'");
    let mut items: Vec<Item> =
        query(&store.connection(), &sql, [])?.iter().map(Saved::item).collect();
    sort(&mut items);
    items.sort_by_key(|i| i.done);
    Ok(items)
}

/// Moves each repeating reminder that has already reminded on to its latest time that has
/// come, so it reminds again: tomorrow's medicine is due even if today's was not ticked off.
fn roll_reminders(conn: &Connection, now: i64) -> Result<(), StoreError> {
    let sql = format!(
        "SELECT {COLUMNS} FROM calendar_items
         WHERE kind = 'reminder' AND repeat IS NOT NULL AND alerted = 1 AND done = 0"
    );
    for row in query(conn, &sql, [])? {
        let (Some(start), Some(since)) = (row.start(), row.alert_at) else { continue };
        if let Some((day, at)) = latest_due(start, row.repeat, row.time.as_deref(), since, now) {
            conn.execute(
                "UPDATE calendar_items SET current = ?1, alert_at = ?2, alerted = 0 WHERE id = ?3",
                params![day.to_string(), at, row.id],
            )?;
        }
    }
    Ok(())
}

/// Takes the events and reminders whose time to remind has come (`alert_at <= now`), so
/// each reminds once; a repeating event is set to remind before its next day. Ones more
/// than `stale_ms` late (mouce was not running) are taken quietly and not returned: a
/// reminder for yesterday's meeting is no help.
pub fn take_due(store: &AppStore, now: i64, stale_ms: i64) -> Result<Vec<Item>, StoreError> {
    let mut conn = store.connection();
    let tx = conn.transaction()?;
    roll_reminders(&tx, now)?;
    let sql = format!(
        "SELECT {COLUMNS} FROM calendar_items
         WHERE alerted = 0 AND done = 0 AND alert_at IS NOT NULL AND alert_at <= ?1
         ORDER BY alert_at, id"
    );
    let due = query(&tx, &sql, params![now])?;
    for row in &due {
        let next = match (row.kind, row.repeat, row.start(), row.remind_before) {
            (Kind::Event, Some(repeat), Some(start), Some(before)) => {
                reminders_from(start, Some(repeat), row.time.as_deref(), before, now + 1).next()
            }
            _ => None,
        };
        match next {
            Some((_, at)) => tx.execute(
                "UPDATE calendar_items SET alert_at = ?1, alerted = 0 WHERE id = ?2",
                params![at, row.id],
            )?,
            None => {
                tx.execute("UPDATE calendar_items SET alerted = 1 WHERE id = ?1", params![row.id])?
            }
        };
    }
    tx.commit()?;
    Ok(due
        .iter()
        .filter(|row| row.alert_at.is_some_and(|at| now - at <= stale_ms))
        .map(Saved::item)
        .collect())
}

/// Reminds about `id` again at `at` (Unix milliseconds).
pub fn snooze(store: &AppStore, id: i64, at: i64) -> Result<(), StoreError> {
    store.connection().execute(
        "UPDATE calendar_items SET alert_at = ?1, alerted = 0 WHERE id = ?2",
        params![at, id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    fn store() -> (tempfile::TempDir, AppStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = AppStore::open(dir.path().to_path_buf()).unwrap();
        init(&s).unwrap();
        (dir, s)
    }

    /// `date` at `time` on this computer's clock.
    fn at(date: &str, time: &str) -> i64 {
        date::local_ms(Day::parse(date).unwrap(), date::minutes(time).unwrap()).unwrap()
    }

    fn event(title: &str, date: &str, time: Option<&str>) -> Input {
        Input {
            kind: Kind::Event,
            title: title.into(),
            date: date.into(),
            time: time.map(Into::into),
            remind_before: None,
            repeat: None,
        }
    }

    fn reminder(title: &str, date: &str, time: &str) -> Input {
        Input {
            kind: Kind::Reminder,
            title: title.into(),
            date: date.into(),
            time: Some(time.into()),
            remind_before: None,
            repeat: None,
        }
    }

    fn titles(items: Vec<Item>) -> Vec<String> {
        items.into_iter().map(|i| i.title).collect()
    }

    /// Long ago, so nothing saved in a test is in the past.
    fn early() -> i64 {
        at("2026-01-01", "00:00")
    }

    #[test]
    fn times_are_made_two_digit_and_must_be_real() {
        assert_eq!(normal_time("9:05").as_deref(), Some("09:05"));
        assert_eq!(normal_time(" 15:00 ").as_deref(), Some("15:00"));
        assert_eq!(normal_time("00:00").as_deref(), Some("00:00"));
        for bad in ["24:00", "12:60", "12", "12:5", "ab:cd", ":30", "123:00", "-1:00", "12:00:00"] {
            assert_eq!(normal_time(bad), None, "{bad}");
        }
    }

    #[test]
    fn check_tidies_the_title_time_and_a_weekday_start() {
        let input = Input {
            title: "  Dentist \n appointment ".into(),
            ..event("", "2026-10-31", Some("9:30"))
        };
        let checked = check(input).unwrap();
        assert_eq!(checked.title, "Dentist appointment");
        assert_eq!(checked.time.as_deref(), Some("09:30"));
        let weekdays =
            Input { repeat: Some(Repeat::Weekdays), ..event("Stand-up", "2026-10-31", None) };
        assert_eq!(check(weekdays).unwrap().date, "2026-11-02", "a Saturday start moves to Monday");
    }

    #[test]
    fn check_refuses_what_cannot_be_saved() {
        assert!(check(event("  ", "2026-10-31", None)).is_err());
        assert!(check(event(&"x".repeat(MAX_TITLE + 1), "2026-10-31", None)).is_err());
        assert!(check(event(&"é".repeat(MAX_TITLE), "2026-10-31", None)).is_ok());
        assert!(check(event("a", "2026-02-30", None)).is_err());
        assert!(check(event("a", "2026-10-31", Some("25:00"))).is_err());
        let too_early =
            Input { remind_before: Some(MAX_REMIND_BEFORE + 1), ..event("a", "2026-10-31", None) };
        assert!(check(too_early).is_err());
        let mut r = reminder("a", "2026-10-31", "09:00");
        assert!(check(r.clone()).is_ok());
        r.time = None;
        assert!(check(r).is_err(), "a reminder needs a time");
    }

    #[test]
    fn a_reminder_has_no_minutes_before() {
        let r = Input { remind_before: Some(10), ..reminder("a", "2026-10-31", "09:00") };
        assert_eq!(check(r).unwrap().remind_before, None);
    }

    #[test]
    fn the_reminder_time_is_worked_out_on_this_computers_clock() {
        let (_d, s) = store();
        let timed =
            Input { remind_before: Some(10), ..event("Dentist", "2026-10-31", Some("15:00")) };
        assert_eq!(add(&s, &timed, early()).unwrap().alert_at, Some(at("2026-10-31", "14:50")));
        let all_day = Input { remind_before: Some(24 * 60), ..event("Trip", "2026-10-31", None) };
        assert_eq!(add(&s, &all_day, early()).unwrap().alert_at, Some(at("2026-10-30", "09:00")));
        assert_eq!(
            add(&s, &event("No reminder", "2026-10-31", None), early()).unwrap().alert_at,
            None
        );
        let r = add(&s, &reminder("Call", "2026-10-31", "18:00"), early()).unwrap();
        assert_eq!(r.alert_at, Some(at("2026-10-31", "18:00")));
        // One that does not repeat keeps its time even when it is already past.
        let late =
            add(&s, &reminder("Late", "2026-01-01", "08:00"), at("2026-10-31", "12:00")).unwrap();
        assert_eq!(late.alert_at, Some(at("2026-01-01", "08:00")));
    }

    #[test]
    fn items_round_trip_and_list_by_day_all_day_first() {
        let (_d, s) = store();
        add(&s, &event("Late", "2026-10-30", Some("18:00")), early()).unwrap();
        let all_day = add(&s, &event("Holiday", "2026-10-30", None), early()).unwrap();
        add(&s, &reminder("Call", "2026-10-30", "08:00"), early()).unwrap();
        add(&s, &event("Next month", "2026-11-01", None), early()).unwrap();
        assert_eq!(
            titles(between(&s, "2026-10-01", "2026-10-31").unwrap()),
            ["Holiday", "Call", "Late"]
        );
        assert_eq!(get(&s, all_day.id).unwrap(), Some(all_day.clone()));
        assert_eq!(all_day.start_date, "2026-10-30");
        assert!(between(&s, "nonsense", "2026-10-31").unwrap().is_empty());
    }

    #[test]
    fn a_repeating_event_shows_on_each_of_its_days() {
        let (_d, s) = store();
        let weekly =
            Input { repeat: Some(Repeat::Weekly), ..event("Gym", "2026-10-05", Some("07:30")) };
        let saved = add(&s, &weekly, early()).unwrap();
        let found = between(&s, "2026-10-12", "2026-10-31").unwrap();
        let days: Vec<_> = found.iter().map(|i| i.date.as_str()).collect();
        assert_eq!(days, ["2026-10-12", "2026-10-19", "2026-10-26"]);
        assert!(found.iter().all(|i| i.id == saved.id && i.start_date == "2026-10-05"));
        assert!(
            between(&s, "2026-09-01", "2026-10-04").unwrap().is_empty(),
            "not before it starts"
        );
    }

    #[test]
    fn reminders_list_open_ones_first() {
        let (_d, s) = store();
        let a = add(&s, &reminder("A", "2026-10-30", "09:00"), early()).unwrap();
        add(&s, &reminder("B", "2026-10-31", "09:00"), early()).unwrap();
        add(&s, &event("Not a reminder", "2026-10-29", None), early()).unwrap();
        assert!(set_done(&s, a.id, true, early()).unwrap().unwrap().done);
        assert_eq!(titles(reminders(&s).unwrap()), ["B", "A"]);
        clear_done(&s).unwrap();
        assert_eq!(titles(reminders(&s).unwrap()), ["B"]);
    }

    #[test]
    fn only_reminders_can_be_done() {
        let (_d, s) = store();
        let e = add(&s, &event("E", "2026-10-30", None), early()).unwrap();
        assert_eq!(set_done(&s, e.id, true, early()).unwrap(), None);
        assert_eq!(set_done(&s, 999, true, early()).unwrap(), None);
    }

    #[test]
    fn update_and_delete() {
        let (_d, s) = store();
        let e = add(&s, &event("E", "2026-10-30", None), early()).unwrap();
        let changed =
            update(&s, e.id, &event("F", "2026-10-31", Some("10:00")), early()).unwrap().unwrap();
        assert_eq!((changed.title.as_str(), changed.time.as_deref()), ("F", Some("10:00")));
        assert_eq!(update(&s, 999, &event("G", "2026-10-31", None), early()).unwrap(), None);
        delete(&s, e.id).unwrap();
        assert_eq!(get(&s, e.id).unwrap(), None);
    }

    #[test]
    fn each_item_reminds_once_when_its_time_comes() {
        let (_d, s) = store();
        add(&s, &reminder("Later", "2026-10-30", "10:00"), early()).unwrap();
        let now = add(&s, &reminder("Now", "2026-10-30", "09:00"), early()).unwrap();
        add(&s, &event("No reminder", "2026-10-30", Some("09:00")), early()).unwrap();
        let stale = 60 * MIN;
        assert_eq!(take_due(&s, at("2026-10-30", "09:30"), stale).unwrap(), vec![now]);
        assert!(take_due(&s, at("2026-10-30", "09:30"), stale).unwrap().is_empty(), "taken once");
        assert_eq!(titles(take_due(&s, at("2026-10-30", "10:00"), stale).unwrap()), ["Later"]);
    }

    #[test]
    fn long_missed_reminders_are_taken_quietly() {
        let (_d, s) = store();
        add(&s, &reminder("Old", "2026-10-01", "09:00"), early()).unwrap();
        add(&s, &reminder("Recent", "2026-10-30", "09:00"), early()).unwrap();
        let now = at("2026-10-30", "09:30");
        assert_eq!(titles(take_due(&s, now, 60 * MIN).unwrap()), ["Recent"]);
        assert!(take_due(&s, now, i64::MAX / 2).unwrap().is_empty(), "the old one is not kept");
    }

    #[test]
    fn done_reminders_do_not_remind() {
        let (_d, s) = store();
        let r = add(&s, &reminder("R", "2026-10-30", "09:00"), early()).unwrap();
        set_done(&s, r.id, true, early()).unwrap();
        assert!(take_due(&s, at("2026-10-30", "10:00"), 60 * MIN).unwrap().is_empty());
    }

    #[test]
    fn snoozing_and_a_new_time_remind_again() {
        let (_d, s) = store();
        let r = add(&s, &reminder("R", "2026-10-30", "09:00"), early()).unwrap();
        let nine = at("2026-10-30", "09:00");
        assert_eq!(take_due(&s, nine, MIN).unwrap().len(), 1);
        snooze(&s, r.id, nine + 10 * MIN).unwrap();
        assert!(take_due(&s, nine + 5 * MIN, MIN).unwrap().is_empty());
        assert_eq!(take_due(&s, nine + 10 * MIN, MIN).unwrap().len(), 1);

        // The same time again: already reminded, so it stays quiet.
        let snoozed = Input { title: "R2".into(), ..reminder("", "2026-10-30", "09:10") };
        update(&s, r.id, &snoozed, early()).unwrap();
        assert!(take_due(&s, nine + 20 * MIN, MIN * 60).unwrap().is_empty());
        // A new time reminds again.
        update(&s, r.id, &reminder("R2", "2026-10-30", "09:30"), early()).unwrap();
        assert_eq!(take_due(&s, nine + 30 * MIN, MIN).unwrap().len(), 1);
    }

    #[test]
    fn a_repeating_event_reminds_before_each_of_its_days() {
        let (_d, s) = store();
        let daily = Input {
            remind_before: Some(10),
            repeat: Some(Repeat::Daily),
            ..event("Stand-up", "2026-10-29", Some("09:00"))
        };
        // Saved on the 30th at noon: its first reminder is tomorrow's.
        let saved = add(&s, &daily, at("2026-10-30", "12:00")).unwrap();
        assert_eq!(saved.alert_at, Some(at("2026-10-31", "08:50")));
        let first = take_due(&s, at("2026-10-31", "08:50"), MIN).unwrap();
        assert_eq!(titles(first), ["Stand-up"]);
        assert_eq!(get(&s, saved.id).unwrap().unwrap().alert_at, Some(at("2026-11-01", "08:50")));
        assert!(take_due(&s, at("2026-10-31", "12:00"), MIN).unwrap().is_empty());
        assert_eq!(take_due(&s, at("2026-11-01", "08:50"), MIN).unwrap().len(), 1);
    }

    #[test]
    fn a_repeating_reminder_moves_on_when_done_and_when_its_next_time_comes() {
        let (_d, s) = store();
        let daily =
            Input { repeat: Some(Repeat::Daily), ..reminder("Medicine", "2026-10-30", "09:00") };
        let saved = add(&s, &daily, at("2026-10-30", "08:00")).unwrap();
        assert_eq!((saved.date.as_str(), saved.start_date.as_str()), ("2026-10-30", "2026-10-30"));

        // Ticked off before it is due: it moves to tomorrow and stays to do.
        let ticked = set_done(&s, saved.id, true, at("2026-10-30", "08:30")).unwrap().unwrap();
        assert_eq!((ticked.date.as_str(), ticked.done), ("2026-10-31", false));
        assert_eq!(ticked.alert_at, Some(at("2026-10-31", "09:00")));
        assert_eq!(titles(reminders(&s).unwrap()), ["Medicine"]);
        assert!(take_due(&s, at("2026-10-30", "10:00"), MIN).unwrap().is_empty(), "not today");

        // Reminded on the 31st but not ticked: on 2 November it is that day's again.
        assert_eq!(take_due(&s, at("2026-10-31", "09:00"), MIN).unwrap().len(), 1);
        let due = take_due(&s, at("2026-11-02", "09:00"), MIN).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].date, "2026-11-02");
        let shown = between(&s, "2026-10-01", "2026-11-30").unwrap();
        assert_eq!(shown.iter().map(|i| i.date.as_str()).collect::<Vec<_>>(), ["2026-11-02"]);
    }

    #[test]
    fn a_repeating_reminder_starts_from_its_next_time() {
        let (_d, s) = store();
        let weekly =
            Input { repeat: Some(Repeat::Weekly), ..reminder("Bins", "2026-10-26", "19:00") };
        let saved = add(&s, &weekly, at("2026-10-30", "12:00")).unwrap();
        assert_eq!(saved.date, "2026-11-02");
        assert_eq!(saved.start_date, "2026-10-26");
    }

    #[test]
    fn a_table_from_before_repeats_gets_the_new_columns() {
        let dir = tempfile::tempdir().unwrap();
        let s = AppStore::open(dir.path().to_path_buf()).unwrap();
        s.connection()
            .execute_batch(
                "CREATE TABLE calendar_items (id INTEGER PRIMARY KEY, kind TEXT NOT NULL,
                 title TEXT NOT NULL, date TEXT NOT NULL, time TEXT, remind_before INTEGER,
                 alert_at INTEGER, alerted INTEGER NOT NULL DEFAULT 0,
                 done INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO calendar_items (kind, title, date) VALUES ('event', 'Old', '2026-10-30');",
            )
            .unwrap();
        init(&s).unwrap();
        init(&s).unwrap();
        let found = between(&s, "2026-10-30", "2026-10-30").unwrap();
        assert_eq!((found[0].title.as_str(), found[0].repeat), ("Old", None));
    }
}
