// A thin C API over Qt Quick, so Rust can drive QML items imperatively.
// Everything is a QObject*; properties go through QObject::setProperty, so
// the shim knows nothing about particular controls.

#pragma once

#include <QObject>
#include <cstdint>

typedef void (*mq_event_fn)(uint64_t node, int32_t event);

// Forwards one signal of one object to Rust as (node, event).
class Receiver : public QObject {
    Q_OBJECT
public:
    Receiver(QObject* parent, uint64_t node, int32_t event) : QObject(parent), node(node), event(event) {}
public slots:
    void fire();
private:
    uint64_t node;
    int32_t event;
};

extern "C" {
void mq_init(mq_event_fn callback);
void mq_process_events(void);
QObject* mq_load(const char* qml, char** error);
void mq_destroy(QObject* object);
QObject* mq_find_child(QObject* object, const char* name);
void mq_set_parent_item(QObject* item, QObject* parent, int32_t index);
int32_t mq_child_count(QObject* item);
QObject* mq_child_at(QObject* item, int32_t index);
void mq_set_geometry(QObject* item, double x, double y, double w, double h);
void mq_ensure_polished(QObject* item);

void mq_set_str(QObject* object, const char* name, const char* value);
char* mq_get_str(QObject* object, const char* name);
void mq_free(char* s);
void mq_set_bool(QObject* object, const char* name, int32_t value);
int32_t mq_get_bool(QObject* object, const char* name);
void mq_set_real(QObject* object, const char* name, double value);
double mq_get_real(QObject* object, const char* name);
QObject* mq_get_object(QObject* object, const char* name);
void mq_set_node(QObject* object, uint64_t node);
uint64_t mq_node_of(QObject* object);

int32_t mq_connect(QObject* object, const char* signal, uint64_t node, int32_t event);
QObject* mq_focus_item(QObject* window);
void mq_force_focus(QObject* item);
int32_t mq_a11y_action(QObject* item, const char* action);
char* mq_a11y_name(QObject* item);
void mq_key(QObject* window, int32_t key, const char* text);
int32_t mq_grab(QObject* window, uint8_t** rgba, int32_t* w, int32_t* h, double* scale);
void mq_free_pixels(uint8_t* rgba);
char* mq_style_name(void);
void mq_set_tab_order(QObject* window, QObject** items, int32_t count);
char* mq_graphics_api(QObject* window);
}
