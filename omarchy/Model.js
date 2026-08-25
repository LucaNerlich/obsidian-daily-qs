// Pure parsing/formatting shared by BarWidget.qml and Panel.qml.

/**
 * Parse a JSON status line into a normalized snapshot object.
 * @param {*} line - The input containing a JSON-encoded snapshot.
 * @return {Object|null} Normalized snapshot, or null for invalid input.
 */
function parseLine(line) {
  var text = String(line || "").trim();
  if (text === "") return null;
  var parsed;
  try {
    parsed = JSON.parse(text);
  } catch (e) {
    return null;
  }
  if (parsed === null || typeof parsed !== "object") return null;

  var state = String(parsed.state || "");
  if (state !== "ok" && state !== "error") return null;

  if (state === "error") {
    return {
      state: "error",
      date: "",
      path: "",
      exists: false,
      openCount: 0,
      doneCount: 0,
      todos: [],
      obsidianUri: "",
      carryOverCount: 0,
      isToday: false,
      templateName: "",
      createdFromTemplate: false,
      errorCode: safeText(parsed.errorCode) || "io",
      error: safeText(parsed.error)
    };
  }

  var todos = [];
  if (Array.isArray(parsed.todos)) {
    for (var i = 0; i < parsed.todos.length; i++) {
      var item = parsed.todos[i];
      if (!item || typeof item !== "object") continue;
      var lineNo = Number(item.line);
      if (!isFinite(lineNo) || lineNo < 1) continue;
      var depth = Number(item.depth);
      if (!isFinite(depth) || depth < 0) depth = 0;
      var parentLine = null;
      if (typeof item.parentLine === "number" && isFinite(item.parentLine) && item.parentLine >= 1)
        parentLine = Math.floor(item.parentLine);
      todos.push({
        line: Math.floor(lineNo),
        checked: item.checked === true,
        text: safeTodoText(item.text),
        depth: Math.min(32, Math.floor(depth)),
        parentLine: parentLine
      });
    }
  }

  var openCount = typeof parsed.openCount === "number" && isFinite(parsed.openCount)
    ? Math.max(0, Math.floor(parsed.openCount))
    : todos.filter(function(t) { return !t.checked; }).length;
  var doneCount = typeof parsed.doneCount === "number" && isFinite(parsed.doneCount)
    ? Math.max(0, Math.floor(parsed.doneCount))
    : todos.filter(function(t) { return t.checked; }).length;
  var carryOverCount = typeof parsed.carryOverCount === "number" && isFinite(parsed.carryOverCount)
    ? Math.max(0, Math.floor(parsed.carryOverCount))
    : 0;

  return {
    state: "ok",
    date: safeText(parsed.date),
    path: safeText(parsed.path),
    exists: parsed.exists === true,
    openCount: openCount,
    doneCount: doneCount,
    todos: todos,
    obsidianUri: safeUri(parsed.obsidianUri),
    carryOverCount: carryOverCount,
    isToday: parsed.isToday === true,
    templateName: safeText(parsed.templateName),
    createdFromTemplate: parsed.createdFromTemplate === true,
    errorCode: "",
    error: ""
  };
}

/** Parse `week` command JSON. */
function parseWeekLine(line) {
  var text = String(line || "").trim();
  if (text === "") return null;
  var parsed;
  try {
    parsed = JSON.parse(text);
  } catch (e) {
    return null;
  }
  if (!parsed || typeof parsed !== "object") return null;
  if (String(parsed.state || "") === "error") {
    return { state: "error", days: [], error: safeText(parsed.error) };
  }
  var days = [];
  if (Array.isArray(parsed.days)) {
    for (var i = 0; i < parsed.days.length; i++) {
      var d = parsed.days[i];
      if (!d || typeof d !== "object") continue;
      days.push({
        date: safeText(d.date),
        openCount: Math.max(0, Math.floor(Number(d.openCount) || 0)),
        doneCount: Math.max(0, Math.floor(Number(d.doneCount) || 0)),
        exists: d.exists === true,
        isToday: d.isToday === true
      });
    }
  }
  return { state: "ok", days: days, error: "" };
}

function safeText(value) {
  if (typeof value !== "string") return "";
  if (value.indexOf("<") !== -1 || value.indexOf(">") !== -1 || value.indexOf("&") !== -1)
    return "";
  return value;
}

function safeTodoText(value) {
  if (typeof value !== "string") return "";
  return value.replace(/[\u0000-\u001f\u007f]/g, "");
}

function safeUri(value) {
  if (typeof value !== "string") return "";
  if (value.indexOf("obsidian://") !== 0) return "";
  if (value.indexOf("<") !== -1 || value.indexOf(">") !== -1 || value.indexOf('"') !== -1)
    return "";
  return value;
}

