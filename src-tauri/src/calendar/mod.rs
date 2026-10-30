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
//! on; it reminds before each one. A day can be left out of it (`calendar_skips`), which is
//! also how one day is changed on its own: that day is left out and saved as an event of
//! its own. A repeating reminder is about one day at a time (`current`): ticking it off
//! moves it on, and so does its next time coming round.

pub mod alerts;
pub mod date;
pub mod understand;

use crate::store::{AppStore, StoreError};
use date::{Day, Repeat};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// Timed events only: "HH:MM" on the same day, after `time`.
    pub end_time: Option<String>,
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
    #[serde(default)]
    pub end_time: Option<String>,
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
    let end_time = match (input.kind, &time, input.end_time.as_deref()) {
        (Kind::Event, Some(start), Some(end)) => {
            let end = normal_time(end).ok_or("Pick a time for the end")?;
            if end <= *start {
                return Err("The end must be after the start".to_string());
            }
            Some(end)
        }
        _ => None,
    };
    if input.remind_before.is_some_and(|m| m > MAX_REMIND_BEFORE) {
        return Err("A reminder can be at most a week before".to_string());
    }
    let remind_before = if input.kind == Kind::Event { input.remind_before } else { None };
    let date = date::first_day(day, input.repeat).to_string();
    Ok(Input { title, date, time, end_time, remind_before, ..input })
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
/// `after` or later, leaving out `skips`. A series that does not repeat has its one day,
/// however long ago.
fn reminders_from<'a>(
    start: Day,
    repeat: Option<Repeat>,
    time: Option<&'a str>,
    before: u32,
    after: i64,
    skips: &'a [Day],
) -> impl Iterator<Item = (Day, i64)> + 'a {
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
    .filter(move |(day, _)| !skips.contains(day))
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
    reminders_from(start, repeat, time, 0, since + 1, &[]).take_while(|(_, at)| *at <= now).last()
}

/// The row as saved.
struct Saved {
    id: i64,
    kind: Kind,
    title: String,
    date: String,
    time: Option<String>,
    end_time: Option<String>,
    remind_before: Option<u32>,
    repeat: Option<Repeat>,
    /// Repeating reminders: the day it is about now.
    current: Option<String>,
    alert_at: Option<i64>,
    done: bool,
    /// Repeating events: the days left out (filled in by `with_skips`).
    skips: Vec<Day>,
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
            end_time: self.end_time.clone(),
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

const COLUMNS: &str =
    "id, kind, title, date, time, remind_before, repeat, current, alert_at, done, end_time";

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
        end_time: row.get(10)?,
        skips: Vec::new(),
    })
}

/// Fills in the days left out of the repeating events among `rows`.
fn with_skips(conn: &Connection, mut rows: Vec<Saved>) -> Result<Vec<Saved>, StoreError> {
    if !rows.iter().any(|r| r.repeat.is_some()) {
        return Ok(rows);
    }
    let mut skips: HashMap<i64, Vec<Day>> = HashMap::new();
    let mut stmt = conn.prepare("SELECT item_id, date FROM calendar_skips")?;
    let found = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    for pair in found {
        let (id, date) = pair?;
        if let Some(day) = Day::parse(&date) {
            skips.entry(id).or_default().push(day);
        }
    }
    for row in &mut rows {
        row.skips = skips.remove(&row.id).unwrap_or_default();
    }
    Ok(rows)
}

fn query(
    conn: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
) -> Result<Vec<Saved>, StoreError> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(args, saved_from)?.collect::<Result<Vec<_>, _>>()?;
    with_skips(conn, rows)
}

