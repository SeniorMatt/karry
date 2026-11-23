import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.karry

Kirigami.ScrollablePage {
    title: qsTr("Shortcuts")
    readonly property var list: Backend.issues ? JSON.parse(Backend.issues) : []

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
            visible: list.length > 0
            text: qsTr("Fixes edit your shortcut file directly, after a snapshot you can undo. Log out and back in for them to take full effect.")
        }
        Kirigami.PlaceholderMessage {
            Layout.fillWidth: true
            visible: list.length === 0
            icon.name: "dialog-ok"
            text: qsTr("No shortcut problems found")
        }
        Repeater {
            model: list
            delegate: Kirigami.AbstractCard {
                id: card
                required property var modelData
                Layout.fillWidth: true
                opacity: 0
                Component.onCompleted: opacity = 1
                Behavior on opacity { NumberAnimation { duration: Kirigami.Units.longDuration } }
                contentItem: ColumnLayout {
                    spacing: Kirigami.Units.smallSpacing
                    Kirigami.Heading { level: 4; text: card.modelData.message; wrapMode: Text.Wrap; Layout.fillWidth: true }
                    Repeater {
                        model: card.modelData.bindings
                        QQC2.Label { required property var modelData; opacity: 0.7; text: modelData.friendly + "  (" + modelData.key + ")" }
                    }
                    QQC2.TextField {
                        id: keys
                        Layout.fillWidth: true
                        visible: card.modelData.fixes.some(f => f.needs_input)
                        placeholderText: qsTr("New shortcut, for example Meta+E")
                    }
                    Flow {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing
                        Repeater {
                            model: card.modelData.fixes
                            QQC2.Button {
                                required property var modelData
                                text: modelData.label
                                enabled: !Backend.busy && (!modelData.needs_input || keys.text !== "")
                                onClicked: Backend.fix(card.modelData.id, modelData.id, keys.text)
                            }
                        }
                    }
                }
            }
        }
    }
}
