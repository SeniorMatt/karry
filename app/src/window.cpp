#include "window.h"
#include "preview.h"
#include "welcome.h"
#include <KAboutData>
#include <KMessageWidget>
#include <QCheckBox>
#include <QClipboard>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFile>
#include <QGridLayout>
#include <QFileDialog>
#include <QInputDialog>
#include <QJsonDocument>
#include <QMessageBox>
#include <QProgressBar>
#include <QDate>
#include <QDir>
#include <QFontDatabase>

static QString ago(const QDateTime &t)
{
    const qint64 s = t.secsTo(QDateTime::currentDateTimeUtc());
    if (s < 60)
        return i18n("just now");
    if (s < 3600)
        return i18n("%1 min ago", s / 60);
    if (s < 86400)
        return i18np("%1 hour ago", "%1 hours ago", s / 3600);
    return i18np("%1 day ago", "%1 days ago", s / 86400);
}

static void section(const QString &title, QVBoxLayout *v)
{
    v->addSpacing(14);
    v->addWidget(label(title, 1.2, true));
}

Window::Window()
{
    setWindowTitle("Karry");
    m_cli = new Cli(this);
    auto *central = new QWidget;
    auto *col = new QVBoxLayout(central);
    col->setContentsMargins(0, 0, 0, 0);
    col->setSpacing(0);
    m_bar = new KMessageWidget;
    m_bar->setWordWrap(true);
    m_bar->setCloseButtonVisible(true);
    m_bar->hide();
    m_busy = new QProgressBar;
    m_busy->setFixedHeight(3);
    m_busy->setTextVisible(false);
    m_busy->setRange(0, 1);
    m_root = new FadeStack;
    col->addWidget(m_bar);
    col->addWidget(m_busy);
    col->addWidget(m_root, 1);
    setCentralWidget(central);
    connect(m_cli, &Cli::busyChanged, this, [this](bool b) { m_busy->setRange(0, b ? 0 : 1); });
    connect(m_cli, &Cli::failed, this, [this](const QString &e) { message(e, true); });

    m_welcome = new Welcome(m_cli);
    m_root->addWidget(m_welcome);
    auto *main = new QWidget;
    auto *h = new QHBoxLayout(main);
    h->setContentsMargins(0, 0, 0, 0);
    h->setSpacing(0);
    m_nav = new QListWidget;
    m_nav->setFixedWidth(200);
    m_nav->setFrameShape(QFrame::NoFrame);
    m_nav->setIconSize({22, 22});
    m_nav->setSpacing(2);
    m_nav->setStyleSheet("QListWidget::item { padding: 8px 6px; }");
    const QList<QPair<QString, QString>> items = {{i18n("Overview"), "go-home"}, {i18n("Shortcuts"), "preferences-desktop-keyboard-shortcut"}, {i18n("Snapshots"), "view-history"}, {i18n("Settings"), "configure"}};
    for (const auto &i : items)
        m_nav->addItem(new QListWidgetItem(QIcon::fromTheme(i.second), i.first));
    m_pages = new FadeStack;
    for (int i = 0; i < PreviewPage; i++)
        m_pages->addWidget(new QWidget);
    m_preview = new Preview(m_cli);
    m_pages->addWidget(m_preview);
    h->addWidget(m_nav);
    h->addWidget(m_pages, 1);
    m_root->addWidget(main);
    m_nav->setCurrentRow(0);
    auto go = [this] { m_pages->go(m_nav->currentRow()); };
    connect(m_nav, &QListWidget::currentRowChanged, this, go);
    connect(m_nav, &QListWidget::itemClicked, this, go);

    connect(m_welcome, &Welcome::cancelled, this, &Window::reload);
    connect(m_welcome, &Welcome::finished, this, [this](bool join) {
        if (join)
            openPreview(i18n("Review before joining"), i18n("Nothing is applied until you press Sync."), {"sync", "--dry-run"}, {"sync"}, false);
        reload();
    });
    connect(m_welcome, &Welcome::restoreFile, this, [this](const QString &f) {
        openPreview(i18n("Restore from file"), f, {"restore", f, "--dry-run"}, {"restore", f}, false);
    });
    connect(m_preview, &Preview::applied, this, &Window::applied);
    connect(m_preview, &Preview::cancelled, this, [this] {
        m_pages->go(m_nav->currentRow());
        reload();
    });

    const QString dbg = m_debug = qEnvironmentVariable("KARRY_PAGE");
    if (dbg == "welcome" || dbg == "where" || dbg == "include" || dbg == "join") {
        m_cli->run({"info"}, this, [=, this](const QJsonValue &i) {
            const QString host = i.toObject()["source"].toObject()["hostname"].toString("thinkpad");
            m_welcome->open(dbg == "where" ? Welcome::Where : dbg == "include" ? Welcome::Include : dbg == "join" ? Welcome::Join : Welcome::Intro, false, host);
            m_root->go(0);
        });
        return;
    }
    reload();
}

