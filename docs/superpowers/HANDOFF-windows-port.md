# Handoff — Windows port, resume at Sub-project 6 (Release)

**Written:** 2026-10-05. **Branch:** `feat/windows-app`. SP5 is on top of `5e25355`, followed by this handoff commit. The working tree is clean. **Platform:** Windows 11, this machine. User language: **Vietnamese** (reply in Vietnamese).

This file is the single source of truth for resuming. Read it, then write the Sub-project 6 plan and execute it the same way SP1–SP5 were done.

---

## 1. Where the project stands

Goal: a Windows 11 port of the macOS "Claude Dashboard", at feature parity, in Rust + Slint, reusing `apps/linux/core`. The overall design spec is `docs/superpowers/specs/2026-10-04-windows-app-design.md` (6 sub-projects). All work so far is on `feat/windows-app`, and each sub-project has its own plan and its own SDD ledger under `.superpowers/sdd/<plan-name>/progress.md`.

- **SP1 — Core on Windows + bridge + extension** (plan `2026-10-04-windows-core-bridge.md`). Core runs on Windows: DPAPI at rest, `v10` cookies, `v20` reported as app-bound. Also the `apps/windows/bridge` native-messaging host, the `apps/windows/extension` MV3 extension (fixed id `cadpjcfajhlgdaipepkdojcehfmkkedh`), `contract/windows.md`, and the CI workflow.
- **SP2 — App shell (Slint)** (plan `2026-10-04-windows-app-shell.md`): Mica window, sidebar, Dashboard grid, per-account page, tray ring + flyout, refresh loop, single instance, reload pipe.
- **SP3 — Setup & Settings** (plan `2026-10-04-windows-setup-settings.md`): Add Account wizard, Settings › Accounts, Settings › General, launch at startup.
- **SP4 — Charts** (plan `2026-10-04-windows-charts.md`): per-account interactive chart and the Overview chart.
- **SP5 — Command Log** (plan `2026-10-05-windows-command-log.md`, commits `2485a4b..5e25355`).
  - New `core` modules: `command_log` (SQLite, 500 rows, 4096-byte tail), `command_classifier`, `auto_run` (latch), `claude_code` (+ tier-2 sort), `run_commands` (`run-commands.json`), `Settings.shell`, and `settings::update` (an RMW mutex).
  - New app modules: `shell.rs`, `terminal.rs` (wt.exe / conhost), `runner.rs` (Job Object, KILL_ON_JOB_CLOSE, suspended → assign → resume), `commands.rs`, `log_view.rs`, `run_command.rs`.
  - New UI: the Command Log pane, Run Command panel, auto-run on reset, shell picker, Claude Code green dot, and Help pane. No sidebar row says "Coming soon" any more.
  - The final review (opus) said "With fixes"; one fix wave closed it and the re-review was clean.

Every sub-project's final review returned no merge-blockers.

### NOT done / caveats (tell the user, don't paper over)
- **CI is green for SP1–SP5.** The user confirmed both the `linux` and `windows` jobs passed on `9e94ca6`. `gh` is **not installed** on this machine, so the controller cannot read CI results itself; ask the user to check https://github.com/haiz/claude-dashboard/actions after each push.
- **GUI click-through run-checks are still owed** (they need clicking; screenshots only covered the Dashboard grid, the Claude Code dot, the enabled sidebar rows, and the card glyph no longer overlapping the plan pill).
  - **SP3–SP4:** the Settings panes, the wizard, and the charts.
  - **SP5:**
    1. Run `echo hi` → a row reading `exit 0`.
    2. Type `htop` → the Terminal checkbox turns on. Click it off, then edit to `vim x` → it follows classification again.
    3. Run `ping -n 100 127.0.0.1`, then Cancel → the row reads "Cancelled" and no `PING.EXE` is left.
    4. Cancel, then reopen the panel for another account → not stuck on "Running…".
    5. A cmd command with quotes opens correctly under wt.
    6. Git Bash `find … -exec … \;` under wt.
    7. The Settings › General › Commands card is not clipped, and the shell choice persists across a restart.
- **Pre-existing:** `apps/linux/helper/tests/add_key_transport.rs` fails 5/7 tests on Windows. The tests point the store via XDG, so they are Unix-only. Windows CI never runs them, and Linux CI is the gate. They are not `cfg(unix)`-gated; triage this at finishing.
- **Deferred minors:** each `progress.md` has a backlog (search for `minor (deferred)`). `superpowers:finishing-a-development-branch` triages it once SP6 is done. None of them blocks a merge.

---

## 2. How the work is run (the loop that produced SP2–SP5)

Process skills: `superpowers:writing-plans` → `superpowers:subagent-driven-development`. Brainstorming is done at the architecture level and the spec is approved. A new sub-project does NOT re-brainstorm; it goes straight to a plan.

