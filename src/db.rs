use std::path::Path;

use anyhow::{Context, Result};
use chrono::{Datelike, IsoWeek, NaiveDate};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::parser::{DayEntry, WeekEntry, WorkLog};

/// A single row from the `entries` table.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryRow {
    pub id: i64,
    pub date: NaiveDate,
    pub item_text: String,
    pub created_at: String,
    pub sort_order: i32,
}

/// A single row from the `contacts` table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ContactRow {
    pub id: i64,
    pub handle: String,
    pub full_name: String,
    pub email: String,
    pub created_at: String,
}

/// Open (or create) the SQLite database at `path` and ensure the schema exists.
pub fn init_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("opening database at {}", path.display()))?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS entries (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            date        TEXT    NOT NULL,
            item_text   TEXT    NOT NULL,
            created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
            sort_order  INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_entries_date ON entries(date);
        CREATE TABLE IF NOT EXISTS contacts (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            handle     TEXT    NOT NULL UNIQUE,
            full_name  TEXT    NOT NULL,
            email      TEXT    NOT NULL,
            created_at TEXT    NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .context("creating schema")?;

    Ok(conn)
}

/// Insert a new contact.
pub fn insert_contact(conn: &Connection, handle: &str, full_name: &str, email: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO contacts (handle, full_name, email) VALUES (?1, ?2, ?3)",
        params![handle, full_name, email],
    )
    .context("inserting contact")?;
    Ok(())
}

/// Return all contacts ordered by handle ascending.
pub fn get_all_contacts(conn: &Connection) -> Result<Vec<ContactRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, handle, full_name, email, created_at FROM contacts ORDER BY handle ASC",
    )?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                handle: row.get(1)?,
                full_name: row.get(2)?,
                email: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .map(|r| r.context("reading contact row"))
        .collect::<Result<Vec<_>>>()?;

    Ok(rows)
}

