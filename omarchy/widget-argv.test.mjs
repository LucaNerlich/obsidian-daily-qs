// Guard for the widget's process arguments (#102).
//
// QML has no test runtime here or in CI (`qmllint` is not installed and there
// is no QML job), so this reads `omarchy/BarWidget.qml` as text and pins the
// invariant the marketplace review blocked on: nothing private — the
// absolute vault path, the archive pattern, todo text — may reach a spawned
// process's argv, where any local user can read it from /proc/<pid>/cmdline.
//
// It is a textual guard: it catches a regression in the launch wiring (a
// reintroduced `--vault`, a launch that forgets `--stdin`), not a typo in the
// QML. Keep it in step with the file.
//
// Run: node omarchy/widget-argv.test.mjs

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const src = readFileSync(new URL("./BarWidget.qml", import.meta.url), "utf8");

function includes(needle, why) {
  assert.ok(src.includes(needle), `${why}\n  missing: ${needle}`);
}

function excludes(needle, why) {
  assert.ok(!src.includes(needle), `${why}\n  found: ${needle}`);
}

test("the vault path and archive pattern never reach argv", () => {
  excludes('"--vault"', "the backend must not be launched with --vault from the widget");
  excludes(
    '"--archive-folder"',
    "the archive pattern rides the payload; argv stays free of settings paths",
  );
  excludes("vaultArgs", "the argv helper for the vault root must stay gone");
  excludes("withVault", "the argv helper for the vault root must stay gone");
});

test("the private payload carries the vault and archive settings", () => {
  includes("payload.vault = root.vaultPath", "the vault root must travel on stdin");
  includes(
    "payload.archiveFolder = root.archiveFolder",
    "the archive pattern must travel on stdin",
  );
  includes(
    "for (var i = 0; i < keys.length; i++)",
    "todo text fields must be copied into the payload",
  );
});

test("every backend launch enables stdin, writes the payload and closes it", () => {
  // actionProc / weekProc / watchProc, one `--stdin` each.
  const stdinFlags = src.match(/"--stdin"/g) || [];
  assert.equal(
    stdinFlags.length,
    3,
    `expected --stdin on the three launches (action, week, watch), found ${stdinFlags.length}`,
  );

  includes('var full = args.concat(["--stdin"])', "runAction must add --stdin itself");
  includes("actionProc.stdinEnabled = true", "runAction must open stdin before start");
  includes(
    "actionProc.write(root.pendingActionStdin)",
    "the action payload must be written after start",
  );

  includes("weekProc.stdinEnabled = true", "the week launch must open stdin");
  includes(
    "weekProc.write(root.pendingWeekStdin)",
    "the week payload must be written after start",
  );

  includes("watchProc.stdinEnabled = true", "the watch launch must open stdin");
  includes(
    "watchProc.write(root.pendingWatchStdin)",
    "the watch payload must be written after start",
  );
});

test("the one argv carry (saving the setting) stays explicit", () => {
  // omarchy's settings CLI takes the value as an argument (bin/omarchy-bar has
  // no stdin channel); the widget keeps it for the user-initiated save from
  // the setup state only. Anything else that spawns the backend must not.
  includes(
    '["omarchy", "bar", "set", "luca.obsidian-daily", "vaultPath", expanded]',
    "the settings save must be the only place the path is passed as an argument",
  );
});
