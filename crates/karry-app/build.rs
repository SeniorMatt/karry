use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    let files = ["Main", "Overview", "Preview", "Shortcuts", "Snapshots", "Wizard"];
    CxxQtBuilder::new_qml_module(QmlModule::new("org.kde.karry").qml_files(files.map(|f| format!("qml/{f}.qml"))))
        .qrc_resources(["org.kde.karry.svg"])
        .qt_module("Quick")
        .files(["src/backend.rs"])
        .build();
}
