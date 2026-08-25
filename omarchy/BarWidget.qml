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
    var d = root.viewDate || Model.todayIso()
    var args = ["toggle", "--date", d, "--line", String(Math.floor(n))]
    // Guard against a stale snapshot flipping the wrong checkbox.
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
    text: ""
    labelVisible: false
    hasVisualContent: true
    fixedWidth: vertical ? -1 : contentRow.implicitWidth + Style.space(16)
    foreground: root.statusState === "error" ? root.urgent : Color.bar.text
    activeColor: Color.bar.active
    active: root.statusState === "error"
    horizontalMargin: 8.5
    verticalPadding: 6
    tooltipText: root.tooltipText
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.LeftButton) root.toggle()
    }

    readonly property color iconColor: button.active && button.useActiveColor
      ? button.activeColor : button.foreground

    Row {
      id: contentRow
      anchors.centerIn: parent
      spacing: Style.space(5)

      ObsidianIcon {
        iconSize: Style.space(13)
        anchors.verticalCenter: parent.verticalCenter
        color: button.iconColor
      }

      Text {
        visible: !(button.vertical)
        anchors.verticalCenter: parent.verticalCenter
        text: root.labelText
        textFormat: Text.PlainText
        color: button.iconColor
        font.family: button.fontFamily
        font.pixelSize: Style.font.caption
        font.bold: true
      }
    }
  }
}
