#include <stdio.h>
#include <stdlib.h>

typedef struct { unsigned char r, g, b; } Rgb;

int main(void) {
    long n = 4000000;
    Rgb *pixels = malloc(n * sizeof(Rgb));
    for (long i = 0; i < n; i++) { pixels[i].r = 1; pixels[i].g = 2; pixels[i].b = 3; }
    long total = 0;
    for (int round = 0; round < 8; round++)
        for (long i = 0; i < n; i++)
            total += pixels[i].r + pixels[i].g + pixels[i].b;
    printf("%ld\n", total);
    free(pixels);
    return 0;
}
