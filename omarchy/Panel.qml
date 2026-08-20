import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import qs.Commons
import qs.Ui
import "Model.js" as Model

// Native Quattro popup for today's Obsidian daily note todos.
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

  readonly property string statusState: hasWatcher ? String(watcher.statusState || "ok") : "ok"
  readonly property string date: hasWatcher ? String(watcher.date || "") : ""
  readonly property bool exists: hasWatcher ? watcher.exists === true : false
  readonly property int openCount: hasWatcher ? Number(watcher.openCount || 0) : 0
  readonly property int doneCount: hasWatcher ? Number(watcher.doneCount || 0) : 0
  readonly property var todos: hasWatcher ? (watcher.todos || []) : []
  readonly property string error: hasWatcher ? String(watcher.error || "") : ""

  readonly property var status: ({
    state: root.statusState,
    date: root.date,
    exists: root.exists,
    openCount: root.openCount,
    doneCount: root.doneCount,
    todos: root.todos,
    error: root.error
  })
  readonly property string metaText: Model.metaLine(status)
  readonly property string emptyText: Model.emptyMessage(status)

  function addTodo() {
    if (!hasWatcher || typeof watcher.addTodo !== "function") return
    watcher.addTodo(inputField.text)
    inputField.text = ""
  }

  function toggleTodo(line) {
    if (!hasWatcher || typeof watcher.toggleTodo !== "function") return
    watcher.toggleTodo(line)
  }

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.barIdentity, direction)
    return false
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(360))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(520))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
    }

    Column {
      id: column
      width: parent.width
      spacing: Style.space(10)

      PanelHero {
        width: parent.width
        title: "Obsidian Daily"
        meta: root.metaText
        detail: root.date !== "" ? root.date : "Today's daily note"
        foreground: root.foreground
        fontFamily: root.fontFamily

        iconComponent: Component {
          Text {
            text: "\u2610"
            color: root.statusState === "error" ? root.urgent : root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.display
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
        }

        PanelActionButton {
          iconText: "+"
          tooltipText: "Add todo"
          foreground: root.foreground
          fontFamily: root.fontFamily
          onClicked: root.addTodo()
        }
      }

      Text {
        width: parent.width
        visible: root.todos.length === 0
        text: root.emptyText
        color: Qt.darker(root.foreground, 1.4)
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        wrapMode: Text.WordWrap
      }

      Repeater {
        model: root.todos

        delegate: RowLayout {
          width: column.width
          spacing: Style.space(8)

          Text {
            text: modelData.checked ? "\u2611" : "\u2610"
            color: modelData.checked ? Qt.darker(root.foreground, 1.5) : root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            MouseArea {
              anchors.fill: parent
              cursorShape: Qt.PointingHandCursor
              onClicked: root.toggleTodo(modelData.line)
            }
          }

          Text {
            Layout.fillWidth: true
            text: modelData.text
            color: modelData.checked ? Qt.darker(root.foreground, 1.5) : root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            font.strikeout: modelData.checked === true
            elide: Text.ElideRight
            wrapMode: Text.NoWrap
            MouseArea {
              anchors.fill: parent
              cursorShape: Qt.PointingHandCursor
              onClicked: root.toggleTodo(modelData.line)
            }
          }
        }
      }
    }
  }
}
