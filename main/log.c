#include <stdio.h>
#include <stdlib.h>
#include "log.h"

#define BUFFER_SIZE 100
#define FLUSH_SIZE 50

int buffer_index;
LogFrame* buffer;


void push_frame(LogFrame lf) {
    buffer[buffer_index] = lf;
    buffer_index++;
}

int flush(LogFrame* buf) {
    FILE* fptr;

    char filename[32];
    sprintf(filename, "file%d.txt", BUFFER_SIZE);
    fptr = fopen(filename, "a");

    if (fptr == NULL) {
        perror("unable to open file");
        return 1;
    }

    fprintf(fptr, "velocity,acceleration,height,time,degrees,brake_position\n");

    for (int i = 0; i < BUFFER_SIZE; i++) {
        LogFrame frame = buf[i];
        fprintf(fptr, "%f,%f,%f,%f,%d,%f\n",
                frame.velocity,
                frame.acceleration,
                frame.height,
                frame.time,
                frame.degrees,
                frame.brake_position);
    }

    fclose(fptr);
    return 0;
}


void init() {
    buffer = malloc(BUFFER_SIZE * sizeof(LogFrame));
    buffer_index = 0;
}

void finalize() {
    free(buffer);
}
