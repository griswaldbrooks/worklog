# Revision History

## Problem

The worklog stores entries in a mutable SQLite table. When an entry is edited or deleted, the previous state is lost. There is no undo, no audit trail, and no way to recover from accidental edits. Bugs in client-side JavaScript (e.g., the autocomplete text deletion bug) can silently overwrite entry content with truncated or empty text.

## Design

Every mutation to an entry creates a new **revision** — a snapshot of the entry's full state at that point in time. The current state is always the latest revision.

Deletions use **soft delete** — a `deleted_at` timestamp on the `entries` table. This allows undo of deletions without re-inserting rows (which would change IDs). All read queries filter with `WHERE deleted_at IS NULL`.

Revisions are kept indefinitely. Manual compaction is available via the maintenance page.

### Schema changes

Add a `deleted_at` column to `entries`:

```sql
ALTER TABLE entries ADD COLUMN deleted_at TEXT DEFAULT NULL;
```

All existing read queries (`get_all_entries`, `get_max_sort_order`, etc.) add `WHERE deleted_at IS NULL`.

New revisions table:

```sql
CREATE TABLE entry_revisions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    entry_id    INTEGER NOT NULL,
    date        TEXT    NOT NULL,
    item_text   TEXT    NOT NULL,
    sort_order  INTEGER NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (entry_id) REFERENCES entries(id)
);
CREATE INDEX idx_revisions_entry_id ON entry_revisions(entry_id);
```

No `state` field needed — the `entries.deleted_at` column tracks deletion state. Revisions only record content snapshots.

### Lifecycle

**Create:** Insert the entry into `entries`, then append revision #1.

```
entry_revisions: [{entry_id: 42, item_text: "Attended standup"}]
entries:         [{id: 42, item_text: "Attended standup", deleted_at: null}]
```

**Edit:** Update `entries` with the new text, then append a new revision with the updated state.

```
entry_revisions: [{..., item_text: "Attended standup"},
                  {..., item_text: "Attended standup and retro"}]
entries:         [{id: 42, item_text: "Attended standup and retro", deleted_at: null}]
```

**Delete (soft):** Set `deleted_at` on the entry. No revision appended — deletion is a status change, not a content change.

```
entries: [{id: 42, item_text: "Attended standup and retro", deleted_at: "2026-03-19 10:15:00"}]
```

**Undo edit:** Read the second-to-last revision for the entry. Restore `entries` to that state. Remove the latest revision. If there is only one revision (the creation), undo means deleting the entry.

**Undo delete:** Clear `deleted_at` on the entry. The entry reappears with its full revision history intact.

### Revision count

| Scenario | Revision count |
|----------|---------------|
| Created, never changed | 1 |
| Created, edited once | 2 |
| Created, edited 3 times | 4 |
| Created, deleted | 4 (no new revision, just soft-deleted) |
| Created, deleted, undone | 4 (deleted_at cleared) |

### What a revision stores

Each revision is a full snapshot, not a diff. This keeps undo simple — restore the snapshot, no need to compute or apply deltas.

Fields captured: `date`, `item_text`, `sort_order`.

### Undo mechanics

The undo target is **the last action globally**, not a specific entry. The server tracks the most recent mutation (entry ID + action type). `POST /undo` reverses it:

- **Last action was an edit:** restore previous revision, remove latest revision.
- **Last action was a delete:** clear `deleted_at` on the entry.
- **Last action was a create:** delete the entry and its single revision.

The UI shows a transient toast after any mutation: "Entry edited — Undo" or "Entry deleted — Undo". Click it (or Ctrl+Z) before it fades to trigger the undo.

### Retention

Revisions are kept indefinitely. Manual compaction is available via the maintenance page (see below). No automated pruning — the user decides when to clean up.

### What is NOT in scope

- **Branching undo** (vim-style undo tree): Not needed for a single-user app. Linear undo is sufficient.
- **Rebuilding state from revisions**: The `entries` table is the source of truth. Revisions are a safety net, not the primary data store.
- **Automated retention policy**: Keep everything. Compact manually when desired.
- **Redo**: Not planned for v1. Could be added by keeping undone revisions marked rather than deleting them.

## Maintenance page

A `/maintenance` page provides visibility into data health and manual controls.

### Stats dashboard

| Metric | Description |
|--------|-------------|
| Live entries | Count of entries where `deleted_at IS NULL` |
| Soft-deleted entries | Count of entries where `deleted_at IS NOT NULL` |
| Total revisions | Count of all rows in `entry_revisions` |
| Revisions per entry (avg) | Total revisions / live entries |
| Database size | File size of `worklog.db` |
| Backups | Count of files in `backups/`, timestamp of most recent |
| Contacts | Count of contacts |

### Actions

- **Compact revisions** — for each entry, keep only the N most recent revisions (configurable, default: keep latest only). Removes historical snapshots older than a chosen threshold.
- **Purge deleted** — permanently delete all soft-deleted entries and their revisions. Irreversible.
- **Backup now** — trigger `backup-worklog.sh` and report the result.
- **Export markdown** — same as the current `/export` route, but also accessible here.

### Safety

- Compact and purge actions require confirmation ("This will permanently remove N revisions. Continue?").
- A backup is automatically triggered before any compact or purge operation.

## UI

### Undo toast

After any mutation (edit, delete, create), a toast notification appears at the bottom of the page:

```
┌──────────────────────────────────┐
│  Entry updated.  [Undo]         │
└──────────────────────────────────┘
```

- Fades after 8 seconds.
- Clicking "Undo" calls `POST /undo` and reloads the page.
- Only one undo level is available at a time (the most recent action).

### Ctrl+Z

When no textarea is focused, Ctrl+Z triggers the same undo as clicking the toast button.

## Implementation order

1. Add `deleted_at` column to `entries` table in `init_db`. Update all read queries to filter `WHERE deleted_at IS NULL`.
2. Add `entry_revisions` table to `init_db` schema.
3. Write `append_revision(conn, entry_id, date, item_text, sort_order)` in `db.rs`.
4. Modify `delete_entry` to soft-delete (set `deleted_at`) instead of `DELETE`.
5. Call `append_revision` from every content mutation path: `insert_entry`, `update_entry`, `update_sort_order`.
6. Add server-side "last action" tracking (in `AppState`, behind the mutex).
7. Write `undo_last_action` logic — dispatch on action type (edit/delete/create).
8. Add `POST /undo` route.
9. Add undo toast UI with Ctrl+Z support.
10. Add `/maintenance` page with stats dashboard and compact/purge actions.
11. Tests: unit tests for revision CRUD and undo, Playwright tests for undo flow and maintenance page.

## Prior art

| System | Pattern | Notes |
|--------|---------|-------|
| Git | Content-addressable DAG | Immutable objects, reflog as safety net |
| SVN | Append-only revision files | Numbered, immutable revisions |
| Google Docs | Operation log + OT | Ops are the source of truth |
| Notion | Snapshot + ops log | Periodic snapshots, ops between them |
| Vim/Neovim | Persistent undo tree | Branching tree, no change ever lost |
| MediaWiki | Revision table | Full snapshot per edit, linear history |
| Twitter/X | Immutable version chain | New ID per edit, linked via edit_history |

This design is closest to **MediaWiki's revision table** — full snapshots, linear history, current state in a separate table for fast reads — combined with **Gmail's undo send** pattern for the toast-based undo UI.
