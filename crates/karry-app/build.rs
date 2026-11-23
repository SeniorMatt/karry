use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("org.kde.karry")
            .qml_file("qml/Main.qml").qml_file("qml/Overview.qml").qml_file("qml/Preview.qml").qml_file("qml/Shortcuts.qml").qml_file("qml/Snapshots.qml").qml_file("qml/Wizard.qml"),
    )
    .qt_module("Quick")
    .files(["src/backend.rs"])
    .build();
}
