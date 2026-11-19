//! The chip's tables: its ROMs, and opal's tables of the operators, the vibrato and the
//! routings.

/// Each frequency multiple's factor, doubled (the multiple 0 is a half).
pub(crate) const MUL_TIMES_2: [u32; 16] =
    [1, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 20, 24, 24, 30, 30];

/// The key scale level ROM, by the top four bits of the frequency number.
pub(crate) const KSL_ROM: [u8; 16] = [
    0, 32, 40, 45, 48, 51, 53, 55, 56, 58, 59, 60, 61, 62, 63, 64,
];

/// The decapped OPL2/OPL3 exponent ROM: raw ten-bit values in ascending order.
#[rustfmt::skip]
pub(crate) const EXPONENT_ROM: [u16; 256] = [
    0x000, 0x003, 0x006, 0x008, 0x00B, 0x00E, 0x011, 0x014,
    0x016, 0x019, 0x01C, 0x01F, 0x022, 0x025, 0x028, 0x02A,
    0x02D, 0x030, 0x033, 0x036, 0x039, 0x03C, 0x03F, 0x042,
    0x045, 0x048, 0x04B, 0x04E, 0x051, 0x054, 0x057, 0x05A,
    0x05D, 0x060, 0x063, 0x066, 0x069, 0x06C, 0x06F, 0x072,
    0x075, 0x078, 0x07B, 0x07E, 0x082, 0x085, 0x088, 0x08B,
    0x08E, 0x091, 0x094, 0x098, 0x09B, 0x09E, 0x0A1, 0x0A4,
    0x0A8, 0x0AB, 0x0AE, 0x0B1, 0x0B5, 0x0B8, 0x0BB, 0x0BE,
    0x0C2, 0x0C5, 0x0C8, 0x0CC, 0x0CF, 0x0D2, 0x0D6, 0x0D9,
    0x0DC, 0x0E0, 0x0E3, 0x0E7, 0x0EA, 0x0ED, 0x0F1, 0x0F4,
    0x0F8, 0x0FB, 0x0FF, 0x102, 0x106, 0x109, 0x10C, 0x110,
    0x114, 0x117, 0x11B, 0x11E, 0x122, 0x125, 0x129, 0x12C,
    0x130, 0x134, 0x137, 0x13B, 0x13E, 0x142, 0x146, 0x149,
    0x14D, 0x151, 0x154, 0x158, 0x15C, 0x160, 0x163, 0x167,
    0x16B, 0x16F, 0x172, 0x176, 0x17A, 0x17E, 0x181, 0x185,
    0x189, 0x18D, 0x191, 0x195, 0x199, 0x19C, 0x1A0, 0x1A4,
    0x1A8, 0x1AC, 0x1B0, 0x1B4, 0x1B8, 0x1BC, 0x1C0, 0x1C4,
    0x1C8, 0x1CC, 0x1D0, 0x1D4, 0x1D8, 0x1DC, 0x1E0, 0x1E4,
    0x1E8, 0x1EC, 0x1F0, 0x1F5, 0x1F9, 0x1FD, 0x201, 0x205,
    0x209, 0x20E, 0x212, 0x216, 0x21A, 0x21E, 0x223, 0x227,
    0x22B, 0x230, 0x234, 0x238, 0x23C, 0x241, 0x245, 0x249,
    0x24E, 0x252, 0x257, 0x25B, 0x25F, 0x264, 0x268, 0x26D,
    0x271, 0x276, 0x27A, 0x27F, 0x283, 0x288, 0x28C, 0x291,
    0x295, 0x29A, 0x29E, 0x2A3, 0x2A8, 0x2AC, 0x2B1, 0x2B5,
    0x2BA, 0x2BF, 0x2C4, 0x2C8, 0x2CD, 0x2D2, 0x2D6, 0x2DB,
    0x2E0, 0x2E5, 0x2E9, 0x2EE, 0x2F3, 0x2F8, 0x2FD, 0x302,
    0x306, 0x30B, 0x310, 0x315, 0x31A, 0x31F, 0x324, 0x329,
    0x32E, 0x333, 0x338, 0x33D, 0x342, 0x347, 0x34C, 0x351,
    0x356, 0x35B, 0x360, 0x365, 0x36A, 0x370, 0x375, 0x37A,
    0x37F, 0x384, 0x38A, 0x38F, 0x394, 0x399, 0x39F, 0x3A4,
    0x3A9, 0x3AE, 0x3B4, 0x3B9, 0x3BF, 0x3C4, 0x3C9, 0x3CF,
    0x3D4, 0x3DA, 0x3DF, 0x3E4, 0x3EA, 0x3EF, 0x3F5, 0x3FA,
];

