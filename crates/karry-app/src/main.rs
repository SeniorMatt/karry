mod backend;

use cxx_qt_lib::{QGuiApplication, QString, QQmlApplicationEngine, QUrl};

fn main() {
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        std::env::set_var("QT_QUICK_CONTROLS_STYLE", "org.kde.desktop");
    }
    let mut app = QGuiApplication::new();
    QGuiApplication::set_desktop_file_name(&QString::from("org.kde.karry"));
    let mut engine = QQmlApplicationEngine::new();
    if let Some(e) = engine.as_mut() {
        e.load(&QUrl::from("qrc:/qt/qml/org/kde/karry/qml/Main.qml"));
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
