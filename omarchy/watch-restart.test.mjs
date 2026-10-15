// Decision table for the watch-process restart machine in `omarchy/BarWidget.qml`.
//
// QML has no test runtime here or in CI (`qmllint` is not installed and there is
// no QML job), so this encodes the handlers' rules as data and pins the
// outcomes. It is a *transliteration* of the QML, not a load of it: it catches
// a regression in the rules, not a typo in the QML. Keep the two in step when
// either changes.
//
// Run: node omarchy/watch-restart.test.mjs

import assert from "node:assert/strict";
import test from "node:test";

const MIN_HEALTHY_RUN_MS = 10000;

/// A stand-in for the widget's watch `Process` plus the properties the exit
/// classification reads. Mirrors the QML handlers exactly.
function machine(overrides = {}) {
  const now = { t: 1_000_000 };
  const m = {
    now,
    statusState: "ok",
    watchRestarting: false,
    watchBundledFailed: false,
    watchFallbackFailed: false,
    interval: 5000,
    timerArmed: false,
    proc: {
      running: true,
      startedOnce: true,
      startedAtMs: now.t - 60000,
      lastError: "",
      binary: "bundled",
    },
    ...overrides,
  };

  m.restartWatch = () => {
    if (m.proc.running) m.watchRestarting = true;
    m.proc.running = false;
    m.interval = 200;
    m.timerArmed = true;
  };

  m.onStarted = () => {
    m.proc.startedOnce = true;
    m.proc.startedAtMs = m.now.t;
    m.proc.lastError = "";
    m.watchRestarting = false;
  };

  // Called when the process has stopped (`running === false`).
  m.onRunningChanged = () => {
    if (m.proc.running) return;
    const failedStart = !m.proc.startedOnce;
    const deliberateStop = m.watchRestarting;
    m.watchRestarting = false;
    m.proc.startedOnce = false;

    if (deliberateStop && !failedStart) {
      m.proc.lastError = "";
      return;
    }

    const shortLived =
      !failedStart && m.now.t - m.proc.startedAtMs < MIN_HEALTHY_RUN_MS;
    if (!failedStart && !shortLived) {
      m.proc.lastError = "";
      m.interval = 5000;
      m.timerArmed = true;
      if (m.statusState !== "error") m.statusState = "error";
      return;
    }

    if (m.statusState !== "error") m.statusState = "error";
    if (m.proc.binary === "bundled") m.watchBundledFailed = true;
    else if (m.proc.binary === "fallback") m.watchFallbackFailed = true;

    if (m.watchBundledFailed && m.watchFallbackFailed) return;
    m.interval = 2000;
    m.timerArmed = true;
  };

  return m;
}

test("a deliberate restart of a healthy watch does not latch or error", () => {
  const m = machine();
  m.restartWatch();
  m.onRunningChanged();
  assert.equal(m.watchBundledFailed, false, "must not latch the binary");
  assert.equal(m.statusState, "ok", "a settings change is not an outage");
  assert.equal(m.interval, 200, "keeps the short delay");
  assert.equal(m.watchRestarting, false, "intent consumed");
});

test("a deliberate restart within 10s does not latch either (#48)", () => {
  const m = machine();
  m.proc.startedAtMs = m.now.t - 1000; // shorter than minHealthyRunMs
  m.restartWatch();
  m.onRunningChanged();
  assert.equal(m.watchBundledFailed, false);
  assert.equal(m.statusState, "ok");
});

test("an idle restart does not arm the intent flag", () => {
  const m = machine();
  m.proc.running = false;
  m.restartWatch();
  assert.equal(m.watchRestarting, false, "nothing will exit, so nothing to classify");
  assert.equal(m.timerArmed, true);
});

test("a short-lived unexpected crash still latches and reports", () => {
  const m = machine();
  m.proc.startedAtMs = m.now.t - 2000;
  m.proc.running = false;
  m.onRunningChanged();
  assert.equal(m.watchBundledFailed, true);
  assert.equal(m.statusState, "error");
  assert.equal(m.interval, 2000);
});

test("a never-started process still latches", () => {
  const m = machine();
  m.proc.startedOnce = false;
  m.proc.running = false;
  m.onRunningChanged();
  assert.equal(m.watchBundledFailed, true);
  assert.equal(m.statusState, "error");
});

test("an unexpected exit after a long run reports the error but does not latch", () => {
  const m = machine();
  m.proc.startedAtMs = m.now.t - 30000;
  m.proc.running = false;
  m.onRunningChanged();
  assert.equal(m.watchBundledFailed, false, "a long run is not a broken binary");
  assert.equal(m.statusState, "error", "but it must not look healthy either");
  assert.equal(m.interval, 5000);
});

test("an exec failure after a deliberate restart still latches", () => {
  const m = machine();
  m.restartWatch();
  m.onRunningChanged(); // our own stop: skipped
  m.proc.startedOnce = false; // the new process never reached onStarted
  m.proc.running = false;
  m.onRunningChanged();
  assert.equal(m.watchBundledFailed, true, "a broken binary must still be caught");
  assert.equal(m.statusState, "error");
});

test("a short exit during the retry window latches", () => {
  const m = machine();
  m.proc.startedAtMs = m.now.t - 1500; // armed 2000ms retry already running
  m.proc.running = false;
  m.onRunningChanged();
  assert.equal(m.watchBundledFailed, true);
  assert.equal(m.interval, 2000);
});

test("a slow SIGTERM after our restart is still classified as ours", () => {
  // The timer fires before the old process actually exits, then the exit
  // arrives: the intent flag must survive until it is consumed.
  const m = machine();
  m.restartWatch();
  m.timerArmed = true; // timer fired and started a new process already
  m.onRunningChanged(); // the *old* process's exit now arrives
  assert.equal(m.watchBundledFailed, false);
  assert.equal(m.statusState, "ok");
});

test("two rapid setting changes stay clean", () => {
  const m = machine();
  m.restartWatch();
  m.onRunningChanged();
  m.proc.running = true;
  m.onStarted();
  m.restartWatch();
  m.onRunningChanged();
  assert.equal(m.watchBundledFailed, false);
  assert.equal(m.statusState, "ok");
});

test("the fallback binary is latched independently of the bundled one", () => {
  const m = machine();
  m.proc.binary = "fallback";
  m.proc.startedAtMs = m.now.t - 1000;
  m.proc.running = false;
  m.onRunningChanged();
  assert.equal(m.watchBundledFailed, false);
  assert.equal(m.watchFallbackFailed, true);
  assert.equal(m.watchRestarting, false);
});