void Window::debugPage(const QString &name)
{
    const QStringList names = {"overview", "shortcuts", "snapshots", "settings"};
    if (names.contains(name))
        m_nav->setCurrentRow(names.indexOf(name));
    else if (name == "preview")
        openPreview(i18n("Review changes"), i18n("From thinkpad"), {"sync", "--dry-run"}, {"sync"}, false);
    else if (name == "restore")
        openPreview(i18n("Restore from backup"), i18n("From thinkpad"), {"restore", "--dry-run"}, {"restore"}, false);
}

void Window::message(const QString &text, bool error)
{
    m_bar->setMessageType(error ? KMessageWidget::Error : KMessageWidget::Positive);
    m_bar->setText(text);
    m_bar->animatedShow();
}

void Window::reload()
{
    m_cli->run({"info"}, this, [this](const QJsonValue &i) {
        m_info = i.toObject();
        if (m_info["config"].isNull()) {
            m_welcome->open(Welcome::Intro);
            m_root->go(0);
            return;
        }
        QList<QStringList> cmds = {{"status"}, {"snapshots"}, {"doctor"}};
        if (!m_info["local_only"].toBool())
            cmds << QStringList{"sync", "--dry-run"};
        auto got = std::make_shared<QList<QJsonValue>>(cmds.size());
        auto left = std::make_shared<int>(cmds.size());
        for (int k = 0; k < cmds.size(); k++)
            m_cli->run(cmds[k], this, [=, this](const QJsonValue &v) {
                (*got)[k] = v;
                if (--*left)
                    return;
                m_status = (*got)[0].toObject();
                m_snaps = (*got)[1].toArray();
                m_issues = (*got)[2].toArray();
                m_incoming = got->size() > 3 ? (*got)[3].toObject()["incoming"].toArray() : QJsonArray();
                m_cats = {};
                for (const QJsonValue &c : m_status["categories"].toArray())
                    m_cats[c.toObject()["id"].toString()] = c;
                rebuild();
                m_root->go(1);
                if (!m_debug.isEmpty())
                    debugPage(std::exchange(m_debug, {}));
            });
    });
}

void Window::rebuild()
{
    const QList<QWidget *> built = {buildOverview(), buildShortcuts(), buildSnapshots(), buildSettings()};
    const int current = m_pages->currentIndex();
    for (int i = 0; i < built.size(); i++) {
        QWidget *old = m_pages->widget(i);
        m_pages->removeWidget(old);
        old->deleteLater();
        m_pages->insertWidget(i, built[i]);
    }
    m_pages->setCurrentIndex(current);
}

QDateTime Window::lastSynced() const
{
    QFile f(m_info["config"].toObject()["path"].toString() + "/machines/" + m_info["machine"].toObject()["id"].toString() + "/manifest.json");
    if (!f.open(QIODevice::ReadOnly))
        return {};
    const QJsonObject m = QJsonDocument::fromJson(f.readAll()).object();
    QString s = m["last_sync"].toString(m["created"].toString());
    return QDateTime::fromString(s.left(19) + "Z", Qt::ISODate);
}

