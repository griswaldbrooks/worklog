---
paths:
  - "src/**/*.rs"
  - "Cargo.toml"
  - "Cargo.lock"
---

# Rust Style

## Formatting

- Use `cargo fmt` — do not manually format.
- Use `cargo clippy` for linting.

## Error Handling

- Use `thiserror` for library error types, `anyhow` for application-level errors.
- Prefer `?` operator over `.unwrap()` in production code. `.unwrap()` is fine in tests.
- Return `Result` from functions that can fail — don't panic in library code.

## Naming

- Follow standard Rust conventions: `snake_case` for functions/variables, `PascalCase` for types/traits, `SCREAMING_SNAKE_CASE` for constants.

## Dependencies

- Keep `Cargo.toml` dependencies sorted alphabetically.
- Use workspace features when appropriate.
- Pin major versions (e.g., `axum = "0.8"` not `axum = "*"`).

## Documentation

- Document public API items with `///` doc comments.
- No ticket numbers or bead IDs in code comments.
- Document WHY, not WHAT.
