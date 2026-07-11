# Project Pi extensions

## `worklog-checkpoint.ts`

Automates milestone token accounting and work-log scaffolding.

```text
/worklog-start Add serial diagnostics
# implement, test, and commit the implementation
/worklog-finish <implementation-commit-sha>
# review and commit docs/work-log.md
```

`/worklog-start` records the current session's cumulative assistant usage and UTC/Warsaw start timestamps in `.pi/worklog-checkpoint.json` (ignored by Git).

`/worklog-finish` verifies the implementation commit, calculates the token delta from the checkpoint, obtains `git show --numstat` file statistics, and appends a structured entry to `docs/work-log.md`. It does not commit the work-log entry automatically.

The extension is project-local and loads after this project is trusted by Pi. Reload it with `/reload` after changing the extension.
