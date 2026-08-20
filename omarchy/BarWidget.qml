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

  readonly property var panelItem: panelLoader.item
  readonly property bool opened: panelItem ? panelItem.opened === true : false

  property string statusState: "ok"
  property string date: ""
  property string path: ""
  property bool exists: false
  property int openCount: 0
  property int doneCount: 0
  property var todos: []
  property string error: ""

  readonly property var status: ({
    state: root.statusState,
    date: root.date,
    path: root.path,
    exists: root.exists,
    openCount: root.openCount,
    doneCount: root.doneCount,
    todos: root.todos,
    error: root.error
  })
  readonly property string labelText: Model.labelText(status)
  readonly property string tooltipText: Model.tooltipText(status)
  readonly property color urgent: bar ? bar.urgent : Color.urgent

  function open() { if (panelItem) panelItem.open() }
  function close() { if (panelItem) panelItem.close() }
  function toggle() { if (panelItem) panelItem.toggle() }

  function applyLine(line) {
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
  }

  function clearStatus() {
    root.statusState = "ok"
    root.date = ""
    root.path = ""
    root.exists = false
    root.openCount = 0
    root.doneCount = 0
    root.todos = []
    root.error = ""
  }

  function runAction(args) {
    if (actionProc.running) return
    if (!args || !args.length) return
    root.pendingActionArgs = args
    actionProc.retried = false
    actionProc.command = [root.actionBinary].concat(args)
    actionProc.running = true
  }

  function addTodo(text) {
    var trimmed = String(text || "").trim()
    if (trimmed === "") return
    root.runAction(["add", "--text", trimmed])
  }

  function toggleTodo(line) {
    var n = Number(line)
    if (!isFinite(n) || n < 1) return
    root.runAction(["toggle", "--line", String(Math.floor(n))])
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

  Component.onCompleted: watchProc.running = true

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
    stdout: SplitParser {
      onRead: function(line) { root.applyLine(line) }
    }
    onStarted: {
      watchProc.startedOnce = true
      root.watchFailures = 0
    }
    onExited: {
      root.clearStatus()
      watchRestartTimer.restart()
    }
    onRunningChanged: {
      if (watchProc.running) return
      var failedStart = !watchProc.startedOnce
      watchProc.startedOnce = false
      if (failedStart) {
        root.clearStatus()
        root.watchFailures += 1
        if (root.watchFailures >= root.fallbackThreshold) {
          root.watchFailures = 0
          root.watchFallback = !root.watchFallback
        }
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
      onRead: function(line) { root.applyLine(line) }
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
