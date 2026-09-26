#include "shim.h"

#include <QAccessible>
#include <QApplication>
#include <QHash>
#include <QImage>
#include <QKeyEvent>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QSGRendererInterface>
#include <QPointer>
#include <cstring>

static mq_event_fn g_callback = nullptr;
static QQmlEngine* g_engine = nullptr;
static QHash<QString, QQmlComponent*> g_components;

void Receiver::fire() {
    if (g_callback) g_callback(node, event);
}

static char* dup(const QString& s) {
    QByteArray bytes = s.toUtf8();
    char* out = static_cast<char*>(malloc(bytes.size() + 1));
    memcpy(out, bytes.constData(), bytes.size() + 1);
    return out;
}

extern "C" {

void mq_init(mq_event_fn callback) {
    g_callback = callback;
    static int argc = 1;
    static char name[] = "kirigami-spike";
    static char* argv[] = {name, nullptr};
    // Kirigami apps use QApplication: the desktop style draws with QStyle.
    new QApplication(argc, argv);
    if (qEnvironmentVariableIsEmpty("QT_QUICK_CONTROLS_STYLE")) QQuickStyle::setStyle("org.kde.desktop");
    g_engine = new QQmlEngine();
}

void mq_process_events(void) { QCoreApplication::processEvents(QEventLoop::AllEvents); }

// One component per distinct QML text, compiled once.
QObject* mq_load(const char* qml, char** error) {
    QString text = QString::fromUtf8(qml);
    QQmlComponent* component = g_components.value(text);
    if (!component) {
        component = new QQmlComponent(g_engine);
        component->setData(text.toUtf8(), QUrl());
        g_components.insert(text, component);
    }
    QObject* object = component->create();
    if (!object) {
        if (error) *error = dup(component->errorString());
        return nullptr;
    }
    QQmlEngine::setObjectOwnership(object, QQmlEngine::CppOwnership);
    return object;
}

void mq_destroy(QObject* object) { delete object; }

QObject* mq_find_child(QObject* object, const char* name) {
    return object->findChild<QObject*>(QString::fromUtf8(name));
}

void mq_set_parent_item(QObject* item, QObject* parent, int32_t index) {
    auto* child = qobject_cast<QQuickItem*>(item);
    auto* host = qobject_cast<QQuickItem*>(parent);
    if (!child) return;
    child->setParentItem(host);
    if (!host) return;
    // Children are in stacking order; the newest goes last.
    QList<QQuickItem*> children = host->childItems();
    if (index >= 0 && index < children.size() - 1) child->stackBefore(children[index]);
}

int32_t mq_child_count(QObject* item) {
    auto* quick = qobject_cast<QQuickItem*>(item);
    return quick ? quick->childItems().size() : 0;
}

QObject* mq_child_at(QObject* item, int32_t index) {
    return qobject_cast<QQuickItem*>(item)->childItems().value(index);
}

void mq_set_geometry(QObject* item, double x, double y, double w, double h) {
    auto* quick = qobject_cast<QQuickItem*>(item);
    quick->setPosition(QPointF(x, y));
    quick->setSize(QSizeF(w, h));
}

void mq_ensure_polished(QObject* item) {
    if (auto* quick = qobject_cast<QQuickItem*>(item)) quick->ensurePolished();
}

void mq_set_str(QObject* o, const char* name, const char* value) { o->setProperty(name, QString::fromUtf8(value)); }
char* mq_get_str(QObject* o, const char* name) { return dup(o->property(name).toString()); }
void mq_free(char* s) { free(s); }
void mq_set_bool(QObject* o, const char* name, int32_t value) { o->setProperty(name, value != 0); }
int32_t mq_get_bool(QObject* o, const char* name) { return o->property(name).toBool(); }
void mq_set_real(QObject* o, const char* name, double value) { o->setProperty(name, value); }
double mq_get_real(QObject* o, const char* name) { return o->property(name).toDouble(); }
QObject* mq_get_object(QObject* o, const char* name) { return o->property(name).value<QObject*>(); }
void mq_set_node(QObject* o, uint64_t node) { o->setProperty("_mitsuamiNode", QVariant::fromValue<quint64>(node)); }

// The nearest item up the tree that stands for a node.
uint64_t mq_node_of(QObject* o) {
    for (auto* item = qobject_cast<QQuickItem*>(o); item; item = item->parentItem()) {
        QVariant node = item->property("_mitsuamiNode");
        if (node.isValid()) return node.toULongLong();
    }
    return 0;
}

int32_t mq_connect(QObject* object, const char* signal, uint64_t node, int32_t event) {
    // String-based connect: QML controls' signals live on private types.
    QByteArray sig = QByteArray("2") + signal;
    auto* receiver = new Receiver(object, node, event);
    return QObject::connect(object, sig.constData(), receiver, SLOT(fire())) ? 1 : 0;
}

QObject* mq_focus_item(QObject* window) {
    auto* quick = qobject_cast<QQuickWindow*>(window);
    return quick ? quick->activeFocusItem() : nullptr;
}

void mq_force_focus(QObject* item) { qobject_cast<QQuickItem*>(item)->forceActiveFocus(Qt::OtherFocusReason); }

// What a screen reader does: the item's accessible action by name
// ("Press", "Toggle", "SetFocus", …).
int32_t mq_a11y_action(QObject* item, const char* action) {
    QAccessibleInterface* iface = QAccessible::queryAccessibleInterface(item);
    if (!iface) return -1;
    QAccessibleActionInterface* actions = iface->actionInterface();
    if (!actions) return -2;
    QString name = QString::fromUtf8(action);
    if (!actions->actionNames().contains(name)) return -3;
    actions->doAction(name);
    return 0;
}

char* mq_a11y_name(QObject* item) {
    QAccessibleInterface* iface = QAccessible::queryAccessibleInterface(item);
    if (!iface) return dup(QStringLiteral("<no interface>"));
    return dup(QStringLiteral("%1 (role %2, actions %3)")
                   .arg(iface->text(QAccessible::Name))
                   .arg(int(iface->role()))
                   .arg(iface->actionInterface() ? iface->actionInterface()->actionNames().join(",") : "-"));
}

void mq_key(QObject* window, int32_t key, const char* text) {
    auto* quick = qobject_cast<QQuickWindow*>(window);
    QString t = QString::fromUtf8(text);
    QKeyEvent press(QEvent::KeyPress, key, Qt::NoModifier, t);
    QKeyEvent release(QEvent::KeyRelease, key, Qt::NoModifier, t);
    QCoreApplication::sendEvent(quick, &press);
    QCoreApplication::sendEvent(quick, &release);
}

int32_t mq_grab(QObject* window, uint8_t** rgba, int32_t* w, int32_t* h, double* scale) {
    auto* quick = qobject_cast<QQuickWindow*>(window);
    if (!quick) return 0;
    QImage image = quick->grabWindow().convertToFormat(QImage::Format_RGBA8888);
    if (image.isNull()) return 0;
    *w = image.width();
    *h = image.height();
    *scale = image.devicePixelRatio();
    *rgba = static_cast<uint8_t*>(malloc(size_t(*w) * *h * 4));
    for (int y = 0; y < *h; y++) memcpy(*rgba + size_t(y) * *w * 4, image.constScanLine(y), size_t(*w) * 4);
    return 1;
}

void mq_free_pixels(uint8_t* rgba) { free(rgba); }

char* mq_style_name(void) { return dup(QQuickStyle::name()); }
}

