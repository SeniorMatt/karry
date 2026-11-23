import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami
import org.kde.karry

Kirigami.ScrollablePage {
    id: page
    readonly property var p: JSON.parse(Backend.preview)
    readonly property bool isSync: p.kind === "sync"
    property var remote: ({})
    title: isSync ? qsTr("Sync preview") : qsTr("Restore preview")

    readonly property var groups: {
        const names = {}
        for (const c of (applicationWindow().st?.categories ?? [])) names[c.id] = c.name
        const g = {}
        for (const c of p.plan.changes) (g[c.category] = g[c.category] || []).push(c)
        return Object.keys(g).map(k => ({ name: names[k] || k, changes: g[k] }))
    }
    function line(c) {
        switch (c.type) {
        case "SetKey": return c.group + " › " + c.key + ":  " + (c.old ?? qsTr("(unset)")) + "  →  " + c.value
        case "DeleteKey": return c.group + " › " + c.key + ":  " + c.old + "  →  " + qsTr("(removed)")
        case "WriteFile": return (c.existing ? qsTr("Replace ") : qsTr("Create ")) + c.store_path
        default: return ""
        }
    }
    function pick(c, useRemote) {
        const k = c.store_path + "|" + c.group + "|" + c.key
        const r = Object.assign({}, remote)
        if (useRemote) r[k] = true; else delete r[k]
        remote = r
    }

    actions: [
        Kirigami.Action { text: qsTr("Apply"); icon.name: "dialog-ok-apply"; enabled: !Backend.busy
            onTriggered: isSync ? Backend.applySync(JSON.stringify(Object.keys(remote))) : Backend.applyRestore() },
        Kirigami.Action { text: qsTr("Cancel"); icon.name: "dialog-cancel"; onTriggered: applicationWindow().pageStack.pop() }
    ]

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
            type: Kirigami.MessageType.Warning
            visible: p.plan.needs_logout
            text: qsTr("Log out and back in to finish applying these changes.")
        }
        Repeater {
            model: (p.plan.warnings || []).concat(p.warnings || [])
            Kirigami.InlineMessage { required property var modelData; Layout.fillWidth: true; visible: true; type: Kirigami.MessageType.Warning; text: modelData }
        }
        Repeater {
            model: p.incoming || []
            QQC2.Label { required property var modelData; Layout.fillWidth: true; elide: Text.ElideMiddle; text: modelData.summary }
        }
        Kirigami.PlaceholderMessage {
            Layout.fillWidth: true
            visible: groups.length === 0 && (p.conflicts || []).length === 0
            icon.name: "dialog-ok"
            text: qsTr("Nothing to change")
        }

        Repeater {
            model: p.conflicts || []
            delegate: Kirigami.AbstractCard {
                id: cf
                required property var modelData
                Layout.fillWidth: true
                header: Kirigami.Heading { level: 4; text: qsTr("Conflict: %1 › %2").arg(cf.modelData.group).arg(cf.modelData.key || cf.modelData.store_path) }
                contentItem: ColumnLayout {
                    Repeater {
                        model: [false, true]
                        delegate: QQC2.RadioButton {
                            required property bool modelData
                            Layout.fillWidth: true
                            checked: !modelData
                            text: (modelData ? qsTr("Use remote: %1").arg(cf.modelData.remote ?? qsTr("(unset)")) : qsTr("Keep local: %1").arg(cf.modelData.local ?? qsTr("(unset)")))
                            onToggled: page.pick(cf.modelData, modelData)
                            contentItem: QQC2.Label { text: parent.text; wrapMode: Text.Wrap; leftPadding: parent.indicator.width + parent.spacing; verticalAlignment: Text.AlignVCenter }
                        }
                    }
                }
            }
        }

        Repeater {
            model: page.groups
            delegate: Kirigami.AbstractCard {
                id: g
                required property var modelData
                Layout.fillWidth: true
                header: Kirigami.Heading { level: 4; text: g.modelData.name }
                contentItem: ColumnLayout {
                    Repeater {
                        model: g.modelData.changes
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            QQC2.Label {
                                Layout.fillWidth: true
                                wrapMode: Text.Wrap
                                text: modelData.type === "InstallHint" ? qsTr("Install %1").arg(modelData.id) : (modelData.type === "NeedsLogout" ? modelData.reason : page.line(modelData))
                            }
                            QQC2.TextField { visible: modelData.type === "InstallHint"; readOnly: true; text: modelData.command || ""; Layout.preferredWidth: Kirigami.Units.gridUnit * 18 }
                            QQC2.ToolButton { visible: modelData.type === "InstallHint"; icon.name: "edit-copy"; onClicked: applicationWindow().copy(modelData.command) }
                        }
                    }
                }
            }
        }

        Repeater {
            model: p.plan.shortcut_issues || []
            Kirigami.InlineMessage { required property var modelData; Layout.fillWidth: true; visible: true; icon.name: "preferences-desktop-keyboard-shortcut"; text: modelData.message }
        }
    }
}
