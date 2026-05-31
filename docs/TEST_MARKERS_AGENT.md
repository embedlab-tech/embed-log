# Test: Agent discovers and reports all markers via CLI

## Objective

Verify a fresh agent can discover the skill system, load the sessions skill, and use the CLI to find and print all markers from recorded sessions — without reading raw files.

## Prompt to use

> Find and print all markers from our embed-log sessions in the logs directory. Use only the CLI (`embed-log sessions`) — do not read markers.json files directly.

## Expected behavior

1. Agent reads `AGENTS.md` → discovers the Skill system section
2. Agent runs `embed-log skill list` → sees `sessions` skill
3. Agent loads the skill via `read skill://sessions` or `embed-log skill show sessions`
4. Agent reads the "Agent usage" section → learns to prefer CLI over file reads
5. Agent reads the "Cross-session queries" section → finds the marker discovery workflow
6. Agent runs:
   ```
   embed-log sessions list --json | python3 -c "import json, sys; [print(s['session_id'], s['_alias'], s['markers']) for s in json.load(sys.stdin) if s['markers'] > 0]"
   ```
7. For each session with markers, agent runs:
   ```
   embed-log sessions marker list <session-id>
   ```
8. Agent prints all markers to the user

## Pass criteria

- Agent never reads `markers.json`, `manifest.json`, or `.log` files directly
- Agent uses `embed-log sessions list --json` for session discovery
- Agent uses `embed-log sessions marker list <session-id>` for per-session marker listing
- All 14 sessions with markers are reported (46 total markers)
- No raw file reads via `read` tool or shell `cat`/`grep`/etc.

## Fail criteria

- Agent reads `markers.json` files directly from disk
- Agent reads `manifest.json` to check marker counts
- Agent globs log directories instead of using the CLI
- Agent misses sessions that have markers
