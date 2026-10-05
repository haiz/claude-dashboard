# Handoff — Windows port, resume at Sub-project 5 (Command Log)

**Written:** 2026-10-05. **Branch:** `feat/windows-app` (head `54b6ad0`, 48 commits ahead of `main`, working tree clean). **Platform:** Windows 11, this machine. User language: **Vietnamese** (reply in Vietnamese).

This file is the single source of truth for picking the work back up. Read it, then write the Sub-project 5 plan and execute it the same way SP1–SP4 were done.

---

## 1. Where the project stands

Goal: a Windows 11 port of the macOS "Claude Dashboard", at feature parity, Rust + Slint, reusing `apps/linux/core`. Overall design spec: `docs/superpowers/specs/2026-10-04-windows-app-design.md` (6 sub-projects). Done so far, all on `feat/windows-app`, each with its own plan + SDD ledger under `.superpowers/sdd/<plan-name>/progress.md`:

- **SP1 — Core on Windows + bridge + extension** (plan `2026-10-04-windows-core-bridge.md`). Done inline (Sonnet was rate-limited that day). core builds/tests on Windows (APPDATA/LOCALAPPDATA paths, DPAPI at-rest via `userprotect.rs`, `cookie/win.rs` v10 decode / v20→AppBound, `discover_windows_profiles_under`, `store::lock_store`, `core::key_intake`). `apps/windows/bridge` native-messaging host + `apps/windows/extension` MV3 (fixed id `cadpjcfajhlgdaipepkdojcehfmkkedh`). `contract/windows.md`, `.github/workflows/ci.yml`.
- **SP2 — App shell (Slint)** (plan `2026-10-04-windows-app-shell.md`). `core::{colors,geometry,format,rows}` ported from `apps/linux/lib/`. `apps/windows/app`: Mica window + system theme, sidebar, Dashboard card grid with ring gauges, per-account page, tray ring + Mica flyout popover, refresh loop, single-instance mutex, reload-pipe consumer, tray-resident (close hides). Controller screenshot-verified Dashboard + empty state.
- **SP3 — Setup & Settings** (plan `2026-10-04-windows-setup-settings.md`). `core::{settings,scan,startup}`. Add Account wizard (extension/scan/paste), Settings›Accounts (delete+mute/re-sync/unmute), Settings›General (About+AboutSlint, live Auto Refresh, Launch-at-startup via HKCU Run, Updates placeholder).
- **SP4 — Charts** (plan `2026-10-04-windows-charts.md`). `UsageLogStore::series`/`series_all`, `core::chart` (ported from `lib/chart.js`), app `chart_model.rs`, per-account interactive chart (`ui/chart.slint`/`src/chart.rs`) + Overview multi-account chart. "View chart" button + sidebar "Overview" enabled.

Each sub-project's final whole-branch review (opus) returned **PASS, no merge-blockers**.

### NOT done / caveats (tell the user, don't paper over)
- **Nothing is merged and CI has NEVER actually run.** The branch is 48 commits of Windows-only work. SP1's **Linux** side (helper integration tests, Linux build) is only *reasoned* correct — the `linux` CI job has not been executed. Strongly recommend the user push the branch / open a PR to get CI (both `linux` and `windows` jobs) green before or alongside more sub-projects.
- **GUI screenshot coverage is partial.** Controller verified the Dashboard grid + empty state (SP2). The Settings panes, Add Account wizard, and both charts are **code-reviewed + unit-tested but not visually run-checked** — they need a human/controller run-check (they require clicking to reach).
- A backlog of **deferred minors** is recorded in each `progress.md` (search `minor (deferred)`), to be triaged by `superpowers:finishing-a-development-branch` before merge. None are merge-blockers.

---

## 2. How the work is run (the loop that produced SP2–SP4)

Process skills: `superpowers:brainstorming` (already done at the architecture level — the spec is approved; a new sub-project does NOT re-brainstorm, it goes straight to a plan) → `superpowers:writing-plans` → `superpowers:subagent-driven-development`.

For each sub-project:
1. **Write the plan** to `docs/superpowers/plans/2026-10-05-windows-command-log.md` (header + Global Constraints + Review Focus + file structure + bite-size tasks). `docs/superpowers/` is **gitignored** — commit plans with `git add -f`. Present it to the user for approval before executing.
2. **Set up SDD**: `bash <skill>/scripts/sdd-workspace <plan>` → ledger dir; write `progress.md` with the plan path as line 1, a pre-flight conflict-scan table, and any rulings. (`<skill>` = `C:/Users/cthai/.claude/plugins/cache/claude-plugins-official/superpowers/6.4.1/skills/subagent-driven-development`.)
3. **Per task**: record BASE (`git rev-parse HEAD`); `bash <skill>/scripts/task-brief <plan> N`; dispatch an implementer subagent (**model: sonnet**, `general-purpose`) with the brief path + interfaces + the report-file path; on DONE, `bash <skill>/scripts/review-package <plan> BASE HEAD` and dispatch a reviewer subagent (**sonnet**) with brief+report+diff paths; run the fix loop (resume the same implementer via `SendMessage to: <agentId>`) until the review is clean; append a `Task N: complete` line to the ledger.
4. **Final whole-branch review** on **opus** over the sub-project's commit range, pointed at the ledger's deferred-minors for triage.
5. Update `CLAUDE.md` + `contract/` in the last task. Do **not** merge — that's the user's call.

