#include "welcome.h"
#include <QFileDialog>
#include <QInputDialog>
#include <QJsonArray>
#include <QJsonObject>
#include <QListWidget>
#include <QProgressBar>

static const QStringList kLocked = {"displays", "secrets"};

QWidget *Welcome::page(const QString &title, const QString &sub, QVBoxLayout *&body)
{
    auto *w = new QWidget;
    body = new QVBoxLayout(w);
    body->setContentsMargins(0, 0, 0, 0);
    body->setSpacing(12);
    body->addWidget(label(title, 1.8, true));
    if (!sub.isEmpty())
        body->addWidget(label(sub, 1.0, false, true));
    body->addSpacing(8);
    return w;
}

Welcome::Welcome(Cli *cli, QWidget *parent)
    : FadeStack(parent)
    , m_cli(cli)
{
    QVBoxLayout *v;
    auto centered = [](QWidget *w, QWidget *x) { static_cast<QVBoxLayout *>(w->layout())->addWidget(x, 0, Qt::AlignHCenter); };

    // Intro
    auto *intro = new QWidget;
    v = new QVBoxLayout(intro);
    v->setSpacing(14);
    auto *logo = icon("org.kde.karry", 64);
    v->addWidget(logo, 0, Qt::AlignHCenter);
    v->addSpacing(8);
    auto *t = label(i18n("Keep your Plasma setup safe"), 2.0, true);
    t->setAlignment(Qt::AlignHCenter);
    auto *s = label(i18n("Back up your themes, panels, shortcuts and settings. Restore them any time, or share them with your other computers."), 1.0, false, true);
    s->setAlignment(Qt::AlignHCenter);
    v->addWidget(t);
    v->addWidget(s);
    v->addSpacing(12);
    auto *start = button(i18n("Get started"), "go-next", true);
    start->setMinimumWidth(180);
    connect(start, &QPushButton::clicked, this, [this] { open(Where); });
    centered(intro, start);
    addWidget(shell(intro, 520, true));

    // Where to keep it
    QVBoxLayout *b;
    auto *where = page(i18n("Where to keep it"), i18n("Karry stores your setup as plain files in one folder."), b);
    auto *local = new QPushButton;
    local->setDefault(true);
    local->setMinimumHeight(76);
    auto *lh = new QHBoxLayout(local);
    lh->setContentsMargins(16, 8, 16, 8);
    auto *lt = new QVBoxLayout;
    lt->setSpacing(2);
    lt->addStretch(1);
    lt->addWidget(label(i18n("On this computer"), 1.15, true));
    lt->addWidget(label(i18n("~/Karry. No account needed."), 1.0, false, true));
    lt->addStretch(1);
    lh->addWidget(icon("folder-home", 32));
    lh->addLayout(lt, 1);
    lh->addWidget(icon("go-next", 22));
    for (auto *c : local->findChildren<QWidget *>())
        c->setAttribute(Qt::WA_TransparentForMouseEvents);
    connect(local, &QPushButton::clicked, this, [this] { choose({"init", "--local"}); });
    b->addWidget(local);
    b->addSpacing(16);
    b->addWidget(label(i18n("Sync with other computers"), 1.15, true));
    b->addWidget(label(i18n("Pick a folder that Syncthing or Nextcloud already syncs, or a git repository."), 1.0, false, true));
    auto *row = new QHBoxLayout;
    auto *folder = button(i18n("Choose folder…"), "folder-open", false, m_cli);
    auto *git = button(i18n("Git repository…"), "network-server", false, m_cli);
    row->addWidget(folder);
    row->addWidget(git);
    row->addStretch(1);
    b->addLayout(row);
    connect(folder, &QPushButton::clicked, this, [this] {
        const QString d = QFileDialog::getExistingDirectory(this, i18n("Choose a folder"), QDir::homePath());
        if (!d.isEmpty())
            choose({"init", "--folder", d});
    });
    connect(git, &QPushButton::clicked, this, [this] {
        const QString u = QInputDialog::getText(this, i18n("Git repository"), i18n("Repository address:")).trimmed();
        if (!u.isEmpty())
            choose({"init", "--git", u});
    });
    b->addSpacing(16);
    auto *file = linkButton(i18n("Restore from a .karry file…"));
    connect(file, &QPushButton::clicked, this, [this] {
        const QString f = QFileDialog::getOpenFileName(this, i18n("Restore from file"), QDir::homePath(), i18n("Karry files (*.karry)"));
        if (!f.isEmpty())
            emit restoreFile(f);
    });
    auto *back = linkButton(i18n("Back"));
    connect(back, &QPushButton::clicked, this, [this] { m_fromSettings ? emit cancelled() : open(Intro); });
    auto *links = new QHBoxLayout;
    links->addWidget(back);
    links->addStretch(1);
    links->addWidget(file);
    b->addLayout(links);
    addWidget(shell(where, 600, true));

    // Join
    auto *join = new QWidget;
    v = new QVBoxLayout(join);
    v->setSpacing(14);
    v->addWidget(icon("computer", 64), 0, Qt::AlignHCenter);
    m_joinTitle = label({}, 1.8, true);
    m_joinTitle->setAlignment(Qt::AlignHCenter);
    auto *js = label(i18n("Preview what would change on this computer first. Nothing is applied until you confirm."), 1.0, false, true);
    js->setAlignment(Qt::AlignHCenter);
    v->addWidget(m_joinTitle);
    v->addWidget(js);
    v->addSpacing(12);
    auto *preview = button(i18n("Preview changes"), "view-list-details", true, m_cli);
    connect(preview, &QPushButton::clicked, this, [this] { emit finished(true); });
    centered(join, preview);
    auto *jback = linkButton(i18n("Back"));
    connect(jback, &QPushButton::clicked, this, [this] { open(Where, m_fromSettings); });
    centered(join, jback);
    addWidget(shell(join, 520, true));

    // Include
    auto *inc = page(i18n("What to include"), i18n("You can change this later. Passwords and accounts are never copied."), b);
    m_list = new QListWidget;
    flatList(m_list);
    m_list->setItemDelegate(new RowDelegate(m_list));
    b->addWidget(m_list);
    b->addSpacing(12);
    auto *iback = linkButton(i18n("Back"));
    auto *go = button(i18n("Back up now"), "document-save", true, m_cli);
    connect(iback, &QPushButton::clicked, this, [this] { open(Where, m_fromSettings); });
    auto *ib = new QHBoxLayout;
    ib->addWidget(iback);
    ib->addStretch(1);
    ib->addWidget(go);
    b->addLayout(ib);
    connect(go, &QPushButton::clicked, this, [this] {
        QStringList only;
        for (int i = 0; i < m_list->count(); i++)
            if (m_list->item(i)->checkState() == Qt::Checked)
                only << m_list->item(i)->data(Qt::UserRole).toString();
        open(Progress);
        m_cli->run({"backup", "--only", only.join(',')}, this, [this](const QJsonValue &) {
            m_busy->hide();
            m_done->show();
        });
    });
    addWidget(shell(inc, 600, true));

    // Progress, then done
    auto *prog = new QWidget;
    v = new QVBoxLayout(prog);
    v->setSpacing(14);
    m_busy = new QWidget;
    auto *bv = new QVBoxLayout(m_busy);
    auto *bt = label(i18n("Backing up your setup…"), 1.5, true);
    bt->setAlignment(Qt::AlignHCenter);
    auto *bar = new QProgressBar;
    bar->setRange(0, 0);
    bar->setTextVisible(false);
    bv->addWidget(bt);
    bv->addWidget(bar);
    m_done = new QWidget;
    auto *dv = new QVBoxLayout(m_done);
    dv->setSpacing(14);
    dv->addWidget(icon("dialog-ok-apply", 64), 0, Qt::AlignHCenter);
    auto *dt = label(i18n("You're all set"), 1.8, true);
    dt->setAlignment(Qt::AlignHCenter);
    m_doneText = label({}, 1.0, false, true);
    m_doneText->setAlignment(Qt::AlignHCenter);
    auto *openBtn = button(i18n("Open Karry"), "go-next", true);
    connect(openBtn, &QPushButton::clicked, this, [this] { emit finished(false); });
    dv->addWidget(dt);
    dv->addWidget(m_doneText);
    dv->addSpacing(12);
    dv->addWidget(openBtn, 0, Qt::AlignHCenter);
    v->addWidget(m_busy);
    v->addWidget(m_done);
    addWidget(shell(prog, 520, true));

    // A failed backup puts the user back on the include step
    connect(m_cli, &Cli::failed, this, [this] {
        if (currentIndex() == Progress && m_busy->isVisible())
            open(Include, m_fromSettings);
    });
}

