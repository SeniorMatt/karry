#include "cli.h"
#include <KLocalizedString>
#include <QCoreApplication>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QPointer>
#include <QProcess>
#include <QStandardPaths>

QString Cli::binary()
{
    const QString beside = QCoreApplication::applicationDirPath() + "/karry-cli";
    return QFile::exists(beside) ? beside : QStandardPaths::findExecutable("karry-cli");
}

void Cli::run(const QStringList &args, QObject *context, Done done)
{
    const QString bin = binary();
    if (bin.isEmpty()) {
        emit failed(i18n("karry-cli was not found. Reinstall Karry."));
        return;
    }
    auto *p = new QProcess(this);
    if (m_jobs++ == 0)
        emit busyChanged(true);
    QPointer<QObject> ctx = context;
    connect(p, &QProcess::finished, this, [=, this] {
        const QJsonDocument doc = QJsonDocument::fromJson(p->readAllStandardOutput());
        const QString err = doc.isObject() ? doc.object().value("error").toString() : QString();
        const QString stderrText = QString::fromUtf8(p->readAllStandardError()).trimmed();
        const bool ok = err.isEmpty() && !doc.isNull() && p->exitCode() == 0;
        p->deleteLater();
        if (--m_jobs == 0)
            emit busyChanged(false);
        if (!ok)
            emit failed(!err.isEmpty() ? err : !stderrText.isEmpty() ? stderrText : i18n("karry-cli returned something unexpected."));
        else if (ctx)
            done(doc.isArray() ? QJsonValue(doc.array()) : QJsonValue(doc.object()));
    });
    connect(p, &QProcess::errorOccurred, this, [=, this](QProcess::ProcessError e) {
        if (e != QProcess::FailedToStart)
            return;
        p->deleteLater();
        if (--m_jobs == 0)
            emit busyChanged(false);
        emit failed(i18n("Could not start karry-cli."));
    });
    p->start(bin, QStringList{"--json"} + args);
}
