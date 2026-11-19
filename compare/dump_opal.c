/*
    The C side of the comparison with opal_nrp: runs opal, and the format readers of its
    example player, over synthetic data and prints a running hash of their output, as
    examples/dump.rs does with opal_nrp. compare/compare.sh builds it and compares the two.

      dump_opal chip <seed> <count>             register programs, each on a chip of its own
      dump_opal music <seed> <count>            made-up DRO, IMF and HSC files, played
      dump_opal tracks <seed> <frames> <file>...  the files given, played

    The generator, the order of its draws and the hashes are those of tests/made_up/mod.rs.
    (Arguments are drawn in separate statements, since C leaves their order unspecified.)
*/

#include "opal/opal.h"

#include "format_dro.h"
#include "format_hsc.h"
#include "format_imf.h"
#include "player_format.h"

#include <inttypes.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ---- The generator and the hash ---- */

static uint64_t state;

static void seedRandom(uint64_t seed)
{
    state = (seed * UINT64_C(0x9E3779B97F4A7C15)) ^ UINT64_C(0x0123456789ABCDEF);
}

static uint32_t next(void)
{
    state = state * UINT64_C(6364136223846793005) + UINT64_C(1442695040888963407);
    return (uint32_t)(state >> 32);
}

static uint32_t below(uint32_t n)
{
    return next() % n;
}

static uint8_t byte(void)
{
    return (uint8_t)next();
}

typedef struct
{
    uint64_t value;
} Hash;

static Hash hashNew(void)
{
    Hash h = {UINT64_C(14695981039346656037)};
    return h;
}

static void hashAdd(Hash* h, const void* data, size_t len)
{
    const uint8_t* p = (const uint8_t*)data;
    for (size_t i = 0; i < len; i++)
    {
        h->value ^= p[i];
        h->value *= UINT64_C(1099511628211);
    }
}

static void hashU32(Hash* h, uint32_t v)
{
    uint8_t b[4] = {(uint8_t)v, (uint8_t)(v >> 8), (uint8_t)(v >> 16), (uint8_t)(v >> 24)};
    hashAdd(h, b, 4);
}

static void hashSample(Hash* h, int16_t left, int16_t right)
{
    uint16_t l = (uint16_t)left;
    uint16_t r = (uint16_t)right;
    uint8_t b[4] = {(uint8_t)l, (uint8_t)(l >> 8), (uint8_t)r, (uint8_t)(r >> 8)};
    hashAdd(h, b, 4);
}

static void hashByte(Hash* h, uint8_t v)
{
    hashAdd(h, &v, 1);
}

/* ---- Registers and values ---- */

static const uint32_t RATES[12] = {49716, 44100, 48000, 22050, 11025, 8000, 96000, 192000, 2000, 4001, 1000000, 2147483647u};

static uint32_t chipRate(void)
{
    uint32_t k = below(16);
    if (k == 0)
    {
        return 0;
    }
    if (k <= 12)
    {
        return RATES[k - 1];
    }
    return 2000 + below(198001);
}

static uint16_t makeRegister(void)
{
    uint16_t bank = below(2) == 0 ? 0 : 0x100;
    uint32_t kind = below(10);
    if (kind <= 3)
    {
        static const uint16_t groups[5] = {0x20, 0x40, 0x60, 0x80, 0xE0};
        uint16_t group = groups[below(5)];
        uint16_t offset = (uint16_t)below(0x16);
        return (uint16_t)(bank + group + offset);
    }
    if (kind <= 6)
    {
        static const uint16_t groups[3] = {0xA0, 0xB0, 0xC0};
        uint16_t group = groups[below(3)];
        uint16_t offset = (uint16_t)below(0x10);
        return (uint16_t)(bank + group + offset);
    }
    if (kind <= 8)
    {
        static const uint16_t control[9] = {0x01, 0x02, 0x03, 0x04, 0x08, 0xBD, 0x104, 0x105, 0x105};
        uint16_t reg = control[below(9)];
        uint16_t extra = below(8) == 0 ? bank : 0;
        return (uint16_t)(reg + extra);
    }
    if (below(4) == 0)
    {
        return (uint16_t)next();
    }
    return (uint16_t)below(0x200);
}

