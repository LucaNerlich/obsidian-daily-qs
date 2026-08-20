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
      todos: [
        { line: 3, checked: false, text: "Ship" },
        { line: 4, checked: true, text: "Done" },
      ],
    }),
  );
  assert.equal(snap.state, "ok");
  assert.equal(snap.openCount, 1);
  assert.equal(snap.todos.length, 2);
  assert.equal(snap.todos[0].text, "Ship");
});

test("parseLine strips markup in error/text", () => {
  const snap = Model.parseLine(
    JSON.stringify({ state: "error", error: "bad <script>" }),
  );
  assert.equal(snap.state, "error");
  assert.equal(snap.error, "");
});

test("labelText and tooltipText", () => {
  assert.equal(Model.labelText(null), "\u2610 \u2026");
  assert.equal(
    Model.labelText({ state: "ok", exists: true, openCount: 3, doneCount: 1 }),
    "\u2610 3",
  );
  assert.match(
    Model.tooltipText({
      state: "ok",
      exists: true,
      openCount: 2,
      doneCount: 1,
      date: "2026-08-20",
    }),
    /2 open/,
  );
});

test("emptyMessage", () => {
  assert.match(
    Model.emptyMessage({ state: "ok", exists: false, todos: [] }),
    /No daily note/,
  );
  assert.match(
    Model.emptyMessage({ state: "ok", exists: true, todos: [] }),
    /No todos/,
  );
});

console.log("All Model.js tests passed.");
