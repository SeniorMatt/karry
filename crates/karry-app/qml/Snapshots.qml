import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.karry

Kirigami.ScrollablePage {
    title: qsTr("Snapshots")
    readonly property var list: Backend.snapshots ? JSON.parse(Backend.snapshots) : []

    header: Kirigami.InlineMessage {
        type: Kirigami.MessageType.Error
        text: Backend.error
        visible: Backend.error !== ""
    }

    ListView {
        model: list
        spacing: Kirigami.Units.smallSpacing
        add: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Kirigami.Units.longDuration } }
        remove: Transition { NumberAnimation { property: "opacity"; to: 0; duration: Kirigami.Units.shortDuration } }
        displaced: Transition { NumberAnimation { properties: "y"; duration: Kirigami.Units.longDuration; easing.type: Easing.OutCubic } }

        Kirigami.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: parent.count === 0
            icon.name: "edit-undo-history"
            text: qsTr("No snapshots yet")
            explanation: qsTr("Karry takes one before it changes anything.")
        }
        delegate: Kirigami.AbstractCard {
            required property var modelData
            width: ListView.view.width
            contentItem: RowLayout {
                ColumnLayout {
                    Layout.fillWidth: true
                    Kirigami.Heading { level: 4; text: modelData.label }
                    QQC2.Label { opacity: 0.7; text: modelData.created + " · " + (modelData.files.length === 1 ? qsTr("%1 file") : qsTr("%1 files")).arg(modelData.files.length) }
                }
                QQC2.Button {
                    flat: true
                    icon.name: "edit-undo"
                    text: qsTr("Undo")
                    enabled: !Backend.busy
                    onClicked: Backend.undo(modelData.id)
                }
            }
        }
    }
}