static uint8_t makeValue(uint16_t reg)
{
    uint8_t value = byte();
    if (below(2) == 0)
    {
        return value;
    }
    switch (reg & 0xF0)
    {
        case 0x40:
        case 0x50:
            return (uint8_t)(value & 0xDF);
        case 0x60:
        case 0x70:
            return (uint8_t)(value | 0x80);
        case 0xB0:
            return (uint8_t)(value | 0x20);
        default:
            return value;
    }
}

static void voice(Opal* chip, bool buffered)
{
    uint16_t ch = (uint16_t)below(9);
    uint16_t bank = below(3) == 0 ? 0x100 : 0;
    uint16_t first = (uint16_t)((ch / 3) * 8 + ch % 3);
    uint16_t regs[13];
    uint8_t vals[13];
    int n = 0;
    uint16_t offsets[2] = {first, (uint16_t)(first + 3)};
    for (int k = 0; k < 2; k++)
    {
        uint16_t o = offsets[k];
        regs[n] = (uint16_t)(bank + 0x20 + o);
        vals[n++] = byte();
        regs[n] = (uint16_t)(bank + 0x40 + o);
        vals[n++] = (uint8_t)(byte() & 0x9F);
        regs[n] = (uint16_t)(bank + 0x60 + o);
        vals[n++] = (uint8_t)(byte() | 0x80);
        regs[n] = (uint16_t)(bank + 0x80 + o);
        vals[n++] = byte();
        regs[n] = (uint16_t)(bank + 0xE0 + o);
        vals[n++] = byte();
    }
    regs[n] = (uint16_t)(bank + 0xC0 + ch);
    vals[n++] = byte();
    regs[n] = (uint16_t)(bank + 0xA0 + ch);
    vals[n++] = byte();
    regs[n] = (uint16_t)(bank + 0xB0 + ch);
    vals[n++] = (uint8_t)(byte() | 0x20);
    for (int i = 0; i < n; i++)
    {
        if (buffered)
        {
            opalWriteRegBuffered(chip, regs[i], vals[i]);
        }
        else
        {
            opalWriteReg(chip, regs[i], vals[i]);
        }
    }
}

static uint32_t render(Opal* chip, uint32_t count, Hash* hash)
{
    uint32_t audible = 0;
    for (uint32_t i = 0; i < count; i++)
    {
        int16_t left;
        int16_t right;
        opalSample(chip, &left, &right);
        if (left != 0 || right != 0)
        {
            audible++;
        }
        hashSample(hash, left, right);
    }
    hashByte(hash, opalReadStatus(chip));
    return audible;
}

/* ---- The chip dump ---- */

static Opal chipState;
static Opal copyState;

static void dumpChip(uint32_t count)
{
    uint64_t samples = 0;
    uint64_t audible = 0;
    for (uint32_t program = 0; program < count; program++)
    {
        Hash hash = hashNew();
        Opal* chip = &chipState;
        opalInit(chip, (int)chipRate());
        uint32_t actions = 20 + below(200);
        for (uint32_t a = 0; a < actions; a++)
        {
            uint32_t kind = below(100);
            if (kind <= 29)
            {
                uint16_t reg = makeRegister();
                uint8_t value = makeValue(reg);
                opalWriteReg(chip, reg, value);
            }
            else if (kind <= 44)
            {
                uint16_t reg = makeRegister();
                uint8_t value = makeValue(reg);
                opalWriteRegBuffered(chip, reg, value);
            }
            else if (kind <= 52)
            {
                bool buffered = below(2) == 0;
                voice(chip, buffered);
            }
            else if (kind <= 54)
            {
                opalFlushWriteBuf(chip);
            }
            else if (kind <= 58)
            {
                uint16_t channel;
                uint32_t which = below(4);
                if (which == 0)
                {
                    channel = (uint16_t)below(20);
                }
                else if (which == 1)
                {
                    channel = (uint16_t)(0x100 + below(11));
                }
                else
                {
                    channel = (uint16_t)next();
                }
                uint8_t pan = byte();
                opalPan(chip, channel, pan);
            }
            else if (kind == 59)
            {
                opalSetSampleRate(chip, (int)chipRate());
            }
            else if (kind <= 61)
            {
                copyState = *chip;
                uint32_t n = 1 + below(200);
                audible += render(&copyState, n, &hash);
                samples += n;
            }
            else
            {
                uint32_t n = 1 + below(600);
                audible += render(chip, n, &hash);
                samples += n;
            }
        }
        printf("program %u: %016" PRIx64 "\n", program, hash.value);
    }
    printf("chip programs: %u, samples: %" PRIu64 ", audible: %" PRIu64 "\n", count, samples, audible);
}

