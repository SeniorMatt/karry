#include "window.h"
#include <KAboutData>
#include <KLocalizedString>
#include <QApplication>

int main(int argc, char **argv)
{
    QApplication app(argc, argv);
    KLocalizedString::setApplicationDomain("karry");
    KAboutData about("karry", "Karry", KARRY_VERSION, i18n("Back up, restore and sync your Plasma setup"), KAboutLicense::GPL_V3);
    KAboutData::setApplicationData(about);
    app.setApplicationDisplayName("Karry");
    app.setDesktopFileName("org.kde.karry");
    app.setWindowIcon(QIcon::fromTheme("org.kde.karry"));
    Window w;
    const QStringList size = qEnvironmentVariable("KARRY_SIZE", "1000x720").split('x');
    w.resize(size[0].toInt(), size[1].toInt());
    w.show();
    return app.exec();
}