fn get_saved(conn: &Connection, id: i64) -> Result<Option<Saved>, StoreError> {
    let sql = format!("SELECT {COLUMNS} FROM calendar_items WHERE id = ?1");
    let row = conn.query_row(&sql, params![id], saved_from).optional()?;
    Ok(with_skips(conn, row.into_iter().collect())?.pop())
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
            WHERE alerted = 0 AND done = 0;
        CREATE TABLE IF NOT EXISTS calendar_skips (
            item_id INTEGER NOT NULL,
            date TEXT NOT NULL,
            PRIMARY KEY (item_id, date)
        );",
    )?;
    add_column(&conn, "repeat", "TEXT")?;
    add_column(&conn, "current", "TEXT")?;
    add_column(&conn, "end_time", "TEXT")?;
    Ok(())
}

pub fn get(store: &AppStore, id: i64) -> Result<Option<Item>, StoreError> {
    Ok(get_saved(&store.connection(), id)?.map(|s| s.item()))
}

/// When a newly saved (or changed) item first reminds, and for a repeating reminder the
/// day that is: its first time from `now` on, leaving out `skips`.
fn first_reminder(input: &Input, now: i64, skips: &[Day]) -> (Option<String>, Option<i64>) {
    let Some(start) = Day::parse(&input.date) else { return (None, None) };
    let before = match input.kind {
        Kind::Reminder => 0,
        Kind::Event => match input.remind_before {
            Some(b) => b,
            None => return (None, None),
        },
    };
    let first =
        reminders_from(start, input.repeat, input.time.as_deref(), before, now, skips).next();
    let current = match (input.kind, input.repeat, first) {
        (Kind::Reminder, Some(_), Some((day, _))) => Some(day.to_string()),
        _ => None,
    };
    (current, first.map(|(_, at)| at))
}

/// Saves a new event or reminder (already `check`ed) at `now`.
pub fn add(store: &AppStore, input: &Input, now: i64) -> Result<Item, StoreError> {
    add_on(&store.connection(), input, now)
}

fn add_on(conn: &Connection, input: &Input, now: i64) -> Result<Item, StoreError> {
    let (current, alert_at) = first_reminder(input, now, &[]);
    conn.execute(
        "INSERT INTO calendar_items
            (kind, title, date, time, end_time, remind_before, repeat, current, alert_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            input.kind.as_str(),
            input.title,
            input.date,
            input.time,
            input.end_time,
            input.remind_before,
            input.repeat.map(Repeat::as_str),
            current,
            alert_at
        ],
    )?;
    let id = conn.last_insert_rowid();
    let saved = get_saved(conn, id)?;
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
    let conn = store.connection();
    // Days left out of a series stay left out when the series changes.
    let skips = get_saved(&conn, id)?.map(|s| s.skips).unwrap_or_default();
    let (current, alert_at) = first_reminder(input, now, &skips);
    let changed = conn.execute(
        "UPDATE calendar_items SET
            alerted = CASE WHEN alert_at IS ?8 THEN alerted ELSE 0 END,
            kind = ?1, title = ?2, date = ?3, time = ?4, remind_before = ?5, repeat = ?6,
            current = ?7, alert_at = ?8, end_time = ?9
         WHERE id = ?10",
        params![
            input.kind.as_str(),
            input.title,
            input.date,
            input.time,
            input.remind_before,
            input.repeat.map(Repeat::as_str),
            current,
            alert_at,
            input.end_time,
            id
        ],
    )?;
    if changed == 0 {
        return Ok(None);
    }
    Ok(get_saved(&conn, id)?.map(|s| s.item()))
}

pub fn delete(store: &AppStore, id: i64) -> Result<(), StoreError> {
    let conn = store.connection();
    conn.execute("DELETE FROM calendar_items WHERE id = ?1", params![id])?;
    conn.execute("DELETE FROM calendar_skips WHERE item_id = ?1", params![id])?;
    Ok(())
}

/// Leaves day `date` out of repeating event `id` ("just this day") at `now`; if that was
/// the day it was going to remind about next, it reminds about the one after instead.
/// Returns `None` when there is no such repeating event.
pub fn skip_day(
    store: &AppStore,
    id: i64,
    date: &str,
    now: i64,
) -> Result<Option<Item>, StoreError> {
    skip_on(&store.connection(), id, date, now)
}