function expandPath(path, home) {
  var value = String(path || "").trim();
  var homePath = String(home || "").trim();
  if (value === "") return "";
  if (value === "~") return homePath;
  if (value.indexOf("~/") === 0) return homePath + value.substring(1);
  if (value.indexOf("$HOME/") === 0) return homePath + value.substring(5);
  if (value.charAt(0) !== "/" && homePath !== "") return homePath + "/" + value;
  return value;
}

function labelText(status) {
  if (!status) return "\u2026";
  if (status.state === "error") return "!";
  if (!status.exists) return "\u00B7";
  var total = status.doneCount + status.openCount;
  return String(status.doneCount) + "/" + String(total);
}

function progressRatio(status) {
  if (!status || status.state !== "ok" || !status.exists) return 0;
  var total = status.doneCount + status.openCount;
  if (total <= 0) return 1;
  return status.doneCount / total;
}

function shouldConceal(status, hideWhenDone, hideWhenEmpty) {
  if (!status || status.state === "error") return false;
  if (hideWhenEmpty && (!status.exists || (status.openCount + status.doneCount) === 0))
    return true;
  if (hideWhenDone && status.exists && status.openCount === 0 && status.doneCount > 0)
    return true;
  return false;
}

function tooltipText(status) {
  if (!status) return "Obsidian Daily";
  if (status.state === "error") {
    return status.error ? ("Obsidian Daily — " + status.error) : "Obsidian Daily — error";
  }
  var date = status.date || "today";
  if (!status.exists) return "Obsidian Daily — no note for " + date;
  return "Obsidian Daily — " + status.doneCount + "/" + (status.doneCount + status.openCount)
    + " done (" + date + ")";
}

function metaLine(status) {
  if (!status) return "";
  if (status.state === "error") return status.error || "Error";
  if (!status.exists) {
    if (status.templateName)
      return "No daily note yet · template " + status.templateName;
    return "No daily note yet";
  }
  return status.doneCount + "/" + (status.doneCount + status.openCount)
    + " done · " + status.openCount + " open";
}

function emptyMessage(status, openOnly, query) {
  if (!status) return "Loading…";
  if (status.state === "error") return status.error || "Unable to read vault";
  if (!status.exists) {
    if (status.templateName)
      return "No daily note for this day. Add a todo to create it from template "
        + status.templateName + ".";
    return "No daily note for this day. Add a todo to create it.";
  }
  if (!status.todos || status.todos.length === 0) return "No todos in this note.";
  var q = String(query || "").trim();
  if (q !== "") return "No todos match \"" + q + "\".";
  if (openOnly) {
    var open = status.todos.filter(function(t) { return !t.checked; });
    if (open.length === 0) return "No open todos.";
  }
  return "";
}

function isVaultSetupError(status) {
  if (!status || status.state !== "error") return false;
  var code = String(status.errorCode || "");
  return code === "missing_vault" || code === "bad_vault";
}

function visibleTodos(status, openOnly, query) {
  if (!status || !status.todos) return [];

  var todos = status.todos;
  var q = String(query || "").trim().toLowerCase();
  if (!openOnly && q === "") return todos;

  var byLine = {};
  for (var i = 0; i < todos.length; i++) byLine[todos[i].line] = todos[i];

  var keep = {};
  function keepChain(line) {
    while (line && byLine[line]) {
      if (keep[line] === true) return;
      keep[line] = true;
      line = byLine[line].parentLine;
    }
  }

  for (var j = 0; j < todos.length; j++) {
    var todo = todos[j];
    if (openOnly && todo.checked) continue;
    if (q !== "" && todo.text.toLowerCase().indexOf(q) === -1) continue;
    keepChain(todo.line);
  }

  return todos.filter(function(t) { return keep[t.line] === true; });
}

function shiftDate(dateStr, deltaDays) {
  var text = String(dateStr || "");
  var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (!m) return "";
  var d = new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])));
  if (isNaN(d.getTime())) return "";
  d.setUTCDate(d.getUTCDate() + Number(deltaDays));
  var y = d.getUTCFullYear();
  var mo = d.getUTCMonth() + 1;
  var day = d.getUTCDate();
  return y + "-" + (mo < 10 ? "0" : "") + mo + "-" + (day < 10 ? "0" : "") + day;
}

function todayIso() {
  var d = new Date();
  var y = d.getFullYear();
  var mo = d.getMonth() + 1;
  var day = d.getDate();
  return y + "-" + (mo < 10 ? "0" : "") + mo + "-" + (day < 10 ? "0" : "") + day;
}

function weekdayShort(dateStr) {
  var text = String(dateStr || "");
  var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (!m) return "";
  var d = new Date(Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3])));
  if (isNaN(d.getTime())) return "";
  var names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  // getUTCDay: 0=Sun … convert to Mon-first
  var idx = (d.getUTCDay() + 6) % 7;
  return names[idx];
}