QWidget *Window::buildOverview()
{
    const bool local = m_info["local_only"].toBool();
    const QJsonArray cats = m_status["categories"].toArray();
    int issues = 0, changed = 0;
    for (const QJsonValue &c : cats) {
        issues += c.toObject()["issues"].toInt();
        changed += c.toObject()["changed"].toInt();
    }
    const QDateTime last = lastSynced();
    const int inc = m_incoming.size();
    QString ic = "dialog-ok-apply", title = local ? i18n("Everything is backed up") : i18n("Everything is in sync");
    if (issues) {
        ic = "dialog-warning";
        title = i18np("%1 shortcut problem", "%1 shortcut problems", issues);
    } else if (inc) {
        ic = "view-refresh";
        title = i18np("%1 change from %2", "%1 changes from %2", inc, m_incoming[0].toObject()["machine"].toString());
    } else if (changed) {
        ic = "document-save";
        title = i18np("%1 change to save", "%1 changes to save", changed);
    }
    const QString sub = last.isValid() ? (local ? i18n("Last backup %1", ago(last)) : i18n("Last synced %1", ago(last))) : (local ? i18n("Not backed up yet") : i18n("Not synced yet"));

    auto *w = new QWidget;
    auto *v = new QVBoxLayout(w);
    v->setContentsMargins(0, 0, 0, 0);
    auto *hero = new QHBoxLayout;
    hero->setSpacing(16);
    hero->addWidget(icon(ic, 64));
    auto *text = new QVBoxLayout;
    text->setSpacing(2);
    text->addStretch(1);
    text->addWidget(label(title, 1.6, true));
    text->addWidget(label(sub, 1.0, false, true));
    text->addStretch(1);
    hero->addLayout(text, 1);
    auto *restore = button(i18n("Restore…"), "document-revert", false, m_cli);
    auto *main = local ? button(i18n("Back up now"), "document-save", true, m_cli) : button(i18n("Sync now"), "view-refresh", true, m_cli);
    hero->addWidget(restore, 0, Qt::AlignVCenter);
    hero->addWidget(main, 0, Qt::AlignVCenter);
    v->addLayout(hero);
    connect(restore, &QPushButton::clicked, this, [this] { openPreview(i18n("Restore from backup"), i18n("This computer's saved setup"), {"restore", "--dry-run"}, {"restore"}, false); });
    connect(main, &QPushButton::clicked, this, [this, local] {
        if (local)
            m_cli->run({"backup"}, this, [this](const QJsonValue &r) {
                message(i18n("Backed up %1 files.", r.toObject()["shared"].toInt() + r.toObject()["machine"].toInt()), false);
                reload();
            });
        else
            openPreview(i18n("Review changes"), i18n("Sync with %1", m_info["config"].toObject()["path"].toString()), {"sync", "--dry-run"}, {"sync"}, true);
    });

    v->addSpacing(20);
    v->addWidget(label(i18n("What's included"), 1.2, true));
    auto *list = new QListWidget;
    flatList(list);
    list->setItemDelegate(new RowDelegate(list));
    list->setAttribute(Qt::WA_TransparentForMouseEvents);
    list->setFocusPolicy(Qt::NoFocus);
    for (const QJsonValue &cv : cats) {
        const QJsonObject c = cv.toObject();
        const QString id = c["id"].toString();
        auto *it = new QListWidgetItem(QIcon::fromTheme(c["icon"].toString()), c["name"].toString());
        it->setData(CountRole, i18np("%1 item", "%1 items", c["items"].toInt()));
        if (id == "displays" || id == "secrets") {
            it->setData(StatusRole, id == "secrets" ? i18n("never copied") : i18n("this machine only"));
            it->setData(StatusIconRole, "object-locked");
            if (id == "secrets")
                it->setData(CountRole, QString());
        } else if (c["issues"].toInt()) {
            it->setData(StatusRole, i18np("%1 problem", "%1 problems", c["issues"].toInt()));
            it->setData(StatusIconRole, "dialog-warning");
            it->setData(StatusAccentRole, true);
        } else if (c["changed"].toInt()) {
            it->setData(StatusRole, i18n("%1 changed", c["changed"].toInt()));
            it->setData(StatusAccentRole, true);
        } else {
            it->setData(StatusRole, local ? i18n("backed up") : i18n("in sync"));
        }
        list->addItem(it);
    }
    list->setFixedHeight(list->count() * 44 + 4);
    v->addWidget(list);
    return shell(w, 760);
}