// Tab and Shift+Tab follow a window-wide order of our own, wrapping
// around; Qt Quick's own chain follows item order within each parent.
class TabOrder : public QObject {
public:
    TabOrder(QQuickWindow* window) : QObject(window), window(window) {}
    QList<QPointer<QQuickItem>> order;
protected:
    bool eventFilter(QObject*, QEvent* event) override {
        if (event->type() != QEvent::KeyPress) return false;
        auto* key = static_cast<QKeyEvent*>(event);
        bool forward = key->key() == Qt::Key_Tab && !(key->modifiers() & Qt::ShiftModifier);
        bool backward = key->key() == Qt::Key_Backtab || (key->key() == Qt::Key_Tab && (key->modifiers() & Qt::ShiftModifier));
        if ((!forward && !backward) || order.isEmpty()) return false;
        int n = order.size(), at = -1;
        for (QQuickItem* f = window->activeFocusItem(); f && at < 0; f = f->parentItem())
            for (int i = 0; i < n; i++) if (order[i] == f) { at = i; break; }
        for (int s = 1; s <= n; s++) {
            int i = at < 0 ? (forward ? s - 1 : n - s) : (forward ? (at + s) % n : (at + n - s) % n);
            QQuickItem* item = order[i];
            if (item && item->isEnabled() && item->isVisible()) {
                item->forceActiveFocus(forward ? Qt::TabFocusReason : Qt::BacktabFocusReason);
                return true;
            }
        }
        return true;
    }
private:
    QQuickWindow* window;
};

extern "C" void mq_set_tab_order(QObject* window, QObject** items, int32_t count) {
    auto* quick = qobject_cast<QQuickWindow*>(window);
    auto* filter = static_cast<TabOrder*>(quick->property("_mitsuamiTabOrder").value<QObject*>());
    if (!filter) {
        filter = new TabOrder(quick);
        quick->installEventFilter(filter);
        quick->setProperty("_mitsuamiTabOrder", QVariant::fromValue<QObject*>(filter));
    }
    filter->order.clear();
    for (int i = 0; i < count; i++) filter->order.append(qobject_cast<QQuickItem*>(items[i]));
}

extern "C" char* mq_graphics_api(QObject* window) {
    auto* quick = qobject_cast<QQuickWindow*>(window);
    switch (quick->rendererInterface()->graphicsApi()) {
    case QSGRendererInterface::Software: return dup("software");
    case QSGRendererInterface::OpenGL: return dup("opengl");
    case QSGRendererInterface::Vulkan: return dup("vulkan");
    default: return dup("other");
    }
}
