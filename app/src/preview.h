#pragma once
#include "ui.h"
#include <QJsonObject>

// Shows what a restore or sync would change; Apply is the only thing that writes.
class Preview : public QWidget
{
    Q_OBJECT
public:
    Preview(Cli *cli, QWidget *parent = nullptr);
    void load(const QJsonObject &report, const QJsonObject &categories, const QString &title, const QString &subtitle, const QStringList &applyArgs);
signals:
    void applied(const QJsonObject &report);
    void cancelled();

private:
    Cli *m_cli;
    QVBoxLayout *m_layout;
    QScrollArea *m_scroll = nullptr;
    QWidget *m_footer = nullptr;
};
