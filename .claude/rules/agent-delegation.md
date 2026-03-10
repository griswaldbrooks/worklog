# Agent Delegation

Only use agents when the user explicitly requests them.

When the user generically requests agents (e.g., "Use agents.") without naming specific ones, run the full pipeline: `coder` → `code-reviewer` → `test-runner`. If the user explicitly asks for particular agent(s) (e.g., "Run the test-runner"), delegate only to those requested. Never present coder output to the user without review and build verification.

| Agent | Use For |
|-------|---------|
| `coder` | Writing/implementing code, refactoring |
| `code-reviewer` | Reviewing code before commits |
| `test-runner` | Building and running tests |

**Don't delegate:** simple questions, quick file reads, git operations, or any task where the user hasn't asked for agents.