/* ---- Songs ---- */

typedef struct
{
    uint8_t* data;
    size_t len;
    size_t cap;
} Bytes;

static void push(Bytes* b, uint8_t v)
{
    if (b->len == b->cap)
    {
        b->cap = b->cap ? 2 * b->cap : 256;
        b->data = (uint8_t*)realloc(b->data, b->cap);
    }
    b->data[b->len++] = v;
}

static void pushU16(Bytes* b, uint16_t v)
{
    push(b, (uint8_t)v);
    push(b, (uint8_t)(v >> 8));
}

static void pushU32(Bytes* b, uint32_t v)
{
    pushU16(b, (uint16_t)v);
    pushU16(b, (uint16_t)(v >> 16));
}

static void append(Bytes* b, const Bytes* other)
{
    for (size_t i = 0; i < other->len; i++)
    {
        push(b, other->data[i]);
    }
}

static void songWrite(uint8_t* reg, uint8_t* value)
{
    uint8_t r = (uint8_t)makeRegister();
    *reg = r;
    *value = makeValue(r);
}

static Bytes makeDro(void)
{
    Bytes body = {0};
    if (below(3) != 0)
    {
        uint8_t reg = (uint8_t)below(8);
        while (reg < 0xF6)
        {
            push(&body, reg);
            push(&body, byte());
            uint32_t stepBy = 1 + below(24);
            reg = (uint8_t)(reg + stepBy > 255 ? 255 : reg + stepBy);
        }
        if (below(2) == 0)
        {
            push(&body, 0x03);
            reg = (uint8_t)below(8);
            while (reg < 0xF6)
            {
                push(&body, reg);
                push(&body, byte());
                uint32_t stepBy = 1 + below(40);
                reg = (uint8_t)(reg + stepBy > 255 ? 255 : reg + stepBy);
            }
        }
    }
    uint32_t commands = below(600);
    for (uint32_t c = 0; c < commands; c++)
    {
        uint32_t kind = below(20);
        if (kind <= 2)
        {
            push(&body, 0x00);
            push(&body, byte());
        }
        else if (kind == 3)
        {
            push(&body, 0x01);
            push(&body, byte());
            push(&body, (uint8_t)below(4));
        }
        else if (kind == 4)
        {
            push(&body, 0x02);
        }
        else if (kind == 5)
        {
            push(&body, 0x03);
        }
        else if (kind <= 7)
        {
            uint8_t r;
            uint8_t v;
            songWrite(&r, &v);
            push(&body, 0x04);
            push(&body, r);
            push(&body, v);
        }
        else
        {
            uint8_t r;
            uint8_t v;
            songWrite(&r, &v);
            push(&body, r);
            push(&body, v);
        }
    }
    if (below(10) == 0)
    {
        body.len = below((uint32_t)body.len + 1);
    }

    bool early = below(4) == 0;
    Bytes data = {0};
    const char* sig = "DBRAWOPL";
    for (int i = 0; i < 8; i++)
    {
        push(&data, (uint8_t)sig[i]);
    }
    uint32_t version = 0x00010000;
    if (below(12) == 0)
    {
        version = 0x00020000 | below(3);
    }
    pushU32(&data, version);
    pushU32(&data, next());
    uint32_t length;
    uint32_t which = below(5);
    if (which == 0)
    {
        length = 0;
    }
    else if (which == 1)
    {
        length = below((uint32_t)body.len + 8);
    }
    else
    {
        length = (uint32_t)body.len;
    }
    pushU32(&data, length);
    uint8_t hardware;
    which = below(5);
    if (which == 0)
    {
        hardware = 0;
    }
    else if (which <= 2)
    {
        hardware = 1;
    }
    else if (which == 3)
    {
        hardware = 2;
    }
    else
    {
        hardware = byte();
    }
    push(&data, hardware);
    if (!early)
    {
        uint8_t padding = below(6) == 0 ? byte() : 0;
        push(&data, padding);
        push(&data, 0);
        push(&data, 0);
    }
    append(&data, &body);
    free(body.data);
    return data;
}

