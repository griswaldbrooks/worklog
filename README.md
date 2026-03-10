# worklog

Personal worklog for tracking daily activities, meetings, mentoring, and project progress at PickNik.

Rust web server (Axum) with SQLite backend and a local web UI.

## Setup

```bash
cp .env.example .env
# Edit .env to set WORKLOG_DATA_DIR
cargo run
# Open http://127.0.0.1:3030
```

## Tests

```bash
cargo test              # 56 Rust unit tests
npx playwright test     # 8 e2e visual regression tests
```