QWidget *Window::buildShortcuts()
{
    auto *w = new QWidget;
    auto *v = new QVBoxLayout(w);
    v->setContentsMargins(0, 0, 0, 0);
    v->setSpacing(10);
    v->addWidget(label(i18n("Shortcuts"), 1.6, true));
    v->addWidget(label(i18n("Clashes and leftovers in your keyboard shortcuts."), 1.0, false, true));
    v->addSpacing(12);
    if (m_issues.isEmpty()) {
        v->addSpacing(40);
        v->addWidget(icon("dialog-ok-apply", 64), 0, Qt::AlignHCenter);
        auto *l = label(i18n("No shortcut problems"), 1.3, true);
        l->setAlignment(Qt::AlignHCenter);
        v->addWidget(l);
    }
    for (const QJsonValue &iv : m_issues) {
        const QJsonObject issue = iv.toObject();
        auto *row = new QHBoxLayout;
        row->setSpacing(14);
        row->addWidget(icon("dialog-warning", 22), 0, Qt::AlignTop);
        auto *col = new QVBoxLayout;
        col->setSpacing(4);
        col->addWidget(label(issue["message"].toString(), 1.05, true));
        QStringList keys;
        for (const QJsonValue &b : issue["bindings"].toArray())
            if (!b.toObject()["key"].toString().isEmpty())
                keys << QString("%1: %2").arg(b.toObject()["friendly"].toString(), b.toObject()["key"].toString());
        if (!keys.isEmpty())
            col->addWidget(label(keys.join("  ·  "), 1.0, false, true));
        auto *fixes = new QGridLayout;
        fixes->setContentsMargins(0, 4, 0, 0);
        int n = 0;
        for (const QJsonValue &fv : issue["fixes"].toArray()) {
            const QJsonObject fix = fv.toObject();
            auto *b = button(fix["label"].toString(), {}, false, m_cli);
            connect(b, &QPushButton::clicked, this, [=, this] {
                QStringList args = {"fix", issue["id"].toString(), fix["id"].toString()};
                if (fix["needs_input"].toBool()) {
                    const QString k = QInputDialog::getText(this, i18n("New shortcut"), i18n("Type the new shortcut (for example Meta+K):"));
                    if (k.isEmpty())
                        return;
                    args << k;
                }
                m_cli->run(args, this, [this](const QJsonValue &r) {
                    message(r.toObject()["plan"].toObject()["needs_logout"].toBool() ? i18n("Fixed. Log out and back in to finish.") : i18n("Fixed."), false);
                    reload();
                });
            });
            fixes->addWidget(b, n / 3, n % 3, Qt::AlignLeft);
            n++;
        }
        fixes->setColumnStretch(3, 1);
        col->addLayout(fixes);
        row->addLayout(col, 1);
        v->addLayout(row);
        v->addSpacing(14);
    }
    return shell(w, 760);
}

QWidget *Window::buildSnapshots()
{
    auto *w = new QWidget;
    auto *v = new QVBoxLayout(w);
    v->setContentsMargins(0, 0, 0, 0);
    v->setSpacing(10);
    v->addWidget(label(i18n("Snapshots"), 1.6, true));
    v->addWidget(label(i18n("Karry saves a snapshot before every restore or sync. Undo puts those files back."), 1.0, false, true));
    v->addSpacing(12);
    if (m_snaps.isEmpty()) {
        v->addSpacing(40);
        v->addWidget(icon("view-history", 64), 0, Qt::AlignHCenter);
        auto *l = label(i18n("No snapshots yet"), 1.3, true);
        l->setAlignment(Qt::AlignHCenter);
        v->addWidget(l);
    }
    for (int i = m_snaps.size() - 1; i >= 0; i--) {
        const QJsonObject s = m_snaps[i].toObject();
        auto *row = new QHBoxLayout;
        row->setSpacing(14);
        row->addWidget(icon("view-history", 22));
        auto *col = new QVBoxLayout;
        col->setSpacing(0);
        const QDateTime t = QDateTime::fromString(s["created"].toString().left(19) + "Z", Qt::ISODate);
        col->addWidget(label(s["label"].toString(), 1.05, true));
        col->addWidget(label(i18n("%1 · %2 files", QLocale().toString(t.toLocalTime(), QLocale::ShortFormat), s["files"].toArray().size()), 1.0, false, true));
        row->addLayout(col, 1);
        auto *undo = button(i18n("Undo"), "edit-undo", false, m_cli);
        connect(undo, &QPushButton::clicked, this, [=, this] {
            if (QMessageBox::question(this, i18n("Undo snapshot"), i18n("Put back the files saved in this snapshot?")) != QMessageBox::Yes)
                return;
            m_cli->run({"undo", s["id"].toString()}, this, [this](const QJsonValue &) {
                message(i18n("Restored. Log out and back in to finish."), false);
                reload();
            });
        });
        row->addWidget(undo);
        v->addLayout(row);
        v->addSpacing(8);
    }
    return shell(w, 760);
}

