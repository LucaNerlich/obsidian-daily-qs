import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";
import vm from "node:vm";

const __dirname = dirname(fileURLToPath(import.meta.url));
const source = readFileSync(join(__dirname, "Model.js"), "utf8");
const sandbox = { console };
vm.createContext(sandbox);
vm.runInContext(source, sandbox);
const Model = sandbox;

function test(name, fn) {
  try {
    fn();
    console.log(`ok - ${name}`);
  } catch (err) {
    console.error(`not ok - ${name}`);
    throw err;
  }
}

test("parseLine rejects garbage", () => {
  assert.equal(Model.parseLine(""), null);
  assert.equal(Model.parseLine("not-json"), null);
  assert.equal(Model.parseLine('{"state":"nope"}'), null);
});

test("parseLine ok snapshot", () => {
  const snap = Model.parseLine(
    JSON.stringify({
      state: "ok",
      date: "2026-08-20",
      path: "/vault/Daily/2026-08-20.md",
      exists: true,
      openCount: 1,
      doneCount: 1,
      carryOverCount: 2,
      isToday: true,
      obsidianUri: "obsidian://open?path=/vault/Daily/2026-08-20.md",
      todos: [
        { line: 3, checked: false, text: "Ship", depth: 0 },
        { line: 4, checked: true, text: "Done", depth: 1, parentLine: 3 },
      ],
    }),
  );
  assert.equal(snap.state, "ok");
  assert.equal(snap.openCount, 1);
  assert.equal(snap.carryOverCount, 2);
  assert.equal(snap.todos.length, 2);
  assert.equal(snap.todos[0].text, "Ship");
  assert.match(snap.obsidianUri, /^obsidian:\/\//);
});

test("parseLine carries depth and parentLine", () => {
  const snap = Model.parseLine(
    JSON.stringify({
      state: "ok",
      todos: [
        { line: 1, checked: false, text: "top", depth: 0, parentLine: null },
        { line: 2, checked: false, text: "child", depth: 1, parentLine: 1 },
        { line: 3, checked: false, text: "grand", depth: 2, parentLine: 2 },
      ],
    }),
  );
  assert.equal(snap.todos[0].depth, 0);
  assert.equal(snap.todos[0].parentLine, null);
  assert.equal(snap.todos[1].depth, 1);
  assert.equal(snap.todos[1].parentLine, 1);
  assert.equal(snap.todos[2].depth, 2);
  assert.equal(snap.todos[2].parentLine, 2);
});

test("parseLine clamps invalid depth/parentLine", () => {
  const snap = Model.parseLine(
    JSON.stringify({
      state: "ok",
      todos: [
        { line: 1, checked: false, text: "bad-depth", depth: -3, parentLine: "x" },
        { line: 2, checked: false, text: "deep", depth: 999, parentLine: 0 },
      ],
    }),
  );
  assert.equal(snap.todos[0].depth, 0);
  assert.equal(snap.todos[0].parentLine, null);
  assert.equal(snap.todos[1].depth, 32);
  assert.equal(snap.todos[1].parentLine, null);
});

test("parseLine strips markup in error/text", () => {
  const snap = Model.parseLine(
    JSON.stringify({ state: "error", error: "bad <script>" }),
  );
  assert.equal(snap.state, "error");
  assert.equal(snap.error, "");
});

test("labelText done/total", () => {
  assert.equal(Model.labelText(null), "\u2610 \u2026");
  assert.equal(
    Model.labelText({ state: "ok", exists: true, openCount: 3, doneCount: 1 }),
    "\u2610 1/4",
  );
  assert.match(
    Model.tooltipText({
      state: "ok",
      exists: true,
      openCount: 2,
      doneCount: 1,
      date: "2026-08-20",
    }),
    /1\/3 done/,
  );
});

test("visibleTodos openOnly", () => {
  const status = {
    todos: [
      { line: 1, checked: false, text: "a" },
      { line: 2, checked: true, text: "b" },
    ],
  };
  assert.equal(Model.visibleTodos(status, false).length, 2);
  assert.equal(Model.visibleTodos(status, true).length, 1);
});

test("visibleTodos openOnly keeps ancestors of open children", () => {
  const status = {
    todos: [
      { line: 1, checked: true, text: "parent", depth: 0, parentLine: null },
      { line: 2, checked: false, text: "child", depth: 1, parentLine: 1 },
      { line: 3, checked: true, text: "sibling", depth: 0, parentLine: null },
    ],
  };
  const visible = Model.visibleTodos(status, true);
  assert.equal(visible.length, 2);
  assert.equal(visible[0].text, "parent");
  assert.equal(visible[1].text, "child");
});

test("visibleTodos openOnly walks the full ancestor chain", () => {
  const status = {
    todos: [
      { line: 1, checked: true, text: "root", depth: 0, parentLine: null },
      { line: 2, checked: true, text: "mid", depth: 1, parentLine: 1 },
      { line: 3, checked: false, text: "leaf", depth: 2, parentLine: 2 },
      { line: 4, checked: true, text: "unrelated", depth: 0, parentLine: null },
    ],
  };
  const visible = Model.visibleTodos(status, true);
  assert.deepEqual(visible.map((t) => t.text), ["root", "mid", "leaf"]);
});

test("shiftDate", () => {
  assert.equal(Model.shiftDate("2026-08-20", -1), "2026-08-19");
  assert.equal(Model.shiftDate("2026-08-20", 1), "2026-08-21");
  assert.equal(Model.shiftDate("bad", 1), "");
});

test("emptyMessage", () => {
  assert.match(
    Model.emptyMessage({ state: "ok", exists: false, todos: [] }, false),
    /No daily note/,
  );
  assert.match(
    Model.emptyMessage({ state: "ok", exists: true, todos: [] }, false),
    /No todos/,
  );
  assert.match(
    Model.emptyMessage(
      {
        state: "ok",
        exists: true,
        todos: [{ line: 1, checked: true, text: "x" }],
      },
      true,
    ),
    /No open/,
  );
});

console.log("All Model.js tests passed.");
