# Sessions CLI Improvements Plan

## Goal

Turn `embed-log sessions` from a browsing/export CLI into a practical investigation tool, while fixing correctness gaps in docs and command consistency.

---

## Principles

1. **Search should extend existing commands first**
   - Prefer `sessions logs --grep ...` over inventing `sessions grep`
   - Prefer `sessions list --search ...` over adding a separate search tree

2. **Behavior must be consistent**
   - Shared flags like `--json` should either work everywhere they appear or be removed from commands that ignore them

3. **Low-risk progression**
   - Fix correctness/docs first
   - Add intra-session search next
   - Add cross-session narrowing after that

---

## Phase 1 — Correctness and CLI consistency

### 1.1 Fix sessions skill/doc inaccuracies
Update the sessions skill and any README references to reflect actual behavior.

#### Required fixes
- `--json` support:
  - document that it works for:
    - `sessions list`
    - `sessions info`
    - `sessions snippet list`
  - document that other subcommands currently accept it but ignore it
- `export`:
  - document `--missing` only works with `--format html`
  - document valid time filter combinations:
    - `--after + --before` valid
    - `--first` and `--last` mutually exclusive
    - reject conflicting 3+ time filters
  - document `--first-log-at` only affects HTML export
  - document raw export does not update `html_status`
- `open`:
  - document that it fails if `session.html` does not exist
  - tell user to run `sessions export <id>` first
- `snippet show`:
  - document default = most recent snippet
  - document matching behavior:
    - substring / suffix match
    - multiple matches fail and require disambiguation
  - mention `--last` is currently redundant / no-op semantically
- `delete`:
  - fix wording around `--yes`
  - it is not required; it only skips confirmation

### 1.2 Clean up `--json` exposure
Pick one of these approaches:

#### Recommended
Remove shared `--json` from commands that do not use it:
- `logs`
- `open`
- `delete`
- `marker list`
- `marker show`
- `snippet show`
- `snippet delete`
- `export` if not implementing JSON there immediately

#### Alternative
Keep `--json` shared and implement JSON output consistently across all subcommands.

**Recommendation:** remove it from commands that don’t support it yet. Less surprising.

### 1.3 Decide what to do with `snippet --last`
Two options:
- keep it and explicitly branch on it
- remove it from parser/help because default already means “last”

**Recommendation:** keep it for readability, but actually honor it explicitly in code so docs are honest.

### Acceptance
- Help text matches real behavior
- Sessions skill is accurate
- No subcommand advertises behavior it does not implement

---

## Phase 2 — Make `sessions logs` useful for investigation

### 2.1 Add text filtering
Extend `sessions logs` with:

- `--grep TEXT`
- `--regex`
- `--ignore-case`

### Behavior
- default: plain substring match
- `--regex`: use Python regex
- `--ignore-case`: applies to both substring and regex modes
- filtering happens after pane selection, before output

### 2.2 Add bounded output
Add:

- `--tail N`
- `--head N`

Optional:
- reject using both together, or define precedence

**Recommendation:** reject both together.

### 2.3 Add context output
Add:

- `--context N`

Useful with grep:
- prints matching lines plus N surrounding lines
- if no grep is specified, reject `--context`

### 2.4 Add time filtering to `logs`
Mirror the useful subset from export:
- `--after`
- `--before`
- maybe `--first` and `--last`

**Recommendation:** implement `--after` and `--before` first only.  
Do not copy every export time flag unless needed.

### 2.5 Output modes
Keep plain text as default.
Optional future improvement:
- `--json` for structured matching output:
  - pane
  - timestamp
  - line
  - line number if derivable

### Acceptance
Examples that should work:

```bash
embed-log sessions logs 31f0 --grep "timeout"
embed-log sessions logs 31f0 --grep "error" --ignore-case --tail 50
embed-log sessions logs 31f0 --regex "temp=.*[5-9][0-9]" --pane SENSOR_A
embed-log sessions logs 31f0 --grep "panic" --context 3
embed-log sessions logs 31f0 --after 5m --before 1m --grep "reset"
```

