#pragma once

#include <stdint.h>

typedef struct {
    int64_t esp_seconds;
    float velocity;
    float acceleration;
    float height;
    float time;
    float degrees;
    float brake_position;

} LogFrame;

void init_log();

void push_frame(LogFrame lf);

void finalize();