fn skip_on(conn: &Connection, id: i64, date: &str, now: i64) -> Result<Option<Item>, StoreError> {
    let Some(day) = Day::parse(date) else { return Ok(None) };
    let found = get_saved(conn, id)?.filter(|s| s.kind == Kind::Event && s.repeat.is_some());
    let Some(mut saved) = found else { return Ok(None) };
    conn.execute(
        "INSERT OR IGNORE INTO calendar_skips (item_id, date) VALUES (?1, ?2)",
        params![id, day.to_string()],
    )?;
    saved.skips.push(day);
    if let (Some(start), Some(before)) = (saved.start(), saved.remind_before) {
        let time = saved.time.as_deref();
        let next = reminders_from(start, saved.repeat, time, before, now, &saved.skips).next();
        conn.execute(
            "UPDATE calendar_items SET
                alerted = CASE WHEN alert_at IS ?1 THEN alerted ELSE 0 END, alert_at = ?1
             WHERE id = ?2",
            params![next.map(|(_, at)| at), id],
        )?;
    }
    Ok(get_saved(conn, id)?.map(|s| s.item()))
}

/// Changes day `date` of repeating event `id` on its own: the day is left out of the
/// series, and `input` (already `check`ed) is saved as an event of its own. Returns the new
/// event, or `None` when there is no such repeating event.
pub fn change_day(
    store: &AppStore,
    id: i64,
    date: &str,
    input: &Input,
    now: i64,
) -> Result<Option<Item>, StoreError> {
    let mut conn = store.connection();
    let tx = conn.transaction()?;
    if skip_on(&tx, id, date, now)?.is_none() {
        return Ok(None);
    }
    let alone = Input { repeat: None, ..input.clone() };
    let added = add_on(&tx, &alone, now)?;
    tx.commit()?;
    Ok(Some(added))
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
            let after = since.max(now - 1) + 1;
            let next = reminders_from(start, Some(repeat), time, 0, after, &[]).next();
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
                let shown = days.into_iter().filter(|d| !row.skips.contains(d));
                items.extend(shown.map(|d| row.on(d.to_string())));
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
                let time = row.time.as_deref();
                reminders_from(start, Some(repeat), time, before, now + 1, &row.skips).next()
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

const WEEKDAYS: [&str; 7] =
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// "Friday 30 October 2026"
fn long_day(day: Day) -> String {
    let weekday = WEEKDAYS[day.weekday() as usize];
    format!("{weekday} {} {} {}", day.day, MONTHS[day.month as usize - 1], day.year)
}

/// "15:00-16:00", "15:00" or "all day".
fn times(item: &Item) -> String {
    match (&item.time, &item.end_time) {
        (Some(start), Some(end)) => format!("{start}-{end}"),
        (Some(start), None) => start.clone(),
        _ => "all day".to_string(),
    }
}

/// One line for an item on a day: "12:30-13:30 Team lunch", "Reminder at 18:00: Call mum".
fn plan_line(item: &Item) -> String {
    let repeat = item.repeat.map(|r| format!(" (repeats {})", r.as_str())).unwrap_or_default();
    match item.kind {
        Kind::Event => format!("- {} {}{repeat}", times(item), item.title),
        Kind::Reminder => format!("- Reminder at {}: {}{repeat}", times(item), item.title),
    }
}

/// The user's plans for the AI to answer questions about: today's date and time, then
/// each day from today for `days` days that has something on it, with its events and the
/// reminders due that day, then reminders that are overdue or due later. Days are named
/// plainly ("Tomorrow, Saturday 31 October 2026") so a small model need not work them out.
/// Titles are the user's own words.
pub fn plans_text(store: &AppStore, now: i64, days: i64) -> Result<String, StoreError> {
    let (today, minute) =
        date::local_day(now).ok_or_else(|| StoreError::Db("could not read the clock".into()))?;
    let last = today.add_days(days - 1);
    let mut lines = vec![format!(
        "Today is {} and the time is {:02}:{:02}.",
        long_day(today),
        minute / 60,
        minute % 60
    )];
    let open: Vec<Item> = reminders(store)?.into_iter().filter(|r| !r.done).collect();
    let in_range = |i: &Item| Day::parse(&i.date).is_some_and(|d| d >= today && d <= last);
    let mut on_days: Vec<Item> = between(store, &today.to_string(), &last.to_string())?
        .into_iter()
        .filter(|i| i.kind == Kind::Event)
        .chain(open.iter().filter(|r| in_range(r)).cloned())
        .collect();
    sort(&mut on_days);
    let mut shown_day = None;
    for item in &on_days {
        let Some(day) = Day::parse(&item.date) else { continue };
        if shown_day != Some(day) {
            let name = match day.number() - today.number() {
                0 => format!("Today, {}", long_day(day)),
                1 => format!("Tomorrow, {}", long_day(day)),
                _ => long_day(day),
            };
            lines.push(format!("{name}:"));
            shown_day = Some(day);
        }
        lines.push(plan_line(item));
    }
    lines.push(format!("Nothing else is planned from today to {}.", long_day(last)));
    let overdue: Vec<&Item> =
        open.iter().filter(|r| Day::parse(&r.date).is_some_and(|d| d < today)).collect();
    if !overdue.is_empty() {
        lines.push("Overdue reminders:".to_string());
        for r in overdue {
            let day = Day::parse(&r.date).map(long_day).unwrap_or_default();
            lines.push(format!("- {} (was due {day} at {})", r.title, times(r)));
        }
    }
    let later: Vec<&Item> =
        open.iter().filter(|r| Day::parse(&r.date).is_some_and(|d| d > last)).collect();
    if !later.is_empty() {
        lines.push("Later reminders:".to_string());
        for r in later {
            let day = Day::parse(&r.date).map(long_day).unwrap_or_default();
            lines.push(format!("- {} (due {day} at {})", r.title, times(r)));
        }
    }
    Ok(lines.join("\n"))
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
            end_time: None,
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
            end_time: None,
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

    #[test]
    fn an_end_time_is_kept_for_timed_events_only() {
        let (_d, s) = store();
        let lunch =
            Input { end_time: Some("13:30".into()), ..event("Lunch", "2026-10-30", Some("12:30")) };
        let saved = add(&s, &check(lunch.clone()).unwrap(), early()).unwrap();
        assert_eq!(saved.end_time.as_deref(), Some("13:30"));
        let all_day = Input { time: None, ..lunch.clone() };
        assert_eq!(check(all_day).unwrap().end_time, None);
        let backwards = Input { end_time: Some("12:00".into()), ..lunch.clone() };
        assert!(check(backwards).is_err());
        assert!(check(Input { end_time: Some("12:30".into()), ..lunch.clone() }).is_err());
        assert!(check(Input { end_time: Some("late".into()), ..lunch }).is_err());
        let r = Input { end_time: Some("10:00".into()), ..reminder("R", "2026-10-30", "09:00") };
        assert_eq!(check(r).unwrap().end_time, None);
    }

    #[test]
    fn a_day_can_be_left_out_of_a_repeating_event() {
        let (_d, s) = store();
        let daily = Input {
            remind_before: Some(10),
            repeat: Some(Repeat::Daily),
            ..event("Stand-up", "2026-10-29", Some("09:00"))
        };
        let saved = add(&s, &daily, at("2026-10-30", "12:00")).unwrap();
        assert_eq!(saved.alert_at, Some(at("2026-10-31", "08:50")));
        // Leaving out tomorrow moves its reminder on to the day after.
        let skipped =
            skip_day(&s, saved.id, "2026-10-31", at("2026-10-30", "12:00")).unwrap().unwrap();
        assert_eq!(skipped.alert_at, Some(at("2026-11-01", "08:50")));
        let days: Vec<_> =
            between(&s, "2026-10-30", "2026-11-01").unwrap().into_iter().map(|i| i.date).collect();
        assert_eq!(days, ["2026-10-30", "2026-11-01"]);
        // Changing the series keeps the day left out.
        let later = Input { time: Some("09:30".into()), ..daily };
        update(&s, saved.id, &later, at("2026-10-30", "12:00")).unwrap();
        assert!(between(&s, "2026-10-31", "2026-10-31").unwrap().is_empty());
        // Only repeating events have days to leave out.
        let once = add(&s, &event("Once", "2026-10-30", None), early()).unwrap();
        assert_eq!(skip_day(&s, once.id, "2026-10-30", early()).unwrap(), None);
        // Deleting the series forgets its left-out days.
        delete(&s, saved.id).unwrap();
        let left: i64 = s
            .connection()
            .query_row("SELECT COUNT(*) FROM calendar_skips", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn one_day_of_a_series_can_change_on_its_own() {
        let (_d, s) = store();
        let weekly =
            Input { repeat: Some(Repeat::Weekly), ..event("Gym", "2026-10-27", Some("07:00")) };
        let series = add(&s, &weekly, early()).unwrap();
        let moved =
            Input { time: Some("18:00".into()), ..event("Gym", "2026-11-04", Some("18:00")) };
        let alone = change_day(&s, series.id, "2026-11-03", &moved, early()).unwrap().unwrap();
        assert_ne!(alone.id, series.id);
        assert_eq!(alone.repeat, None);
        let shown: Vec<_> = between(&s, "2026-11-01", "2026-11-11")
            .unwrap()
            .into_iter()
            .map(|i| (i.date, i.time.unwrap()))
            .collect();
        assert_eq!(
            shown,
            [
                ("2026-11-04".to_string(), "18:00".to_string()),
                ("2026-11-10".into(), "07:00".into())
            ]
        );
        assert_eq!(change_day(&s, 999, "2026-11-03", &moved, early()).unwrap(), None);
    }

    #[test]
    fn the_plans_text_lists_each_day_with_its_events_and_reminders() {
        let (_d, s) = store();
        let lunch = Input {
            end_time: Some("13:30".into()),
            ..event("Team lunch", "2026-10-30", Some("12:30"))
        };
        add(&s, &lunch, early()).unwrap();
        let gym =
            Input { repeat: Some(Repeat::Weekly), ..event("Gym", "2026-10-27", Some("07:00")) };
        add(&s, &gym, early()).unwrap();
        add(&s, &event("Far away", "2026-12-25", None), early()).unwrap();
        add(&s, &reminder("Call mum", "2026-10-31", "18:00"), early()).unwrap();
        add(&s, &reminder("Pay rent", "2026-10-29", "09:00"), early()).unwrap();
        add(&s, &reminder("Renew passport", "2026-12-01", "09:00"), early()).unwrap();
        let done = add(&s, &reminder("Old", "2026-10-30", "09:00"), early()).unwrap();
        set_done(&s, done.id, true, early()).unwrap();
        let text = plans_text(&s, at("2026-10-30", "11:05"), 7).unwrap();
        assert_eq!(
            text,
            "Today is Friday 30 October 2026 and the time is 11:05.\n\
             Today, Friday 30 October 2026:\n\
             - 12:30-13:30 Team lunch\n\
             Tomorrow, Saturday 31 October 2026:\n\
             - Reminder at 18:00: Call mum\n\
             Tuesday 3 November 2026:\n\
             - 07:00 Gym (repeats weekly)\n\
             Nothing else is planned from today to Thursday 5 November 2026.\n\
             Overdue reminders:\n\
             - Pay rent (was due Thursday 29 October 2026 at 09:00)\n\
             Later reminders:\n\
             - Renew passport (due Tuesday 1 December 2026 at 09:00)"
        );
        let (_e, empty) = store();
        let none = plans_text(&empty, at("2026-10-30", "11:05"), 7).unwrap();
        assert!(none.ends_with("\nNothing else is planned from today to Thursday 5 November 2026."));
    }
}
