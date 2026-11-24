#pragma once
#include <QJsonValue>
#include <QObject>
#include <functional>

// Runs karry-cli --json asynchronously. Errors go to failed(); done() only runs on success.
class Cli : public QObject
{
    Q_OBJECT
public:
    using Done = std::function<void(const QJsonValue &)>;
    using QObject::QObject;
    void run(const QStringList &args, QObject *context, Done done);
    bool busy() const { return m_jobs > 0; }
    static QString binary();
signals:
    void busyChanged(bool busy);
    void failed(const QString &message);

private:
    int m_jobs = 0;
};