### Tests
Add coverage for:
- substring search
- regex search
- ignore-case
- pane filter + grep combination
- tail/head boundaries
- context output
- invalid combinations

---

## Phase 3 — Make `sessions list` useful for narrowing the candidate set

### 3.1 Add metadata search
Add:

- `--search TEXT`

Search fields:
- session id
- alias
- app name
- job id
- config path

### 3.2 Add simple state filters
Add:

- `--with-markers`
- `--no-html`
- `--html-ready`
- `--app NAME`

### 3.3 Add time filters
Add:

- `--after ISO_OR_DATE`
- `--before ISO_OR_DATE`

Apply against session `started_at`.

### 3.4 Keep sorting and limit
Existing:
- `--sort`
- `--limit`

Ensure filters apply before sorting/limit.

### Acceptance
Examples:

```bash
embed-log sessions list --search build-123
embed-log sessions list --with-markers
embed-log sessions list --no-html
embed-log sessions list --app demo --limit 10
embed-log sessions list --after 2026-05-01 --before 2026-05-30
```

### Tests
Add coverage for:
- free-text match
- app match
- markers-only filter
- no-html filter
- date range filtering
- limit after filtering

---

## Phase 4 — Improve marker and snippet discoverability

### 4.1 Marker search
Add to `sessions marker list`:
- `--search TEXT`
- `--pane NAME`

This enables:

```bash
embed-log sessions marker list 31f0 --search timeout
embed-log sessions marker list 31f0 --pane SENSOR_A
```

### 4.2 Snippet search
Optional, lower priority:
- `sessions snippet list --search TEXT`
- search in label / filename

### Acceptance
Marker descriptions can be filtered without opening HTML.

---

## Phase 5 — Optional JSON normalization

Only do this after Phases 2–4 if needed.

### Goal
Make JSON output consistent across searchable commands:
- `sessions list --json`
- `sessions info --json`
- `sessions logs --json`
- `sessions marker list --json`
- `sessions marker show --json`
- `sessions snippet list --json`
- maybe `sessions export --json` returning export metadata

### Recommendation
Do **not** start here. It expands scope a lot without solving the main user need.

---

## Phase 6 — Docs and skill updates

### Update
- `docs/SESSIONS_CLI_SKILL.md`
- README CLI reference
- maybe `docs/SAMPLE_COMMANDS.md`

### Add examples for search workflows
Examples to include:
- find sessions with markers but no HTML
- search one session for an error string
- filter logs to one pane and one time range
- search markers by description

---

## Phase 7 — Verification

### Automated
- backend unit tests for all new parser flags and behavior
- tests for invalid flag combinations
- tests for date/duration parsing edge cases
- ensure existing backend tests still pass

### Manual smoke checks
- `embed-log --help`
- `embed-log sessions --help`
- `embed-log sessions list --search ...`
- `embed-log sessions logs ... --grep ...`
- `embed-log sessions marker list ... --search ...`

---

## Recommended implementation order

1. **Phase 1** — correctness/docs cleanup
2. **Phase 2** — `sessions logs` grep/search
3. **Phase 3** — `sessions list` filters
4. **Phase 4** — marker search
5. **Phase 6** — final docs pass
6. **Phase 7** — verification

---

## Recommended MVP scope

If you want the smallest high-value batch first, do exactly this:

### MVP
- fix sessions docs/skill correctness
- remove misleading `--json` from unsupported commands
- add `sessions logs`:
  - `--grep`
  - `--regex`
  - `--ignore-case`
  - `--tail`
  - `--context`
- add `sessions list`:
  - `--search`
  - `--with-markers`
  - `--no-html`
  - `--app`

That will solve most real debugging use cases without overcomplicating the CLI.