/// Fetch a single contact by its primary key.
///
/// Returns `None` if no row with that id exists.
#[allow(dead_code)]
pub fn get_contact_by_id(conn: &Connection, id: i64) -> Result<Option<ContactRow>> {
    let mut stmt = conn
        .prepare("SELECT id, handle, full_name, email, created_at FROM contacts WHERE id = ?1")?;

    let mut rows = stmt
        .query_map(params![id], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                handle: row.get(1)?,
                full_name: row.get(2)?,
                email: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .map(|r| r.context("reading contact row"));

    rows.next().transpose()
}

/// Fetch a single contact by handle (case-insensitive).
///
/// Returns `None` if no matching contact exists.
// Exported for future callers; currently exercised only by tests.
#[allow(dead_code)]
pub fn get_contact_by_handle(conn: &Connection, handle: &str) -> Result<Option<ContactRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, handle, full_name, email, created_at FROM contacts WHERE lower(handle) = lower(?1)",
    )?;

    let mut rows = stmt
        .query_map(params![handle], |row| {
            Ok(ContactRow {
                id: row.get(0)?,
                handle: row.get(1)?,
                full_name: row.get(2)?,
                email: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .map(|r| r.context("reading contact row"));

    rows.next().transpose()
}

/// Update an existing contact's handle, full name, and email.
pub fn update_contact(
    conn: &Connection,
    id: i64,
    handle: &str,
    full_name: &str,
    email: &str,
) -> Result<()> {
    let rows_changed = conn
        .execute(
            "UPDATE contacts SET handle = ?1, full_name = ?2, email = ?3 WHERE id = ?4",
            params![handle, full_name, email, id],
        )
        .context("updating contact")?;

    anyhow::ensure!(rows_changed == 1, "contact with id {id} not found");
    Ok(())
}

/// Delete a contact by its primary key.
pub fn delete_contact(conn: &Connection, id: i64) -> Result<()> {
    let rows_changed = conn
        .execute("DELETE FROM contacts WHERE id = ?1", params![id])
        .context("deleting contact")?;

    anyhow::ensure!(rows_changed == 1, "contact with id {id} not found");
    Ok(())
}

/// Insert one bullet item for `date` at the given `sort_order` position.
pub fn insert_entry(conn: &Connection, date: NaiveDate, item: &str, sort_order: i32) -> Result<()> {
    conn.execute(
        "INSERT INTO entries (date, item_text, sort_order) VALUES (?1, ?2, ?3)",
        params![date.to_string(), item, sort_order],
    )
    .context("inserting entry")?;
    Ok(())
}

/// Return all entries ordered newest date first, then by sort_order ascending
/// within each day.
pub fn get_all_entries(conn: &Connection) -> Result<Vec<EntryRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, date, item_text, created_at, sort_order
         FROM entries
         ORDER BY date DESC, sort_order ASC",
    )?;

    let rows = stmt
        .query_map([], |row| {
            let date_str: String = row.get(1)?;
            Ok((
                row.get::<_, i64>(0)?,
                date_str,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
            ))
        })?
        .map(|r| {
            let (id, date_str, item_text, created_at, sort_order) =
                r.context("reading entry row")?;
            let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                .with_context(|| format!("parsing date '{date_str}'"))?;
            Ok(EntryRow {
                id,
                date,
                item_text,
                created_at,
                sort_order,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(rows)
}

/// Return entries for a specific date, ordered by sort_order ascending.
// Exported for use by future callers (e.g. a per-day API route); suppress the
// dead_code lint since it is exercised by tests but not yet by any route.
#[allow(dead_code)]
pub fn get_entries_by_date(conn: &Connection, date: NaiveDate) -> Result<Vec<EntryRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, date, item_text, created_at, sort_order
         FROM entries
         WHERE date = ?1
         ORDER BY sort_order ASC",
    )?;

    let rows = stmt
        .query_map(params![date.to_string()], |row| {
            let date_str: String = row.get(1)?;
            Ok((
                row.get::<_, i64>(0)?,
                date_str,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
            ))
        })?
        .map(|r| {
            let (id, date_str, item_text, created_at, sort_order) =
                r.context("reading entry row")?;
            let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                .with_context(|| format!("parsing date '{date_str}'"))?;
            Ok(EntryRow {
                id,
                date,
                item_text,
                created_at,
                sort_order,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(rows)
}

/// Fetch a single entry by its primary key.
///
/// Returns `None` if no row with that id exists.
pub fn get_entry_by_id(conn: &Connection, id: i64) -> Result<Option<EntryRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, date, item_text, created_at, sort_order
         FROM entries
         WHERE id = ?1",
    )?;

    let mut rows = stmt
        .query_map(params![id], |row| {
            let date_str: String = row.get(1)?;
            Ok((
                row.get::<_, i64>(0)?,
                date_str,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i32>(4)?,
            ))
        })?
        .map(|r| {
            let (row_id, date_str, item_text, created_at, sort_order) =
                r.context("reading entry row")?;
            let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d")
                .with_context(|| format!("parsing date '{date_str}'"))?;
            Ok(EntryRow {
                id: row_id,
                date,
                item_text,
                created_at,
                sort_order,
            })
        });

    rows.next().transpose()
}

/// Update the date and text of an existing entry.
pub fn update_entry(conn: &Connection, id: i64, date: NaiveDate, item_text: &str) -> Result<()> {
    let rows_changed = conn
        .execute(
            "UPDATE entries SET date = ?1, item_text = ?2 WHERE id = ?3",
            params![date.to_string(), item_text, id],
        )
        .context("updating entry")?;

    // Signal a logic error when the caller passes an id that does not exist,
    // rather than silently doing nothing.
    anyhow::ensure!(rows_changed == 1, "entry with id {id} not found");
    Ok(())
}

/// Delete an entry by its primary key.
pub fn delete_entry(conn: &Connection, id: i64) -> Result<()> {
    let rows_changed = conn
        .execute("DELETE FROM entries WHERE id = ?1", params![id])
        .context("deleting entry")?;

    anyhow::ensure!(rows_changed == 1, "entry with id {id} not found");
    Ok(())
}

/// Update the sort_order of an existing entry.
pub fn update_sort_order(conn: &Connection, id: i64, sort_order: i32) -> Result<()> {
    let rows_changed = conn
        .execute(
            "UPDATE entries SET sort_order = ?1 WHERE id = ?2",
            params![sort_order, id],
        )
        .context("updating sort_order")?;

    anyhow::ensure!(rows_changed == 1, "entry with id {id} not found");
    Ok(())
}

/// Return the highest sort_order for `date`, or -1 if the date has no entries.
///
/// Callers can use `get_max_sort_order(conn, date)? + 1` to compute the
/// sort_order for the next item appended to that day.
pub fn get_max_sort_order(conn: &Connection, date: NaiveDate) -> Result<i32> {
    let result: Option<i32> = conn
        .query_row(
            "SELECT MAX(sort_order) FROM entries WHERE date = ?1",
            params![date.to_string()],
            |row| row.get(0),
        )
        .context("querying max sort_order")?;

    Ok(result.unwrap_or(-1))
}

/// Import entries from a parsed `WorkLog`, skipping duplicates (same date +
/// item_text already present).
///
/// Returns the count of newly inserted entries.
pub fn import_from_worklog(conn: &Connection, worklog: &WorkLog) -> Result<usize> {
    let mut count = 0usize;

    for week in &worklog.weeks {
        for day in &week.days {
            for (idx, item) in day.items.iter().enumerate() {
                // Check for an existing row with the same date and text so
                // re-importing is idempotent.
                let exists: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM entries WHERE date = ?1 AND item_text = ?2",
                        params![day.date.to_string(), item],
                        |row| row.get::<_, i64>(0),
                    )
                    .context("checking for duplicate entry")?
                    > 0;

                if !exists {
                    insert_entry(conn, day.date, item, idx as i32)
                        .context("inserting imported entry")?;
                    count += 1;
                }
            }
        }
    }

    Ok(count)
}

/// Build a `WorkLog` from all entries in the database, grouped by ISO week.
///
/// Weeks are ordered newest first; days within each week are ordered
/// chronologically; items within each day follow sort_order.
pub fn export_to_worklog(conn: &Connection) -> Result<WorkLog> {
    // Fetch everything newest-date-first, sort_order-ascending within a day.
    let all = get_all_entries(conn)?;

    let mut weeks: Vec<WeekEntry> = Vec::new();

    for row in &all {
        let iso: IsoWeek = row.date.iso_week();
        let week_number = iso.week();

        // Find or create the week bucket.
        let iso_year = iso.year();
        let week_idx = match weeks
            .iter()
            .position(|w| w.week_number == week_number && w.iso_year == Some(iso_year))
        {
            Some(i) => i,
            None => {
                weeks.push(WeekEntry {
                    week_number,
                    iso_year: Some(iso_year),
                    days: Vec::new(),
                });
                weeks.len() - 1
            }
        };

        let week = &mut weeks[week_idx];

        // Find or create the day bucket within the week.
        let day_idx = match week.days.iter().position(|d| d.date == row.date) {
            Some(i) => i,
            None => {
                week.days.push(DayEntry {
                    date: row.date,
                    items: Vec::new(),
                });
                week.days.len() - 1
            }
        };

        week.days[day_idx].items.push(row.item_text.clone());
    }

    // Days within each week should be chronological for the writer output.
    for week in &mut weeks {
        week.days.sort_by_key(|d| d.date);
    }

    Ok(WorkLog { weeks })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn open_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                date        TEXT    NOT NULL,
                item_text   TEXT    NOT NULL,
                created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
                sort_order  INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_entries_date ON entries(date);
            CREATE TABLE IF NOT EXISTS contacts (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                handle     TEXT    NOT NULL UNIQUE,
                full_name  TEXT    NOT NULL,
                email      TEXT    NOT NULL,
                created_at TEXT    NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        conn
    }

    // --- Simple / error cases first ---

    #[test]
    fn test_get_entry_by_id_not_found() {
        let conn = open_memory_db();
        let result = get_entry_by_id(&conn, 9999).unwrap();
        assert!(result.is_none(), "missing id should return None");
    }

    #[test]
    fn test_get_entry_by_id_found() {
        let conn = open_memory_db();
        insert_entry(&conn, date(2026, 3, 9), "Standup", 0).unwrap();

        // Retrieve the auto-assigned id so the test does not hard-code it.
        let all = get_all_entries(&conn).unwrap();
        let inserted_id = all[0].id;

        let entry = get_entry_by_id(&conn, inserted_id)
            .unwrap()
            .expect("entry should be found");
        assert_eq!(entry.date, date(2026, 3, 9));
        assert_eq!(entry.item_text, "Standup");
    }

    #[test]
    fn test_update_entry() {
        let conn = open_memory_db();
        insert_entry(&conn, date(2026, 3, 9), "Original text", 0).unwrap();
        let all = get_all_entries(&conn).unwrap();
        let id = all[0].id;

        update_entry(&conn, id, date(2026, 3, 10), "Updated text").unwrap();

        let updated = get_entry_by_id(&conn, id).unwrap().unwrap();
        assert_eq!(updated.date, date(2026, 3, 10), "date should be updated");
        assert_eq!(
            updated.item_text, "Updated text",
            "item_text should be updated"
        );
    }

    #[test]
    fn test_delete_entry() {
        let conn = open_memory_db();
        insert_entry(&conn, date(2026, 3, 9), "To be deleted", 0).unwrap();
        let all = get_all_entries(&conn).unwrap();
        let id = all[0].id;

        delete_entry(&conn, id).unwrap();

        let result = get_entry_by_id(&conn, id).unwrap();
        assert!(result.is_none(), "entry should be gone after deletion");

        let remaining = get_all_entries(&conn).unwrap();
        assert!(remaining.is_empty(), "no entries should remain");
    }

    #[test]
    fn test_init_db_creates_tables() {
        // Use a unique file under the system temp dir so the test does not
        // interfere with the running application's database.
        let db_path = std::env::temp_dir().join("worklog-test-init-db.db");
        // Remove any leftover from a previous run to keep the test isolated.
        let _ = std::fs::remove_file(&db_path);

        let conn = init_db(&db_path).expect("init_db should succeed");

        // Verify the table exists by inserting a row and reading it back.
        conn.execute(
            "INSERT INTO entries (date, item_text, sort_order) VALUES ('2026-03-09', 'test', 0)",
            [],
        )
        .expect("insert should work after init_db");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "entries table should exist and accept rows");

        // Verify the contacts table also exists.
        conn.execute(
            "INSERT INTO contacts (handle, full_name, email) VALUES ('test', 'Test User', 'test@example.com')",
            [],
        )
        .expect("contacts table should exist after init_db");

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_insert_and_retrieve_entry() {
        let conn = open_memory_db();
        insert_entry(&conn, date(2026, 3, 9), "Attended standup", 0)
            .expect("insert should succeed");

        let rows = get_all_entries(&conn).expect("get_all_entries should succeed");
        assert_eq!(rows.len(), 1, "should retrieve one entry");
        assert_eq!(rows[0].date, date(2026, 3, 9));
        assert_eq!(rows[0].item_text, "Attended standup");
        assert_eq!(rows[0].sort_order, 0);
    }

    #[test]
    fn test_entries_ordered_by_date_desc_then_sort_order() {
        let conn = open_memory_db();

        // Insert older date first, then newer date, with two items each.
        insert_entry(&conn, date(2026, 3, 9), "Mon item 1", 0).unwrap();
        insert_entry(&conn, date(2026, 3, 9), "Mon item 2", 1).unwrap();
        insert_entry(&conn, date(2026, 3, 10), "Tue item 1", 0).unwrap();
        insert_entry(&conn, date(2026, 3, 10), "Tue item 2", 1).unwrap();

        let rows = get_all_entries(&conn).unwrap();
        assert_eq!(rows.len(), 4);

        // Newest date should appear first.
        assert_eq!(rows[0].date, date(2026, 3, 10), "newest date first");
        assert_eq!(rows[0].item_text, "Tue item 1", "sort_order 0 before 1");
        assert_eq!(rows[1].item_text, "Tue item 2");
        assert_eq!(rows[2].date, date(2026, 3, 9), "older date after newer");
        assert_eq!(rows[2].item_text, "Mon item 1");
        assert_eq!(rows[3].item_text, "Mon item 2");
    }

    #[test]
    fn test_get_entries_by_date() {
        let conn = open_memory_db();
        insert_entry(&conn, date(2026, 3, 9), "Monday task", 0).unwrap();
        insert_entry(&conn, date(2026, 3, 10), "Tuesday task", 0).unwrap();

        let rows = get_entries_by_date(&conn, date(2026, 3, 9)).unwrap();
        assert_eq!(
            rows.len(),
            1,
            "should return only entries for the requested date"
        );
        assert_eq!(rows[0].item_text, "Monday task");
    }

    #[test]
    fn test_get_max_sort_order() {
        let conn = open_memory_db();

        // Empty day returns -1.
        let max = get_max_sort_order(&conn, date(2026, 3, 9)).unwrap();
        assert_eq!(max, -1, "no entries should return -1");

        insert_entry(&conn, date(2026, 3, 9), "Item A", 0).unwrap();
        insert_entry(&conn, date(2026, 3, 9), "Item B", 1).unwrap();
        insert_entry(&conn, date(2026, 3, 9), "Item C", 2).unwrap();

        let max = get_max_sort_order(&conn, date(2026, 3, 9)).unwrap();
        assert_eq!(max, 2, "should return highest sort_order present");
    }

    #[test]
    fn test_update_sort_order() {
        let conn = open_memory_db();
        insert_entry(&conn, date(2026, 3, 9), "Item A", 0).unwrap();
        insert_entry(&conn, date(2026, 3, 9), "Item B", 1).unwrap();

        let all = get_all_entries(&conn).unwrap();
        let id_a = all[0].id;
        let id_b = all[1].id;

        update_sort_order(&conn, id_a, 1).unwrap();
        update_sort_order(&conn, id_b, 0).unwrap();

        let reordered = get_all_entries(&conn).unwrap();
        assert_eq!(
            reordered[0].item_text, "Item B",
            "B should be first after reorder"
        );
        assert_eq!(
            reordered[1].item_text, "Item A",
            "A should be second after reorder"
        );
    }

    #[test]
    fn test_import_from_worklog() {
        let conn = open_memory_db();
        let worklog = WorkLog {
            weeks: vec![WeekEntry {
                week_number: 11,
                iso_year: None,
                days: vec![DayEntry {
                    date: date(2026, 3, 9),
                    items: vec!["Item one".into(), "Item two".into()],
                }],
            }],
        };

        let count = import_from_worklog(&conn, &worklog).unwrap();
        assert_eq!(count, 2, "should import two entries");

        let rows = get_all_entries(&conn).unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn test_import_skips_duplicates() {
        let conn = open_memory_db();
        let worklog = WorkLog {
            weeks: vec![WeekEntry {
                week_number: 11,
                iso_year: None,
                days: vec![DayEntry {
                    date: date(2026, 3, 9),
                    items: vec!["Only item".into()],
                }],
            }],
        };

        let first = import_from_worklog(&conn, &worklog).unwrap();
        assert_eq!(first, 1, "first import should insert 1 entry");

        let second = import_from_worklog(&conn, &worklog).unwrap();
        assert_eq!(second, 0, "re-import of same data should insert 0 entries");

        let rows = get_all_entries(&conn).unwrap();
        assert_eq!(rows.len(), 1, "database should still have exactly one row");
    }

    #[test]
    fn test_export_to_worklog_groups_by_week() {
        let conn = open_memory_db();

        // Two days in the same week (ISO week 11, 2026).
        insert_entry(&conn, date(2026, 3, 9), "Mon item", 0).unwrap();
        insert_entry(&conn, date(2026, 3, 10), "Tue item", 0).unwrap();

        // One day in a different week (ISO week 10, 2026).
        insert_entry(&conn, date(2026, 3, 2), "Older item", 0).unwrap();

        let worklog = export_to_worklog(&conn).unwrap();
        assert_eq!(worklog.weeks.len(), 2, "should have two week groups");

        // Newest week first.
        assert_eq!(worklog.weeks[0].week_number, 11);
        assert_eq!(worklog.weeks[0].days.len(), 2);
        assert_eq!(worklog.weeks[1].week_number, 10);
        assert_eq!(worklog.weeks[1].days.len(), 1);
    }

    // --- contacts CRUD ---

    #[test]
    fn test_get_all_contacts_empty() {
        let conn = open_memory_db();
        let contacts = get_all_contacts(&conn).unwrap();
        assert!(contacts.is_empty(), "fresh db should have no contacts");
    }

    #[test]
    fn test_get_contact_by_id_not_found() {
        let conn = open_memory_db();
        let result = get_contact_by_id(&conn, 9999).unwrap();
        assert!(result.is_none(), "missing id should return None");
    }

    #[test]
    fn test_get_contact_by_handle_not_found() {
        let conn = open_memory_db();
        let result = get_contact_by_handle(&conn, "nobody").unwrap();
        assert!(result.is_none(), "unknown handle should return None");
    }

    #[test]
    fn test_insert_and_retrieve_contact() {
        let conn = open_memory_db();
        insert_contact(&conn, "alice", "Alice Smith", "alice@example.com").unwrap();

        let contacts = get_all_contacts(&conn).unwrap();
        assert_eq!(contacts.len(), 1, "should have one contact");
        assert_eq!(contacts[0].handle, "alice");
        assert_eq!(contacts[0].full_name, "Alice Smith");
        assert_eq!(contacts[0].email, "alice@example.com");
    }

    #[test]
    fn test_get_contact_by_id_found() {
        let conn = open_memory_db();
        insert_contact(&conn, "bob", "Bob Jones", "bob@example.com").unwrap();
        let all = get_all_contacts(&conn).unwrap();
        let id = all[0].id;

        let contact = get_contact_by_id(&conn, id)
            .unwrap()
            .expect("contact should be found by id");
        assert_eq!(contact.handle, "bob");
        assert_eq!(contact.full_name, "Bob Jones");
    }

    #[test]
    fn test_get_contact_by_handle_found() {
        let conn = open_memory_db();
        insert_contact(&conn, "carol", "Carol White", "carol@example.com").unwrap();

        let contact = get_contact_by_handle(&conn, "carol")
            .unwrap()
            .expect("contact should be found by handle");
        assert_eq!(contact.email, "carol@example.com");
    }

    #[test]
    fn test_get_contact_by_handle_case_insensitive() {
        let conn = open_memory_db();
        insert_contact(&conn, "Dave", "Dave Brown", "dave@example.com").unwrap();

        let contact = get_contact_by_handle(&conn, "dave")
            .unwrap()
            .expect("lowercase lookup should find mixed-case handle");
        assert_eq!(contact.handle, "Dave");

        let contact2 = get_contact_by_handle(&conn, "DAVE")
            .unwrap()
            .expect("uppercase lookup should find mixed-case handle");
        assert_eq!(contact2.handle, "Dave");
    }

    #[test]
    fn test_update_contact() {
        let conn = open_memory_db();
        insert_contact(&conn, "eve", "Eve Original", "eve@example.com").unwrap();
        let all = get_all_contacts(&conn).unwrap();
        let id = all[0].id;

        update_contact(&conn, id, "eve", "Eve Updated", "eve2@example.com").unwrap();

        let updated = get_contact_by_id(&conn, id).unwrap().unwrap();
        assert_eq!(updated.full_name, "Eve Updated", "full_name should update");
        assert_eq!(updated.email, "eve2@example.com", "email should update");
    }

    #[test]
    fn test_delete_contact() {
        let conn = open_memory_db();
        insert_contact(&conn, "frank", "Frank Lee", "frank@example.com").unwrap();
        let all = get_all_contacts(&conn).unwrap();
        let id = all[0].id;

        delete_contact(&conn, id).unwrap();

        let result = get_contact_by_id(&conn, id).unwrap();
        assert!(result.is_none(), "contact should be gone after deletion");

        let remaining = get_all_contacts(&conn).unwrap();
        assert!(remaining.is_empty(), "no contacts should remain");
    }

    #[test]
    fn test_contacts_ordered_by_handle() {
        let conn = open_memory_db();
        insert_contact(&conn, "zara", "Zara Z", "z@example.com").unwrap();
        insert_contact(&conn, "alice", "Alice A", "a@example.com").unwrap();
        insert_contact(&conn, "mike", "Mike M", "m@example.com").unwrap();

        let contacts = get_all_contacts(&conn).unwrap();
        assert_eq!(contacts[0].handle, "alice", "should be ordered by handle");
        assert_eq!(contacts[1].handle, "mike");
        assert_eq!(contacts[2].handle, "zara");
    }

    #[test]
    fn test_round_trip_import_export() {
        let conn = open_memory_db();
        let original = WorkLog {
            weeks: vec![
                WeekEntry {
                    week_number: 10,
                    iso_year: None,
                    days: vec![DayEntry {
                        date: date(2026, 3, 2),
                        items: vec!["Old task".into()],
                    }],
                },
                WeekEntry {
                    week_number: 11,
                    iso_year: None,
                    days: vec![DayEntry {
                        date: date(2026, 3, 9),
                        items: vec!["New task A".into(), "New task B".into()],
                    }],
                },
            ],
        };

        import_from_worklog(&conn, &original).unwrap();
        let exported = export_to_worklog(&conn).unwrap();

        // Export returns newest week first; original is ordered oldest-first,
        // so compare by finding the matching weeks rather than assuming order.
        assert_eq!(
            exported.weeks.len(),
            2,
            "round-trip should preserve week count"
        );

        let week11 = exported
            .weeks
            .iter()
            .find(|w| w.week_number == 11)
            .expect("week 11 should be present");
        assert_eq!(week11.days[0].items, vec!["New task A", "New task B"]);

        let week10 = exported
            .weeks
            .iter()
            .find(|w| w.week_number == 10)
            .expect("week 10 should be present");
        assert_eq!(week10.days[0].items, vec!["Old task"]);
    }
}
