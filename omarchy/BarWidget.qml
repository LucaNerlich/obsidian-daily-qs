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
  property string errorCode: ""
  property string obsidianUri: ""
  property int carryOverCount: 0
  property bool isToday: true
  property string templateName: ""
  property bool createdFromTemplate: false

  // Panel view (may differ from today when day-switching).
  property string viewDate: ""
  property string viewStatusState: "ok"
  property string viewPath: ""
  property bool viewExists: false
  property int viewOpenCount: 0
  property int viewDoneCount: 0
  property var viewTodos: []
  property string viewError: ""
  property string viewErrorCode: ""
  property string viewObsidianUri: ""
  property int viewCarryOverCount: 0
  property bool viewIsToday: true
  property string viewTemplateName: ""
  property bool viewCreatedFromTemplate: false
  property var weekDays: []

  readonly property string homeDir: Quickshell.env("HOME") || ""
  readonly property string vaultPathSetting: String(setting("vaultPath", "") || "")
  readonly property string vaultPath: Model.expandPath(vaultPathSetting, homeDir)
  readonly property string todoHeading: String(setting("todoHeading", "") || "").trim()
  readonly property bool openOnlyDefault: setting("openOnly", false) === true
  readonly property bool hideWhenDone: setting("hideWhenDone", false) === true
  readonly property bool hideWhenEmpty: setting("hideWhenEmpty", false) === true

  readonly property var status: ({
    state: root.statusState,
    date: root.date,
    path: root.path,
    exists: root.exists,
    openCount: root.openCount,
    doneCount: root.doneCount,
    todos: root.todos,
    error: root.error,
    errorCode: root.errorCode,
    obsidianUri: root.obsidianUri,
    carryOverCount: root.carryOverCount,
    isToday: root.isToday,
    templateName: root.templateName,
    createdFromTemplate: root.createdFromTemplate
  })
  readonly property string labelText: Model.labelText(status)
  readonly property string tooltipText: Model.tooltipText(status)
  readonly property real progress: Model.progressRatio(status)
  readonly property bool conceal: Model.shouldConceal(status, hideWhenDone, hideWhenEmpty)
  readonly property color urgent: bar ? bar.urgent : Color.urgent

  function vaultArgs() {
    var args = []
    if (root.vaultPath !== "")
      args.push("--vault", root.vaultPath)
    return args
  }

  function headingArgs() {
    var args = []
    if (root.todoHeading !== "")
      args.push("--heading", root.todoHeading)
    return args
  }

  function withVault(args) {
    return root.vaultArgs().concat(args)
  }

  function restartWatch() {
    watchProc.running = false
    watchRestartTimer.interval = 200
    watchRestartTimer.restart()
  }

  function saveVaultPath(path) {
    var expanded = Model.expandPath(path, root.homeDir)
    if (expanded === "") return
    settingsProc.command = ["omarchy", "bar", "set", "luca.obsidian-daily", "vaultPath", expanded]
    settingsProc.running = true
  }

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
    root.errorCode = parsed.errorCode || ""
    root.obsidianUri = parsed.obsidianUri || ""
    root.carryOverCount = parsed.carryOverCount || 0
    root.isToday = parsed.isToday === true
    root.templateName = parsed.templateName || ""
    root.createdFromTemplate = parsed.createdFromTemplate === true
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
    root.viewErrorCode = parsed.errorCode || ""
    root.viewObsidianUri = parsed.obsidianUri || ""
    root.viewCarryOverCount = parsed.carryOverCount || 0
    root.viewIsToday = parsed.isToday === true
    root.viewTemplateName = parsed.templateName || ""
    root.viewCreatedFromTemplate = parsed.createdFromTemplate === true
  }

  function applyViewLine(line) {
    var parsed = Model.parseLine(String(line || ""))
    if (parsed && parsed.date && root.viewDate !== "" && parsed.date !== root.viewDate)
      return
    root.applyViewParsed(parsed)
  }

  function runAction(args) {
    if (!args || !args.length) return
    if (actionProc.running) {
      root.actionQueue.push(args)
      return
    }
    var full = root.withVault(args)
    root.pendingActionArgs = full
    actionProc.retried = false
    actionProc.command = [root.actionBinary].concat(full)
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
    root.runAction(["status", "--date", d].concat(root.headingArgs()))
    root.refreshWeek()
  }

  function refreshWeek() {
    var d = root.viewDate || Model.todayIso()
    weekProc.command = [root.actionBinary].concat(root.withVault(["week", "--date", d]))
    weekProc.running = true
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

  function goToDate(dateStr) {
    var d = String(dateStr || "")
    if (!/^\d{4}-\d{2}-\d{2}$/.test(d)) return
    root.viewDate = d
    root.refreshView()
  }

  function addTodo(text, underLine) {
    var trimmed = String(text || "").trim()
    if (trimmed === "") return
    var d = root.viewDate || Model.todayIso()
    var args = ["add", "--date", d, "--text", trimmed]
    var n = Number(underLine)
    if (isFinite(n) && n >= 1)
      args.push("--under-line", String(Math.floor(n)))
    root.runAction(args)
  }

  function toggleTodo(line, text) {
    var n = Number(line)
    if (!isFinite(n) || n < 1) return
    var d = root.viewDate || Model.todayIso()
    var args = ["toggle", "--date", d, "--line", String(Math.floor(n))]
    if (typeof text === "string" && text !== "")
      args.push("--expect-text", text)
    root.runAction(args)
  }

  function editTodo(line, expectText, newText) {
    var n = Number(line)
    if (!isFinite(n) || n < 1) return
    var trimmed = String(newText || "").trim()
    if (trimmed === "") return
    var d = root.viewDate || Model.todayIso()
    var args = ["edit", "--date", d, "--line", String(Math.floor(n)), "--text", trimmed]
    if (typeof expectText === "string" && expectText !== "")
      args.push("--expect-text", expectText)
    root.runAction(args)
  }

  function deleteTodo(line, text, withChildren) {
    var n = Number(line)
    if (!isFinite(n) || n < 1) return
    var d = root.viewDate || Model.todayIso()
    var args = ["delete", "--date", d, "--line", String(Math.floor(n))]
    if (typeof text === "string" && text !== "")
      args.push("--expect-text", text)
    if (withChildren === true)
      args.push("--with-children")
    root.runAction(args)
  }

  function indentTodo(line, text, delta) {
    var n = Number(line)
    if (!isFinite(n) || n < 1) return
    var d = root.viewDate || Model.todayIso()
    var cmd = Number(delta) < 0 ? "outdent" : "indent"
    var args = [cmd, "--date", d, "--line", String(Math.floor(n))]
    if (typeof text === "string" && text !== "")
      args.push("--expect-text", text)
    root.runAction(args)
  }

  function undoLast() {
    root.runAction(["undo"])
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
  onSettingsChanged: {
    injectPanel()
    root.restartWatch()
  }
  onVaultPathChanged: root.restartWatch()
  onTodoHeadingChanged: root.restartWatch()

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
    id: settingsProc
  }

  Process {
    id: weekProc
    stdout: SplitParser {
      onRead: function(line) {
        var parsed = Model.parseWeekLine(String(line || ""))
        if (parsed && parsed.state === "ok")
          root.weekDays = parsed.days || []
      }
    }
  }

  Process {
    id: watchProc
    command: [root.watchBinary].concat(root.withVault(["watch"].concat(root.headingArgs())))
    property bool startedOnce: false
    property real startedAtMs: 0
    readonly property int minHealthyRunMs: 10000
    stdout: SplitParser {
      onRead: function(line) { root.applyTodayLine(line) }
    }
    onStarted: {
      watchProc.startedOnce = true
      watchProc.startedAtMs = Date.now()
    }
    onExited: {
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
        root.watchFailures = 0
      }
      watchRestartTimer.interval = 5000
      watchRestartTimer.restart()
    }
  }

  Timer {
    id: watchRestartTimer
    interval: 5000
    repeat: false
    onTriggered: {
      watchProc.command = [root.watchBinary].concat(root.withVault(["watch"].concat(root.headingArgs())))
      watchProc.running = true
    }
  }

  Process {
    id: actionProc
    property bool startedOnce: false
    property bool retried: false
    stdout: SplitParser {
      onRead: function(line) {
        // Week summaries are handled by weekProc; ignore non-snapshot lines.
        if (String(line || "").indexOf('"days"') !== -1 && String(line || "").indexOf('"todos"') === -1)
          return
        root.applyViewLine(line)
        var parsed = Model.parseLine(String(line || ""))
        if (parsed && parsed.date && parsed.date === Model.todayIso())
          root.applyTodayLine(line)
        if (parsed && parsed.state === "ok")
          root.refreshWeek()
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
    concealed: root.conceal
    fixedWidth: vertical ? -1 : contentRow.implicitWidth + Style.space(16)
    foreground: root.statusState === "error" ? root.urgent : Color.bar.text
    activeColor: Color.bar.active
    active: root.statusState === "error" || root.carryOverCount > 0
    horizontalMargin: 8.5
    verticalPadding: 6
    tooltipText: root.tooltipText
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.LeftButton) root.toggle()
      else if (buttonCode === Qt.MiddleButton) root.openInObsidian()
      else if (buttonCode === Qt.RightButton) root.openInObsidian()
    }

    readonly property color iconColor: button.active && button.useActiveColor
      ? button.activeColor : button.foreground

    Row {
      id: contentRow
      anchors.centerIn: parent
      spacing: Style.space(5)

      Item {
        width: Style.space(14)
        height: width
        anchors.verticalCenter: parent.verticalCenter

        // Progress ring behind the mark.
        Rectangle {
          anchors.fill: parent
          radius: width / 2
          color: "transparent"
          border.width: 1.5
          border.color: Qt.rgba(button.iconColor.r, button.iconColor.g, button.iconColor.b, 0.25)
        }
        Rectangle {
          anchors.fill: parent
          radius: width / 2
          color: "transparent"
          border.width: 1.5
          border.color: button.iconColor
          // Approximate fill via opacity when progress high; full ring otherwise.
          opacity: 0.15 + 0.85 * root.progress
          visible: root.statusState === "ok" && root.exists
        }

        ObsidianIcon {
          anchors.centerIn: parent
          iconSize: Style.space(11)
          color: button.iconColor
        }
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
