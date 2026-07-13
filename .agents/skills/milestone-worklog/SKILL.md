---
name: milestone-worklog
description: Tracks completed implementation milestones in docs/work-log.md with task summary, UTC/Warsaw timestamps, implementation commit SHA, token delta, validation, and per-file added/removed-line statistics. Use whenever completing a meaningful coding, documentation, release, or reliability task in this repository.
---

# Milestone Work Log

Use this workflow for a meaningful completed milestone. Do not create entries for exploration, questions, failed attempts, or formatting-only changes.

## Start a milestone

1. State a concise task summary and expected validation.
2. Start Pi token accounting before implementation:

   ```text
   /worklog-start <task summary>
   ```

   If the command is unavailable, continue the task and record token delta as unavailable. Do not invent token figures.

3. Keep unrelated working-tree changes untouched.

## Complete a milestone

1. Run relevant tests or validation and report the exact command/result.
2. Review the diff and commit the implementation changes.
3. Run:

   ```text
   /worklog-finish <implementation-commit-sha>
   ```

   This project-local Pi extension appends the required `docs/work-log.md` entry using real session-usage deltas and `git show --numstat` file statistics.

4. Review the generated work-log entry. Ensure its summary describes the implementation commit, not the subsequent work-log commit.
5. Commit the work-log entry separately.

## Required entry contents

Every entry must contain:

- task summary;
- start and completion timestamps in UTC and Warsaw time;
- implementation commit SHA and subject;
- token delta split into input, output, cache read, and cache write when available;
- exact validation command and outcome;
- a per-file table: filename, added lines, removed lines, concise summary.

## Fallback when the extension is unavailable

Use only measured values:

```bash
git rev-parse --short HEAD
git show --format='' --numstat <implementation-sha>
date -u '+%Y-%m-%d %H:%M:%S UTC'
TZ=Europe/Warsaw date '+%Y-%m-%d %H:%M:%S %Z (%z)'
```

Write `Model-token delta: unavailable` when no before checkpoint exists.

## Guardrails

- Never invent commit SHAs, timestamps, test results, token counts, or line statistics.
- Never include unrelated files in the file-change table.
- Do not overwrite existing work-log history.
- Do not amend the implementation commit to include the work-log unless the user explicitly requests it.
