#include <stdint.h>

typedef struct {
    int64_t esp_seconds;
    double velocity;
    double acceleration;
    double height;
    double time;
    int degrees;
    double brake_position;

} LogFrame;

void init();

void push_frame(LogFrame lf);

void finalize();