QWidget *Window::buildSettings()
{
    const QJsonObject cfg = m_info["config"].toObject();
    auto *w = new QWidget;
    auto *v = new QVBoxLayout(w);
    v->setContentsMargins(0, 0, 0, 0);
    v->setSpacing(8);
    v->addWidget(label(i18n("Settings"), 1.6, true));
    section(i18n("Storage"), v);
    auto *where = label(cfg["kind"].toString() == "git" ? i18n("Git repository %1\nLocal copy: %2", cfg["url"].toString(), cfg["path"].toString()) : i18n("Folder: %1", cfg["path"].toString()), 1.0, false, true);
    where->setTextInteractionFlags(Qt::TextSelectableByMouse);
    v->addWidget(where);
    auto *setup = button(i18n("Set up sync…"), "folder-sync", false, m_cli);
    connect(setup, &QPushButton::clicked, this, [this] {
        m_welcome->open(Welcome::Where, true);
        m_root->go(0);
    });
    auto *r1 = new QHBoxLayout;
    r1->addWidget(setup);
    r1->addStretch(1);
    v->addLayout(r1);

    section(i18n("Backup file"), v);
    v->addWidget(label(i18n("A .karry file holds your whole setup, to move it by USB stick or email."), 1.0, false, true));
    auto *exp = button(i18n("Export to file…"), "document-export", false, m_cli);
    auto *imp = button(i18n("Restore from file…"), "document-import", false, m_cli);
    connect(exp, &QPushButton::clicked, this, [this] {
        const QString f = QFileDialog::getSaveFileName(this, i18n("Export setup"), QDir::homePath() + "/kde-setup-" + QDate::currentDate().toString(Qt::ISODate) + ".karry", i18n("Karry files (*.karry)"));
        if (!f.isEmpty())
            m_cli->run({"export", f}, this, [this, f](const QJsonValue &) { message(i18n("Saved %1.", f), false); });
    });
    connect(imp, &QPushButton::clicked, this, [this] {
        const QString f = QFileDialog::getOpenFileName(this, i18n("Restore from file"), QDir::homePath(), i18n("Karry files (*.karry)"));
        if (!f.isEmpty())
            openPreview(i18n("Restore from file"), f, {"restore", f, "--dry-run"}, {"restore", f}, false);
    });
    auto *r2 = new QHBoxLayout;
    r2->addWidget(exp);
    r2->addWidget(imp);
    r2->addStretch(1);
    v->addLayout(r2);

    section(i18n("About"), v);
    auto *about = new QHBoxLayout;
    about->addWidget(icon("org.kde.karry", 32));
    about->addWidget(label(i18n("Karry %1\nBack up, restore and sync your Plasma setup.", QString(KARRY_VERSION)), 1.0, false, true), 1);
    v->addLayout(about);
    auto *un = linkButton(i18n("Uninstall Karry…"));
    connect(un, &QPushButton::clicked, this, &Window::uninstall);
    v->addSpacing(24);
    auto *r3 = new QHBoxLayout;
    r3->addWidget(un);
    r3->addStretch(1);
    v->addLayout(r3);
    return shell(w, 760);
}