static Bytes makeImf(void)
{
    Bytes stream = {0};
    uint32_t commands = below(500);
    for (uint32_t c = 0; c < commands; c++)
    {
        uint8_t r;
        uint8_t v;
        songWrite(&r, &v);
        uint16_t delay;
        uint32_t which = below(4);
        if (which <= 1)
        {
            delay = 0;
        }
        else if (which == 2)
        {
            delay = (uint16_t)below(40);
        }
        else
        {
            delay = (uint16_t)((uint16_t)next() % 400);
        }
        push(&stream, r);
        push(&stream, v);
        pushU16(&stream, delay);
    }
    if (below(8) == 0)
    {
        stream.len = below((uint32_t)stream.len + 1);
    }
    Bytes data = {0};
    if (below(3) == 0)
    {
        pushU32(&data, 0);
    }
    else
    {
        uint16_t length = (uint16_t)stream.len;
        if (below(4) == 0)
        {
            length = (uint16_t)next();
        }
        pushU16(&data, length);
    }
    append(&data, &stream);
    free(stream.data);
    return data;
}

static Bytes makeHsc(void)
{
    Bytes data = {0};
    for (int p = 0; p < 128; p++)
    {
        uint8_t record[12];
        for (int i = 0; i < 12; i++)
        {
            record[i] = byte();
        }
        record[2] &= 0xDF;
        record[4] |= 0x80;
        record[5] |= 0x80;
        for (int i = 0; i < 12; i++)
        {
            push(&data, record[i]);
        }
    }
    uint32_t patterns = 1 + below(12);
    uint32_t orders = 1 + below(51);
    for (uint32_t k = 0; k < 51; k++)
    {
        uint8_t order = 0xFF;
        if (k < orders)
        {
            uint32_t which = below(12);
            if (which == 0)
            {
                order = (uint8_t)(0x80 + below(0x40));
            }
            else if (which == 1)
            {
                order = 0xFF;
            }
            else
            {
                order = (uint8_t)below(patterns);
            }
        }
        push(&data, order);
    }
    for (uint32_t c = 0; c < patterns * 64 * 9; c++)
    {
        uint8_t note;
        uint32_t which = below(10);
        if (which <= 3)
        {
            note = 0;
        }
        else if (which == 4)
        {
            note = 0x7F;
        }
        else if (which == 5)
        {
            note = 0x80;
        }
        else
        {
            note = (uint8_t)(1 + below(96));
        }
        uint8_t effect;
        which = below(6);
        if (which <= 2)
        {
            effect = 0;
        }
        else if (which == 3)
        {
            effect = 0x01;
        }
        else
        {
            effect = byte();
        }
        push(&data, note);
        push(&data, effect);
    }
    uint32_t which = below(10);
    if (which == 0)
    {
        push(&data, byte());
    }
    else if (which == 1)
    {
        data.len = below((uint32_t)data.len);
    }
    return data;
}

