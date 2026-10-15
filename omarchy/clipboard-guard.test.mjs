// Guard for QTBUG-149610 / marketplace #11653.
//
// Editable Qt TextFields connect to QClipboard::dataChanged and can pull an
// unbounded Wayland clipboard offer with no paste action. Panel inputs must
// stay on GuardedTextField (permanently readOnly + bounded wl-paste).
//
// Run: node omarchy/clipboard-guard.test.mjs

import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const dir = dirname(fileURLToPath(import.meta.url));
const panel = readFileSync(join(dir, "Panel.qml"), "utf8");
const guarded = readFileSync(join(dir, "GuardedTextField.qml"), "utf8");

test("Panel uses GuardedTextField for every text input", () => {
  const guardedUses = panel.match(/\bGuardedTextField\b/g) || [];
  assert.equal(guardedUses.length, 4, `expected 4 GuardedTextField uses, found ${guardedUses.length}`);

  // Bare Controls/Ui TextField instances (not the GuardedTextField type name).
  const bare = panel.match(/(?<!Guarded)TextField\s*\{/g) || [];
  assert.equal(bare.length, 0, `unguarded TextField { in Panel.qml:\n${bare}`);
});

test("GuardedTextField never becomes editable to Qt", () => {
  assert.match(guarded, /^\s*readOnly:\s*true\s*$/m, "readOnly must stay true");
  assert.doesNotMatch(
    guarded,
    /readOnly\s*[:=]\s*false|readOnly\s*=\s*false/,
    "nothing may set readOnly false",
  );
  assert.doesNotMatch(guarded, /\.paste\s*\(/, "must not call Qt paste()");
  assert.match(guarded, /wl-paste/, "paste must go through wl-paste");
  assert.match(guarded, /timeout\s+2/, "paste must have a 2s deadline");
  assert.match(guarded, /pasteLimitBytes/, "paste must declare a byte cap");
  assert.match(guarded, /head -c/, "paste must truncate with head -c");
});

test("no other plugin QML file introduces an editable TextField", () => {
  const qmlFiles = readdirSync(dir).filter((name) => name.endsWith(".qml"));
  for (const name of qmlFiles) {
    if (name === "GuardedTextField.qml" || name === "Panel.qml")
      continue;
    const src = readFileSync(join(dir, name), "utf8");
    assert.doesNotMatch(
      src,
      /(?<!Guarded)TextField\s*\{/,
      `${name} must not declare an unguarded TextField`,
    );
    assert.doesNotMatch(src, /TextInput\s*\{/, `${name} must not declare TextInput`);
    assert.doesNotMatch(src, /TextArea\s*\{/, `${name} must not declare TextArea`);
  }
});
