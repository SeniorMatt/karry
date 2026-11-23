import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.karry

Kirigami.ApplicationWindow {
    id: root
    title: "Karry"
    width: Kirigami.Units.gridUnit * 50
    height: Kirigami.Units.gridUnit * 38

    property bool firstRun: true
    property string dbg: Backend.debugPage()
    property bool logout: false
    readonly property var st: Backend.status ? JSON.parse(Backend.status) : null

    function go(page) {
        pageStack.replace(page)
    }
    function showOverview() { pageStack.replace(overview) }
    function showShortcuts() { pageStack.replace(shortcutsPage) }
    function copy(text) {
        clip.text = text
        clip.selectAll()
        clip.copy()
        showPassiveNotification(qsTr("Copied"))
    }
    TextEdit { id: clip; visible: false }

    Component.onCompleted: {
        const steps = { welcome: 0, folder: 1, include: 2, join: 5 }
        firstRun = dbg in steps || (dbg === "" && !Backend.configured)
        Backend.refresh()
        if (firstRun) pageStack.push(wizard, { step: steps[dbg] ?? 0 })
        else pageStack.push(overview)
        if (dbg === "shortcuts") go(shortcutsPage)
        if (dbg === "snapshots") go(snapshotsPage)
    }

    globalDrawer: root.firstRun ? null : drawerC.createObject(root)
    Component { id: drawerC; Kirigami.GlobalDrawer {
        isMenu: false
        collapsible: true
        collapseButtonVisible: false
        modal: false
        title: "Karry"
        titleIcon: "qrc:/qt/qml/org/kde/karry/org.kde.karry.svg"
        actions: [
            Kirigami.Action { text: qsTr("Overview"); icon.name: "go-home"; onTriggered: root.go(overview) },
            Kirigami.Action { text: qsTr("Shortcuts"); icon.name: "preferences-desktop-keyboard-shortcut"; onTriggered: root.go(shortcutsPage) },
            Kirigami.Action { text: qsTr("Snapshots"); icon.name: "edit-undo-history"; onTriggered: root.go(snapshotsPage) }
        ]
    } }

    Component { id: wizard; Wizard {} }
    Component { id: overview; Overview {} }
    Component { id: shortcutsPage; Shortcuts {} }
    Component { id: snapshotsPage; Snapshots {} }
    Component { id: previewPage; Preview {} }

    Connections {
        target: Backend
        function onFinished(task, result) {
            if (Backend.error !== "") return
            const r = result ? JSON.parse(result) : {}
            if (task === "preview") {
                pageStack.push(previewPage)
                return
            }
            if (task === "refresh" && dbg === "preview") { dbg = ""; Backend.previewSync() }
            if (task === "refresh" || task === "init") return
            if (task === "backup") showPassiveNotification(qsTr("Backed up %1 shared and %2 machine files").arg(r.shared).arg(r.machine))
            else if (task === "sync") showPassiveNotification(r.pushed ? qsTr("Synced and pushed") : qsTr("Synced"))
            else if (task === "undo") showPassiveNotification(qsTr("Restored the snapshot"))
            else if (task === "fix") showPassiveNotification(qsTr("Fixed. A snapshot was taken first."))
            if (r.logout) root.logout = true
            if (task === "sync" || task === "restore") {
                if (firstRun) { firstRun = false; pageStack.clear(); pageStack.push(overview) }
                else if (pageStack.depth > 1) pageStack.pop()
            }
        }
    }
}
