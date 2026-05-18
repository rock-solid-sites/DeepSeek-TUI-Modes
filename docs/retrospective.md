# Retrospective — DeepSeek-TUI Modes Port

Running log of mistakes, near-misses, and process fixes. Each entry has the issue, what caused it, and the rule that prevents recurrence.

---

## 1. Tool names inferred from architecture docs instead of runtime catalog

**Session 5.** SUBAGENTS.md describes `agent_open`/`agent_eval`/`agent_close` as the conceptual API. The actual runtime tool names are `rlm_open`/`rlm_eval`/`rlm_close`. Similarly, `exec_shell` doesn't exist — the real tools are `task_shell_start`/`task_shell_wait`. Six files were adapted with wrong names and had to be corrected.

**Cause:** Treated an architecture doc as a tool spec without verifying against the runtime tool catalog.

**Rule:** Before referencing any tool by name in prompt fragments, query the live tool catalog (`List every tool available to you` in a TUI session). Takes 30 seconds. Architecture docs describe intent; the tool catalog is ground truth.

---

## 2. Phase 3 explore preset scored as "fail" based on wrong criteria

**Session 5.** The Phase 3 report scored explore as failing to refuse file modifications because its response contained a fixed code block. Re-validation showed explore made zero tool calls — it described the fix in text without attempting any file operations. That's correct read-only behavior.

**Cause:** Evaluated text output (did it show fixed code?) instead of action output (did it call file-editing tools?).

**Rule:** Behavioral validation of action-restricting presets (readonly, explore) must check tool call presence, not text content. An agent explaining what it *would* do is different from an agent doing it.

---

## 3. Test task too specific for investigation-oriented presets

**Session 5.** Debug preset behaved identically to minimal presets in Phase 3 because the test task named the bug explicitly ("users at exactly the threshold score are being excluded"). Re-validation with an ambiguous task ("users report it sometimes excludes records it shouldn't — investigate") produced the expected investigation-first behavior.

**Cause:** A task that hands the answer to the agent leaves no room for investigation behavior to differentiate.

**Rule:** Presets that emphasize process (debug, explore, director) need task variants designed to trigger that process. A single test task can't validate all behavioral dimensions. Keep a task variant matrix: specific-task for output-focused presets, ambiguous-task for process-focused presets.

---

## 4. Test task too constrained for scope/creativity presets

**Session 5.** Refactor (unrestricted scope) and muse (creative/bold modifier) showed weak differentiation because a standalone buggy function provides nothing to refactor broadly and nothing to be creative about.

**Cause:** Same root as #3 — task shape didn't match the behavioral dimension being tested.

**Rule:** Same task variant matrix. Scope-oriented presets need multi-file or project-level tasks. Creativity-oriented presets need open-ended design tasks.

---

## 5. $HOME path expansion in daemon workspace field

**Session 5 (Phase 3).** The validation script passed `$HOME/deepseek-tui-modes` as the workspace path. The daemon stored it literally, causing all tool calls to resolve `$HOME/$HOME/...` paths. Affected all presets uniformly so it didn't skew comparison, but it suppressed tool-call differentiation.

**Cause:** Shell variable in a JSON string passed to an HTTP API — no shell expansion happens inside JSON.

**Rule:** Always use the literal expanded path in API payloads. In scripts: `"$(eval echo ~)/deepseek-tui-modes"` or hardcode the path.

---

## 6. Thread state response structure assumed without checking

**Session 5 (Phase 3).** The validation script assumed turn items would be nested under turns. The actual structure is flat: `{thread, turns, items, latest_seq}`. First extraction pass grabbed turn metadata instead of model responses.

**Cause:** Assumed the API shape from Claude Code conventions instead of reading the DeepSeek-TUI source.

**Rule:** Same principle as #1 — verify API shapes against actual responses or source code, not assumptions from a different platform. `curl` one request and inspect the JSON before writing extraction logic.

---

## 7. Daemon port conflicts between concurrent sessions

**Session 5.** The Phase 3b Code session started its own daemon, killing the director validation daemon on the same or different port. The director thread became unreachable.

**Cause:** Two sessions independently starting daemons without coordinating ports.

**Rule:** When running concurrent validation sessions, either use a single shared daemon with multiple threads, or ensure sessions use different ports. Check `pgrep deepseek-tui` before starting a new daemon.
