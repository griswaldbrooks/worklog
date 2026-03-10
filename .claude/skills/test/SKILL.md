---
name: test
description: "Build and run tests for the Rust project"
---

# Test

## Context

- Current branch: !`git branch --show-current`
- Changed files: !`git diff --name-only HEAD`

## Task

Build the project and run its tests.

### Step 1: Build

```bash
cargo build 2>&1
```

### Step 2: Run tests

```bash
cargo test 2>&1
```

For a specific test:
```bash
cargo test <test_name> -- --nocapture 2>&1
```

### Step 3: Lint

```bash
cargo clippy 2>&1
```

Report: total tests, passed, failed, any warnings or clippy issues.
