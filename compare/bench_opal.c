/*
    How fast opal plays: what examples/bench.rs plays with opal_nrp, the same minute of all 18
    channels at 48 kHz, with the same generator. Build it against opal's core:

        cc -std=c11 -O2 -I <opal>/include compare/bench_opal.c <opal>/src/opal.c -o bench-opal
        bench-opal [runs]
*/

#define _POSIX_C_SOURCE 199309L

#include "opal/opal.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#define RATE 48000
#define SECONDS 60

static uint64_t state;

static uint8_t byte(void)
{
    state = state * UINT64_C(6364136223846793005) + UINT64_C(1442695040888963407);
    return (uint8_t)(state >> 32);
}

static Opal chip;
static int16_t buffer[2 * 4800];

static void setUp(void)
{
    opalWriteReg(&chip, 0x105, 0x01);
    opalWriteReg(&chip, 0x104, 0x09);
    for (uint16_t bank = 0; bank <= 0x100; bank += 0x100)
    {
        for (uint16_t offset = 0; offset < 0x16; offset++)
        {
            if ((offset & 7) >= 6)
            {
                continue;
            }
            uint8_t v = byte();
            opalWriteReg(&chip, (uint16_t)(bank + 0x20 + offset), v);
            v = (uint8_t)(byte() & 0x9F);
            opalWriteReg(&chip, (uint16_t)(bank + 0x40 + offset), v);
            v = (uint8_t)(byte() | 0x80);
            opalWriteReg(&chip, (uint16_t)(bank + 0x60 + offset), v);
            v = byte();
            opalWriteReg(&chip, (uint16_t)(bank + 0x80 + offset), v);
            v = byte();
            opalWriteReg(&chip, (uint16_t)(bank + 0xE0 + offset), v);
        }
        for (uint16_t ch = 0; ch < 9; ch++)
        {
            uint8_t v = (uint8_t)(byte() | 0x30);
            opalWriteReg(&chip, (uint16_t)(bank + 0xC0 + ch), v);
            v = byte();
            opalWriteReg(&chip, (uint16_t)(bank + 0xA0 + ch), v);
        }
    }
    opalWriteReg(&chip, 0xBD, 0xE0);
}

static int64_t play(void)
{
    state = (UINT64_C(1) * UINT64_C(0x9E3779B97F4A7C15)) ^ UINT64_C(0x0123456789ABCDEF);
    opalInit(&chip, RATE);
    setUp();
    int64_t sum = 0;
    for (int t = 0; t < SECONDS * 10; t++)
    {
        for (uint16_t ch = 0; ch < 18; ch++)
        {
            uint16_t reg = ch < 9 ? (uint16_t)(0xB0 + ch) : (uint16_t)(0x1B0 + ch - 9);
            opalWriteReg(&chip, reg, byte());
        }
        uint8_t rhythm = (uint8_t)(0xE0 | (byte() & 0x1F));
        opalWriteReg(&chip, 0xBD, rhythm);
        for (int i = 0; i < 4800; i++)
        {
            opalSample(&chip, &buffer[2 * i], &buffer[2 * i + 1]);
        }
        for (int i = 0; i < 2 * 4800; i++)
        {
            sum += buffer[i];
        }
    }
    return sum;
}

int main(int argc, char** argv)
{
    int runs = argc > 1 ? atoi(argv[1]) : 5;
    double best = 1e30;
    int64_t sum = 0;
    for (int r = 0; r < runs; r++)
    {
        struct timespec a;
        struct timespec b;
        clock_gettime(CLOCK_MONOTONIC, &a);
        sum = play();
        clock_gettime(CLOCK_MONOTONIC, &b);
        double seconds = (double)(b.tv_sec - a.tv_sec) + (double)(b.tv_nsec - a.tv_nsec) / 1e9;
        if (seconds < best)
        {
            best = seconds;
        }
        printf("%.1f ms\n", seconds * 1000.0);
    }
    printf("best %.1f ms for %d s of sound (sum %lld)\n", best * 1000.0, SECONDS, (long long)sum);
    return 0;
}