For each sub-project:
1. **Write the plan** to `docs/superpowers/plans/2026-10-0X-windows-release.md`: header, Global Constraints, Rulings, Review Focus, file structure, bite-size tasks. `docs/superpowers/` is **gitignored**, so commit plans with `git add -f`. Present the plan to the user for approval before executing.
2. **Set up SDD.** Run `bash <skill>/scripts/sdd-workspace <plan>` to get the ledger dir. Write `progress.md` with the plan path on line 1, a pre-flight conflict-scan table, and the rulings. `<skill>` = `C:/Users/cthai/.claude/plugins/cache/claude-plugins-official/superpowers/6.4.1/skills/subagent-driven-development`.
   - **Reusable contracts (SP5 pattern, saves prompt size).** Write `global-constraints.md` (the plan's Global Constraints + Rulings + environment notes), `implementer-contract.md`, `reviewer-contract.md` and `rereview-contract.md` into the ledger dir. Copy the SP5 ones from `.superpowers/sdd/2026-10-05-windows-command-log/` and adapt them. Then each dispatch is short: the brief path, the contract paths, the interfaces from earlier tasks, and the controller rulings.
   - Pre-extract every brief: `bash <skill>/scripts/task-brief <plan> N`.
3. **Per task:**
   - Record BASE.
   - Dispatch the implementer (**model: sonnet**, `general-purpose`).
   - On DONE, run `bash <skill>/scripts/review-package <plan> BASE HEAD`, then dispatch the reviewer (sonnet; **opus** for risky unsafe/concurrency code, as T7 runner was).
   - Fix loop: resume the same implementer via `SendMessage` (load it with `ToolSearch select:SendMessage`), then a scoped re-review on the fix range.
   - Append `Task N: complete` to the ledger.
4. **Final whole-branch review** on **opus** over the sub-project's range, pointed at the ledger's deferred minors. Then ONE fix wave and one re-review.
5. Update `CLAUDE.md` and `contract/` in the last task. Do **not** merge; that is the user's call.

Commit author must be explicit (git user is unset): `git -c user.email="backend@gotitapp.co" -c user.name="cthai" commit -m "..."`. Trailer: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. **Re-read the session's attribution system-reminder each session, because the model name changes.** Subagents see a different reminder (Sonnet) and will ask; the contract's line wins.

### Dispatch hygiene that mattered
- Contracts: **no nested subagents**; the implementer edits only its task's files; the reviewer is read-only. Always name the model.
- Diffs and reports live as files under the ledger dir; never paste big blobs into prompts.
- Pure logic → TDD. GUI → build + smoke run + screenshot, with only the named pure helpers unit-tested (a recorded ruling every time).
- Pre-flight scan pays off. In SP5 it caught two test expectations that contradicted their own code (Rulings A and B) before dispatch.

---

## 3. Environment specifics (this Windows machine)

- **cargo** comes via rustup but is not always on PATH in the Bash tool. In PowerShell, run `$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"` first. Toolchain **1.98.0** is pinned; the workspaces declare `rust-version = 1.89`.
- **No `python`** (the Store alias stub only) and **no `gh`**. Use `node` for scripting.
- Build/test commands:
  - core: `cd apps/linux && cargo test -p claude-dashboard-core`. Don't use `--workspace` here on Windows, because of the helper XDG tests.
  - Windows workspace: `cd apps/windows && cargo test --workspace` and `cargo clippy --all-targets -- -D warnings`.
  - extension: `cd apps/windows/extension && node --test`.
- **Run the app:** `cd apps/windows && cargo run -p claude-dashboard`. `CLAUDE_DASHBOARD_SMOKE=1` self-quits after about 1.2 s. `CLAUDE_DASHBOARD_FAKE_ROWS=1` seeds alice/bob/carol with chart history, and bob is marked as the active Claude Code account.
- **Screenshot recipe:** build, `Start-Process` the exe (`apps/windows/target/debug/claude-dashboard.exe`) with the env flags, `Start-Sleep -Seconds 4`, capture with `System.Windows.Forms`/`System.Drawing` `CopyFromScreen` to a PNG, `Stop-Process -Force`, then Read the PNG. Panes that need a click can't be reached this way.
- Git warns `LF will be replaced by CRLF`; this is harmless.

---

## 4. Recurring gotchas

1. **Enabling or routing UI elements is the #1 repeat defect.** Require the implementer to read the edited lines back and paste them verbatim, and have the reviewer confirm them in the diff. This held in SP5: no wiring defects.
2. Edit `.slint`, `.rs` and `.md` files with the Edit tool. Never use perl, sed or `Set-Content`, which miss silently or add a BOM. Verify with `head -c 3`.
3. Store writes (`accounts.json` / `extension-sources.json`) hold `core::store::lock_store()` across the whole read-modify-write. `settings.json` writes go through `core::settings::update` (in-process mutex). `run-commands.json` has its own mutex.
4. `core::format` / `chart::format_tick` are timezone-naive; the GUI shifts by `model::local_offset_s()`.
5. Remove temporary `#![allow(dead_code)]` once the UI wires a module in.
6. The session key must never appear in a log, UI string, command-log row or error.
7. GUI long work runs off the UI thread (`std::thread::spawn` + `slint::invoke_from_event_loop`). Timers stop on close.
8. **Slint specifics learned in SP5:**
   - A one-way `checked:` binding detaches after a user click; use an `in-out` `<=>` chain.
   - A later-declared `TouchArea` sits on top, so declare the card's own TouchArea first and the inner buttons after it.
   - A child's `y:` inside a `HorizontalLayout` is ignored.

---

## 5. Sub-project 6 — Release (what to plan + build)

**Spec reference:** the "Release" section of the spec (lines 248–259) plus "Registration" (lines 159–165). **Out of scope** (spec): code signing, winget/Scoop, publishing to the Chrome Web Store or Edge Add-ons, and `v20` decryption.

Targets:
- **`.github/workflows/release-windows.yml`** on `windows-latest`, triggered by a release. It builds and uploads `ClaudeDashboard-x64.msi` and `claude-dashboard-extension.zip`. Mirror how the macOS release flow attaches artifacts (`scripts/release.sh`, `gh release create`); check whether the Windows job uploads to the release the macOS script creates.
- **MSI via `cargo-wix`, per user, no admin.**
  - It installs to `%LOCALAPPDATA%\Programs\ClaudeDashboard` (`claude-dashboard.exe` + `claude-dashboard-bridge.exe`) and adds a Start Menu shortcut.
  - It writes the host manifest `com.claude_dashboard.bridge.json` next to the bridge and registers it under `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.claude_dashboard.bridge` (Chrome, Brave and Arc read this key) and `HKCU\Software\Microsoft\Edge\NativeMessagingHosts\…`.
  - `allowed_origins` lists the unpacked ID (`cadpjcfajhlgdaipepkdojcehfmkkedh`) and reserves the Store IDs. See `contract/windows.md` "Registration" for the exact manifest shape the bridge expects.
  - Uninstall removes the registry keys.
- **Auto-update,** like macOS `apps/macos/ClaudeDashboard/Services/UpdateService.swift`: poll GitHub `releases/latest`, compare versions, download the `.msi` asset, run `msiexec /i <msi> /passive`, relaunch. Wire it into Settings › General › Updates, which currently shows "Coming soon" (`app.slint` ~line 271). Pure logic (version compare, asset pick from the release JSON) goes in a tested module; the Linux port has `apps/linux/lib/update.js` + `tests/update.test.js` to port from.
- **Version sync:** `scripts/sync-version.sh` and `scripts/release.sh` must also bump `apps/windows/Cargo.toml` (`[workspace.package] version`, currently `1.18.1` with a comment pointing at SP6) and `apps/windows/extension/manifest.json`. `scripts/test-sync-version.sh` must cover them.
- **CLAUDE.md / contract:** document the release flow, the MSI layout and registry keys, and the update flow; add the Windows release steps to the "Releasing" section.

**Likely task decomposition:**
1. Version sync (scripts + test-sync-version), TDD via the shell test.
2. Update-check pure logic (version compare, asset selection) in a tested module (core or app).
3. Update apply (download + `msiexec /passive` + relaunch) + the Settings › General › Updates UI (GUI run-check).
4. WiX/`cargo-wix` config: files, per-user install dir, Start Menu shortcut, native-messaging manifest + HKCU keys, uninstall cleanup. Verified with a local `cargo wix` build plus an install/uninstall check on this machine (ask the user before running an installer, since it is a side effect outside the repo).
5. Extension zip packaging.
6. `release-windows.yml`.
7. Docs, contract, CLAUDE.md.

**Review-Focus candidates:**
- The MSI installs without admin rights, and an upgrade over an older version keeps the user's data (`%APPDATA%` / `%LOCALAPPDATA%\claude-dashboard` is never touched by the installer).
- Uninstall leaves no dangling native-messaging keys.
- Auto-update never downgrades, ignores pre-releases and drafts, and handles a missing `.msi` asset or offline gracefully.
- The running app is closed or handled correctly when `msiexec` replaces its exe; the single-instance mutex must not block the relaunch.
- Version strings stay in sync across `VERSION`, the Cargo workspace and the extension manifest.

---

## 6. First actions for the next session

1. Reply in Vietnamese. CI was green on `9e94ca6`; if new commits have been pushed since, ask the user for their CI result (no `gh` here).
2. Offer the owed GUI run-checks (§1), since the user has to click through them.
3. Invoke `superpowers:writing-plans`, write the SP6 plan, present it for approval, and commit it with `git add -f`.
4. On approval, run `superpowers:subagent-driven-development` per §2, reusing the SP5 contract files.
5. After SP6, run `superpowers:finishing-a-development-branch`: triage every ledger's deferred minors, then present the merge/PR options. The user decides.