/// The log-sine ROM: a quarter of a sine wave, as attenuations.
#[rustfmt::skip]
pub(crate) const LOG_SIN: [u16; 256] = [
    2137, 1731, 1543, 1419, 1326, 1252, 1190, 1137, 1091, 1050, 1013,  979,  949,  920,  894,  869,
     846,  825,  804,  785,  767,  749,  732,  717,  701,  687,  672,  659,  646,  633,  621,  609,
     598,  587,  576,  566,  556,  546,  536,  527,  518,  509,  501,  492,  484,  476,  468,  461,
     453,  446,  439,  432,  425,  418,  411,  405,  399,  392,  386,  380,  375,  369,  363,  358,
     352,  347,  341,  336,  331,  326,  321,  316,  311,  307,  302,  297,  293,  289,  284,  280,
     276,  271,  267,  263,  259,  255,  251,  248,  244,  240,  236,  233,  229,  226,  222,  219,
     215,  212,  209,  205,  202,  199,  196,  193,  190,  187,  184,  181,  178,  175,  172,  169,
     167,  164,  161,  159,  156,  153,  151,  148,  146,  143,  141,  138,  136,  134,  131,  129,
     127,  125,  122,  120,  118,  116,  114,  112,  110,  108,  106,  104,  102,  100,   98,   96,
      94,   92,   91,   89,   87,   85,   83,   82,   80,   78,   77,   75,   74,   72,   70,   69,
      67,   66,   64,   63,   62,   60,   59,   57,   56,   55,   53,   52,   51,   49,   48,   47,
      46,   45,   43,   42,   41,   40,   39,   38,   37,   36,   35,   34,   33,   32,   31,   30,
      29,   28,   27,   26,   25,   24,   23,   23,   22,   21,   20,   20,   19,   18,   17,   17,
      16,   15,   15,   14,   13,   13,   12,   12,   11,   10,   10,    9,    9,    8,    8,    7,
       7,    7,    6,    6,    5,    5,    5,    4,    4,    4,    3,    3,    3,    2,    2,    2,
       2,    1,    1,    1,    1,    1,    1,    1,    0,    0,    0,    0,    0,    0,    0,    0,
];

/// The channel each operator belongs to.
#[rustfmt::skip]
pub(crate) const OPERATOR_OWNER: [u8; 36] = [
     0,  1,  2,  0,  1,  2,  3,  4,  5,  3,  4,  5,  6,  7,  8,  6,  7,  8,
     9, 10, 11,  9, 10, 11, 12, 13, 14, 12, 13, 14, 15, 16, 17, 15, 16, 17,
];

/// The bit of register `BDh` that keys each rhythm operator, from operator 12 on.
pub(crate) const RHYTHM_KEY_BIT: [u8; 6] = [4, 0, 2, 4, 3, 1];

/// One of the vibrato's eight steps: how many halves of the frequency number's top bits it
/// adds, and whether it takes them away instead.
#[derive(Clone, Copy)]
pub(crate) struct VibratoStep {
    pub(crate) halves: i32,
    pub(crate) negative: bool,
}

const fn step(halves: i32, negative: bool) -> VibratoStep {
    VibratoStep { halves, negative }
}

/// The vibrato's cycle.
pub(crate) const VIBRATO_STEPS: [VibratoStep; 8] = [
    step(0, false),
    step(1, false),
    step(2, false),
    step(1, false),
    step(0, false),
    step(1, true),
    step(2, true),
    step(1, true),
];

/// How a voice's operators connect: how many it has, which take their phase from the one
/// before (`chain_mask`), which are heard (`carrier_mask`), how much each heard one counts, and
/// whether the first feeds back on itself.
#[derive(Clone, Copy)]
pub(crate) struct Algorithm {
    pub(crate) operator_count: u8,
    pub(crate) chain_mask: u8,
    pub(crate) carrier_mask: u8,
    pub(crate) gain: i32,
    pub(crate) self_feedback: bool,
}

impl Algorithm {
    /// Whether the operator at `position` modulates itself.
    pub(crate) const fn feeds_back(self, position: usize) -> bool {
        position == 0 && self.self_feedback
    }

    /// Whether the operator at `position` is modulated by the one before it.
    pub(crate) const fn chains(self, position: usize) -> bool {
        (self.chain_mask >> position) & 1 != 0
    }

    /// Whether the operator at `position` is heard.
    pub(crate) const fn hears(self, position: usize) -> bool {
        (self.carrier_mask >> position) & 1 != 0
    }

    /// Whether the voice takes a four-operator pair of channels.
    pub(crate) const fn spans_pair(self) -> bool {
        self.operator_count == 4
    }
}

const fn algorithm(
    operator_count: u8,
    chain_mask: u8,
    carrier_mask: u8,
    gain: i32,
    self_feedback: bool,
) -> Algorithm {
    Algorithm {
        operator_count,
        chain_mask,
        carrier_mask,
        gain,
        self_feedback,
    }
}

/// Two operators, the first modulating the second.
pub(crate) const FM: u8 = 0;
/// Two operators, both heard.
pub(crate) const ADDITIVE: u8 = 1;
/// The first of the four-operator routings; the pair's two connection bits pick among four.
pub(crate) const CHAIN: u8 = 2;
/// The bass drum, its first operator modulating the second.
pub(crate) const KICK_FM: u8 = 6;
/// The bass drum, its second operator alone heard.
pub(crate) const KICK_DIRECT: u8 = 7;
/// The other rhythm channels: both operators heard, neither modulated.
pub(crate) const PERCUSSION: u8 = 8;

/// The routings, by the numbers above.
pub(crate) const ALGORITHMS: [Algorithm; 9] = [
    algorithm(2, 0x2, 0x2, 1, true),
    algorithm(2, 0x0, 0x3, 1, true),
    algorithm(4, 0xE, 0x8, 1, true),
    algorithm(4, 0xA, 0xA, 1, true),
    algorithm(4, 0xC, 0x9, 1, true),
    algorithm(4, 0x4, 0xD, 1, true),
    algorithm(2, 0x2, 0x2, 2, true),
    algorithm(2, 0x0, 0x2, 2, true),
    algorithm(2, 0x0, 0x3, 2, false),
];
