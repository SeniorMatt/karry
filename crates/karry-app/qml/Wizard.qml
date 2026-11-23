import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami
import org.kde.karry

Kirigami.Page {
    id: page
    title: step === 5 ? qsTr("Join your setup") : qsTr("Welcome to Karry")
    property int step: 0
    property string folder: ""
    property var off: ({})
    property bool joinLoaded: false
    // the join page loads the source machine itself; refresh() is ignored while another job runs, so retry when idle
    function loadJoin() { if (step === 5 && !found && !joinLoaded && !Backend.busy) { joinLoaded = true; Backend.refresh() } }
    onStepChanged: loadJoin()
    Component.onCompleted: loadJoin()
    readonly property var found: applicationWindow().st ? applicationWindow().st.found : null
    readonly property var cats: {
        const c = applicationWindow().st ? applicationWindow().st.categories.slice() : []
        if (!c.some(x => x.id === "secrets")) c.push({ id: "secrets", name: qsTr("Passwords"), icon: "password-copy", items: 0 })
        return c
    }
    function isLocked(c) { return c.machine_only || c.id === "secrets" }
    function chosen() { return cats.filter(c => !isLocked(c) ? !off[c.id] : c.machine_only).map(c => c.id) }

    FolderDialog {
        id: dialog
        onAccepted: page.folder = decodeURIComponent(selectedFolder.toString().replace("file://", ""))
    }
    Connections {
        target: Backend
        function onBusyChanged() { page.loadJoin() }
        function onFinished(task, result) {
            if (Backend.error !== "") { if (task === "backup") page.step = 2; return }
            if (task === "init") page.step = 2
            else if (task === "backup") page.step = 4
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Kirigami.Units.largeSpacing
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            type: Kirigami.MessageType.Error
            text: Backend.error
            visible: Backend.error !== ""
        }
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            // one container per step; only the current one is visible, fading in
            Repeater {
                model: 6
                delegate: Item {
                    id: pane
                    required property int index
                    anchors.fill: parent
                    visible: opacity > 0
                    opacity: page.step === index ? 1 : 0
                    Behavior on opacity { NumberAnimation { duration: Kirigami.Units.longDuration } }
                    Loader {
                        anchors.fill: parent
                        active: pane.visible
                        sourceComponent: [welcome, where, include, working, done, join][pane.index]
                    }
                }
            }
        }
    }

    Component {
        id: welcome
        Kirigami.PlaceholderMessage {
            icon.source: "qrc:/qt/qml/org/kde/karry/org.kde.karry.svg"
            icon.width: Kirigami.Units.gridUnit * 8
            icon.height: icon.width
            text: qsTr("Keep your Plasma setup safe and in sync")
            explanation: qsTr("Karry backs up your desktop settings and brings them to your other computers.")
            helpfulAction: Kirigami.Action { text: qsTr("Get started"); icon.name: "go-next"; onTriggered: page.step = 1 }
        }
    }
    Component {
        id: where
        ColumnLayout {
            spacing: Kirigami.Units.largeSpacing
            Item { Layout.fillHeight: true }
            Kirigami.Heading { text: qsTr("Where should Karry keep your setup?"); Layout.alignment: Qt.AlignHCenter }
            RowLayout {
                Layout.fillWidth: true
                uniformCellSizes: true
                spacing: Kirigami.Units.largeSpacing
                Kirigami.AbstractCard {
                    Layout.fillWidth: true; Layout.alignment: Qt.AlignTop
                    contentItem: ColumnLayout {
                        spacing: Kirigami.Units.largeSpacing
                        Kirigami.Icon { source: "folder-cloud"; Layout.alignment: Qt.AlignHCenter; implicitWidth: Kirigami.Units.iconSizes.huge; implicitHeight: implicitWidth }
                        Kirigami.Heading { level: 3; text: qsTr("A synced folder"); Layout.alignment: Qt.AlignHCenter }
                        QQC2.Label { text: qsTr("Syncthing, Nextcloud, Dropbox…"); opacity: 0.7; Layout.alignment: Qt.AlignHCenter }
                        QQC2.Button { text: page.folder || qsTr("Choose folder…"); icon.name: "folder-open"; Layout.alignment: Qt.AlignHCenter; onClicked: dialog.open() }
                        Item { Layout.fillHeight: true }
                        QQC2.Button { text: qsTr("Use this folder"); icon.name: "go-next"; enabled: page.folder !== "" && !Backend.busy; Layout.alignment: Qt.AlignHCenter; onClicked: Backend.initFolder(page.folder) }
                    }
                }
                Kirigami.AbstractCard {
                    Layout.fillWidth: true; Layout.alignment: Qt.AlignTop
                    contentItem: ColumnLayout {
                        spacing: Kirigami.Units.largeSpacing
                        Kirigami.Icon { source: "network-server"; Layout.alignment: Qt.AlignHCenter; implicitWidth: Kirigami.Units.iconSizes.huge; implicitHeight: implicitWidth }
                        Kirigami.Heading { level: 3; text: qsTr("A git remote"); Layout.alignment: Qt.AlignHCenter }
                        QQC2.TextField { id: url; Layout.fillWidth: true; placeholderText: "git@example.org:me/karry.git" }
                        Item { Layout.fillHeight: true }
                        QQC2.Button { text: qsTr("Use this remote"); icon.name: "go-next"; enabled: url.text !== "" && !Backend.busy; Layout.alignment: Qt.AlignHCenter; onClicked: Backend.initGit(url.text) }
                    }
                }
            }
            QQC2.ProgressBar { Layout.fillWidth: true; indeterminate: true; opacity: Backend.busy ? 1 : 0; Behavior on opacity { NumberAnimation { duration: Kirigami.Units.longDuration } } }
            Item { Layout.fillHeight: true }
        }
    }
    Component {
        id: include
        ColumnLayout {
            Kirigami.Heading { text: qsTr("What should Karry include?"); Layout.alignment: Qt.AlignHCenter }
            QQC2.ScrollView {
                Layout.alignment: Qt.AlignHCenter
                Layout.fillWidth: true
                Layout.maximumWidth: Kirigami.Units.gridUnit * 30
                Layout.fillHeight: true
                ColumnLayout {
                    width: parent.width
                    Repeater {
                        model: page.cats
                        delegate: QQC2.SwitchDelegate {
                            required property var modelData
                            Layout.fillWidth: true
                            icon.name: page.isLocked(modelData) ? "lock" : modelData.icon
                            text: modelData.name + "  ·  " + (modelData.id === "secrets" ? qsTr("never copied") : modelData.machine_only ? qsTr("this machine only") : (modelData.items === 1 ? qsTr("%1 item") : qsTr("%1 items")).arg(modelData.items))
                            enabled: !page.isLocked(modelData)
                            checked: modelData.machine_only || !page.off[modelData.id] && modelData.id !== "secrets"
                            onToggled: { const o = Object.assign({}, page.off); o[modelData.id] = !checked; page.off = o }
                        }
                    }
                }
            }
            QQC2.Button {
                Layout.alignment: Qt.AlignHCenter
                highlighted: true
                icon.name: page.found ? "go-next" : "document-save"
                text: page.found ? qsTr("Continue") : qsTr("Back up now")
                onClicked: { if (page.found) page.step = 5; else { page.step = 3; Backend.backup(JSON.stringify(page.chosen())) } }
            }
        }
    }
    Component {
        id: working
        ColumnLayout {
            Item { Layout.fillHeight: true }
            Kirigami.Heading { text: qsTr("Making your first backup…"); Layout.alignment: Qt.AlignHCenter }
            QQC2.ProgressBar { Layout.fillWidth: true; Layout.leftMargin: Kirigami.Units.gridUnit * 4; Layout.rightMargin: Layout.leftMargin; indeterminate: true }
            Item { Layout.fillHeight: true }
        }
    }
    Component {
        id: done
        Kirigami.PlaceholderMessage {
            icon.name: "dialog-ok"
            text: qsTr("All set")
            explanation: qsTr("Your setup is backed up. Use Sync on your other computers to bring it over.")
            helpfulAction: Kirigami.Action { text: qsTr("Open Karry"); icon.name: "go-home"; onTriggered: { applicationWindow().firstRun = false; applicationWindow().showOverview() } }
        }
    }
    Component {
        id: join
        Kirigami.PlaceholderMessage {
            icon.name: "folder-sync"
            text: page.found ? qsTr("Found the setup from %1").arg(page.found.hostname) : ""
            explanation: page.found ? qsTr("%1. Karry will show what changes on this computer before anything is applied.").arg(page.found.distro + (page.found.plasma ? ", Plasma " + page.found.plasma : "")) : ""
            helpfulAction: Kirigami.Action { text: qsTr("Review changes"); icon.name: "go-next"; enabled: !Backend.busy; onTriggered: Backend.previewSync() }
        }
    }
}
