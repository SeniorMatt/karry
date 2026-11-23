import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.karry

Kirigami.ScrollablePage {
    id: page
    title: qsTr("Overview")
    readonly property var st: applicationWindow().st

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

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
            showCloseButton: true
            onVisibleChanged: if (!visible) applicationWindow().logout = false
        }

        Kirigami.AbstractCard {
            Layout.fillWidth: true
            contentItem: RowLayout {
                spacing: Kirigami.Units.largeSpacing
                Kirigami.Icon { source: "computer"; implicitWidth: Kirigami.Units.iconSizes.huge; implicitHeight: implicitWidth }
                ColumnLayout {
                    Kirigami.Heading { level: 2; text: page.st ? page.st.machine.hostname : "…" }
                    QQC2.Label {
                        opacity: 0.7
                        text: page.st ? qsTr("%1, Plasma %2").arg(page.st.machine.distro).arg(page.st.machine.plasma || "?") : ""
                    }
                    QQC2.Label {
                        opacity: 0.7
                        visible: page.st && page.st.config
                        text: page.st && page.st.config ? qsTr("Setup kept in %1").arg(page.st.config.path) : ""
                    }
                }
                Item { Layout.fillWidth: true }
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: Kirigami.Units.largeSpacing
            QQC2.Button {
                icon.name: "document-save"
                text: qsTr("Back up now")
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.3
                highlighted: true
                enabled: !Backend.busy
                onClicked: Backend.backup("")
            }
            QQC2.Button {
                icon.name: "view-refresh"
                text: qsTr("Sync")
                font.pointSize: Kirigami.Theme.defaultFont.pointSize * 1.3
                enabled: !Backend.busy
                onClicked: Backend.previewSync()
            }
            QQC2.Button {
                flat: true
                icon.name: "document-revert"
                text: qsTr("Restore from backup…")
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

        Kirigami.CardsLayout {
            Layout.fillWidth: true
            Repeater {
                model: page.st ? page.st.categories : []
                delegate: Kirigami.AbstractCard {
                    id: card
                    required property var modelData
                    opacity: 0
                    scale: hover.hovered ? 1.02 : 1
                    Component.onCompleted: opacity = 1
                    Behavior on opacity { NumberAnimation { duration: Kirigami.Units.longDuration } }
                    Behavior on scale { NumberAnimation { duration: Kirigami.Units.shortDuration } }
                    HoverHandler { id: hover }
                    contentItem: RowLayout {
                        spacing: Kirigami.Units.largeSpacing
                        Kirigami.Icon { source: card.modelData.icon; implicitWidth: Kirigami.Units.iconSizes.medium; implicitHeight: implicitWidth }
                        ColumnLayout {
                            Layout.fillWidth: true
                            Kirigami.Heading { level: 4; text: card.modelData.name }
                            QQC2.Label { opacity: 0.7; text: (card.modelData.items === 1 ? qsTr("%1 item") : qsTr("%1 items")).arg(card.modelData.items) }
                            Flow {
                                Layout.fillWidth: true
                                spacing: Kirigami.Units.smallSpacing
                                Kirigami.Chip { closable: false; checkable: false; text: card.modelData.changed > 0 ? qsTr("%1 changed").arg(card.modelData.changed) : qsTr("Up to date") }
                                Kirigami.Chip { visible: card.modelData.machine_only; closable: false; checkable: false; icon.name: "lock"; text: qsTr("This machine only") }
                                Kirigami.Chip { visible: card.modelData.issues > 0; closable: false; checkable: false; icon.name: "dialog-warning"; text: (card.modelData.issues === 1 ? qsTr("%1 issue") : qsTr("%1 issues")).arg(card.modelData.issues) }
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
                Kirigami.Icon { source: "edit-undo-history"; implicitWidth: Kirigami.Units.iconSizes.medium; implicitHeight: implicitWidth }
                ColumnLayout {
                    Layout.fillWidth: true
                    Kirigami.Heading { level: 4; text: qsTr("Last snapshot") }
                    QQC2.Label { opacity: 0.7; text: page.st && page.st.last_snapshot ? page.st.last_snapshot.label + " · " + page.st.last_snapshot.created : "" }
                }
                QQC2.Button {
                    icon.name: "edit-undo"
                    text: qsTr("Undo")
                    enabled: !Backend.busy
                    onClicked: Backend.undo(page.st.last_snapshot.id)
                }
            }
        }
    }
}
