mod db;
mod parser;
mod writer;

use anyhow::Context;
use axum::{
    Form, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use chrono::{Datelike, IsoWeek, Local, NaiveDate};
use db::{
    ContactRow, delete_contact, delete_entry, export_to_worklog, get_all_contacts, get_all_entries,
    get_entry_by_id, get_max_sort_order, import_from_worklog, init_db, insert_contact,
    insert_entry, update_contact, update_entry, update_sort_order,
};
use parser::{DayEntry, WeekEntry, WorkLog, parse_worklog};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use writer::write_worklog;

/// Shared application state backed by a SQLite connection.
///
/// The Mutex ensures the single non-Send Connection is never accessed
/// concurrently from multiple Tokio tasks.
#[derive(Clone)]
struct AppState {
    conn: Arc<Mutex<Connection>>,
    display_name: String,
}

// ---------------------------------------------------------------------------
// Error type for route handlers
// ---------------------------------------------------------------------------

/// A thin wrapper so route handlers can use `?` with `anyhow::Error` and still
/// produce a proper HTTP 500 response instead of panicking.
struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Internal error: {}", self.0),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

type RouteResult<T> = Result<T, AppError>;

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

/// GET `/` — render the work log as HTML, queried from SQLite.
async fn index(State(state): State<AppState>) -> RouteResult<Html<String>> {
    let conn = state.conn.lock().await;
    let all_entries = get_all_entries(&conn).context("querying all entries")?;
    let contacts = get_all_contacts(&conn).context("querying contacts")?;
    drop(conn);

    Ok(Html(render_index(
        &all_entries,
        &contacts,
        &state.display_name,
    )))
}

/// GET `/new` — show the entry form.
async fn new_entry_form(State(state): State<AppState>) -> Html<String> {
    let today = Local::now().date_naive();
    // Display date pre-filled in the canonical worklog format.
    let today_str = today.format("%b %-d, %Y").to_string();
    Html(render_new_form(&today_str, &state.display_name))
}

/// Form payload sent by POST `/entries`.
#[derive(Deserialize)]
struct NewEntryForm {
    date: String,
    items: String,
}

/// POST `/entries` — insert new bullet items into SQLite.
async fn add_entry(
    State(state): State<AppState>,
    Form(payload): Form<NewEntryForm>,
) -> RouteResult<Redirect> {
    let date = parse_form_date(&payload.date).unwrap_or_else(|| Local::now().date_naive());

    let item = payload.items.trim().to_string();

    if item.is_empty() {
        return Ok(Redirect::to("/"));
    }

    let conn = state.conn.lock().await;

    let next_order = get_max_sort_order(&conn, date).context("querying sort order")? + 1;
    insert_entry(&conn, date, &item, next_order).context("inserting entry")?;

    Ok(Redirect::to("/"))
}

/// JSON payload sent by POST `/entries/{id}`.
#[derive(Deserialize)]
struct EditEntryForm {
    item_text: String,
}

/// POST `/entries/{id}` — update an existing entry's text via JSON and return plain text "ok".
///
/// The entry's date is preserved from the existing record; only the text is updated.
async fn update_entry_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    axum::Json(payload): axum::Json<EditEntryForm>,
) -> RouteResult<(StatusCode, &'static str)> {
    let conn = state.conn.lock().await;
    let entry = get_entry_by_id(&conn, id)
        .context("querying entry")?
        .ok_or_else(|| AppError(anyhow::anyhow!("Entry {id} not found")))?;
    update_entry(&conn, id, entry.date, &payload.item_text).context("updating entry")?;
    drop(conn);
    Ok((StatusCode::OK, "ok"))
}

/// JSON payload sent by POST `/entries/{id}/reorder`.
#[derive(Deserialize)]
struct ReorderForm {
    sort_order: i32,
}

/// POST `/entries/{id}/reorder` — update an entry's sort_order via JSON.
async fn reorder_entry_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    axum::Json(payload): axum::Json<ReorderForm>,
) -> RouteResult<(StatusCode, &'static str)> {
    let conn = state.conn.lock().await;
    update_sort_order(&conn, id, payload.sort_order).context("reordering entry")?;
    drop(conn);
    Ok((StatusCode::OK, "ok"))
}

/// POST `/entries/{id}/delete` — delete an entry and return plain text "ok".
async fn delete_entry_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> RouteResult<(StatusCode, &'static str)> {
    let conn = state.conn.lock().await;
    delete_entry(&conn, id).context("deleting entry")?;
    drop(conn);
    Ok((StatusCode::OK, "ok"))
}

/// GET `/contacts` — render the contacts management page.
async fn contacts_page(State(state): State<AppState>) -> RouteResult<Html<String>> {
    let conn = state.conn.lock().await;
    let contacts = get_all_contacts(&conn).context("querying contacts")?;
    drop(conn);
    Ok(Html(render_contacts(&contacts, &state.display_name)))
}

/// Form payload for POST `/contacts`.
#[derive(Deserialize)]
struct NewContactForm {
    handle: String,
    full_name: String,
    email: String,
}

/// POST `/contacts` — add a new contact and redirect back.
async fn add_contact_handler(
    State(state): State<AppState>,
    Form(payload): Form<NewContactForm>,
) -> RouteResult<Redirect> {
    let handle = payload.handle.trim().to_string();
    let full_name = payload.full_name.trim().to_string();
    let email = payload.email.trim().to_string();

    if handle.is_empty() || full_name.is_empty() {
        return Ok(Redirect::to("/contacts"));
    }

    // Only allow alphanumeric, underscore, and hyphen in handles so that
    // resolve_mentions can reliably match @handle tokens in entry text.
    if !handle
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Ok(Redirect::to("/contacts"));
    }

    let conn = state.conn.lock().await;
    insert_contact(&conn, &handle, &full_name, &email).context("inserting contact")?;
    drop(conn);

    Ok(Redirect::to("/contacts"))
}

/// JSON payload for POST `/contacts/{id}`.
#[derive(Deserialize)]
struct EditContactForm {
    handle: String,
    full_name: String,
    email: String,
}

