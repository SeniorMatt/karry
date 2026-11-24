#pragma once
#include "ui.h"
#include <QDateTime>
#include <QJsonArray>
#include <QJsonObject>
#include <QListWidget>
#include <QMainWindow>

class KMessageWidget;
class QProgressBar;
class Welcome;
class Preview;

class Window : public QMainWindow
{
    Q_OBJECT
public:
    Window();

private:
    enum Page { Overview, Shortcuts, Snapshots, Settings, PreviewPage };
    void reload();
    void rebuild();
    void message(const QString &text, bool error);
    void openPreview(const QString &title, const QString &sub, const QStringList &previewArgs, const QStringList &applyArgs, bool autoApply);
    void applied(const QJsonObject &report);
    void debugPage(const QString &name);
    void uninstall();
    QWidget *buildOverview();
    QWidget *buildShortcuts();
    QWidget *buildSnapshots();
    QWidget *buildSettings();
    QDateTime lastSynced() const;

    Cli *m_cli;
    FadeStack *m_root, *m_pages;
    Welcome *m_welcome;
    Preview *m_preview;
    QListWidget *m_nav;
    KMessageWidget *m_bar;
    QProgressBar *m_busy;
    QJsonObject m_info, m_status, m_cats;
    QJsonArray m_snaps, m_issues, m_incoming;
    QString m_debug;
};
