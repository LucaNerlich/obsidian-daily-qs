import QtQuick
import QtQuick.Controls as Controls
import Quickshell
import Quickshell.Io
import qs.Ui

// Permanently read-only TextField that edits from key events and pastes only
// through a bounded external read (QTBUG-149610 / marketplace #11653).
//
// On released Qt 6.11–6.12, an editable QQuickTextInput connects to
// QClipboard::dataChanged and pulls the full offer via QMimeData::text() with
// no total byte cap or overall deadline on Wayland. setReadOnly(false) also
// triggers that read. Keeping readOnly true for the field's whole life keeps
// Qt off the clipboard; typing, cut, undo/redo, and paste are handled here.
TextField {
  id: root

  readOnly: true
  cursorVisible: activeFocus

  // Optional hook: return true to consume a key before built-in editing.
  property var interceptKey: null

  property var undoSteps: []
  property var redoSteps: []
  property int typedTo: -1
  property bool ownEdit: false
  property string pasteError: ""

  readonly property int pasteLimitBytes: 65536

  property string _pasteSnapshot: ""
  property int _pasteStart: 0
  property int _pasteEnd: 0
  property bool _pastePrimary: false

  signal pasteRefused(string reason)

  onTextChanged: {
    if (!ownEdit) {
      undoSteps = []
      redoSteps = []
      typedTo = -1
    }
  }

  onActiveFocusChanged: {
    if (!activeFocus)
      boundedPaste.running = false
  }

  function replaceRange(start, end, insertText, typing) {
    insertText = String(insertText || "").slice(0, Math.max(0, maximumLength - (text.length - (end - start))))
    if (start === end && insertText === "")
      return
    if (!(typing && start === end && start === typedTo))
      undoSteps = undoSteps.slice(-49).concat([{ text: text, cursor: cursorPosition }])
    redoSteps = []
    typedTo = typing ? start + insertText.length : -1
    ownEdit = true
    text = text.slice(0, start) + insertText + text.slice(end)
    ownEdit = false
    cursorPosition = start + insertText.length
  }

  function stepBack(redo) {
    var from = redo ? redoSteps : undoSteps
    var current = { text: text, cursor: cursorPosition }
    if (from.length === 0)
      return
    var step = from[from.length - 1]
    if (redo) {
      redoSteps = from.slice(0, -1)
      undoSteps = undoSteps.concat([current])
    } else {
      undoSteps = from.slice(0, -1)
      redoSteps = redoSteps.concat([current])
    }
    typedTo = -1
    ownEdit = true
    text = step.text
    ownEdit = false
    cursorPosition = step.cursor
  }

  function wordEdge(position, forward) {
    var inside = function() { return forward ? position < text.length : position > 0 }
    var next = function() { return text.charAt(forward ? position : position - 1) }
    while (inside() && /\s/.test(next()))
      position += forward ? 1 : -1
    while (inside() && /\S/.test(next()))
      position += forward ? 1 : -1
    return position
  }

  function utf8Length(value) {
    var bytes = 0
    for (var i = 0; i < value.length; i++) {
      var unit = value.charCodeAt(i)
      bytes += unit < 0x80 ? 1 : (unit < 0x800 ? 2 : (unit >= 0xDC00 && unit <= 0xDFFF ? 1 : 3))
    }
    return bytes
  }

  function refusePaste(reason) {
    pasteError = reason
    pasteRefused(reason)
  }

  function pasteBounded(primary) {
    if (!activeFocus || boundedPaste.running)
      return
    pasteError = ""
    _pasteSnapshot = text
    _pasteStart = selectionStart
    _pasteEnd = selectionEnd
    _pastePrimary = primary === true
    boundedPaste.running = true
  }

  function finishPaste(output) {
    var cut = output.lastIndexOf("\n")
    var status = output.slice(cut + 1).trim()
    var content = cut < 0 ? "" : output.slice(0, cut).replace(/\r\n/g, "\n").replace(/[\r\n]/g, " ")
    if (!root.activeFocus || root.text !== root._pasteSnapshot
        || root.selectionStart !== root._pasteStart || root.selectionEnd !== root._pasteEnd)
      return
    if (status === "124") {
      refusePaste("Clipboard did not answer within 2 seconds")
      return
    }
    if (status === "127") {
      refusePaste("wl-paste (wl-clipboard) is required to paste")
      return
    }
    if (content === "")
      return
    if (utf8Length(content) > pasteLimitBytes
        || content.length > maximumLength - (text.length - (_pasteEnd - _pasteStart))) {
      refusePaste("Clipboard text is too long for this field")
      return
    }
    replaceRange(_pasteStart, _pasteEnd, content, false)
  }

  // Navigation, selection, copy, Select All and Enter stay with Qt (work read-only).
  function editKey(event) {
    if (typeof interceptKey === "function" && interceptKey(event))
      return true

    var start = selectionStart
    var end = selectionEnd
    var word = (event.modifiers & Qt.ControlModifier) !== 0

    if (event.matches(StandardKey.Paste)) {
      pasteBounded(false)
      return true
    }
    if (event.matches(StandardKey.Cut)) {
      if (start !== end) {
        copy()
        replaceRange(start, end, "", false)
      }
      return true
    }
    if (event.matches(StandardKey.Undo)) {
      stepBack(false)
      return true
    }
    if (event.matches(StandardKey.Redo)) {
      stepBack(true)
      return true
    }
    if (event.key === Qt.Key_Backspace && start === end)
      start = word ? wordEdge(start, false) : start - (/[\uDC00-\uDFFF]/.test(text.charAt(start - 1)) ? 2 : 1)
    if (event.key === Qt.Key_Delete && start === end)
      end = word ? wordEdge(end, true) : end + (/[\uD800-\uDBFF]/.test(text.charAt(end)) ? 2 : 1)
    if (event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete) {
      replaceRange(Math.max(0, start), Math.min(text.length, end), "", false)
      return true
    }

    // Qt's isAcceptableInput rule: Ctrl / Ctrl+Shift are shortcuts; AltGr may be Ctrl+Alt.
    var mods = event.modifiers & (Qt.ShiftModifier | Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)
    if (event.text === "" || /[\x00-\x1f\x7f]/.test(event.text)
        || mods === Qt.ControlModifier
        || mods === (Qt.ControlModifier | Qt.ShiftModifier))
      return false
    replaceRange(start, end, event.text, true)
    return true
  }

  Keys.onShortcutOverride: function(event) {
    if (event.matches(StandardKey.Paste))
      event.accepted = true
  }

  Keys.onPressed: function(event) {
    if (root.editKey(event))
      event.accepted = true
  }

  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.MiddleButton
    onPressed: function(mouse) {
      root.forceActiveFocus()
      root.cursorPosition = root.positionAt(mouse.x, mouse.y)
      root.pasteBounded(true)
    }
  }

  Controls.ContextMenu.menu: Controls.Menu {
    Controls.MenuItem {
      text: qsTr("Cut")
      enabled: root.selectedText.length > 0
      onTriggered: {
        root.copy()
        root.replaceRange(root.selectionStart, root.selectionEnd, "", false)
      }
    }
    Controls.MenuItem {
      text: qsTr("Copy")
      enabled: root.selectedText.length > 0
      onTriggered: root.copy()
    }
    Controls.MenuItem {
      text: qsTr("Paste")
      onTriggered: {
        root.forceActiveFocus()
        root.pasteBounded(false)
      }
    }
    Controls.MenuItem {
      text: qsTr("Select All")
      onTriggered: root.selectAll()
    }
  }

  // wl-paste in its own process, cut at pasteLimitBytes (+1 ⇒ too long) and
  // stopped after 2s so a huge or stalled offer never reaches the shell.
  Process {
    id: boundedPaste
    command: ["sh", "-c",
      "command -v wl-paste >/dev/null || { printf '\\n127'; exit 0; }; "
      + "timeout 2 sh -c 'wl-paste --no-newline --type text"
      + (root._pastePrimary ? " --primary" : "")
      + " 2>/dev/null | head -c \"$1\"' sh " + (root.pasteLimitBytes + 1)
      + "; printf '\\n%s' \"$?\""]
    stdout: StdioCollector {
      onStreamFinished: root.finishPaste(String(text))
    }
  }

  // Qt draws no caret on a read-only TextInput; draw one while focused.
  cursorDelegate: Rectangle {
    id: guardedCaret
    property bool blinkOn: true
    width: 1
    color: root.color
    visible: root.activeFocus && blinkOn
    function restartBlink() {
      blinkOn = true
      blinkTimer.restart()
    }
    Timer {
      id: blinkTimer
      interval: Math.max(100, Qt.styleHints.cursorFlashTime / 2)
      running: root.activeFocus && Qt.styleHints.cursorFlashTime >= 2
      repeat: true
      onTriggered: guardedCaret.blinkOn = !guardedCaret.blinkOn
    }
    Connections {
      target: root
      function onCursorPositionChanged() { guardedCaret.restartBlink() }
      function onTextChanged() { guardedCaret.restartBlink() }
      function onActiveFocusChanged() { guardedCaret.restartBlink() }
    }
  }
}
