import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Quattro bar entry point for Obsidian daily note todos. Vault I/O lives in
// the Rust backend (`obsidian-daily-qs`); this file owns the bar button, the
// panel routing, and the watch / action process lifecycle.
BarWidget {
  id: root
  moduleName: "luca.obsidian-daily"

  function decodeFileUrl(urlString) {
    var path = String(urlString).replace(/^file:\/\//, "")
    try {
      return decodeURIComponent(path)
    } catch (e) {
      return path
    }
  }
  readonly property string bundledBinary: root.decodeFileUrl(
    Qt.resolvedUrl("bin/obsidian-daily-qs").toString())
  readonly property int fallbackThreshold: 2
  property bool watchFallback: false
  property bool actionFallback: false
  property int watchFailures: 0
  property int actionFailures: 0
  readonly property string watchBinary: watchFallback ? "obsidian-daily-qs" : bundledBinary
  readonly property string actionBinary: actionFallback ? "obsidian-daily-qs" : bundledBinary
  property var pendingActionArgs: []

  // Actions requested while another one is still running; drained in order
  // on completion instead of being dropped.
  property var actionQueue: []

  readonly property var panelItem: panelLoader.item
  readonly property bool opened: panelItem ? panelItem.opened === true : false

  // Today (bar label) — always fed by `watch`.
  property string statusState: "ok"
  property string date: ""
  property string path: ""
  property bool exists: false
  property int openCount: 0
  property int doneCount: 0
  property var todos: []
  property var tasksTodayTodos: []
  property var dataviewTodos: []
  property int tasksTodayOpenCount: 0
  property int tasksTodayDoneCount: 0
  property int dataviewOpenCount: 0
  property int dataviewDoneCount: 0
  property string error: ""
  property string obsidianUri: ""
  property int carryOverCount: 0
  property bool isToday: true

  // Panel view (may differ from today when day-switching).
  property string viewDate: ""
  property string viewStatusState: "ok"
  property string viewPath: ""
  property bool viewExists: false
  property int viewOpenCount: 0
  property int viewDoneCount: 0
  property var viewTodos: []
  property var viewTasksTodayTodos: []
  property var viewDataviewTodos: []
  property int viewTasksTodayOpenCount: 0
  property int viewTasksTodayDoneCount: 0
  property int viewDataviewOpenCount: 0
  property int viewDataviewDoneCount: 0
  property string viewError: ""
  property string viewObsidianUri: ""
  property int viewCarryOverCount: 0
  property bool viewIsToday: true

  readonly property bool openOnlyDefault: setting("openOnly", false) === true

  readonly property var status: ({
    state: root.statusState,
    date: root.date,
    path: root.path,
    exists: root.exists,
    openCount: root.openCount,
    doneCount: root.doneCount,
    todos: root.todos,
    tasksTodayTodos: root.tasksTodayTodos,
    dataviewTodos: root.dataviewTodos,
    tasksTodayOpenCount: root.tasksTodayOpenCount,
    tasksTodayDoneCount: root.tasksTodayDoneCount,
    dataviewOpenCount: root.dataviewOpenCount,
    dataviewDoneCount: root.dataviewDoneCount,
    error: root.error,
    obsidianUri: root.obsidianUri,
    carryOverCount: root.carryOverCount,
    isToday: root.isToday
  })
  readonly property string labelText: Model.labelText(status)
  readonly property string tooltipText: Model.tooltipText(status)
  readonly property color urgent: bar ? bar.urgent : Color.urgent

  function open() {
    if (!panelItem) return
    if (root.viewDate === "") root.viewDate = Model.todayIso()
    root.refreshView()
    panelItem.open()
    Qt.callLater(function() {
      if (panelItem && typeof panelItem.focusCapture === "function")
        panelItem.focusCapture()
    })
  }
  function close() { if (panelItem) panelItem.close() }
  function toggle() {
    if (!panelItem) return
    if (panelItem.opened === true) root.close()
    else root.open()
  }

  function applyTodayLine(line) {
    var parsed = Model.parseLine(String(line || ""))
    if (!parsed) return
    root.statusState = parsed.state
    root.date = parsed.date
    root.path = parsed.path
    root.exists = parsed.exists === true
    root.openCount = parsed.openCount
    root.doneCount = parsed.doneCount
    root.todos = parsed.todos || []
    root.tasksTodayTodos = parsed.tasksTodayTodos || []
    root.dataviewTodos = parsed.dataviewTodos || []
    root.tasksTodayOpenCount = parsed.tasksTodayOpenCount || 0
    root.tasksTodayDoneCount = parsed.tasksTodayDoneCount || 0
    root.dataviewOpenCount = parsed.dataviewOpenCount || 0
    root.dataviewDoneCount = parsed.dataviewDoneCount || 0
    root.error = parsed.error || ""
    root.obsidianUri = parsed.obsidianUri || ""
    root.carryOverCount = parsed.carryOverCount || 0
    root.isToday = parsed.isToday === true
    if (root.viewDate === "" || root.viewDate === parsed.date)
      root.applyViewParsed(parsed)
  }

  function applyViewParsed(parsed) {
    if (!parsed) return
    root.viewStatusState = parsed.state
    root.viewDate = parsed.date || root.viewDate
    root.viewPath = parsed.path
    root.viewExists = parsed.exists === true
    root.viewOpenCount = parsed.openCount
    root.viewDoneCount = parsed.doneCount
    root.viewTodos = parsed.todos || []
    root.viewTasksTodayTodos = parsed.tasksTodayTodos || []
    root.viewDataviewTodos = parsed.dataviewTodos || []
    root.viewTasksTodayOpenCount = parsed.tasksTodayOpenCount || 0
    root.viewTasksTodayDoneCount = parsed.tasksTodayDoneCount || 0
    root.viewDataviewOpenCount = parsed.dataviewOpenCount || 0
    root.viewDataviewDoneCount = parsed.dataviewDoneCount || 0
    root.viewError = parsed.error || ""
    root.viewObsidianUri = parsed.obsidianUri || ""
    root.viewCarryOverCount = parsed.carryOverCount || 0
    root.viewIsToday = parsed.isToday === true
  }

  function applyViewLine(line) {
    var parsed = Model.parseLine(String(line || ""))
    // A response for a day the user already navigated away from is stale;
    // applying it would rewind the panel to the previous viewDate. Error
    // snapshots carry no date and always apply so failures stay visible.
    if (parsed && parsed.date && root.viewDate !== "" && parsed.date !== root.viewDate)
      return
    root.applyViewParsed(parsed)
  }

  // Watch failure paths keep the last data and only mark it stale
  // (statusState "error") instead of resetting to a healthy zeroed state.
  property var lastToggleAt: ({})
  readonly property bool actionBusy: actionProc.running
  // Per-file 1.5s debounce — you measured 1-2s needed to avoid ENOENT.
  // Same file (all Routines tasks share one file) second toggle within 1500ms
  // is dropped; different files can still flip in parallel. Optimistic flip
  // is kept for instant feel, but same-file second tap is ignored.
  function shouldDebounce(key) {
    var now = Date.now()
    var file = String(key).split(":")[0]
    for (var k in root.lastToggleAt) {
      var kFile = String(k).split(":")[0]
      if (kFile === file && now - root.lastToggleAt[k] < 1500) return true
    }
    if (root.lastToggleAt[key] && now - root.lastToggleAt[key] < 1500) return true
    root.lastToggleAt[key] = now
    for (var k2 in root.lastToggleAt) {
      if (now - root.lastToggleAt[k2] > 5000) delete root.lastToggleAt[k2]
    }
    return false
  }
  function optimisticToggle(key, listName) {
    var list = root[listName]
    if (!Array.isArray(list)) return
    for (var i = 0; i < list.length; i++) {
      var it = list[i]
      var itKey = (it.sourceNote || "daily") + ":" + it.line
      if (itKey === key) {
        // flip in place and reassign to trigger QML binding
        var copy = list.slice()
        copy[i] = { line: it.line, checked: !it.checked, text: it.text, depth: it.depth, parentLine: it.parentLine, sourceNote: it.sourceNote }
        root[listName] = copy
        if (listName === "viewTodos") root.viewTodos = copy
        else if (listName === "viewTasksTodayTodos") root.viewTasksTodayTodos = copy
        else if (listName === "viewDataviewTodos") root.viewDataviewTodos = copy
        // also keep bar counts optimistic
        if (listName === "viewTasksTodayTodos" || listName === "viewDataviewTodos") {
          // counts will be corrected by next snapshot; nudge for instant label
          var open = 0, done = 0
          for (var j = 0; j < copy.length; j++) copy[j].checked ? done++ : open++
          if (listName === "viewTasksTodayTodos") { root.viewTasksTodayOpenCount = open; root.viewTasksTodayDoneCount = done }
          else { root.viewDataviewOpenCount = open; root.viewDataviewDoneCount = done }
          // also update bar's today counts if viewDate === today
          if (root.viewDate === root.date) {
            root.tasksTodayOpenCount = root.viewTasksTodayOpenCount
            root.tasksTodayDoneCount = root.viewTasksTodayDoneCount
            root.dataviewOpenCount = root.viewDataviewOpenCount
            root.dataviewDoneCount = root.viewDataviewDoneCount
          }
        } else {
          var o2 = 0, d2 = 0
          for (var k2 = 0; k2 < copy.length; k2++) copy[k2].checked ? d2++ : o2++
          root.viewOpenCount = o2; root.viewDoneCount = d2
          if (root.viewDate === root.date) { root.openCount = o2; root.doneCount = d2 }
        }
        break
      }
    }
    // also keep the other view in sync (watch vs view)
    if (root.viewDate === root.date) {
      // mirror view -> today for instant bar label
      root.tasksTodayTodos = root.viewTasksTodayTodos
      root.dataviewTodos = root.viewDataviewTodos
      root.todos = root.viewTodos
    }
  }

  function runAction(args) {
    if (!args || !args.length) return
    if (actionProc.running) {
      root.actionQueue.push(args)
      return
    }
    root.pendingActionArgs = args
    actionProc.retried = false
    actionProc.command = [root.actionBinary].concat(args)
    actionProc.running = true
  }

  function drainActionQueue() {
    if (actionProc.running) return
    if (root.actionQueue.length === 0) return
    runAction(root.actionQueue.shift())
  }

  function refreshView() {
    var d = root.viewDate || Model.todayIso()
    root.viewDate = d
    root.runAction(["status", "--date", d])
  }

  function shiftView(delta) {
    var next = Model.shiftDate(root.viewDate || Model.todayIso(), delta)
    if (next === "") return
    root.viewDate = next
    root.refreshView()
  }

  function goToday() {
    root.viewDate = Model.todayIso()
    root.refreshView()
  }

  function addTodo(text) {
    var trimmed = String(text || "").trim()
    if (trimmed === "") return
    var d = root.viewDate || Model.todayIso()
    root.runAction(["add", "--date", d, "--text", trimmed])
  }

  function toggleTodo(line, text) {
    var n = Number(line)
    if (!isFinite(n) || n < 1) return
    var key = "daily:" + Math.floor(n)
    if (root.shouldDebounce(key)) return
    var d = root.viewDate || Model.todayIso()
    var args = ["toggle", "--date", d, "--line", String(Math.floor(n))]
    if (typeof text === "string" && text !== "")
      args.push("--expect-text", text)
    root.runAction(args)
  }

  function toggleFile(file, line, text) {
    var n = Number(line)
    if (!isFinite(n) || n < 1) return
    var f = String(file || "").trim()
    if (f === "") return
    var key = f + ":" + Math.floor(n)
    if (root.shouldDebounce(key)) return
    var d = root.viewDate || Model.todayIso()
    var args = ["toggle", "--date", d, "--file", f, "--line", String(Math.floor(n))]
    if (typeof text === "string" && text !== "")
      args.push("--expect-text", text)
    root.runAction(args)
  }

  function carryOver() {
    var d = root.viewDate || Model.todayIso()
    root.runAction(["carry-over", "--date", d])
  }

  function openInObsidian() {
    var d = root.viewDate || Model.todayIso()
    root.runAction(["open", "--date", d])
  }

  function injectPanel() {
    var target = panelItem
    if (!target) return
    if ("bar" in target) target.bar = root.bar
    if ("settings" in target) target.settings = root.settings
    if ("anchorItem" in target) target.anchorItem = button
    if ("hostWidget" in target) target.hostWidget = root
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onBarChanged: injectPanel()
  onSettingsChanged: injectPanel()

  Component.onCompleted: {
    root.viewDate = Model.todayIso()
    watchProc.running = true
  }

  Loader {
    id: panelLoader
    active: true
    source: Qt.resolvedUrl("Panel.qml")
    visible: false
    onLoaded: {
      root.injectPanel()
      Qt.callLater(root.injectPanel)
    }
  }

  Process {
    id: watchProc
    command: [root.watchBinary, "watch"]
    property bool startedOnce: false
    property real startedAtMs: 0
    // A run shorter than this counts as a crash for fallback counting.
    readonly property int minHealthyRunMs: 10000
    stdout: SplitParser {
      onRead: function(line) { root.applyTodayLine(line) }
    }
    onStarted: {
      watchProc.startedOnce = true
      watchProc.startedAtMs = Date.now()
    }
    onExited: {
      // The watch stream only exits on crash or broken pipe; keep the last
      // data but mark it stale instead of showing a healthy zeroed state.
      root.statusState = "error"
      watchRestartTimer.restart()
    }
    onRunningChanged: {
      if (watchProc.running) return
      var failedStart = !watchProc.startedOnce
      var shortLived = !failedStart
        && (Date.now() - watchProc.startedAtMs) < watchProc.minHealthyRunMs
      watchProc.startedOnce = false
      if (failedStart || shortLived) {
        if (root.statusState !== "error") root.statusState = "error"
        root.watchFailures += 1
        if (root.watchFailures >= root.fallbackThreshold) {
          root.watchFailures = 0
          root.watchFallback = !root.watchFallback
        }
      } else {
        // Sustained healthy run before this exit: restart the count.
        root.watchFailures = 0
      }
      watchRestartTimer.restart()
    }
  }

  Timer {
    id: watchRestartTimer
    interval: 5000
    repeat: false
    onTriggered: watchProc.running = true
  }

  Process {
    id: actionProc
    property bool startedOnce: false
    property bool retried: false
    stdout: SplitParser {
      onRead: function(line) {
        // Mutations and status --date refresh the panel view.
        root.applyViewLine(line)
        var parsed = Model.parseLine(String(line || ""))
        // Keep the bar in sync when the action targeted today.
        if (parsed && parsed.date && parsed.date === Model.todayIso())
          root.applyTodayLine(line)
      }
    }
    onStarted: {
      actionProc.startedOnce = true
      root.actionFailures = 0
    }
    onRunningChanged: {
      if (actionProc.running) return
      var failedStart = !actionProc.startedOnce
      actionProc.startedOnce = false
      if (!failedStart || root.pendingActionArgs.length === 0) {
        root.pendingActionArgs = []
        root.drainActionQueue()
        return
      }
      if (actionProc.retried) {
        actionProc.retried = false
        root.pendingActionArgs = []
        root.actionFailures += 1
        if (root.actionFailures >= root.fallbackThreshold) {
          root.actionFailures = 0
          root.actionFallback = !root.actionFallback
        }
        root.drainActionQueue()
        return
      }
      actionProc.retried = true
      actionProc.command = [root.actionBinary].concat(root.pendingActionArgs)
      actionProc.running = true
    }
  }

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.labelText
    foreground: root.statusState === "error" ? root.urgent : Color.bar.text
    activeColor: Color.bar.active
    active: root.statusState === "error"
    horizontalMargin: 8.5
    verticalPadding: 6
    tooltipText: root.tooltipText
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.LeftButton) root.toggle()
    }
  }
}
