#include <stdio.h>
#include <stdlib.h>
#include "log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"

#define BUFFER_SIZE 100
#define FLUSH_SIZE 50

int buffer_index;
LogFrame* buffer;

SemaphoreHandle_t flush_sem;

int file_idx;

int flush() {
    FILE* fptr;

    char filename[32];
    sprintf(filename, "log%d.csv", file_idx);
    fptr = fopen(filename, "a");

    if (fptr == NULL) {
        perror("unable to open file");
        return 1;
    }

    fprintf(fptr, "velocity,acceleration,height,time,degrees,brake_position\n");

    for (int i = 0; i < buffer_index; i++) {
        LogFrame frame = buffer[i];
        fprintf(fptr, "%f,%f,%f,%f,%f,%f\n",
                frame.velocity,
                frame.acceleration,
                frame.height,
                frame.time,
                frame.degrees,
                frame.brake_position);
    }

    fclose(fptr);
    file_idx++;
    return 0;
}

void push_frame(LogFrame lf) {
    buffer[buffer_index] = lf;
    buffer_index++;

    if (buffer_index >= FLUSH_SIZE) {
        xSemaphoreGive(flush_sem);
    }
}

void flush_sd_task(void* arg) {
       while (1) {
           xSemaphoreTake(flush_sem, portMAX_DELAY);
           flush();
           buffer_index = 0;
       }
}


void init_log() {
    buffer = malloc(BUFFER_SIZE * sizeof(LogFrame));
    buffer_index = 0;
    file_idx = 0;


    flush_sem = xSemaphoreCreateBinary();
    xTaskCreatePinnedToCore(
        flush_sd_task,
        "flush_log_to_sd",
        4096,
        NULL,
        5,
        NULL,
        0  //assign cpu core 0
    );
}

void finalize() {
    free(buffer);
}