void Welcome::choose(const QStringList &initArgs)
{
    m_cli->run(initArgs, this, [this](const QJsonValue &cfg) {
        m_cli->run({"info"}, this, [this, cfg](const QJsonValue &info) {
            m_doneText->setText(i18n("Your setup is saved in %1.", cfg.toObject()["path"].toString()));
            const QString host = info.toObject()["source"].toObject()["hostname"].toString();
            host.isEmpty() ? open(Include, m_fromSettings) : open(Join, m_fromSettings, host);
        });
    });
}

void Welcome::open(Step step, bool fromSettings, const QString &host)
{
    m_fromSettings = fromSettings;
    if (step == Join)
        m_joinTitle->setText(i18n("Found the setup from %1", host));
    if (step == Progress) {
        m_busy->show();
        m_done->hide();
    }
    if (step == Include) {
        m_cli->run({"status"}, this, [this](const QJsonValue &st) {
            m_list->clear();
            for (const QJsonValue &cv : st.toObject()["categories"].toArray()) {
                const QJsonObject c = cv.toObject();
                const QString id = c["id"].toString();
                auto *it = new QListWidgetItem(QIcon::fromTheme(c["icon"].toString()), c["name"].toString());
                it->setData(Qt::UserRole, id);
                it->setData(CountRole, i18np("%1 item", "%1 items", c["items"].toInt()));
                if (kLocked.contains(id)) {
                    it->setData(StatusRole, id == "secrets" ? i18n("never copied") : i18n("this machine only"));
                    it->setData(StatusIconRole, "object-locked");
                    it->setCheckState(id == "secrets" ? Qt::Unchecked : Qt::Checked);
                    if (id == "secrets")
                        it->setData(CountRole, QString());
                } else {
                    it->setFlags(it->flags() | Qt::ItemIsUserCheckable);
                    it->setCheckState(Qt::Checked);
                }
                m_list->addItem(it);
            }
            m_list->setFixedHeight(m_list->count() * 44 + 4);
        });
    }
    go(step);
}
