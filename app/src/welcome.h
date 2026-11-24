#pragma once
#include "ui.h"

// First-run steps: intro, where to keep the setup, join (another machine's setup found), what to include, progress.
class Welcome : public FadeStack
{
    Q_OBJECT
public:
    enum Step { Intro, Where, Join, Include, Progress };
    Welcome(Cli *cli, QWidget *parent = nullptr);
    void open(Step step, bool fromSettings = false, const QString &host = {});
signals:
    void finished(bool join);
    void restoreFile(const QString &file);
    void cancelled();

private:
    void choose(const QStringList &initArgs);
    QWidget *page(const QString &title, const QString &sub, QVBoxLayout *&body);

    Cli *m_cli;
    bool m_fromSettings = false;
    QLabel *m_joinTitle, *m_doneText;
    QListWidget *m_list;
    QWidget *m_busy, *m_done;
};