/// POST `/contacts/{id}` — update an existing contact via JSON.
async fn update_contact_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(payload): Json<EditContactForm>,
) -> RouteResult<(StatusCode, &'static str)> {
    let handle = payload.handle.trim();
    if !handle
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        || handle.is_empty()
    {
        return Ok((StatusCode::BAD_REQUEST, "invalid handle"));
    }

    let conn = state.conn.lock().await;
    update_contact(
        &conn,
        id,
        handle,
        payload.full_name.trim(),
        payload.email.trim(),
    )
    .context("updating contact")?;
    drop(conn);
    Ok((StatusCode::OK, "ok"))
}

/// POST `/contacts/{id}/delete` — delete a contact and return plain text "ok".
async fn delete_contact_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> RouteResult<(StatusCode, &'static str)> {
    let conn = state.conn.lock().await;
    delete_contact(&conn, id).context("deleting contact")?;
    drop(conn);
    Ok((StatusCode::OK, "ok"))
}

/// Response shape for the contacts API.
#[derive(Serialize)]
struct ContactApiItem {
    id: i64,
    handle: String,
    full_name: String,
    email: String,
}

/// GET `/api/contacts` — return all contacts as JSON for the autocomplete widget.
async fn api_contacts(State(state): State<AppState>) -> RouteResult<Json<Vec<ContactApiItem>>> {
    let conn = state.conn.lock().await;
    let contacts = get_all_contacts(&conn).context("querying contacts")?;
    drop(conn);

    let items = contacts
        .into_iter()
        .map(|c| ContactApiItem {
            id: c.id,
            handle: c.handle,
            full_name: c.full_name,
            email: c.email,
        })
        .collect();

    Ok(Json(items))
}

/// GET `/export` — export the full work log as plain-text markdown.
///
/// Useful for copying to Rich or other consumers that expect the canonical
/// markdown format.
async fn export_markdown(State(state): State<AppState>) -> RouteResult<String> {
    let conn = state.conn.lock().await;
    let worklog = export_to_worklog(&conn).context("building export")?;
    drop(conn);
    Ok(write_worklog(&worklog))
}

// ---------------------------------------------------------------------------
// Business logic helpers
// ---------------------------------------------------------------------------

/// Parse the date string that comes from the HTML form.
///
/// Accepts the canonical `%b %-d, %Y` format (e.g. `Mar 9, 2026`).
fn parse_form_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%b %-d, %Y").ok()
}

