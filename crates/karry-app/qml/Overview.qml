import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.karry

Kirigami.ScrollablePage {
    id: page
    title: qsTr("Overview")
    readonly property var st: applicationWindow().st
    readonly property int problems: Backend.issues ? JSON.parse(Backend.issues).length : 0
    readonly property int changes: st ? st.categories.reduce((n, c) => n + (c.machine_only ? 0 : c.changed), 0) : 0
    readonly property int state: problems > 0 ? 2 : changes > 0 ? 1 : 0
    readonly property var cats: st ? st.categories.slice().sort((a, b) => (b.items > 0) - (a.items > 0)) : []
    readonly property var hero: [
        { icon: "emblem-ok-symbolic", color: Kirigami.Theme.positiveTextColor, text: qsTr("Everything is in sync") },
        { icon: "view-refresh-symbolic", color: Kirigami.Theme.highlightColor, text: qsTr("%1 local changes to sync").arg(changes) },
        { icon: "dialog-warning-symbolic", color: Kirigami.Theme.neutralTextColor, text: qsTr("%1 shortcut problems").arg(problems) }
    ]
    function ago(iso) {
        const m = Math.round((Date.now() - Date.parse(iso)) / 60000)
        if (isNaN(m)) return qsTr("Not synced yet")
        if (m < 1) return qsTr("Last synced just now")
        if (m < 60) return qsTr("Last synced %1 min ago").arg(m)
        if (m < 1440) return qsTr("Last synced %1 h ago").arg(Math.round(m / 60))
        return qsTr("Last synced %1 days ago").arg(Math.round(m / 1440))
    }

    ColumnLayout {
        spacing: Kirigami.Units.gridUnit

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            type: Kirigami.MessageType.Error
            text: Backend.error
            visible: Backend.error !== ""
        }
        Kirigami.InlineMessage {
            Layout.fillWidth: true
            type: Kirigami.MessageType.Information
            text: qsTr("Log out and back in to finish applying your settings.")
            visible: applicationWindow().logout
        }

        // hero: one calm status, cross-fading between states
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: Kirigami.Units.gridUnit * 11
            Repeater {
                model: page.hero
                delegate: ColumnLayout {
                    id: h
                    required property var modelData
                    required property int index
                    anchors.centerIn: parent
                    spacing: Kirigami.Units.largeSpacing
                    opacity: page.state === index ? 1 : 0
                    visible: opacity > 0
                    Behavior on opacity { NumberAnimation { duration: Kirigami.Units.longDuration } }
                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        implicitWidth: Kirigami.Units.gridUnit * 5
                        implicitHeight: implicitWidth
                        radius: width / 2
                        color: Qt.alpha(h.modelData.color, 0.18)
                        Kirigami.Icon {
                            anchors.centerIn: parent
                            source: h.modelData.icon
                            isMask: true
                            color: h.modelData.color
                            implicitWidth: Kirigami.Units.iconSizes.huge
                            implicitHeight: implicitWidth
                        }
                    }
                    Kirigami.Heading { Layout.alignment: Qt.AlignHCenter; level: 1; text: h.modelData.text }
                    QQC2.Label { Layout.alignment: Qt.AlignHCenter; opacity: 0.7; text: page.ago(page.st ? page.st.synced : "") }
                }
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: Kirigami.Units.largeSpacing
            QQC2.Button {
                icon.name: "view-refresh"
                text: qsTr("Sync now")
                highlighted: true
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.3
                enabled: !Backend.busy
                onClicked: Backend.previewSync()
            }
            QQC2.Button {
                flat: true
                icon.name: "document-revert"
                text: qsTr("Restore…")
                enabled: !Backend.busy
                onClicked: Backend.previewRestore()
            }
        }
        QQC2.ProgressBar {
            Layout.fillWidth: true
            indeterminate: true
            opacity: Backend.busy ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Kirigami.Units.longDuration } }
        }

        Kirigami.AbstractCard {
            Layout.fillWidth: true
            contentItem: RowLayout {
                spacing: Kirigami.Units.largeSpacing
                Kirigami.Icon { source: "computer"; implicitWidth: Kirigami.Units.iconSizes.large; implicitHeight: implicitWidth }
                ColumnLayout {
                    spacing: 0
                    Kirigami.Heading { level: 3; text: page.st ? page.st.machine.hostname : "" }
                    QQC2.Label { opacity: 0.7; text: page.st ? qsTr("%1, Plasma %2").arg(page.st.machine.distro).arg(page.st.machine.plasma || "?") : "" }
                }
                Item { Layout.fillWidth: true }
                QQC2.Label { opacity: 0.7; visible: page.st && page.st.config; text: page.st && page.st.config ? page.st.config.path : "" }
            }
        }

        Kirigami.Heading { level: 3; text: qsTr("What Karry keeps") }
        Kirigami.CardsLayout {
            Layout.fillWidth: true
            Repeater {
                model: page.cats
                delegate: Kirigami.AbstractCard {
                    id: card
                    required property var modelData
                    opacity: 0
                    scale: hover.hovered ? 1.02 : 1
                    Component.onCompleted: opacity = modelData.items > 0 ? 1 : 0.5
                    Behavior on opacity { NumberAnimation { duration: Kirigami.Units.longDuration } }
                    Behavior on scale { NumberAnimation { duration: Kirigami.Units.shortDuration } }
                    HoverHandler { id: hover }
                    contentItem: RowLayout {
                        spacing: Kirigami.Units.largeSpacing
                        Kirigami.Icon { source: card.modelData.icon; implicitWidth: Kirigami.Units.iconSizes.medium; implicitHeight: implicitWidth }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: Kirigami.Units.smallSpacing
                            Kirigami.Heading { level: 4; text: card.modelData.name }
                            QQC2.Label { opacity: 0.7; text: card.modelData.items === 1 ? qsTr("1 item") : qsTr("%1 items").arg(card.modelData.items) }
                            Flow {
                                Layout.fillWidth: true
                                visible: children.some(c => c.visible)
                                spacing: Kirigami.Units.smallSpacing
                                Kirigami.Chip { visible: card.modelData.changed > 0 && card.modelData.items > 0; closable: false; checkable: false; text: qsTr("%1 changed").arg(card.modelData.changed) }
                                Kirigami.Chip { visible: card.modelData.machine_only; closable: false; checkable: false; icon.name: "lock"; text: qsTr("This machine only") }
                                Kirigami.Chip { visible: card.modelData.issues > 0; closable: false; checkable: false; icon.name: "dialog-warning"; text: qsTr("%1 issues").arg(card.modelData.issues) }
                            }
                        }
                    }
                }
            }
        }

        Kirigami.AbstractCard {
            Layout.fillWidth: true
            visible: page.st && page.st.last_snapshot
            contentItem: RowLayout {
                spacing: Kirigami.Units.largeSpacing
                Kirigami.Icon { source: "edit-undo-history"; implicitWidth: Kirigami.Units.iconSizes.medium; implicitHeight: implicitWidth }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Kirigami.Heading { level: 4; text: qsTr("Last snapshot") }
                    QQC2.Label { opacity: 0.7; text: page.st && page.st.last_snapshot ? page.st.last_snapshot.label + " · " + page.st.last_snapshot.created : "" }
                }
                QQC2.Button { flat: true; icon.name: "edit-undo"; text: qsTr("Undo"); enabled: !Backend.busy; onClicked: Backend.undo(page.st.last_snapshot.id) }
            }
        }
    }
}