static Bytes makeSong(const char** name)
{
    static const char* const names[8] = {"song.dro", "song.imf", "SONG.IMF", "song.wlf", "song.hsc", "SONG.HSC", "song", ".imf"};
    Bytes data = {0};
    uint32_t kind = below(10);
    if (kind <= 2)
    {
        *name = "capture.dro";
        data = makeDro();
    }
    else if (kind <= 5)
    {
        *name = below(3) == 0 ? "music.wlf" : "music.imf";
        data = makeImf();
    }
    else if (kind <= 8)
    {
        *name = "module.hsc";
        data = makeHsc();
    }
    else
    {
        uint32_t len = below(2000);
        for (uint32_t i = 0; i < len; i++)
        {
            push(&data, byte());
        }
        *name = names[below(8)];
    }
    if (data.len > 0 && below(6) == 0)
    {
        uint32_t changes = 1 + below(8);
        for (uint32_t c = 0; c < changes; c++)
        {
            uint32_t at = below((uint32_t)data.len);
            data.data[at] = byte();
        }
    }
    return data;
}

/* ---- The player, as examples/player/src/player.c plays to its device ---- */

#define TAIL_MS 250

static const MusicFormat* const formats[] = {&droFormat, &hscFormat, &imfFormat};

typedef struct
{
    Opal opl;
    MusicSource* source;
    uint64_t wait;
    uint64_t lag;
    uint64_t tail;
    bool ended;
    bool signaled;
} Player;

static Player player;

/* player.c's dataCallback, without the device. */
static void fill(Player* p, int16_t* out, uint32_t frameCount)
{
    uint32_t stalledSteps = 0;
    for (uint32_t i = 0; i < frameCount; ++i)
    {
        int16_t left = 0;
        int16_t right = 0;

        while (p->wait == 0 && !p->ended && stalledSteps < STEP_STALL_LIMIT)
        {
            bool done = false;
            uint64_t n = p->source->step(p->source, &p->opl, &done);
            if (done && !p->source->loop)
            {
                p->ended = true;
                opalFlushWriteBuf(&p->opl);
            }
            uint64_t absorbed = (n < p->lag) ? n : p->lag;
            p->lag -= absorbed;
            p->wait += n - absorbed;
            if (p->wait == 0)
            {
                stalledSteps += 1;
            }
        }

        if (p->wait > 0)
        {
            opalSample(&p->opl, &left, &right);
            --p->wait;
        }
        else if (!p->ended)
        {
            opalSample(&p->opl, &left, &right);
            ++p->lag;
        }
        else if (p->tail > 0)
        {
            opalSample(&p->opl, &left, &right);
            --p->tail;
        }
        else if (!p->signaled)
        {
            p->signaled = true;
        }

        out[2 * i + 0] = left;
        out[2 * i + 1] = right;
    }
}

static int16_t buffer[2 * 4096];

static int lastKind;

