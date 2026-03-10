---
paths:
  - "src/**/*.rs"
  - "tests/**/*.rs"
---

# Rust Testing Rules

## Running Tests

- `cargo test` — run all tests
- `cargo test <test_name>` — run specific test
- `cargo test -- --nocapture` — show println output

## Naming

Use `snake_case` for test function names. Be descriptive: `test_parse_empty_worklog` not `test1`.

## Organization

- Unit tests go in a `#[cfg(test)] mod tests` block at the bottom of each source file.
- Integration tests go in `tests/` directory.

## Test Ordering

Order tests from simplest to most complex: error cases first, then trivial cases, then complex scenarios.

## Assertions

- Use `assert_eq!` for equality checks with clear expected/actual values.
- Use `assert!(result.is_ok())` or `assert!(result.is_err())` for Result checks.
- Include descriptive messages: `assert_eq!(result, expected, "parsing week header failed")`.

## TDD Workflow

1. **Red**: Write a failing test first
2. **Green**: Write the minimum code to make it pass
3. **Refactor**: Clean up while keeping tests green

Run `cargo test` after each step to verify.