void Window::openPreview(const QString &title, const QString &sub, const QStringList &pv, const QStringList &ap, bool autoApply)
{
    m_cli->run(pv, this, [=, this](const QJsonValue &v) {
        const QJsonObject r = v.toObject(), plan = r["plan"].toObject();
        if (autoApply && plan["changes"].toArray().isEmpty() && r["conflicts"].toArray().isEmpty() && plan["shortcut_issues"].toArray().isEmpty()) {
            m_cli->run(ap, this, [this](const QJsonValue &a) { applied(a.toObject()); });
            return;
        }
        m_preview->load(r, m_cats, title, sub, ap);
        m_root->go(1);
        m_pages->go(PreviewPage);
    });
}

void Window::applied(const QJsonObject &r)
{
    QString text = r["published"].isArray() ? i18n("Synced.") : i18n("Done.");
    if (r["plan"].toObject()["needs_logout"].toBool())
        text += " " + i18n("Log out and back in to finish.");
    if (!r["snapshot"].isNull())
        text += " " + i18n("You can undo this under Snapshots.");
    message(text, false);
    m_pages->go(m_nav->currentRow());
    reload();
}

void Window::uninstall()
{
    QDialog d(this);
    d.setWindowTitle(i18n("Uninstall Karry"));
    d.setMinimumWidth(520);
    auto *v = new QVBoxLayout(&d);
    v->setSpacing(10);
    v->addWidget(label(i18n("Uninstall Karry"), 1.4, true));
    v->addWidget(label(i18n("These Karry files will be removed. Your KDE settings and the sync folder are never touched."), 1.0, false, true));
    auto *paths = label({});
    paths->setTextInteractionFlags(Qt::TextSelectableByMouse);
    paths->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    auto *keep = new QCheckBox(i18n("Keep snapshots"));
    keep->setChecked(true);
    auto list = [&] {
        m_cli->run(keep->isChecked() ? QStringList{"uninstall", "--keep-snapshots"} : QStringList{"uninstall"}, &d, [paths](const QJsonValue &a) {
            QStringList l;
            for (const QJsonValue &p : a.toArray())
                l << p.toString();
            paths->setText(l.isEmpty() ? i18n("Nothing to remove.") : l.join('\n'));
        });
    };
    list();
    connect(keep, &QCheckBox::toggled, &d, list);
    v->addWidget(paths);
    v->addWidget(keep);
    auto *box = new QDialogButtonBox;
    auto *remove = box->addButton(i18n("Uninstall"), QDialogButtonBox::AcceptRole);
    remove->setIcon(QIcon::fromTheme("edit-delete"));
    box->addButton(QDialogButtonBox::Cancel);
    connect(box, &QDialogButtonBox::accepted, &d, &QDialog::accept);
    connect(box, &QDialogButtonBox::rejected, &d, &QDialog::reject);
    v->addWidget(box);
    if (d.exec() != QDialog::Accepted)
        return;
    QStringList args = {"uninstall", "--yes"};
    if (keep->isChecked())
        args << "--keep-snapshots";
    m_cli->run(args, this, [this](const QJsonValue &) {
        const QString exe = QCoreApplication::applicationFilePath(), appimage = qEnvironmentVariable("APPIMAGE");
        if (exe.startsWith("/usr/")) {
            const QString cmd = "sudo pacman -R karry-git";
            QMessageBox box(QMessageBox::Information, i18n("Karry's data was removed"), i18n("To remove the program itself, run this in a terminal:\n\n%1", cmd), QMessageBox::Close, this);
            auto *copy = box.addButton(i18n("Copy command"), QMessageBox::ActionRole);
            box.exec();
            if (box.clickedButton() == copy)
                QApplication::clipboard()->setText(cmd);
        } else if (!appimage.isEmpty()) {
            if (QMessageBox::question(this, i18n("Karry's data was removed"), i18n("Delete the AppImage too?\n\n%1", appimage)) == QMessageBox::Yes)
                QFile::remove(appimage);
        } else {
            QMessageBox::information(this, i18n("Karry's data was removed"), i18n("You can delete the program normally."));
        }
        QApplication::quit();
    });
}
