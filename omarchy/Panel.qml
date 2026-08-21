import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Native Quattro popup for Obsidian daily note todos (day-switchable).
Panel {
  id: root
  moduleName: "luca.obsidian-daily"
  manageIpc: false

  property var anchorItem: null
  property var hostWidget: null
  readonly property var barIdentity: hostWidget || root
  readonly property var watcher: hostWidget || root
  readonly property bool hasWatcher: watcher !== null && watcher !== root

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  property bool openOnly: hasWatcher ? watcher.openOnlyDefault === true : false
  property string searchText: ""

  readonly property string statusState: hasWatcher ? String(watcher.viewStatusState || "ok") : "ok"
  readonly property string date: hasWatcher ? String(watcher.viewDate || "") : ""
  readonly property bool exists: hasWatcher ? watcher.viewExists === true : false
  readonly property int openCount: hasWatcher ? Number(watcher.viewOpenCount || 0) : 0
  readonly property int doneCount: hasWatcher ? Number(watcher.viewDoneCount || 0) : 0
  readonly property var todos: hasWatcher ? (watcher.viewTodos || []) : []
  readonly property string error: hasWatcher ? String(watcher.viewError || "") : ""
  readonly property int carryOverCount: hasWatcher ? Number(watcher.viewCarryOverCount || 0) : 0
  readonly property bool isToday: hasWatcher ? watcher.viewIsToday === true : true

  readonly property var status: ({
    state: root.statusState,
    date: root.date,
    exists: root.exists,
    openCount: root.openCount,
    doneCount: root.doneCount,
    todos: root.todos,
    error: root.error,
    carryOverCount: root.carryOverCount,
    isToday: root.isToday
  })
  readonly property string metaText: Model.metaLine(status)
  readonly property var shownTodos: Model.visibleTodos(status, root.openOnly, root.searchText)
  readonly property string emptyText: Model.emptyMessage(status, root.openOnly, root.searchText)

  function focusCapture() {
    inputField.forceActiveFocus()
  }

  function addTodo() {
    if (!hasWatcher || typeof watcher.addTodo !== "function") return
    watcher.addTodo(inputField.text)
    inputField.text = ""
  }

  function toggleTodo(line, text) {
    if (!hasWatcher || typeof watcher.toggleTodo !== "function") return
    watcher.toggleTodo(line, text)
  }

  function shiftDay(delta) {
    if (!hasWatcher || typeof watcher.shiftView !== "function") return
    watcher.shiftView(delta)
  }

  function goToday() {
    if (!hasWatcher || typeof watcher.goToday !== "function") return
    watcher.goToday()
  }

  function carryOver() {
    if (!hasWatcher || typeof watcher.carryOver !== "function") return
    watcher.carryOver()
  }

  function openInObsidian() {
    if (!hasWatcher || typeof watcher.openInObsidian !== "function") return
    watcher.openInObsidian()
  }

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.barIdentity, direction)
    return false
  }

  onOpenedChanged: {
    if (root.opened) {
      // A stale search from the previous session would hide todos.
      root.searchText = ""
      Qt.callLater(root.focusCapture)
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(560))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      // While an input field has focus, keys (arrows, backspace, …) belong
      // to the editor. Otherwise Up/Down/j/k scroll the todo list and `/`
      // jumps to the search field.
      blocked: inputField.activeFocus || searchField.activeFocus
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(t) {
        if (t === "/") searchField.forceActiveFocus()
      }
      onMoveRequested: function(dx, dy) {
        if (dy === 0) return
        panelFlick.contentY = Math.max(0, Math.min(
          panelFlick.contentY + dy * Style.space(44),
          Math.max(0, panelFlick.contentHeight - panelFlick.height)))
      }

      Flickable {
        id: panelFlick
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        Column {
          id: column
          width: panelFlick.width
          spacing: Style.space(10)

          PanelHero {
            width: parent.width
            title: "Obsidian Daily"
            meta: root.metaText
            detail: root.date !== "" ? root.date : "Daily note"
            foreground: root.foreground
            fontFamily: root.fontFamily

            iconComponent: Component {
              Text {
                text: "\u2610"
                textFormat: Text.PlainText
                color: root.statusState === "error" ? root.urgent : root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.display
              }
            }

            trailingControl: Component {
              Row {
                spacing: Style.space(4)

                PanelActionButton {
                  iconText: "\u25C0"
                  tooltipText: "Previous day"
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  onClicked: root.shiftDay(-1)
                }

                PanelActionButton {
                  iconText: "\u25CF"
                  tooltipText: "Today"
                  visible: !root.isToday
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  onClicked: root.goToday()
                }

                PanelActionButton {
                  iconText: "\u25B6"
                  tooltipText: "Next day"
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  onClicked: root.shiftDay(1)
                }

                PanelActionButton {
                  iconText: "\u2398"
                  tooltipText: "Open in Obsidian"
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  onClicked: root.openInObsidian()
                }
              }
            }
          }

          RowLayout {
            width: parent.width
            spacing: Style.space(6)

            TextField {
              id: inputField
              Layout.fillWidth: true
              placeholderText: "Add a todo…"
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              onAccepted: root.addTodo()
              Keys.onEscapePressed: root.close()
              // "/" in the empty add field jumps to search instead of
              // inserting; with text present it types normally.
              Keys.onPressed: function(event) {
                if (event.text === "/" && inputField.text === "") {
                  searchField.forceActiveFocus()
                  event.accepted = true
                }
              }
            }

            PanelActionButton {
              iconText: "+"
              tooltipText: "Add todo"
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: root.addTodo()
            }
          }

          RowLayout {
            width: parent.width
            spacing: Style.space(6)

            TextField {
              id: searchField
              Layout.fillWidth: true
              placeholderText: "Search todos… ( / )"
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              text: root.searchText
              onTextEdited: root.searchText = searchField.text
              Keys.onEscapePressed: {
                if (root.searchText !== "") {
                  root.searchText = ""
                  inputField.forceActiveFocus()
                } else {
                  root.close()
                }
              }
            }

            PanelActionButton {
              visible: root.searchText !== ""
              iconText: "\u2715"
              tooltipText: "Clear search"
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: {
                root.searchText = ""
                searchField.forceActiveFocus()
              }
            }
          }

          Row {
            width: parent.width
            spacing: Style.space(8)

            Text {
              text: root.openOnly ? "Open only" : "All todos"
              textFormat: Text.PlainText
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.openOnly = !root.openOnly
              }
            }

            Text {
              visible: root.isToday && root.carryOverCount > 0
              text: "Carry over " + root.carryOverCount
              textFormat: Text.PlainText
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.caption
              font.underline: true
              MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: root.carryOver()
              }
            }
          }

          Text {
            width: parent.width
            visible: root.shownTodos.length === 0
            text: root.emptyText
            textFormat: Text.PlainText
            color: Qt.darker(root.foreground, 1.4)
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            wrapMode: Text.WordWrap
          }

          Repeater {
            model: root.shownTodos

            delegate: RowLayout {
              width: column.width
              spacing: Style.space(8)

              // Indent nested todos by their normalized depth.
              Item {
                Layout.preferredWidth: (modelData.depth || 0) * Style.space(16)
                Layout.preferredHeight: 1
              }

              Text {
                text: modelData.checked ? "\u2611" : "\u2610"
                textFormat: Text.PlainText
                color: modelData.checked ? Qt.darker(root.foreground, 1.5) : root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
                MouseArea {
                  anchors.fill: parent
                  cursorShape: Qt.PointingHandCursor
                  onClicked: root.toggleTodo(modelData.line, modelData.text)
                }
              }

              Text {
                Layout.fillWidth: true
                text: modelData.text
                textFormat: Text.PlainText
                color: modelData.checked ? Qt.darker(root.foreground, 1.5) : root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
                font.strikeout: modelData.checked === true
                elide: Text.ElideRight
                wrapMode: Text.NoWrap
                MouseArea {
                  anchors.fill: parent
                  cursorShape: Qt.PointingHandCursor
                  onClicked: root.toggleTodo(modelData.line, modelData.text)
                }
              }
            }
          }
        }
      }
    }
  }
}