static uint64_t play(const char* name, Bytes data, uint32_t sampleRate, uint32_t imfRateOverride, uint32_t frames, uint64_t* played)
{
    Hash hash = hashNew();
    Player* p = &player;
    memset(p, 0, sizeof *p);
    opalInit(&p->opl, (int)sampleRate);
    imfSetRate(imfRateOverride);

    const MusicFormat* format = NULL;
    for (size_t i = 0; i < sizeof formats / sizeof formats[0]; ++i)
    {
        p->source = formats[i]->load(name, data.data, data.len, sampleRate, &p->opl);
        if (p->source != NULL)
        {
            format = formats[i];
            break;
        }
    }
    *played = 0;
    lastKind = format == &droFormat ? 0 : format == &hscFormat ? 1 : format == &imfFormat ? 2 : 3;
    if (p->source == NULL)
    {
        hashAdd(&hash, "unknown", 7);
        free(data.data);
        return hash.value;
    }
    hashAdd(&hash, format->name, 3);
    hashU32(&hash, format == &imfFormat ? imfRate() : 0);
    hashByte(&hash, p->source->loop ? 1 : 0);

    p->tail = framesForMs(sampleRate, TAIL_MS);
    bool primedDone = false;
    p->wait = p->source->step(p->source, &p->opl, &primedDone);
    p->ended = primedDone && !p->source->loop;
    if (p->ended)
    {
        opalFlushWriteBuf(&p->opl);
    }

    uint32_t done = 0;
    while (done < frames && !p->signaled)
    {
        uint32_t chunk = 1 + below(4096);
        if (chunk > frames - done)
        {
            chunk = frames - done;
        }
        fill(p, buffer, chunk);
        for (uint32_t i = 0; i < chunk; i++)
        {
            hashSample(&hash, buffer[2 * i], buffer[2 * i + 1]);
        }
        done += chunk;
    }
    hashByte(&hash, p->signaled ? 1 : 0);
    p->source->free(p->source);
    free(data.data);
    *played = done;
    return hash.value;
}

static void dumpMusic(uint32_t count)
{
    static const uint32_t rates[6] = {44100, 48000, 22050, 11025, 49716, 96000};
    uint64_t total = 0;
    uint32_t kinds[4] = {0, 0, 0, 0};
    for (uint32_t k = 0; k < count; k++)
    {
        const char* name = NULL;
        Bytes data = makeSong(&name);
        uint32_t sampleRate = rates[below(6)];
        uint32_t imfRateOverride = 0;
        if (below(4) == 0)
        {
            imfRateOverride = below(1500);
        }
        uint32_t frames = below(4 * sampleRate);
        uint64_t played = 0;
        uint64_t h = play(name, data, sampleRate, imfRateOverride, frames, &played);
        total += played;
        kinds[lastKind]++;
        printf("song %u: %016" PRIx64 "\n", k, h);
    }
    printf("music songs: %u (DRO %u, HSC %u, IMF %u, none %u), frames: %" PRIu64 "\n", count, kinds[0], kinds[1], kinds[2], kinds[3], total);
}

static Bytes readFile(const char* path)
{
    Bytes data = {0};
    FILE* f = fopen(path, "rb");
    if (f == NULL)
    {
        fprintf(stderr, "cannot read %s\n", path);
        exit(2);
    }
    int c;
    while ((c = fgetc(f)) != EOF)
    {
        push(&data, (uint8_t)c);
    }
    fclose(f);
    return data;
}

static void dumpTracks(uint32_t frames, int count, char** paths)
{
    static const uint32_t rates[3] = {44100, 48000, 22050};
    for (int k = 0; k < count; k++)
    {
        const char* path = paths[k];
        const char* name = strrchr(path, '/');
        name = name ? name + 1 : path;
        Bytes data = readFile(path);
        uint32_t sampleRate = rates[below(3)];
        uint64_t played = 0;
        uint64_t h = play(name, data, sampleRate, 0, frames, &played);
        printf("%s: %016" PRIx64 "\n", name, h);
    }
    printf("tracks: %d, frames each: %u\n", count, frames);
}

int main(int argc, char** argv)
{
    if (argc < 4)
    {
        fprintf(stderr, "usage: dump_opal <chip|music|tracks> <seed> <count> [files...]\n");
        return 2;
    }
    uint64_t seed = strtoull(argv[2], NULL, 10);
    uint32_t count = (uint32_t)strtoul(argv[3], NULL, 10);
    seedRandom(seed);
    if (strcmp(argv[1], "chip") == 0)
    {
        dumpChip(count);
    }
    else if (strcmp(argv[1], "music") == 0)
    {
        dumpMusic(count);
    }
    else if (strcmp(argv[1], "tracks") == 0)
    {
        dumpTracks(count, argc - 4, argv + 4);
    }
    else
    {
        fprintf(stderr, "unknown dump %s\n", argv[1]);
        return 2;
    }
    return 0;
}
