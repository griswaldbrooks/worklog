---
name: restart-server
description: "Kill the running work-log server and restart it with cargo run. Use when the server needs restarting after code changes, or when asked to restart/reload the server."
---

# Restart Server

## Task

Kill any running instance of the worklog server and restart it.

### Step 1: Kill existing server

```bash
pkill -f "target/debug/worklog-server" 2>/dev/null || true
```

### Step 2: Wait for port to free

```bash
sleep 1
```

### Step 3: Build and start server

Run in background so the skill can verify startup:

```bash
cd /home/griswald/picknik/worklog && cargo run &
```

### Step 4: Verify

Wait for the server to start, then check it is responding:

```bash
sleep 3 && curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3030
```

Report whether the server restarted successfully (HTTP 200) or failed.