Commit author identity must be passed explicitly (git user is unset on this machine):
`git -c user.email="backend@gotitapp.co" -c user.name="cthai" commit -m "..."`.
Commit-message trailer (current attribution): `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` on its own line. **Re-read the session's attribution system-reminder each session — the model name in the trailer changes.**

### Dispatch hygiene that mattered
- Implementer + reviewer contracts: **no nested subagents**; implementer edits only the task's files; reviewer is read-only. Always name the exact model.
- Keep diffs/reports as files (the scripts write them under the ledger dir); don't paste big blobs into prompts.
- Pure logic → TDD (failing test first). GUI → build + run-check; only the **named pure helpers** are unit-tested (this is a recorded ruling in every GUI sub-project).

---

## 3. Environment specifics (this Windows machine)

- **cargo** is via rustup but not always on PATH in the Bash tool. In PowerShell: `$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"` then `cd` to the crate. Toolchain **1.98.0** is pinned (`rust-toolchain.toml`) and auto-activates; workspaces declare `rust-version = 1.89` (because `store::lock_store` uses `std::fs::File::lock`, stable 1.89 — clippy's incompatible-msrv enforces it).
- Build/test commands (also in CLAUDE.md): core `cd apps/linux && cargo test -p claude-dashboard-core`; windows workspace `cd apps/windows && cargo test --workspace`; app only `cargo test -p claude-dashboard`; extension `cd apps/windows/extension && node --test`.
- **Run the app**: `cd apps/windows && cargo run -p claude-dashboard`. Env flags: `CLAUDE_DASHBOARD_SMOKE=1` makes it self-quit after ~1.2s (use in every GUI smoke check so the event loop can't hang a subagent); `CLAUDE_DASHBOARD_FAKE_ROWS=1` seeds 3 sample accounts (alice/bob/carol) with no network — and SP4 also seeds usage-log history for f2/f3 so charts draw (alice left empty for the "No data yet" state).
- **Controller screenshot recipe** (to visually verify a running GUI): build, `Start-Process` the exe with the env flags, `Start-Sleep -Seconds 4`, capture with `System.Windows.Forms`/`System.Drawing` `CopyFromScreen` to a PNG in the scratchpad, `Stop-Process -Force`, then Read the PNG. Panes that need a click (Settings, wizard, charts) can't be reached this way without a dev flag — note that limitation rather than claim a visual check you didn't do.
- Git warns `LF will be replaced by CRLF` constantly — harmless.

---

## 4. Recurring gotchas (cost fix rounds in SP2–SP4 — avoid them)

1. **Enabling a sidebar row is the #1 repeat defect.** Three times an implementer reported a `SidebarRow` enabled+routed while a `perl`/`sed` edit had silently not matched, leaving the pane unreachable (two fix rounds in SP3). For SP5's "Command Log" row (currently `app.slint:358-360`, `enabled: false`), **require the implementer to read the 4 edited lines back and paste them** in the report, and the reviewer to confirm in the diff. Pattern to apply (mirror the Accounts/General/Overview rows): remove `enabled: false`, add `selected: root.selection == Pane.command-log;` and `clicked => { root.selection = Pane.command-log; }`. (`Pane.command-log` already exists.)
2. **GUI implementers edit `.slint` with the Edit tool, not perl/sed** — the shell edits miss silently.
3. A `Set-Content` in PowerShell adds a **UTF-8 BOM** — it crept into `main.rs` once. Edit `.rs`/`.md` with the Edit tool or bash, never `Set-Content`. Verify `head -c 3`.
4. **Store writes** (accounts.json / extension-sources.json) hold `core::store::lock_store()` across the whole read-modify-write, shared with the bridge; a delete-then-mute holds **one** lock across both writes. Reads are lock-free.
5. **`core::format`/`core::chart::format_tick` are timezone-naive (UTC)** — the GUI caller shifts by the local offset (see `model.rs` `local_offset_s` / `GetTimeZoneInformation`). A single offset is fine (slightly off only across a DST boundary — documented).
6. Remove any temporary `#![allow(dead_code)]` once the UI wires a glue module (bit SP3/SP4).
7. The **session key must never** appear in a log, UI string, `AddOutcome`, or error.
8. GUI long work runs off the UI thread (`std::thread::spawn` + `slint::invoke_from_event_loop`); any poll/timer is stopped on close so the smoke self-quit still fires.

---

## 5. Sub-project 5 — Command Log (what to plan + build)

**Spec reference:** the SP5 section of `docs/superpowers/specs/2026-10-04-windows-app-design.md`. **Out of scope:** release/installer/auto-update (that's SP6, the last one).

**Port targets (macOS → Windows), all present under `apps/macos/ClaudeDashboard/`:**
- `Services/CommandRunner.swift` — the one place commands launch. macOS runs `/bin/zsh -lc`, sources `~/.zshrc`, enforces a timeout, kills the whole process tree on timeout/cancel, captures a bounded output tail, records one row to the log. **Windows:** run the user's chosen shell with its profile; put every child in a **Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`** so cancel/timeout kills the whole tree cleanly (simpler than macOS's `ProcessTree` walk).
- `Services/CommandClassifier.swift` + a `CommandResolver` — decides interactive vs non-interactive. macOS expands the leading token via the shell. **Windows resolver:** `Get-Command` (PowerShell) or `type` (bash). The pure classification logic should port to a tested `core` or app module.
- `Services/TerminalLauncher.swift` — macOS uses `osascript`/Terminal. **Windows:** launch interactive commands in **Windows Terminal** (`wt.exe new-tab …`), fall back to `conhost`.
- `Services/RunningProcessRegistry.swift`, `Services/ProcessTree.swift` — process tracking/kill. On Windows most of this collapses into the Job Object.
- `Services/CommandLogStore.swift` + `Models/CommandLogModels.swift` — the run-log (time, account, trigger, exit code, duration, output tail), SQLite. `CommandTrigger` enum: `manual` / `autoReset` (a usage window reset detected) / `autoEmpty` (a refresh produced no 5h/7d usage). Consider a `core` store like `UsageLogStore`, or an app-local SQLite.
- `Models/RunCommandSettings.swift` — per-account saved run command (keyed by account id) + interactive override. Lives in settings; integrate with `core::settings` or a sibling store.
- `Services/ClaudeCodeAccountDetector.swift` — reads `~/.claude.json` `oauthAccount.emailAddress` for the active-Claude-Code account (the sort tier + card badge). **Windows:** `%USERPROFILE%\.claude.json`.
- `Views/CommandLogView.swift`, `Views/RunCommandSheet.swift` — the log table UI and the per-account "run command" editor/runner.
- **Shell selection** (new Settings›General control, or in the Command Log UI): PowerShell 7 if present, else Windows PowerShell; or cmd; or Git Bash — started with the user's profile so functions/aliases resolve (the macOS `source ~/.zshrc` analogue).
- **Auto triggers:** the refresh loop already detects resets/empty; wire `autoReset`/`autoEmpty` to run the saved command (mirror macOS `DashboardViewModel.refreshAll` which calls `runner.run(..., trigger: .autoReset)`).
- **Sidebar:** enable + route the Tools›"Command Log" row (see gotcha #1). ("Help" stays a placeholder unless the user asks — not in SP5 scope.)

**Likely task decomposition (TDD core/pure, run-check GUI):**
1. `core` (or app) command-log store (record/read rows) — TDD.
2. Command classifier port (pure interactive/non-interactive decision + a resolver trait) — TDD.
3. Windows command runner: Job Object run with timeout/cancel/bounded output (the Job Object + process spawn is the risky bit; test what's testable — exit code, output tail, timeout — against a trivial command like `cmd /c echo`).
4. Terminal launcher (wt.exe/conhost) + shell selection — small, mostly build-verified.
5. Active-Claude-Code account detector (`%USERPROFILE%\.claude.json`) — TDD the JSON parse.
6. Command Log UI (log table) + RunCommand editor/runner + wire the Tools sidebar + auto-trigger hookup — GUI run-check.
7. CI/docs/contract.

**Review-Focus candidates:** cancel/timeout kills the whole child tree (Job Object), not just the parent; a classifier that misreads an interactive program doesn't hang a non-interactive run; the saved-command store is keyed by account id and survives delete/re-add; auto-trigger fires at most once per detected reset (no loops); the run never blocks the UI thread; the command/output is captured bounded (no unbounded memory).

---

## 6. First actions for the next session

1. Reply in Vietnamese. Confirm the user still wants SP5 now vs. verifying SP1–SP4 on CI first (recommend CI first — see §1 caveats).
2. If proceeding: invoke `superpowers:writing-plans`, write `docs/superpowers/plans/2026-10-05-windows-command-log.md`, present for approval (`git add -f` to commit it).
3. On approval: `superpowers:subagent-driven-development`, set up the ledger, run the task loop per §2 with the §4 gotchas front-of-mind (especially the sidebar read-back).
4. Keep the user's standing recommendation visible: this branch needs a CI run + an interactive GUI run-check before merge; `finishing-a-development-branch` triages the deferred-minor backlog.
