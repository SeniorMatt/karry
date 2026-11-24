#pragma once
#include "cli.h"
#include <KLocalizedString>
#include <QApplication>
#include <QGraphicsOpacityEffect>
#include <QHBoxLayout>
#include <QLabel>
#include <QPropertyAnimation>
#include <QPushButton>
#include <QScrollArea>
#include <QStackedWidget>
#include <QStyledItemDelegate>
#include <QJsonArray>
#include <QJsonObject>
#include <QListWidget>
#include <QPainter>

inline QLabel *label(const QString &text, double scale = 1.0, bool bold = false, bool dim = false)
{
    auto *l = new QLabel(text);
    QFont f = l->font();
    f.setPointSizeF(f.pointSizeF() * scale);
    f.setBold(bold);
    l->setFont(f);
    l->setWordWrap(true);
    if (dim)
        l->setForegroundRole(QPalette::PlaceholderText);
    return l;
}

inline QLabel *icon(const QString &name, int size)
{
    auto *l = new QLabel;
    l->setPixmap(QIcon::fromTheme(name).pixmap(size, size));
    l->setFixedSize(size, size);
    return l;
}

inline QPushButton *button(const QString &text, const QString &iconName = {}, bool primary = false, Cli *cli = nullptr)
{
    auto *b = new QPushButton(text);
    if (!iconName.isEmpty())
        b->setIcon(QIcon::fromTheme(iconName));
    if (primary) {
        QPalette p = b->palette();
        p.setColor(QPalette::Button, p.color(QPalette::Highlight));
        p.setColor(QPalette::ButtonText, p.color(QPalette::HighlightedText));
        b->setPalette(p);
    }
    if (cli) {
        b->setEnabled(!cli->busy());
        QObject::connect(cli, &Cli::busyChanged, b, [b](bool busy) { b->setEnabled(!busy); });
    }
    return b;
}

inline QPushButton *linkButton(const QString &text)
{
    auto *b = new QPushButton(text);
    b->setCursor(Qt::PointingHandCursor);
    b->setFocusPolicy(Qt::TabFocus);
    b->setStyleSheet("QPushButton { color: palette(link); background: transparent; border: none; padding: 4px 0; } QPushButton:hover { text-decoration: underline; } QPushButton:disabled { color: palette(mid); }");
    return b;
}

// Content capped to maxw, centred horizontally (and vertically if asked), scrolling when the window is small.
inline QScrollArea *shell(QWidget *content, int maxw, bool vcenter = false)
{
    auto *host = new QWidget;
    auto *v = new QVBoxLayout(host);
    v->setContentsMargins(32, vcenter ? 24 : 28, 32, 28);
    auto *h = new QHBoxLayout;
    h->addStretch(1);
    h->addWidget(content, 100, Qt::AlignTop);
    h->addStretch(1);
    content->setMaximumWidth(maxw);
    if (vcenter)
        v->addStretch(1);
    v->addLayout(h);
    v->addStretch(1);
    auto *s = new QScrollArea;
    s->setWidgetResizable(true);
    s->setFrameShape(QFrame::NoFrame);
    s->setWidget(host);
    return s;
}

class FadeStack : public QStackedWidget
{
public:
    using QStackedWidget::QStackedWidget;
    void go(int i)
    {
        if (i == currentIndex())
            return;
        setCurrentIndex(i);
        QWidget *w = currentWidget();
        auto *fx = new QGraphicsOpacityEffect(w);
        w->setGraphicsEffect(fx);
        auto *a = new QPropertyAnimation(fx, "opacity", w);
        a->setDuration(150);
        a->setStartValue(0.0);
        a->setEndValue(1.0);
        QObject::connect(a, &QPropertyAnimation::finished, w, [w] { w->setGraphicsEffect(nullptr); });
        a->start(QAbstractAnimation::DeleteWhenStopped);
    }
};

inline void flatList(QListWidget *l)
{
    l->setFrameShape(QFrame::NoFrame);
    l->viewport()->setAutoFillBackground(false);
    l->setIconSize({22, 22});
    l->setSelectionMode(QAbstractItemView::NoSelection);
    l->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    l->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
}

// Row: icon, name (and check box if checkable) on the left; count and a small status with icon on the right.
enum { CountRole = Qt::UserRole + 1, StatusRole, StatusIconRole, StatusAccentRole };
class RowDelegate : public QStyledItemDelegate
{
public:
    using QStyledItemDelegate::QStyledItemDelegate;
    QSize sizeHint(const QStyleOptionViewItem &, const QModelIndex &) const override { return {0, 44}; }
    void paint(QPainter *p, const QStyleOptionViewItem &opt, const QModelIndex &idx) const override
    {
        QStyleOptionViewItem o = opt;
        initStyleOption(&o, idx);
        o.state &= ~QStyle::State_HasFocus;
        QApplication::style()->drawControl(QStyle::CE_ItemViewItem, &o, p, opt.widget);
        const bool sel = opt.state & QStyle::State_Selected;
        const QPalette::ColorRole dim = sel ? QPalette::HighlightedText : QPalette::PlaceholderText;
        int right = opt.rect.right() - 12;
        const QString iconName = idx.data(StatusIconRole).toString();
        if (!iconName.isEmpty())
            p->drawPixmap(right - 16, opt.rect.center().y() - 8, QIcon::fromTheme(iconName).pixmap(16, 16));
        right -= 22;
        const QString status = idx.data(StatusRole).toString();
        p->setPen(opt.palette.color(idx.data(StatusAccentRole).toBool() && !sel ? QPalette::Highlight : dim));
        QRect r(right - 170, opt.rect.top(), 170, opt.rect.height());
        p->drawText(r, Qt::AlignVCenter | Qt::AlignRight, status);
        const QString count = idx.data(CountRole).toString();
        p->setPen(opt.palette.color(dim));
        p->drawText(QRect(r.left() - 100, r.top(), 90, r.height()), Qt::AlignVCenter | Qt::AlignRight, count);
        p->setPen(QPen(opt.palette.color(QPalette::Mid), 0.5));
        p->drawLine(opt.rect.bottomLeft(), opt.rect.bottomRight());
    }
};
