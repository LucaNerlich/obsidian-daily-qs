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
      todos.push({
        line: Math.floor(lineNo),
        checked: item.checked === true,
        text: safeText(item.text)
      });
    }
  }

  var openCount = typeof parsed.openCount === "number" && isFinite(parsed.openCount)
    ? Math.max(0, Math.floor(parsed.openCount))
    : todos.filter(function(t) { return !t.checked; }).length;
  var doneCount = typeof parsed.doneCount === "number" && isFinite(parsed.doneCount)
    ? Math.max(0, Math.floor(parsed.doneCount))
    : todos.filter(function(t) { return t.checked; }).length;

  return {
    state: "ok",
    date: safeText(parsed.date),
    path: safeText(parsed.path),
    exists: parsed.exists === true,
    openCount: openCount,
    doneCount: doneCount,
    todos: todos,
    error: ""
  };
}

// Qt Text defaults can treat a string that looks like HTML as rich text
// (Text.AutoText). JSON from the helper (and PATH-fallback binary) is
// untrusted from QML's point of view, so drop markup rather than let it
// reach PanelHero / Text. Pair with textFormat: Text.PlainText in QML.
function safeText(value) {
  if (typeof value !== "string") return "";
  if (value.indexOf("<") !== -1 || value.indexOf(">") !== -1 || value.indexOf("&") !== -1)
    return "";
  return value;
}

function labelText(status) {
  if (!status) return "\u2610 \u2026";
  if (status.state === "error") return "\u2610 !";
  if (!status.exists) return "\u2610 ·";
  return "\u2610 " + String(status.openCount);
}

function tooltipText(status) {
  if (!status) return "Obsidian Daily";
  if (status.state === "error") {
    return status.error ? ("Obsidian Daily — " + status.error) : "Obsidian Daily — error";
  }
  var date = status.date || "today";
  if (!status.exists) return "Obsidian Daily — no note for " + date;
  return "Obsidian Daily — " + status.openCount + " open, " + status.doneCount + " done (" + date + ")";
}

function metaLine(status) {
  if (!status) return "";
  if (status.state === "error") return status.error || "Error";
  if (!status.exists) return "No daily note yet";
  return status.openCount + " open · " + status.doneCount + " done";
}

function emptyMessage(status) {
  if (!status) return "Loading…";
  if (status.state === "error") return status.error || "Unable to read vault";
  if (!status.exists) return "No daily note for today. Add a todo to create it.";
  if (!status.todos || status.todos.length === 0) return "No todos in today's note.";
  return "";
}
