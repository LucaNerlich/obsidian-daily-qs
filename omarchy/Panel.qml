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
  readonly property color dim: Qt.darker(foreground, 1.45)
  readonly property color accent: Color.accent
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  property bool openOnly: hasWatcher ? watcher.openOnlyDefault === true : false
  property string searchText: ""
  property int selectedIndex: -1
  property int editingLine: -1
  property string editingOriginal: ""

  readonly property string statusState: hasWatcher ? String(watcher.viewStatusState || "ok") : "ok"
  readonly property string date: hasWatcher ? String(watcher.viewDate || "") : ""
  readonly property bool exists: hasWatcher ? watcher.viewExists === true : false
  readonly property int openCount: hasWatcher ? Number(watcher.viewOpenCount || 0) : 0
  readonly property int doneCount: hasWatcher ? Number(watcher.viewDoneCount || 0) : 0
  readonly property var todos: hasWatcher ? (watcher.viewTodos || []) : []
  readonly property string error: hasWatcher ? String(watcher.viewError || "") : ""
  readonly property string errorCode: hasWatcher ? String(watcher.viewErrorCode || "") : ""
  readonly property int carryOverCount: hasWatcher ? Number(watcher.viewCarryOverCount || 0) : 0
  readonly property bool isToday: hasWatcher ? watcher.viewIsToday === true : true
  readonly property string templateName: hasWatcher ? String(watcher.viewTemplateName || "") : ""
  readonly property var weekDays: hasWatcher ? (watcher.weekDays || []) : []

  readonly property var status: ({
    state: root.statusState,
    date: root.date,
    exists: root.exists,
    openCount: root.openCount,
    doneCount: root.doneCount,
    todos: root.todos,
    error: root.error,
    errorCode: root.errorCode,
    carryOverCount: root.carryOverCount,
    isToday: root.isToday,
    templateName: root.templateName
  })
  readonly property bool vaultSetupError: Model.isVaultSetupError(status)
  readonly property string metaText: Model.metaLine(status)
  readonly property var shownTodos: Model.visibleTodos(status, root.openOnly, root.searchText)
  readonly property string emptyText: Model.emptyMessage(status, root.openOnly, root.searchText)
  readonly property string sectionTitle: root.openOnly ? "OPEN TODOS" : "TODOS"
  readonly property color iconColor: root.statusState === "error" ? root.urgent : root.foreground
  readonly property var selectedTodo: (selectedIndex >= 0 && selectedIndex < shownTodos.length)
    ? shownTodos[selectedIndex] : null

  function focusCapture() {
    if (root.vaultSetupError)
      vaultPathField.forceActiveFocus()
    else
      inputField.forceActiveFocus()
  }

  function addTodo(underSelected) {
    if (!hasWatcher || typeof watcher.addTodo !== "function") return
    var underLine = undefined
    if (underSelected === true && root.selectedTodo)
      underLine = root.selectedTodo.line
    watcher.addTodo(inputField.text, underLine)
    inputField.text = ""
  }

  function toggleTodo(line, text) {
    if (!hasWatcher || typeof watcher.toggleTodo !== "function") return
    watcher.toggleTodo(line, text)
  }

  function editTodo(line, expectText, newText) {
    if (!hasWatcher || typeof watcher.editTodo !== "function") return
    watcher.editTodo(line, expectText, newText)
  }

  function deleteSelected() {
    if (!root.selectedTodo) return
    if (!hasWatcher || typeof watcher.deleteTodo !== "function") return
    watcher.deleteTodo(root.selectedTodo.line, root.selectedTodo.text, true)
  }

  function indentSelected(delta) {
    if (!root.selectedTodo) return
    if (!hasWatcher || typeof watcher.indentTodo !== "function") return
    watcher.indentTodo(root.selectedTodo.line, root.selectedTodo.text, delta)
  }

  function undoLast() {
    if (!hasWatcher || typeof watcher.undoLast !== "function") return
    watcher.undoLast()
  }

  function shiftDay(delta) {
    if (!hasWatcher || typeof watcher.shiftView !== "function") return
    root.cancelEdit()
    watcher.shiftView(delta)
  }

  function goToday() {
    if (!hasWatcher || typeof watcher.goToday !== "function") return
    root.cancelEdit()
    watcher.goToday()
  }

  function goToDate(dateStr) {
    if (!hasWatcher || typeof watcher.goToDate !== "function") return
    root.cancelEdit()
    watcher.goToDate(dateStr)
  }

  function carryOver() {
    if (!hasWatcher || typeof watcher.carryOver !== "function") return
    watcher.carryOver()
  }

  function openInObsidian() {
    if (!hasWatcher || typeof watcher.openInObsidian !== "function") return
    watcher.openInObsidian()
  }

  function saveVaultPath(path) {
    if (!hasWatcher || typeof watcher.saveVaultPath !== "function") return
    watcher.saveVaultPath(path)
  }

  function switchPanel(direction) {
    if (root.bar && typeof root.bar.switchPanelFrom === "function")
      return root.bar.switchPanelFrom(root.barIdentity, direction)
    return false
  }

  function cancelEdit() {
    root.editingLine = -1
    root.editingOriginal = ""
  }

  function startEdit(todo) {
    if (!todo) return
    root.editingLine = todo.line
    root.editingOriginal = todo.text
  }

  function commitEdit(line, newText) {
    var trimmed = String(newText || "").trim()
    if (trimmed === "" || trimmed === root.editingOriginal) {
      root.cancelEdit()
      return
    }
    root.editTodo(line, root.editingOriginal, trimmed)
    root.cancelEdit()
  }

  function moveSelection(dy) {
    if (root.shownTodos.length === 0) {
      root.selectedIndex = -1
      return
    }
    if (root.selectedIndex < 0)
      root.selectedIndex = dy > 0 ? 0 : root.shownTodos.length - 1
    else
      root.selectedIndex = Math.max(0, Math.min(root.shownTodos.length - 1, root.selectedIndex + dy))
    root.scrollSelectedIntoView()
  }

  function activateSelected() {
    if (!root.selectedTodo) return
    root.toggleTodo(root.selectedTodo.line, root.selectedTodo.text)
  }

  function scrollItemIntoView(item) {
    if (!panelFlick || !item) return
    Qt.callLater(function() {
      if (!item) return
      var margin = Style.space(6)
      var point = item.mapToItem(panelFlick.contentItem, 0, 0)
      var top = point.y
      var bottom = top + item.height
      var viewTop = panelFlick.contentY
      var viewBottom = viewTop + panelFlick.height
      var maxY = Math.max(0, panelFlick.contentHeight - panelFlick.height)
      if (top < viewTop + margin)
        panelFlick.contentY = Math.max(0, top - margin)
      else if (bottom > viewBottom - margin)
        panelFlick.contentY = Math.min(maxY, bottom + margin - panelFlick.height)
    })
  }

  function scrollSelectedIntoView() {
    if (root.selectedIndex < 0 || root.selectedIndex >= root.shownTodos.length) return
    var kids = todoColumn.children
    for (var i = 0; i < kids.length; i++) {
      if (kids[i].todoIndex === root.selectedIndex) {
        root.scrollItemIntoView(kids[i])
        return
      }
    }
  }

  function clampSelection() {
    if (root.shownTodos.length === 0) {
      root.selectedIndex = -1
      return
    }
    if (root.selectedIndex >= root.shownTodos.length)
      root.selectedIndex = root.shownTodos.length - 1
  }

  onShownTodosChanged: {
    root.clampSelection()
    if (root.editingLine >= 0) {
      var stillThere = false
      for (var i = 0; i < root.shownTodos.length; i++) {
        if (root.shownTodos[i].line === root.editingLine) {
          stillThere = true
          break
        }
      }
      if (!stillThere) root.cancelEdit()
    }
  }

  onDateChanged: {
    root.selectedIndex = -1
    root.cancelEdit()
    if (panelFlick) panelFlick.contentY = 0
  }

  onOpenedChanged: {
    if (root.opened) {
      root.searchText = ""
      root.selectedIndex = -1
      root.cancelEdit()
      if (panelFlick) panelFlick.contentY = 0
      Qt.callLater(root.focusCapture)
    } else {
      root.cancelEdit()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: root.anchorItem
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(400))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(580))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: inputField.activeFocus || searchField.activeFocus
        || vaultPathField.activeFocus || root.editingLine >= 0
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onActivateRequested: root.activateSelected()
      onDeleteRequested: root.deleteSelected()
      onTextKey: function(t) {
        if (t === "/") {
          searchField.forceActiveFocus()
        } else if (t === "[") {
          root.indentSelected(-1)
        } else if (t === "]") {
          root.indentSelected(1)
        } else if (t === "u" || t === "U") {
          root.undoLast()
        } else if (t === "e" || t === "E") {
          root.startEdit(root.selectedTodo)
        }
      }
      onMoveRequested: function(dx, dy) {
        if (dy === 0) return
        root.moveSelection(dy)
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
          spacing: Style.space(12)

          PanelHero {
            width: parent.width
            title: "Obsidian Daily"
            meta: root.metaText
            detail: root.date !== "" ? root.date : "Daily note"
            foreground: root.foreground
            fontFamily: root.fontFamily

            iconComponent: Component {
              ObsidianIcon {
                iconSize: Style.font.display
                color: root.iconColor
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
                  iconText: "\u2197"
                  tooltipText: "Open in Obsidian"
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  onClicked: root.openInObsidian()
                }
              }
            }
          }

          PanelSeparator {
            width: parent.width
            foreground: root.foreground
          }

          // Guided vault setup when the path is missing or invalid.
          Column {
            width: parent.width
            spacing: Style.space(10)
            visible: root.vaultSetupError

            Text {
              width: parent.width
              text: root.error !== ""
                ? root.error
                : "Set your Obsidian vault path to get started."
              textFormat: Text.PlainText
              color: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              wrapMode: Text.WordWrap
            }

            TextField {
              id: vaultPathField
              width: parent.width
              placeholderText: "~/Documents/vault"
              foreground: root.foreground
              font.family: root.fontFamily
              font.pixelSize: Style.font.body
              Keys.onEscapePressed: root.close()
              onAccepted: root.saveVaultPath(vaultPathField.text)
            }

            Button {
              text: "Save vault path"
              bordered: true
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: root.saveVaultPath(vaultPathField.text)
            }
          }

          // Main todo chrome — hidden while vault setup is required.
          Column {
            width: parent.width
            spacing: Style.space(12)
            visible: !root.vaultSetupError

            // Week strip: seven day cells with open-count dots.
            RowLayout {
              width: parent.width
              spacing: Style.space(4)
              visible: root.weekDays.length > 0

              Repeater {
                model: root.weekDays

                delegate: CursorSurface {
                  id: dayCell
                  required property var modelData
                  Layout.fillWidth: true
                  Layout.preferredHeight: Style.space(48)
                  foreground: root.foreground
                  accent: root.accent
                  hasCursor: false
                  current: modelData.date === root.date
                  bordered: true

                  MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.goToDate(modelData.date)
                  }

                  Column {
                    anchors.centerIn: parent
                    spacing: Style.space(3)

                    Text {
                      anchors.horizontalCenter: parent.horizontalCenter
                      text: Model.weekdayShort(modelData.date)
                      textFormat: Text.PlainText
                      color: modelData.isToday ? root.accent : root.dim
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.caption
                      font.bold: modelData.date === root.date
                    }

                    Text {
                      anchors.horizontalCenter: parent.horizontalCenter
                      text: {
                        var parts = String(modelData.date || "").split("-")
                        return parts.length === 3 ? String(Number(parts[2])) : ""
                      }
                      textFormat: Text.PlainText
                      color: modelData.date === root.date ? root.foreground : root.dim
                      font.family: root.fontFamily
                      font.pixelSize: Style.font.bodySmall
                      font.bold: modelData.date === root.date
                    }

                    Rectangle {
                      anchors.horizontalCenter: parent.horizontalCenter
                      width: Style.space(6)
                      height: Style.space(6)
                      radius: width / 2
                      visible: modelData.openCount > 0
                      color: modelData.date === root.date ? root.accent : root.foreground
                      opacity: modelData.exists ? 1.0 : 0.45
                    }

                    Item {
                      width: Style.space(6)
                      height: Style.space(6)
                      visible: modelData.openCount <= 0
                    }
                  }
                }
              }
            }

            Column {
              width: parent.width
              spacing: Style.space(8)

              RowLayout {
                width: parent.width
                spacing: Style.space(6)

                TextField {
                  id: inputField
                  Layout.fillWidth: true
                  placeholderText: "Add a todo…"
                  foreground: root.foreground
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.body
                  onAccepted: root.addTodo(false)
                  Keys.onEscapePressed: root.close()
                  Keys.onPressed: function(event) {
                    if ((event.key === Qt.Key_Return || event.key === Qt.Key_Enter)
                        && (event.modifiers & Qt.ShiftModifier)) {
                      root.addTodo(true)
                      event.accepted = true
                      return
                    }
                    if (event.text === "/" && inputField.text === "") {
                      searchField.forceActiveFocus()
                      event.accepted = true
                    }
                  }
                }

                PanelActionButton {
                  iconText: "+"
                  tooltipText: "Add todo"
                  bordered: true
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  onClicked: root.addTodo(false)
                }
              }

              RowLayout {
                width: parent.width
                spacing: Style.space(6)

                TextField {
                  id: searchField
                  Layout.fillWidth: true
                  placeholderText: "Search todos… ( / )"
                  foreground: root.foreground
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

              RowLayout {
                width: parent.width
                spacing: Style.space(10)

                Text {
                  text: "Open only"
                  textFormat: Text.PlainText
                  color: root.foreground
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                  font.bold: true
                  Layout.alignment: Qt.AlignVCenter
                  MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.openOnly = !root.openOnly
                  }
                }

                ToggleSwitch {
                  checked: root.openOnly
                  foreground: root.foreground
                  trackHeight: Style.space(18)
                  Layout.alignment: Qt.AlignVCenter
                  onToggled: root.openOnly = !root.openOnly
                }

                Item { Layout.fillWidth: true }

                Button {
                  visible: root.isToday && root.carryOverCount > 0
                  text: "Carry over " + root.carryOverCount
                  bordered: true
                  foreground: root.foreground
                  fontFamily: root.fontFamily
                  fontSize: Style.font.caption
                  horizontalPadding: Style.space(10)
                  verticalPadding: Style.space(4)
                  onClicked: root.carryOver()
                }
              }
            }

            PanelSeparator {
              width: parent.width
              foreground: root.foreground
            }

            Column {
              width: parent.width
              spacing: Style.space(8)

              PanelSectionHeader {
                width: parent.width
                text: root.sectionTitle
                foreground: root.foreground
                fontFamily: root.fontFamily
              }

              Text {
                width: parent.width
                visible: root.shownTodos.length === 0
                topPadding: Style.space(8)
                bottomPadding: Style.space(8)
                text: root.emptyText
                textFormat: Text.PlainText
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
              }

              Column {
                id: todoColumn
                width: parent.width
                spacing: Style.space(4)
                visible: root.shownTodos.length > 0

                Repeater {
                  model: root.shownTodos

                  delegate: CursorSurface {
                    id: todoRow
                    required property var modelData
                    required property int index
                    property int todoIndex: index
                    property bool hovered: todoMouse.containsMouse
                    width: todoColumn.width
                    implicitHeight: todoInner.implicitHeight + Style.space(8)
                    foreground: root.foreground
                    accent: root.accent
                    hasCursor: root.selectedIndex === index || todoRow.hovered
                    current: false

                    MouseArea {
                      id: todoMouse
                      anchors.fill: parent
                      hoverEnabled: true
                      enabled: root.editingLine !== modelData.line
                      cursorShape: Qt.PointingHandCursor
                      acceptedButtons: Qt.LeftButton
                      onClicked: {
                        root.selectedIndex = index
                        root.toggleTodo(modelData.line, modelData.text)
                      }
                      onDoubleClicked: {
                        root.selectedIndex = index
                        root.startEdit(modelData)
                      }
                    }

                    RowLayout {
                      id: todoInner
                      anchors.left: parent.left
                      anchors.right: parent.right
                      anchors.verticalCenter: parent.verticalCenter
                      anchors.leftMargin: Style.space(8) + (modelData.depth || 0) * Style.space(14)
                      anchors.rightMargin: Style.space(8)
                      spacing: Style.space(10)

                      BorderSurface {
                        Layout.preferredWidth: Style.space(18)
                        Layout.preferredHeight: Style.space(18)
                        Layout.alignment: Qt.AlignVCenter
                        radius: Math.max(2, Style.cornerRadius * 0.45)
                        color: modelData.checked
                          ? Style.selectedFillFor(root.foreground, root.accent)
                          : "transparent"
                        borderSpec: Border.controlSpec(
                          modelData.checked ? "selected" : "normal",
                          root.foreground,
                          root.accent)

                        Text {
                          anchors.centerIn: parent
                          visible: modelData.checked === true
                          text: "\u2713"
                          textFormat: Text.PlainText
                          color: root.foreground
                          font.family: root.fontFamily
                          font.pixelSize: Style.font.caption
                          font.bold: true
                        }
                      }

                      TextField {
                        id: editField
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        visible: root.editingLine === modelData.line
                        verticalPadding: Style.space(2)
                        text: modelData.text
                        foreground: root.foreground
                        font.family: root.fontFamily
                        font.pixelSize: Style.font.body
                        onVisibleChanged: {
                          if (visible) {
                            text = modelData.text
                            forceActiveFocus()
                            selectAll()
                          }
                        }
                        onAccepted: root.commitEdit(modelData.line, editField.text)
                        Keys.onEscapePressed: root.cancelEdit()
                      }

                      Text {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        visible: root.editingLine !== modelData.line
                        text: modelData.text
                        textFormat: Text.PlainText
                        color: modelData.checked ? root.dim : root.foreground
                        font.family: root.fontFamily
                        font.pixelSize: Style.font.body
                        font.strikeout: modelData.checked === true
                        elide: Text.ElideRight
                        wrapMode: Text.NoWrap
                      }
                    }
                  }
                }
              }
            }
          }
        }
      }
    }
  }
}
