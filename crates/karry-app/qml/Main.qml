import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.karry

Kirigami.ApplicationWindow {
    id: root
    title: "Karry"
    width: Kirigami.Units.gridUnit * 50
    height: Kirigami.Units.gridUnit * 38

    property bool firstRun: false
    property bool logout: false
    readonly property var st: Backend.status ? JSON.parse(Backend.status) : null

    function go(page) {
        pageStack.replace(page)
    }
    function showOverview() { pageStack.replace(overview) }
    function copy(text) {
        clip.text = text
        clip.selectAll()
        clip.copy()
        showPassiveNotification(qsTr("Copied"))
    }
    TextEdit { id: clip; visible: false }

    Component.onCompleted: {
        firstRun = !Backend.configured
        Backend.refresh()
        pageStack.push(firstRun ? wizard : overview)
    }

    globalDrawer: Kirigami.GlobalDrawer {
        drawerOpen: !root.firstRun
        enabled: !root.firstRun
        opacity: root.firstRun ? 0 : 1
        isMenu: false
        collapsible: true
        collapseButtonVisible: false
        modal: false
        title: "Karry"
        titleIcon: "org.kde.karry"
        actions: [
            Kirigami.Action { text: qsTr("Overview"); icon.name: "go-home"; onTriggered: root.go(overview) },
            Kirigami.Action { text: qsTr("Shortcuts"); icon.name: "preferences-desktop-keyboard-shortcut"; onTriggered: root.go(shortcutsPage) },
            Kirigami.Action { text: qsTr("Snapshots"); icon.name: "edit-undo-history"; onTriggered: root.go(snapshotsPage) }
        ]
    }

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
            if (task === "refresh" || task === "init") return
            if (task === "backup") showPassiveNotification(qsTr("Backed up %1 shared and %2 machine files").arg(r.shared).arg(r.machine))
            else if (task === "sync") showPassiveNotification(r.pushed ? qsTr("Synced and pushed") : qsTr("Synced"))
            else if (task === "undo") showPassiveNotification(qsTr("Restored the snapshot"))
            else if (task === "fix") showPassiveNotification(qsTr("Fixed. A snapshot was taken first."))
            if (r.logout) root.logout = true
            if (task === "sync" || task === "restore") {
                if (pageStack.depth > 1) pageStack.pop()
            }
        }
    }
}
