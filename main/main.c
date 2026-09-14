#include "esp_timer.h"
#include "esp_log.h"
#include "log.h"

#define LOG_HZ 100

//define callback
void log_periodic() {
    LogFrame log;
    log.esp_seconds = esp_timer_get_time();
    push_frame(log);
}

void app_main(void) {
    //configure timer
    const esp_timer_create_args_t timer_args = {
        .callback = &log_periodic,
        .name = "log_timer"
    };

    esp_timer_handle_t timer_handle;

    esp_timer_create(&timer_args, &timer_handle);

    esp_timer_start_periodic(timer_handle, 1000000/LOG_HZ);
}
