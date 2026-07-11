import { execFile } from "node:child_process";
import { mkdir, readFile, writeFile, appendFile } from "node:fs/promises";
import { dirname, join, relative } from "node:path";
import { promisify } from "node:util";

import type { ExtensionAPI, ExtensionCommandContext } from "@earendil-works/pi-coding-agent";

const execFileAsync = promisify(execFile);
const STATE_FILE = ".pi/worklog-checkpoint.json";
const WORK_LOG = "docs/work-log.md";

type Usage = {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  total: number;
};

type Checkpoint = {
  task: string;
  startedAtUtc: string;
  startedAtWarsaw: string;
  usage: Usage;
};

function usageSnapshot(ctx: ExtensionCommandContext): Usage {
  const usage = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 };
  for (const entry of ctx.sessionManager.getBranch()) {
    if (entry.type !== "message" || entry.message.role !== "assistant") continue;
    const messageUsage = entry.message.usage;
    if (!messageUsage) continue;
    usage.input += messageUsage.input ?? 0;
    usage.output += messageUsage.output ?? 0;
    usage.cacheRead += messageUsage.cacheRead ?? 0;
    usage.cacheWrite += messageUsage.cacheWrite ?? 0;
  }
  usage.total = usage.input + usage.output + usage.cacheRead + usage.cacheWrite;
  return usage;
}

function timestamp(now = new Date()) {
  const utc = now.toISOString().replace("T", " ").replace(/\.\d{3}Z$/, " UTC");
  const parts = new Intl.DateTimeFormat("en-CA", {
    timeZone: "Europe/Warsaw",
    year: "numeric", month: "2-digit", day: "2-digit",
    hour: "2-digit", minute: "2-digit", second: "2-digit",
    hourCycle: "h23", timeZoneName: "short",
  }).formatToParts(now);
  const value = (type: string) => parts.find((part) => part.type === type)?.value ?? "?";
  return {
    utc,
    warsaw: `${value("year")}-${value("month")}-${value("day")} ${value("hour")}:${value("minute")}:${value("second")} ${value("timeZoneName")} (Warsaw)`,
  };
}

async function runGit(cwd: string, args: string[]) {
  return (await execFileAsync("git", args, { cwd })).stdout.trim();
}

function formatStats(numstat: string) {
  return numstat.split("\n").filter(Boolean).map((line) => {
    const [added = "0", removed = "0", ...name] = line.split("\t");
    return `| \`${name.join("\t")}\` | ${added} | ${removed} | Changed in implementation commit; replace with concise summary before committing. |`;
  });
}

export default function (pi: ExtensionAPI) {
  pi.registerCommand("worklog-start", {
    description: "Snapshot Pi token usage before a milestone. Usage: /worklog-start <task summary>",
    handler: async (args, ctx) => {
      const task = args.trim();
      if (!task) {
        ctx.ui.notify("Usage: /worklog-start <task summary>", "error");
        return;
      }
      const time = timestamp();
      const checkpoint: Checkpoint = {
        task,
        startedAtUtc: time.utc,
        startedAtWarsaw: time.warsaw,
        usage: usageSnapshot(ctx),
      };
      const path = join(ctx.cwd, STATE_FILE);
      await mkdir(dirname(path), { recursive: true });
      await writeFile(path, `${JSON.stringify(checkpoint, null, 2)}\n`);
      ctx.ui.notify(`Work-log checkpoint started (${checkpoint.usage.total.toLocaleString()} tokens).`, "info");
    },
  });

  pi.registerCommand("worklog-finish", {
    description: "Append a work-log entry after committing. Usage: /worklog-finish <commit SHA>",
    handler: async (args, ctx) => {
      const sha = args.trim();
      if (!sha) {
        ctx.ui.notify("Usage: /worklog-finish <implementation commit SHA>", "error");
        return;
      }
      const checkpointPath = join(ctx.cwd, STATE_FILE);
      let checkpoint: Checkpoint;
      try {
        checkpoint = JSON.parse(await readFile(checkpointPath, "utf8")) as Checkpoint;
      } catch {
        ctx.ui.notify("No checkpoint found. Run /worklog-start first.", "error");
        return;
      }

      let resolvedSha: string;
      let subject: string;
      let numstat: string;
      try {
        resolvedSha = await runGit(ctx.cwd, ["rev-parse", "--short", sha]);
        subject = await runGit(ctx.cwd, ["show", "-s", "--format=%s", resolvedSha]);
        numstat = await runGit(ctx.cwd, ["show", "--format=", "--numstat", resolvedSha]);
      } catch (error) {
        ctx.ui.notify(`Cannot inspect commit ${sha}: ${String(error)}`, "error");
        return;
      }

      const completed = timestamp();
      const after = usageSnapshot(ctx);
      const delta: Usage = {
        input: Math.max(0, after.input - checkpoint.usage.input),
        output: Math.max(0, after.output - checkpoint.usage.output),
        cacheRead: Math.max(0, after.cacheRead - checkpoint.usage.cacheRead),
        cacheWrite: Math.max(0, after.cacheWrite - checkpoint.usage.cacheWrite),
        total: Math.max(0, after.total - checkpoint.usage.total),
      };
      const rows = formatStats(numstat);
      const entry = [
        `## ${completed.utc} / ${completed.warsaw}`,
        "",
        `- **Commit:** \`${resolvedSha}\` — \`${subject}\``,
        `- **Task:** ${checkpoint.task}`,
        `- **Started:** ${checkpoint.startedAtUtc} / ${checkpoint.startedAtWarsaw}`,
        `- **Completed:** ${completed.utc} / ${completed.warsaw}`,
        `- **Model-token delta:** ~${delta.total.toLocaleString()} (input: ~${delta.input.toLocaleString()}, output: ~${delta.output.toLocaleString()}, cache read: ~${delta.cacheRead.toLocaleString()}, cache write: ~${delta.cacheWrite.toLocaleString()})`,
        "",
        `### File changes (\`${resolvedSha}\`)`,
        "",
        "| File | Added | Removed | Summary |",
        "| --- | ---: | ---: | --- |",
        ...(rows.length ? rows : ["| _No file changes_ | 0 | 0 | — |"]),
        "",
      ].join("\n");
      const workLogPath = join(ctx.cwd, WORK_LOG);
      await mkdir(dirname(workLogPath), { recursive: true });
      await appendFile(workLogPath, `\n${entry}`);
      ctx.ui.notify(`Appended ${relative(ctx.cwd, workLogPath)} for ${resolvedSha}. Commit the work-log entry next.`, "info");
    },
  });
}