/// Convert a flat list of `EntryRow`s (newest date first) into a `WorkLog`
/// grouped by ISO year+week.
///
/// The week display label uses the ISO year so entries near year boundaries
/// (e.g. week 53 of 2025 vs week 1 of 2026) are attributed to the correct year.
///
/// Currently only exercised by unit tests; kept for potential future callers
/// that need the structured `WorkLog` type from in-memory rows.
#[allow(dead_code)]
fn entries_to_worklog(entries: Vec<db::EntryRow>) -> WorkLog {
    let mut weeks: Vec<WeekEntry> = Vec::new();

    for row in &entries {
        let iso: IsoWeek = row.date.iso_week();
        let week_number = iso.week();

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

    // Within each week, days should be newest-first to match the visual order
    // expected on the index page.
    for week in &mut weeks {
        week.days.sort_by(|a, b| b.date.cmp(&a.date));
    }

    WorkLog { weeks }
}

// ---------------------------------------------------------------------------
// HTML rendering
// ---------------------------------------------------------------------------

const MENTION_STYLES: &str = r#"
    .mention {
        background: #e8eaf6;
        color: #3949ab;
        padding: 0 4px;
        border-radius: 3px;
        font-weight: 500;
        cursor: default;
    }
    .mention-popup {
        position: absolute;
        background: white;
        border: 1px solid #ccc;
        border-radius: 4px;
        box-shadow: 0 2px 8px rgba(0,0,0,0.15);
        max-height: 150px;
        overflow-y: auto;
        z-index: 1000;
        min-width: 180px;
    }
    .mention-popup-item {
        padding: 4px 8px;
        cursor: pointer;
        font-size: 0.9rem;
    }
    .mention-popup-item:hover, .mention-popup-item.active {
        background: #e8eaf6;
    }
    .mention-popup-item .handle { font-weight: 600; color: #3949ab; }
    .mention-popup-item .name { color: #666; margin-left: 0.5rem; }
"#;

const STYLES: &str = r#"
    body {
        font-family: system-ui, sans-serif;
        max-width: 800px;
        margin: 2rem auto;
        padding: 0 1rem;
        color: #222;
        background: #fafafa;
    }
    h1 { font-size: 1.6rem; margin-bottom: 1.5rem; }
    h2 { font-size: 1.2rem; border-bottom: 1px solid #ddd; padding-bottom: 0.3rem; margin-top: 2rem; }
    h3 { font-size: 1rem; color: #555; margin-bottom: 0.25rem; }
    ul  { margin: 0.25rem 0 1rem 1.5rem; padding: 0; }
    li  { margin: 0.2rem 0; }
    ul:not(.entry-text ul) > li { display: flex; align-items: baseline; list-style: none; }
    ul:not(.entry-text ul) > li::before { content: "\2022"; margin-right: 0.5rem; flex-shrink: 0; }
    .entry-text { flex: 1; }
    .entry-text p { margin: 0; }
    .entry-text ul, .entry-text ol { margin: 0; padding: 0; padding-left: 1.2rem; }
    .entry-text li { display: list-item; margin: 0; }
    a.button, button {
        display: inline-block;
        padding: 0.4rem 0.9rem;
        background: #2563eb;
        color: #fff;
        border: none;
        border-radius: 4px;
        cursor: pointer;
        text-decoration: none;
        font-size: 0.9rem;
    }
    a.button:hover, button:hover { background: #1d4ed8; }
    label { display: block; margin-bottom: 0.25rem; font-weight: 600; }
    input[type=text], textarea {
        width: 100%;
        padding: 0.4rem 0.5rem;
        border: 1px solid #ccc;
        border-radius: 4px;
        font-size: 0.95rem;
        box-sizing: border-box;
    }
    textarea { min-height: 6rem; resize: vertical; }
    .form-group { margin-bottom: 1rem; }
    .hint { font-size: 0.8rem; color: #666; margin-top: 0.2rem; }
    .entry-text {
        cursor: text;
        padding: 2px 4px;
        border-radius: 3px;
        border: 1px solid transparent;
        transition: border-color 0.15s, background-color 0.15s;
    }
    .entry-text:hover {
        background: #f0f4ff;
    }
    .entry-edit-area {
        width: 100%;
        min-height: 3rem;
        padding: 4px 6px;
        border: 1px solid #93b4f5;
        border-radius: 3px;
        background: #f0f4ff;
        font-family: system-ui, sans-serif;
        font-size: 0.95rem;
        resize: vertical;
        box-sizing: border-box;
    }
    .entry-actions {
        margin-left: 0.5rem;
        font-size: 0.8rem;
        visibility: hidden;
        vertical-align: middle;
    }
    li:hover .entry-actions {
        visibility: visible;
    }
    li[draggable="true"] { cursor: grab; }
    li[draggable="true"]:active { cursor: grabbing; }
    li.dragging { opacity: 0.4; }
    li.drag-over { border-top: 2px solid #2563eb; padding-top: 0; }
"#;

/// Render the index page by grouping `EntryRow`s inline, preserving IDs for
/// edit/delete links. The `entries_to_worklog` helper is kept separately for
/// the export route which needs the `WorkLog` type.
fn render_index(entries: &[db::EntryRow], contacts: &[ContactRow], display_name: &str) -> String {
    // Group rows into (week_number, iso_year) → [(date, id, item_text)] buckets
    // while preserving the incoming newest-first ordering.
    struct WeekBucket {
        week_number: u32,
        iso_year: i32,
        days: Vec<DayBucket>,
    }
    struct DayBucket {
        date: NaiveDate,
        items: Vec<(i64, String)>, // (id, item_text)
    }

    let mut weeks: Vec<WeekBucket> = Vec::new();

    for row in entries {
        let iso: IsoWeek = row.date.iso_week();
        let week_number = iso.week();
        let iso_year = iso.year();

        let week_idx = match weeks
            .iter()
            .position(|w| w.week_number == week_number && w.iso_year == iso_year)
        {
            Some(i) => i,
            None => {
                weeks.push(WeekBucket {
                    week_number,
                    iso_year,
                    days: Vec::new(),
                });
                weeks.len() - 1
            }
        };

        let week = &mut weeks[week_idx];
        let day_idx = match week.days.iter().position(|d| d.date == row.date) {
            Some(i) => i,
            None => {
                week.days.push(DayBucket {
                    date: row.date,
                    items: Vec::new(),
                });
                week.days.len() - 1
            }
        };

        week.days[day_idx]
            .items
            .push((row.id, row.item_text.clone()));
    }

    let mut body = String::new();

    let escaped_name = html_escape(display_name);
    body.push_str(&format!(r#"<h1>{escaped_name}</h1>"#));
    body.push_str(
        r#"<p><a href="/new" class="button">+ New Entry</a>&nbsp;<a href="/export" class="button" style="background:#6b7280;">Export Markdown</a>&nbsp;<a href="/contacts" class="button" style="background:#6b7280;">Contacts</a></p>"#,
    );

    if weeks.is_empty() {
        body.push_str("<p><em>No entries yet.</em></p>");
    }

    for week in &weeks {
        body.push_str(&format!(
            "<h2>Week {}, {}</h2>",
            week.week_number, week.iso_year
        ));

        for day in &week.days {
            let date_str = day.date.format("%b %-d, %Y").to_string();
            body.push_str(&format!("<h3>{date_str}</h3>"));

            if day.items.is_empty() {
                body.push_str("<p><em>(no items)</em></p>");
            } else {
                body.push_str("<ul>");
                for (id, item_text_raw) in &day.items {
                    let raw_escaped = html_escape(item_text_raw);
                    let rendered = resolve_mentions(&render_markdown(item_text_raw), contacts);
                    body.push_str(&format!(
                        r##"<li draggable="true" data-id="{id}">
  <span class="entry-text" data-id="{id}" data-original="{raw_escaped}">{rendered}</span>
  <span class="entry-actions">
    <a href="#" class="delete-btn" data-id="{id}" title="Delete">&#128465;</a>
    <form id="del-{id}" method="post" action="/entries/{id}/delete" style="display:none"></form>
  </span>
</li>"##
                    ));
                }
                body.push_str("</ul>");
            }
        }
    }

    wrap_html(display_name, &body)
}

fn render_new_form(today_str: &str, display_name: &str) -> String {
    let escaped_date = html_escape(today_str);
    let body = format!(
        r#"
<h1>New Entry</h1>
<form method="post" action="/entries">
  <div class="form-group">
    <label for="date">Date</label>
    <input type="text" id="date" name="date" value="{escaped_date}" />
    <p class="hint">Format: Mon D, YYYY (e.g. Mar 9, 2026)</p>
  </div>
  <div class="form-group">
    <label for="items">Items</label>
    <textarea id="items" name="items" placeholder="What did you work on?"></textarea>
    <p class="hint">Markdown supported. The entire content becomes one entry.</p>
  </div>
  <button type="submit">Save</button>
  <a href="/" class="button" style="background:#6b7280;margin-left:0.5rem;">Cancel</a>
</form>
"#
    );
    wrap_html(&format!("New Entry — {display_name}"), &body)
}

/// Wrap page content in a minimal but complete HTML document, including the
/// client-side JS for inline editing and delete confirmation.
fn wrap_html(title: &str, body: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<link rel="icon" href="data:image/svg+xml,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><text y='.9em' font-size='90'>📋</text></svg>">
<title>{title}</title>
<style>{STYLES}{MENTION_STYLES}</style>
</head>
<body>
{body}
<script>
document.addEventListener('DOMContentLoaded', () => {{
  // Click to edit — replace rendered HTML with a textarea
  document.querySelectorAll('.entry-text').forEach(el => {{
    el.addEventListener('click', () => {{
      if (el.querySelector('textarea')) return;
      const original = el.dataset.original;
      const ta = document.createElement('textarea');
      ta.className = 'entry-edit-area';
      ta.value = original;
      ta.dataset.id = el.dataset.id;
      el._savedHtml = el.innerHTML;
      el.innerHTML = '';
      el.style.border = 'none';
      el.appendChild(ta);
      ta.focus();
      ta.setSelectionRange(ta.value.length, ta.value.length);
      // Auto-size to content
      ta.style.height = ta.scrollHeight + 'px';
    }});
  }});

  // Save on Ctrl+Enter, cancel on Escape
  document.addEventListener('keydown', (e) => {{
    if (e.target.tagName !== 'TEXTAREA' || !e.target.classList.contains('entry-edit-area')) return;
    const ta = e.target;
    const span = ta.parentElement;
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {{
      e.preventDefault();
      ta.blur();
    }} else if (e.key === 'Escape') {{
      span.innerHTML = span._savedHtml;
      span.style.border = '';
    }}
  }});

  // Save on blur
  document.addEventListener('focusout', (e) => {{
    const ta = e.target;
    if (ta.tagName !== 'TEXTAREA' || !ta.classList.contains('entry-edit-area')) return;
    const span = ta.parentElement;
    if (!span) return;
    const newText = ta.value.trim();
    const original = span.dataset.original;
    if (newText === original) {{
      span.innerHTML = span._savedHtml;
      span.style.border = '';
      return;
    }}
    fetch('/entries/' + span.dataset.id, {{
      method: 'POST',
      headers: {{'Content-Type': 'application/json'}},
      body: JSON.stringify({{item_text: newText}})
    }}).then(r => {{ if (r.ok) location.reload(); }});
  }});

  // Drag-to-reorder entries within a day
  let dragSrc = null;
  document.querySelectorAll('li[draggable="true"]').forEach(li => {{
    li.addEventListener('dragstart', (e) => {{
      dragSrc = li;
      li.classList.add('dragging');
      e.dataTransfer.effectAllowed = 'move';
    }});
    li.addEventListener('dragend', () => {{
      li.classList.remove('dragging');
      document.querySelectorAll('.drag-over').forEach(el => el.classList.remove('drag-over'));
      dragSrc = null;
    }});
    li.addEventListener('dragover', (e) => {{
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
      if (li !== dragSrc && li.parentElement === dragSrc?.parentElement) {{
        li.classList.add('drag-over');
      }}
    }});
    li.addEventListener('dragleave', () => {{
      li.classList.remove('drag-over');
    }});
    li.addEventListener('drop', (e) => {{
      e.preventDefault();
      li.classList.remove('drag-over');
      if (!dragSrc || dragSrc === li) return;
      const ul = li.parentElement;
      if (ul !== dragSrc.parentElement) return;
      // Insert dragged item before the drop target
      ul.insertBefore(dragSrc, li);
      // Send new sort_order for all items in this list
      const items = ul.querySelectorAll('li[draggable="true"]');
      items.forEach((item, idx) => {{
        const id = item.dataset.id;
        fetch('/entries/' + id + '/reorder', {{
          method: 'POST',
          headers: {{'Content-Type': 'application/json'}},
          body: JSON.stringify({{sort_order: idx}})
        }});
      }});
    }});
  }});

  // Delete on trash click
  document.querySelectorAll('.delete-btn').forEach(btn => {{
    btn.addEventListener('click', (e) => {{
      e.preventDefault();
      if (!confirm('Delete this entry?')) return;
      const id = btn.dataset.id;
      fetch('/entries/' + id + '/delete', {{method: 'POST'}})
        .then(r => {{ if (r.ok) location.reload(); }});
    }});
  }});

  // @mention autocomplete for all textareas
  let mentionContacts = [];
  fetch('/api/contacts').then(r => r.json()).then(data => {{ mentionContacts = data; }}).catch(() => {{}});

  let mentionPopup = null;
  let mentionAnchorTA = null;
  let mentionAtPos = -1;
  let mentionActiveIdx = -1;

  function closeMentionPopup() {{
    if (mentionPopup) {{ mentionPopup.remove(); mentionPopup = null; }}
    mentionAnchorTA = null;
    mentionAtPos = -1;
    mentionActiveIdx = -1;
  }}

  function buildMentionPopup(ta, query) {{
    closeMentionPopup();
    const lower = query.toLowerCase();
    const matches = mentionContacts.filter(c =>
      c.handle.toLowerCase().startsWith(lower) ||
      c.full_name.toLowerCase().includes(lower)
    );
    if (matches.length === 0) return;

    mentionAnchorTA = ta;
    mentionActiveIdx = 0;

    mentionPopup = document.createElement('div');
    mentionPopup.className = 'mention-popup';

    // Position below the textarea (simple approach: below+left of textarea)
    const rect = ta.getBoundingClientRect();
    mentionPopup.style.top = (window.scrollY + rect.bottom + 2) + 'px';
    mentionPopup.style.left = (window.scrollX + rect.left) + 'px';

    matches.forEach((contact, idx) => {{
      const item = document.createElement('div');
      item.className = 'mention-popup-item' + (idx === 0 ? ' active' : '');
      const handleSpan = document.createElement('span');
      handleSpan.className = 'handle';
      handleSpan.textContent = '@' + contact.handle;
      const nameSpan = document.createElement('span');
      nameSpan.className = 'name';
      nameSpan.textContent = contact.full_name;
      item.appendChild(handleSpan);
      item.appendChild(nameSpan);
      item.addEventListener('mousedown', (e) => {{
        e.preventDefault();
        insertMention(ta, contact.handle);
      }});
      mentionPopup.appendChild(item);
    }});

    document.body.appendChild(mentionPopup);
  }}

  function setMentionActive(idx) {{
    if (!mentionPopup) return;
    const items = mentionPopup.querySelectorAll('.mention-popup-item');
    items.forEach((el, i) => el.classList.toggle('active', i === idx));
    mentionActiveIdx = idx;
    if (items[idx]) items[idx].scrollIntoView({{block: 'nearest'}});
  }}

  function insertMention(ta, handle) {{
    const val = ta.value;
    const before = val.substring(0, mentionAtPos);
    const after = val.substring(ta.selectionStart);
    ta.value = before + '@' + handle + ' ' + after;
    const newPos = mentionAtPos + handle.length + 2;
    ta.setSelectionRange(newPos, newPos);
    closeMentionPopup();
  }}

  document.addEventListener('input', (e) => {{
    const ta = e.target;
    if (ta.tagName !== 'TEXTAREA') return;
    const pos = ta.selectionStart;
    const text = ta.value.substring(0, pos);
    const atMatch = text.match(/@([\w-]*)$/);
    if (atMatch) {{
      mentionAtPos = pos - atMatch[0].length;
      buildMentionPopup(ta, atMatch[1]);
    }} else {{
      closeMentionPopup();
    }}
  }});

  document.addEventListener('keydown', (e) => {{
    if (!mentionPopup) return;
    const items = mentionPopup.querySelectorAll('.mention-popup-item');
    if (e.key === 'ArrowDown') {{
      e.preventDefault();
      setMentionActive(Math.min(mentionActiveIdx + 1, items.length - 1));
    }} else if (e.key === 'ArrowUp') {{
      e.preventDefault();
      setMentionActive(Math.max(mentionActiveIdx - 1, 0));
    }} else if (e.key === 'Enter' && mentionAnchorTA) {{
      const activeItem = mentionPopup.querySelector('.mention-popup-item.active');
      if (activeItem) {{
        e.preventDefault();
        const handle = activeItem.querySelector('.handle').textContent.slice(1);
        insertMention(mentionAnchorTA, handle);
      }}
    }} else if (e.key === 'Escape') {{
      closeMentionPopup();
    }}
  }}, true);

  document.addEventListener('click', (e) => {{
    if (mentionPopup && !mentionPopup.contains(e.target)) closeMentionPopup();
  }});
}});
</script>
</body>
</html>
"#
    )
}

/// Render a markdown string as inline HTML using GitHub Flavored Markdown.
///
/// Supports links, bold, italic, strikethrough, inline code, autolinks,
/// and emoji shortcodes (e.g. `:heart:` → ❤️).
fn render_markdown(s: &str) -> String {
    use comrak::{Options, markdown_to_html};

    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.shortcodes = true;
    options.render.unsafe_ = false;

    let html = markdown_to_html(s, &options);
    let trimmed = html.trim();

    // Strip the leading <p>…</p> wrapper so the first line renders inline
    // inside <li>.  Any subsequent block elements (sub-lists, additional
    // paragraphs) are kept as-is.
    if let Some(rest) = trimmed.strip_prefix("<p>") {
        if let Some(end) = rest.find("</p>") {
            let inner = &rest[..end];
            let after = rest[end + 4..].trim_start();
            if after.is_empty() {
                inner.to_string()
            } else {
                format!("{inner}\n{after}")
            }
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    }
}

/// Replace `@handle` tokens in rendered HTML with styled mention spans.
///
/// Only handles that exist in `contacts` are replaced; unknown @-words are
/// left as-is so arbitrary email addresses in entries don't get mangled.
/// Matching is case-insensitive on the handle.
fn resolve_mentions(html: &str, contacts: &[ContactRow]) -> String {
    if contacts.is_empty() {
        return html.to_string();
    }

    // Walk the HTML character by character. We only replace @word tokens that
    // appear outside of HTML tags/attributes (i.e. not inside < … >).
    let mut result = String::with_capacity(html.len());
    let chars: Vec<char> = html.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '<' {
            // Copy the entire tag verbatim — don't touch attributes.
            result.push(chars[i]);
            i += 1;
            while i < len {
                result.push(chars[i]);
                if chars[i] == '>' {
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else if chars[i] == '@' {
            // Collect the word following '@'.
            let start = i + 1;
            let mut end = start;
            while end < len
                && (chars[end].is_alphanumeric() || chars[end] == '_' || chars[end] == '-')
            {
                end += 1;
            }

            if end > start {
                let word: String = chars[start..end].iter().collect();
                // Case-insensitive lookup.
                let lower_word = word.to_lowercase();
                let matched = contacts
                    .iter()
                    .find(|c| c.handle.to_lowercase() == lower_word);

                if let Some(contact) = matched {
                    let title = html_escape(&format!("{} ({})", contact.full_name, contact.email));
                    result.push_str(&format!(
                        r#"<span class="mention" title="{title}">@{}</span>"#,
                        html_escape(&contact.handle)
                    ));
                } else {
                    // Unknown handle — pass through unchanged.
                    result.push('@');
                    result.extend(chars[start..end].iter());
                }
                i = end;
            } else {
                // '@' not followed by a word character — pass through.
                result.push('@');
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Render the /contacts management page.
fn render_contacts(contacts: &[ContactRow], display_name: &str) -> String {
    let mut body = String::new();

    body.push_str(r#"<h1>Contacts</h1>"#);
    body.push_str(r#"<p><a href="/" class="button" style="background:#6b7280;">&#8592; Back to worklog</a></p>"#);

    // Add new contact form.
    body.push_str(
        r#"<h2>Add Contact</h2>
<form method="post" action="/contacts">
  <div class="form-group">
    <label for="handle">Handle</label>
    <input type="text" id="handle" name="handle" placeholder="alice" required />
    <p class="hint">Used as @alice in entries. No @ prefix.</p>
  </div>
  <div class="form-group">
    <label for="full_name">Full Name</label>
    <input type="text" id="full_name" name="full_name" placeholder="Alice Smith" required />
  </div>
  <div class="form-group">
    <label for="email">Email</label>
    <input type="text" id="email" name="email" placeholder="alice@example.com" />
  </div>
  <button type="submit">Add Contact</button>
</form>
"#,
    );

    // Contacts table.
    body.push_str("<h2>All Contacts</h2>");

    if contacts.is_empty() {
        body.push_str("<p><em>No contacts yet.</em></p>");
    } else {
        body.push_str(
            r#"<table style="width:100%;border-collapse:collapse;">
<thead>
<tr>
  <th style="text-align:left;padding:6px 8px;border-bottom:2px solid #ddd;">Handle</th>
  <th style="text-align:left;padding:6px 8px;border-bottom:2px solid #ddd;">Full Name</th>
  <th style="text-align:left;padding:6px 8px;border-bottom:2px solid #ddd;">Email</th>
  <th style="padding:6px 8px;border-bottom:2px solid #ddd;"></th>
</tr>
</thead>
<tbody>
"#,
        );

        for contact in contacts {
            let handle_esc = html_escape(&contact.handle);
            let name_esc = html_escape(&contact.full_name);
            let email_esc = html_escape(&contact.email);
            let id = contact.id;

            body.push_str(&format!(
                r##"<tr id="contact-row-{id}">
  <td style="padding:6px 8px;border-bottom:1px solid #eee;">
    <span class="contact-field" data-field="handle" data-id="{id}">{handle_esc}</span>
  </td>
  <td style="padding:6px 8px;border-bottom:1px solid #eee;">
    <span class="contact-field" data-field="full_name" data-id="{id}">{name_esc}</span>
  </td>
  <td style="padding:6px 8px;border-bottom:1px solid #eee;">
    <span class="contact-field" data-field="email" data-id="{id}">{email_esc}</span>
  </td>
  <td style="padding:6px 8px;border-bottom:1px solid #eee;text-align:right;">
    <a href="#" class="contact-delete-btn" data-id="{id}" title="Delete">&#128465;</a>
    <form id="contact-del-{id}" method="post" action="/contacts/{id}/delete" style="display:none"></form>
  </td>
</tr>"##
            ));
        }

        body.push_str("</tbody></table>");
    }

    // Inline editing JS for contact fields.
    let contacts_js = r#"
<script>
document.addEventListener('DOMContentLoaded', () => {
  // Inline editing for contact fields
  document.querySelectorAll('.contact-field').forEach(span => {
    span.style.cursor = 'text';
    span.style.padding = '2px 4px';
    span.style.borderRadius = '3px';
    span.style.border = '1px solid transparent';
    span.addEventListener('mouseenter', () => { span.style.background = '#f0f4ff'; });
    span.addEventListener('mouseleave', () => { if (!span.querySelector('input')) span.style.background = ''; });

    span.addEventListener('click', () => {
      if (span.querySelector('input')) return;
      const original = span.textContent;
      const input = document.createElement('input');
      input.type = 'text';
      input.value = original;
      input.style.width = '100%';
      input.style.padding = '2px 4px';
      input.style.border = '1px solid #93b4f5';
      input.style.borderRadius = '3px';
      input.style.background = '#f0f4ff';
      input.style.fontFamily = 'system-ui, sans-serif';
      input.style.fontSize = '0.95rem';
      span._savedText = original;
      span.textContent = '';
      span.appendChild(input);
      input.focus();
      input.select();

      function save() {
        const newVal = input.value.trim();
        if (newVal === original) {
          span.textContent = original;
          span.style.background = '';
          return;
        }
        const id = span.dataset.id;
        const row = document.getElementById('contact-row-' + id);
        const fields = row.querySelectorAll('.contact-field');
        const data = {};
        fields.forEach(f => {
          data[f.dataset.field] = f === span ? newVal : f.textContent;
        });
        fetch('/contacts/' + id, {
          method: 'POST',
          headers: {'Content-Type': 'application/json'},
          body: JSON.stringify(data)
        }).then(r => { if (r.ok) location.reload(); });
      }

      input.addEventListener('blur', save);
      input.addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); input.blur(); }
        if (e.key === 'Escape') { span.textContent = original; span.style.background = ''; }
      });
    });
  });

  // Delete contact
  document.querySelectorAll('.contact-delete-btn').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.preventDefault();
      if (!confirm('Delete this contact?')) return;
      const id = btn.dataset.id;
      fetch('/contacts/' + id + '/delete', {method: 'POST'})
        .then(r => { if (r.ok) location.reload(); });
    });
  });
});
</script>
"#;

    body.push_str(contacts_js);

    wrap_html(&format!("Contacts — {display_name}"), &body)
}

/// Escape the five special HTML characters for use in HTML attributes.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
        .replace('\n', "&#10;")
}

// ---------------------------------------------------------------------------
// Application entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (silently ignore if missing).
    dotenvy::dotenv().ok();

    let data_dir = std::env::var("WORKLOG_DATA_DIR").unwrap_or_else(|_| ".".to_string());
    let data_path = PathBuf::from(&data_dir);

    let db_path = data_path.join("worklog.db");
    let conn = init_db(&db_path).context("initializing database")?;

    // On a fresh database, import existing markdown files so no data is lost.
    let is_empty: bool = conn
        .query_row("SELECT COUNT(*) FROM entries", [], |r| r.get::<_, i64>(0))
        .context("checking entry count")?
        == 0;

    if is_empty {
        let mut total_imported = 0usize;

        for path in &[
            data_path.join("worklog Archive.md"),
            data_path.join("worklog.md"),
        ] {
            if path.exists() {
                let raw = std::fs::read_to_string(path)
                    .with_context(|| format!("reading {}", path.display()))?;
                match parse_worklog(&raw) {
                    Ok(worklog) => {
                        let n = import_from_worklog(&conn, &worklog)
                            .with_context(|| format!("importing {}", path.display()))?;
                        println!("Imported {n} entries from {}", path.display());
                        total_imported += n;
                    }
                    Err(err) => {
                        eprintln!("Warning: failed to parse {}: {err}", path.display());
                    }
                }
            }
        }

        if total_imported > 0 {
            println!("Total imported: {total_imported} entries");
        }
    }

    let display_name =
        std::env::var("WORKLOG_DISPLAY_NAME").unwrap_or_else(|_| "worklog".to_string());

    let state = AppState {
        conn: Arc::new(Mutex::new(conn)),
        display_name,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/new", get(new_entry_form))
        .route("/entries", post(add_entry))
        .route("/entries/{id}", post(update_entry_handler))
        .route("/entries/{id}/delete", post(delete_entry_handler))
        .route("/entries/{id}/reorder", post(reorder_entry_handler))
        .route("/export", get(export_markdown))
        .route("/contacts", get(contacts_page).post(add_contact_handler))
        .route("/contacts/{id}", post(update_contact_handler))
        .route("/contacts/{id}/delete", post(delete_contact_handler))
        .route("/api/contacts", get(api_contacts))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030").await?;
    println!("Listening on http://127.0.0.1:3030");
    axum::serve(listener, app).await?;

    Ok(())
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

    // --- parse_form_date ---

    #[test]
    fn test_parse_form_date_returns_none_for_empty_string() {
        assert!(
            parse_form_date("").is_none(),
            "empty string should not parse as a date"
        );
    }

    #[test]
    fn test_parse_form_date_returns_none_for_garbage() {
        assert!(
            parse_form_date("not a date").is_none(),
            "garbage should not parse as a date"
        );
    }

    #[test]
    fn test_parse_form_date_parses_canonical_format() {
        let d = parse_form_date("Mar 9, 2026");
        assert_eq!(d, Some(date(2026, 3, 9)), "canonical format should parse");
    }

    #[test]
    fn test_parse_form_date_trims_whitespace() {
        let d = parse_form_date("  Jan 5, 2026  ");
        assert_eq!(
            d,
            Some(date(2026, 1, 5)),
            "should trim surrounding whitespace"
        );
    }

    // --- html_escape ---

    #[test]
    fn test_html_escape_plain_text_unchanged() {
        assert_eq!(
            html_escape("Hello world"),
            "Hello world",
            "plain text should pass through unchanged"
        );
    }

    #[test]
    fn test_html_escape_replaces_all_special_characters() {
        let input = r#"<script>alert('x & "y"')</script>"#;
        let out = html_escape(input);
        assert!(!out.contains('<'), "escaped output must not contain raw <");
        assert!(!out.contains('>'), "escaped output must not contain raw >");
        assert!(
            !out.contains('&')
                || out.contains("&amp;")
                || out.contains("&lt;")
                || out.contains("&gt;")
                || out.contains("&quot;")
                || out.contains("&#39;"),
            "remaining & must be part of an escape sequence"
        );
    }

    // --- render_markdown ---

    #[test]
    fn test_render_markdown_plain_text() {
        let html = render_markdown("Hello world");
        assert_eq!(
            html, "Hello world",
            "single-line plain text should have p stripped"
        );
    }

    #[test]
    fn test_render_markdown_multiline_with_sublist() {
        let input = "Main task\n\n- Sub item A\n- Sub item B";
        let html = render_markdown(input);
        assert!(
            html.contains("<li>"),
            "sub-list items should render as list items: {html}"
        );
        assert!(
            html.contains("Sub item A"),
            "sub-list content preserved: {html}"
        );
    }

    #[test]
    fn test_render_markdown_link() {
        let html = render_markdown("See [#17095](https://github.com/example)");
        assert!(
            html.contains(r#"<a href="https://github.com/example">#17095</a>"#),
            "markdown links should render as HTML anchors: {html}"
        );
    }

    #[test]
    fn test_render_markdown_emoji_shortcode() {
        let html = render_markdown("Great work :heart:");
        assert!(
            html.contains('\u{2764}') || html.contains("❤"),
            "emoji shortcodes should render as unicode: {html}"
        );
    }

    #[test]
    fn test_render_markdown_bold_and_strikethrough() {
        let html = render_markdown("**bold** and ~~struck~~");
        assert!(html.contains("<strong>bold</strong>"), "bold: {html}");
        assert!(html.contains("<del>struck</del>"), "strikethrough: {html}");
    }

    // --- entries_to_worklog ---

    #[test]
    fn test_entries_to_worklog_empty_input() {
        let wl = entries_to_worklog(vec![]);
        assert!(
            wl.weeks.is_empty(),
            "no entries should produce empty WorkLog"
        );
    }

    #[test]
    fn test_entries_to_worklog_groups_by_week() {
        let entries = vec![
            db::EntryRow {
                id: 1,
                date: date(2026, 3, 10),
                item_text: "Tue item".into(),
                created_at: "2026-03-10".into(),
                sort_order: 0,
            },
            db::EntryRow {
                id: 2,
                date: date(2026, 3, 9),
                item_text: "Mon item".into(),
                created_at: "2026-03-09".into(),
                sort_order: 0,
            },
            db::EntryRow {
                id: 3,
                date: date(2026, 3, 2),
                item_text: "Older item".into(),
                created_at: "2026-03-02".into(),
                sort_order: 0,
            },
        ];

        let wl = entries_to_worklog(entries);
        assert_eq!(wl.weeks.len(), 2, "two ISO weeks");
        assert_eq!(wl.weeks[0].week_number, 11, "week 11 first (newest)");
        assert_eq!(wl.weeks[0].days.len(), 2);
        assert_eq!(wl.weeks[1].week_number, 10);
    }

    // --- render_index ---

    fn entry_row(id: i64, d: NaiveDate, text: &str) -> db::EntryRow {
        db::EntryRow {
            id,
            date: d,
            item_text: text.into(),
            created_at: d.to_string(),
            sort_order: 0,
        }
    }

    #[test]
    fn test_render_index_empty_log_shows_no_entries_message() {
        let html = render_index(&[], &[], "worklog");
        assert!(
            html.contains("No entries yet"),
            "empty log should say 'No entries yet'"
        );
    }

    #[test]
    fn test_render_index_shows_week_header_with_year() {
        let entries = vec![entry_row(1, date(2026, 3, 9), "Task")];
        let html = render_index(&entries, &[], "worklog");
        assert!(
            html.contains("Week 11"),
            "rendered HTML should contain week header"
        );
        assert!(
            html.contains("2026"),
            "rendered HTML should contain the year"
        );
    }

    #[test]
    fn test_render_index_shows_items() {
        let entries = vec![entry_row(1, date(2026, 3, 9), "Attended standup")];
        let html = render_index(&entries, &[], "worklog");
        assert!(
            html.contains("Attended standup"),
            "rendered HTML should contain the work item"
        );
        assert!(
            html.contains("Mar 9, 2026"),
            "rendered HTML should contain the formatted date"
        );
    }

    #[test]
    fn test_render_index_escapes_html_in_items() {
        let entries = vec![entry_row(1, date(2026, 3, 9), "Fix <bug> & deploy")];
        let html = render_index(&entries, &[], "worklog");
        assert!(
            !html.contains("<bug>"),
            "raw < and > in items must be HTML-escaped by comrak"
        );
    }

    #[test]
    fn test_render_index_includes_entry_text_class_and_data_id() {
        let entries = vec![entry_row(42, date(2026, 3, 9), "Attended standup")];
        let html = render_index(&entries, &[], "worklog");
        assert!(
            html.contains("class=\"entry-text\""),
            "index should use entry-text class for inline editing: {html}"
        );
        assert!(
            html.contains("data-id=\"42\""),
            "index should include data-id attribute for each entry: {html}"
        );
        assert!(
            html.contains("/entries/42/delete"),
            "index should include a delete route for each entry: {html}"
        );
    }

    // --- render_new_form ---

    #[test]
    fn test_render_new_form_contains_prefilled_date() {
        let html = render_new_form("Mar 9, 2026", "worklog");
        assert!(
            html.contains("Mar 9, 2026"),
            "form should contain the pre-filled date"
        );
    }

    #[test]
    fn test_render_new_form_posts_to_entries() {
        let html = render_new_form("Mar 9, 2026", "worklog");
        assert!(
            html.contains(r#"action="/entries""#),
            "form action should post to /entries"
        );
    }

    // --- custom display name ---

    #[test]
    fn test_render_index_uses_custom_display_name() {
        let html = render_index(&[], &[], "My Work Tracker");
        assert!(
            html.contains("My Work Tracker"),
            "index should show custom display name"
        );
    }

    #[test]
    fn test_render_new_form_uses_custom_display_name() {
        let html = render_new_form("Mar 9, 2026", "My Tracker");
        assert!(
            html.contains("My Tracker"),
            "new form title should contain custom display name"
        );
    }

    // --- favicon ---

    #[test]
    fn test_wrap_html_includes_favicon() {
        let html = wrap_html("test", "body");
        assert!(
            html.contains(r#"rel="icon""#),
            "pages should include a favicon link"
        );
    }

    // --- draggable ---

    #[test]
    fn test_render_index_includes_draggable_attribute() {
        let entries = vec![entry_row(1, date(2026, 3, 9), "Task")];
        let html = render_index(&entries, &[], "worklog");
        assert!(
            html.contains(r#"draggable="true""#),
            "list items should be draggable: {html}"
        );
    }

    // --- resolve_mentions ---

    fn make_contact(id: i64, handle: &str, full_name: &str, email: &str) -> ContactRow {
        ContactRow {
            id,
            handle: handle.to_string(),
            full_name: full_name.to_string(),
            email: email.to_string(),
            created_at: "2026-01-01".to_string(),
        }
    }

    #[test]
    fn test_resolve_mentions_no_contacts_unchanged() {
        let html = "Worked with @alice on the feature";
        let result = resolve_mentions(html, &[]);
        assert_eq!(
            result, html,
            "with no contacts the text should be unchanged"
        );
    }

    #[test]
    fn test_resolve_mentions_unknown_handle_unchanged() {
        let contacts = vec![make_contact(1, "bob", "Bob Jones", "bob@example.com")];
        let html = "Worked with @alice today";
        let result = resolve_mentions(html, &contacts);
        assert!(
            result.contains("@alice"),
            "unknown handle should pass through: {result}"
        );
        assert!(
            !result.contains("mention"),
            "unknown handle must not get mention class: {result}"
        );
    }

    #[test]
    fn test_resolve_mentions_known_handle_replaced() {
        let contacts = vec![make_contact(1, "alice", "Alice Smith", "alice@example.com")];
        let html = "Worked with @alice on the feature";
        let result = resolve_mentions(html, &contacts);
        assert!(
            result.contains(r#"class="mention""#),
            "known handle should get mention span: {result}"
        );
        assert!(
            result.contains("@alice"),
            "handle text should be preserved: {result}"
        );
        assert!(
            result.contains("Alice Smith"),
            "tooltip should include full name: {result}"
        );
        assert!(
            result.contains("alice@example.com"),
            "tooltip should include email: {result}"
        );
    }

    #[test]
    fn test_resolve_mentions_case_insensitive() {
        let contacts = vec![make_contact(1, "Alice", "Alice Smith", "alice@example.com")];
        // Entry uses lowercase, contact stored as mixed case.
        let html = "Worked with @alice today";
        let result = resolve_mentions(html, &contacts);
        assert!(
            result.contains(r#"class="mention""#),
            "case-insensitive lookup should match: {result}"
        );
    }

    #[test]
    fn test_resolve_mentions_skips_handles_inside_tags() {
        // An @-sign inside an HTML attribute should not be touched.
        let contacts = vec![make_contact(1, "alice", "Alice Smith", "alice@example.com")];
        let html = r#"<a href="mailto:alice@example.com">link</a> and @alice"#;
        let result = resolve_mentions(html, &contacts);
        // The mailto href must be untouched.
        assert!(
            result.contains(r#"href="mailto:alice@example.com""#),
            "href content must not be rewritten: {result}"
        );
        // The bare @alice in text content should be replaced.
        assert!(
            result.contains(r#"class="mention""#),
            "text mention should still be replaced: {result}"
        );
    }

    // --- render_contacts ---

    #[test]
    fn test_render_contacts_empty_shows_no_contacts_message() {
        let html = render_contacts(&[], "worklog");
        assert!(
            html.contains("No contacts yet"),
            "empty contacts should say 'No contacts yet': {html}"
        );
    }

    #[test]
    fn test_render_contacts_shows_contact_fields() {
        let contacts = vec![make_contact(1, "alice", "Alice Smith", "alice@example.com")];
        let html = render_contacts(&contacts, "worklog");
        assert!(html.contains("alice"), "handle should appear: {html}");
        assert!(
            html.contains("Alice Smith"),
            "full name should appear: {html}"
        );
        assert!(
            html.contains("alice@example.com"),
            "email should appear: {html}"
        );
    }

    #[test]
    fn test_render_contacts_has_add_form() {
        let html = render_contacts(&[], "worklog");
        assert!(
            html.contains(r#"action="/contacts""#),
            "contacts page should have add form: {html}"
        );
    }

    #[test]
    fn test_render_contacts_has_back_link() {
        let html = render_contacts(&[], "worklog");
        assert!(
            html.contains(r#"href="/""#),
            "contacts page should have a back link to worklog: {html}"
        );
    }

    // --- contacts link on index ---

    #[test]
    fn test_render_index_has_contacts_button() {
        let html = render_index(&[], &[], "worklog");
        assert!(
            html.contains(r#"href="/contacts""#),
            "index page should have a Contacts button: {html}"
        );
    }

    // --- mention CSS in wrap_html ---

    #[test]
    fn test_wrap_html_includes_mention_styles() {
        let html = wrap_html("test", "body");
        assert!(
            html.contains("mention-popup"),
            "wrap_html should include mention popup styles: {html}"
        );
    }
}
