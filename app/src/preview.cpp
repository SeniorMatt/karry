#include "preview.h"
#include <QButtonGroup>
#include <QClipboard>
#include <QFileInfo>
#include <QFontDatabase>
#include <QJsonDocument>
#include <QJsonArray>
#include <QRadioButton>
#include <QTreeWidget>
#include <QTreeWidgetItemIterator>
#include <QHeaderView>
#include <QScrollBar>

Preview::Preview(Cli *cli, QWidget *parent)
    : QWidget(parent)
    , m_cli(cli)
{
    m_layout = new QVBoxLayout(this);
    m_layout->setContentsMargins(0, 0, 0, 0);
}

static QWidget *note(const QString &iconName, const QString &text, QWidget *extra = nullptr)
{
    auto *w = new QWidget;
    auto *h = new QHBoxLayout(w);
    h->setContentsMargins(0, 0, 0, 0);
    h->addWidget(icon(iconName, 22), 0, Qt::AlignTop);
    h->addWidget(label(text), 1);
    if (extra)
        h->addWidget(extra);
    return w;
}

void Preview::load(const QJsonObject &r, const QJsonObject &cats, const QString &title, const QString &subtitle, const QStringList &applyArgs)
{
    const QJsonObject plan = r["plan"].toObject();
    const QJsonArray conflicts = r["conflicts"].toArray();
    auto *page = new QWidget;
    auto *v = new QVBoxLayout(page);
    v->setContentsMargins(0, 0, 0, 0);
    v->setSpacing(10);
    v->addWidget(label(title, 1.6, true));
    v->addWidget(label(subtitle, 1.0, false, true));
    v->addSpacing(8);

    // Changes, grouped by category and file
    auto *tree = new QTreeWidget;
    tree->setHeaderLabels({i18n("Setting"), i18n("Change")});
    tree->setFrameShape(QFrame::NoFrame);
    tree->setIconSize({22, 22});
    tree->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    tree->header()->setSectionResizeMode(1, QHeaderView::ResizeToContents);
    tree->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    QMap<QString, QTreeWidgetItem *> catItems, fileItems;
    QStringList hints;
    int shown = 0;
    for (const QJsonValue &cv : plan["changes"].toArray()) {
        const QJsonObject c = cv.toObject();
        const QString type = c["type"].toString(), cat = c["category"].toString(), file = c["store_path"].toString();
        if (type == "InstallHint") {
            hints << c["command"].toString();
            continue;
        }
        if (type == "NeedsLogout")
            continue;
        auto *&ci = catItems[cat];
        if (!ci) {
            ci = new QTreeWidgetItem(tree, {cats[cat].toObject()["name"].toString(cat)});
            ci->setIcon(0, QIcon::fromTheme(cats[cat].toObject()["icon"].toString()));
            QFont f = ci->font(0);
            f.setBold(true);
            ci->setFont(0, f);
        }
        const QString name = QFileInfo(file).fileName();
        if (type == "WriteFile") {
            new QTreeWidgetItem(ci, {name, c["existing"].toBool() ? i18n("replace file") : i18n("new file")});
        } else {
            auto *&fi = fileItems[file];
            if (!fi)
                fi = new QTreeWidgetItem(ci, {name});
            const QString arrow = type == "DeleteKey" ? i18n("%1 → removed", c["old"].toString()) : c["old"].isNull() ? c["value"].toString() : QString("%1 → %2").arg(c["old"].toString(), c["value"].toString());
            auto *k = new QTreeWidgetItem(fi, {QString("%1 › %2").arg(c["group"].toString().remove('[').remove(']'), c["key"].toString()), arrow});
            k->setToolTip(1, arrow);
            fi->setText(1, i18np("%1 setting", "%1 settings", fi->childCount()));
        }
        shown++;
    }
    if (shown) {
        v->addWidget(label(i18n("Changes"), 1.2, true));
        tree->expandAll();
        int rows = 0;
        for (QTreeWidgetItemIterator it(tree); *it; ++it)
            rows++;
        tree->setFixedHeight(qMin(360, rows * 26 + 34));
        v->addWidget(tree);
    } else {
        delete tree;
    }

    // Conflicts: both computers changed the same setting
    QList<QPair<QString, QRadioButton *>> theirs;
    if (!conflicts.isEmpty()) {
        v->addSpacing(8);
        v->addWidget(label(i18n("Conflicts"), 1.2, true));
        v->addWidget(label(i18n("Both computers changed these. Choose which value to keep."), 1.0, false, true));
        for (const QJsonValue &cv : conflicts) {
            const QJsonObject c = cv.toObject();
            const QString mine = c["local"].isNull() ? i18n("(not set)") : c["local"].toString(), other = c["remote"].isNull() ? i18n("(not set)") : c["remote"].toString();
            v->addWidget(label(c["key"].toString().isEmpty() ? QFileInfo(c["store_path"].toString()).fileName() : QString("%1 › %2").arg(c["group"].toString().remove('[').remove(']'), c["key"].toString()), 1.0, true));
            auto *keep = new QRadioButton(i18n("Keep mine: %1", mine)), *use = new QRadioButton(i18n("Use theirs: %1", other));
            keep->setChecked(true);
            auto *g = new QButtonGroup(page);
            g->addButton(keep);
            g->addButton(use);
            v->addWidget(keep);
            v->addWidget(use);
            theirs << qMakePair(QString("%1|%2|%3").arg(c["store_path"].toString(), c["group"].toString(), c["key"].toString()), use);
        }
    }
    if (!hints.isEmpty()) {
        v->addSpacing(8);
        v->addWidget(label(i18n("Install these first"), 1.2, true));
        v->addWidget(label(i18n("Some themes and plugins come from packages. Karry can't install them for you."), 1.0, false, true));
        for (const QString &cmd : hints) {
            auto *code = label(cmd);
            code->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
            code->setTextInteractionFlags(Qt::TextSelectableByMouse);
            auto *copy = new QPushButton(QIcon::fromTheme("edit-copy"), i18n("Copy"));
            connect(copy, &QPushButton::clicked, this, [cmd] { QApplication::clipboard()->setText(cmd); });
            auto *row = new QWidget;
            auto *h = new QHBoxLayout(row);
            h->setContentsMargins(0, 0, 0, 0);
            h->addWidget(code, 1);
            h->addWidget(copy);
            v->addWidget(row);
        }
    }
    if (plan["needs_logout"].toBool())
        v->addWidget(note("system-log-out", i18n("Log out and back in to finish applying these changes.")));
    for (const QJsonValue &i : plan["shortcut_issues"].toArray())
        v->addWidget(note("dialog-warning", i.toObject()["message"].toString()));
    for (const QJsonValue &w : plan["warnings"].toArray())
        v->addWidget(note("dialog-information", w.toString()));

    if (!shown && hints.isEmpty() && conflicts.isEmpty() && plan["shortcut_issues"].toArray().isEmpty())
        v->addWidget(note("dialog-ok-apply", i18n("Nothing to change on this computer.")));

    auto *cancel = button(i18n("Cancel"), "dialog-cancel", false, m_cli);
    auto *apply = button(applyArgs.value(0) == "sync" ? i18n("Sync") : i18n("Apply"), "dialog-ok-apply", true, m_cli);
    connect(cancel, &QPushButton::clicked, this, &Preview::cancelled);
    connect(apply, &QPushButton::clicked, this, [=, this] {
        QStringList args = applyArgs;
        if (applyArgs.value(0) == "sync") {
            QJsonArray wins;
            for (const auto &t : theirs)
                if (t.second->isChecked())
                    wins << t.first;
            args << "--remote-wins" << QString::fromUtf8(QJsonDocument(wins).toJson(QJsonDocument::Compact));
        }
        m_cli->run(args, this, [this](const QJsonValue &res) { emit applied(res.toObject()); });
    });
    delete m_scroll;
    delete m_footer;
    m_scroll = shell(page, 860);
    m_footer = new QWidget;
    auto *outer = new QHBoxLayout(m_footer);
    auto *bar = new QWidget;
    bar->setMaximumWidth(860);
    auto *buttons = new QHBoxLayout(bar);
    buttons->setContentsMargins(0, 0, 0, 0);
    buttons->addStretch(1);
    buttons->addWidget(cancel);
    buttons->addWidget(apply);
    outer->addStretch(1);
    outer->addWidget(bar, 100);
    outer->addStretch(1);
    // line up with the page content, which sits left of the scroll bar
    QScrollBar *sb = m_scroll->verticalScrollBar();
    auto align = [=] { outer->setContentsMargins(32, 8, 32 + (sb->maximum() > 0 ? sb->sizeHint().width() : 0), 16); };
    connect(sb, &QScrollBar::rangeChanged, this, align);
    align();
    m_layout->addWidget(m_scroll, 1);
    m_layout->addWidget(m_footer);
}
