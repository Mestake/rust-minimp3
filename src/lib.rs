// I assume:
//   - C `int` type is always 32bit
//   - float is 32bit
//
// TODO: rename all the stuff ending with `_t`.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(unused_variables)]

#[macro_use]
extern crate cfg_if;

mod hdr;
mod utils;

use std::ops::*;

// use std::mem::drop;
use hdr::Hdr;
use utils::*;

/////////////////////////////////////////////
// *PUBLIC API START*

pub const MAX_SAMPLES_PER_FRAME: usize = 1152 * 2;

pub struct Mp3DecFrameInfo_t {
    frame_bytes: i32,
    channels: i32,
    hz: i32,
    layer: i32,
    bitrate_kbps: i32,
}

pub struct Mp3Dec_t {
    mdct_overlap: [[f32; 9 * 32]; 2],
    qmf_state: [f32; 15 * 2 * 32],
    reserv: i32,
    free_format_bytes: i32,
    header: [u8; 4],
    reserv_buf: [u8; 511],
}

#[cfg(feature = "float_output")]
type Mp3dSample_t = f32;

#[cfg(feature = "float_output")]
pub fn mp3dec_f32_to_16(input: &mut [f32], out: &[i16]) {
    unimplemented!()
}

#[cfg(not(feature = "float_output"))]
type Mp3dSample_t = i16;

pub fn mp3dec_init(dec: &Mp3Dec_t) {
    unimplemented!()
}

pub fn mp3dec_decode_frame(
    dec: &Mp3Dec_t,
    mp3: &u8,
    mp3_bytes: i32,
    pcm: &Mp3dSample_t,
    info: &Mp3DecFrameInfo_t,
) -> i32 {
    unimplemented!()
}

// *PUBLIC API END*
/////////////////////////////////////////////////

const MAX_FREE_FORMAT_FRAME_SIZE: usize = 2304; // more than ISO spec's

// TODO: make it conditional
const MAX_FRAME_SYNC_MATCHES: usize = 10;
const MAX_L3_FRAME_PAYLOAD_BYTES: usize =
    MAX_FREE_FORMAT_FRAME_SIZE; //  MUST be >= 320000/8/32000*1152 = 1440
const MAX_BITRESERVOIR_BYTES: usize = 511;
const SHORT_BLOCK_TYPE: u8 = 2;
const STOP_BLOCK_TYPE: usize = 3;
const MODE_MONO: u8 = 3;
const MODE_JOINT_STEREO: u8 = 1;

const BITS_DEQUANTIZER_OUT: isize = -1;
const MAX_SCF: isize = 255 + BITS_DEQUANTIZER_OUT * 4 - 210;
const MAX_SCFI: isize = (MAX_SCF + 3) & !3;

// TODO: SIMD stuff

struct Bs_t<'a> {
    buf: &'a [u8],
    pos: u32,
    limit: u32,
}

struct L12ScaleInfo {
    scf: [f32; 3 * 64],
    total_bands: u8,
    stereo_bands: u8,
    bitalloc: [u8; 64],
    scfcod: [u8; 64],
}

struct L12SubbandAlloc_t {
    tab_offset: u8,
    code_tab_width: u8,
    band_count: u8,
}

struct L3GrInfo_t {
    sfbtab: &'static [u8],
    part_23_length: u16,
    big_values: u16,
    scalefac_compress: u16,
    global_gain: u8,
    block_type: u8,
    mixed_block_flag: u8,
    n_long_sfb: u8,
    n_short_sfb: u8,
    table_select: [u8; 3],
    region_count: [u8; 3],
    subblock_gain: [u8; 3],
    preflag: u8,
    scalefac_scale: u8,
    count1_table: u8,
    scfsi: u8,
}

struct Mp3DecScratch_t<'a> {
    bs: Bs_t<'a>,
    maindata: [u8; MAX_BITRESERVOIR_BYTES
        + MAX_L3_FRAME_PAYLOAD_BYTES],
    gr_info: L3GrInfo_t,
    grbuf: [[f32; 576]; 2],
    scf: [f32; 40],
    syn: [[f32; 2 * 32]; 18 + 15],
    ist_pos: [[u8; 39]; 2],
}

impl<'a> Bs_t<'a> {
    fn new(data: &'a [u8], bytes: u32) -> Self {
        Bs_t {
            buf: data,
            pos: 0,
            limit: bytes * 8,
        }
    }

    // TODO: rename to `next_bits`
    fn get_bits(&mut self, n: u32) -> u32 {
        self.pos += n;
        if self.pos > self.limit {
            return 0;
        }

        let p = &self.buf[self.pos.shr(3) as usize..];
        let s: u8 = (self.pos & 7) as u8;

        let mut cache: u32 = 0;
        let mut next = (p[0] & (255 >> s)) as u32;

        let mut shl = (n + s as u32 - 8) as i32;
        let mut i = 1;
        while shl > 0 {
            cache |= next << shl;
            next = p[i] as u32;

            shl -= 8;
            i += 1;
        }

        debug_assert!(shl < 0);
        cache | (next >> -shl)
    }
}

cfg_if! { if #[cfg(not(feature = "only_mp3"))] {

macro_rules! L12SA {
    ($tab_offset:expr, $code_tab_width:expr, $band_count:expr) => (
        L12SubbandAlloc_t {
            tab_offset: $tab_offset,
            code_tab_width: $code_tab_width,
            band_count: $band_count
        }
    )
}

impl L12SubbandAlloc_t {
    fn new(hdr: &Hdr) -> (&'static [Self], u8, u8) {
        static ALLOC_L2M2: [L12SA; 3] = [
                L12SA!(60, 4, 4),
                L12SA!(44, 3, 7),
                L12SA!(44, 2, 19)
        ];

        static ALLOC_L1: [L12SA; 1] = [L12SA!(76, 4, 32)];

        static ALLOC_L2M1: [L12SA; 4] = [
                L12SA!(0, 4, 3),
                L12SA!(16, 4, 8),
                L12SA!(32, 3, 12),
                L12SA!(40, 2, 7)
        ];

        static ALLOC_L2M1_LOWRATE: [L12SA; 2] = [
                    L12SA!(44, 4, 2),
                    L12SA!(44, 3, 10)
        ];

        let mode = hdr.get_stereo_mode();
        let stereo_bands = match mode  {
            MODE_MONO => 0,
            MODE_JOINT_STEREO =>
                hdr.get_stereo_mode_ext() << 2 + 4,
            _ => 32
        };

        type L12SA = L12SubbandAlloc_t;

        let mut alloc: &'static [L12SA];
        let mut nbands;

        if hdr.is_layer_1() {

            alloc = &ALLOC_L1;
            nbands = 32;
        }
        else if hdr.test_mpeg1() == 0 {
            alloc = &ALLOC_L2M2;
            nbands = 30;
        }
        else {
            alloc = &ALLOC_L2M1;
            nbands = 27;

            let sample_rate_idx = hdr.get_sample_rate();
            let mut kbps = hdr.bitrate_kbps() >> (mode != MODE_MONO) as u32;
            if kbps == 0 {
                kbps = 192;
            }

            if kbps < 56 {
                alloc = &ALLOC_L2M1_LOWRATE;
                nbands = if sample_rate_idx == 2 { 12 } else { 8 };
            }
            else if kbps >= 96 && sample_rate_idx != 1
            {
                nbands = 30;
            }
        }

        (alloc, nbands, stereo_bands.min(nbands))
    }


}

macro_rules! DEQ_L12 {
    ( $($x:expr),* ) => ([
        $(
            9.53674316e-07f32 / $x as f32,
            7.56931807e-07f32 / $x as f32,
            6.00777173e-07f32 / $x as f32,
        )*
    ])
}

// `scfcod` and `bands` are merged into one argument
//    (`bands` is the length of `scfcod`)
fn L12_read_scalefactors<'a>(bs: &mut Bs_t,
                         pba: &[u8],
                         scfcod: &[u8],
                         scf: &mut [f32]) {
    static DEQ_L12: [f32; 18*3] = DEQ_L12![
        3, 7, 15,
        31, 63, 127,
        255, 511, 1023,
        2047, 4095, 8191,
        16383, 32767, 65535,
        3, 5, 9
    ];

    let mut pba = pba;

    assert!(scf.len() >= scfcod.len());

    for (i, dst) in scfcod.iter().zip(scf.iter_mut()) {
        let ba = pba[0];
        slice_advance(&mut pba, 1);

        let mask = if ba != 0 { (19 >> *i) & 3 } else { 0 };

        for m in [4, 2, 1].into_iter() {
            let s = if mask & m != 0 {
                let b = bs.get_bits(6);
                let index = ba as u32 * 3 - 6 + b % 3;
                DEQ_L12[index as usize] * (1 << 21 >> (b / 3)) as f32
            }
            else {
                0.0
            };

            *dst = s;
        }
    }
}

fn L12_read_scale_info<'a>(hdr: &Hdr,
                           bs: &mut Bs_t,
                           sci: &mut L12ScaleInfo) {
    static BITALLOC_CODE_TAB: [u8; 92] = [
        0,17, 3, 4, 5,6,7, 8,9,10,11,12,13,14,15,16,
        0,17,18, 3,19,4,5, 6,7, 8, 9,10,11,12,13,16,
        0,17,18, 3,19,4,5,16,
        0,17,18,16,
        0,17,18,19, 4,5,6, 7,8, 9,10,11,12,13,14,15,
        0,17,18, 3,19,4,5, 6,7, 8, 9,10,11,12,13,14,
        0, 2, 3, 4, 5,6,7, 8,9,10,11,12,13,14,15,16
    ];

    let (mut subband_alloc, nbands, stereo_bands) = L12SubbandAlloc_t::new(hdr);

    sci.total_bands = nbands;
    sci.stereo_bands = stereo_bands;

    let mut k = 0;
    let mut ba_code_tab = &BITALLOC_CODE_TAB[..];

    for i in 0..sci.total_bands {
        let i = i as usize;

        let mut ba_bits = 0;
        if i == k {
            k += subband_alloc[0].band_count as usize;
            ba_bits = subband_alloc[0].code_tab_width;

            let offset = subband_alloc[0].tab_offset as usize;
            ba_code_tab = &BITALLOC_CODE_TAB[offset..];
            slice_advance(&mut subband_alloc, 1);
        }

        let mut ba = ba_code_tab[bs.get_bits(ba_bits as u32) as usize];
        sci.bitalloc[2*i] = ba;

        if i < sci.stereo_bands as usize {
            ba = ba_code_tab[bs.get_bits(ba_bits as u32) as usize];
        }
        sci.bitalloc[2*i + 1] =
            if sci.stereo_bands != 0 { ba } else { 0 };
    }

    for i in 0 .. sci.total_bands * 2 {
        let i = i as usize;

        let val = if sci.bitalloc[i] != 0 {
            if hdr.is_layer_1() { 2 } else { bs.get_bits(2) }
        }
        else {
            6
        };

        sci.scfcod[i] = val as u8;
    }

    L12_read_scalefactors(bs,
                          &sci.bitalloc,
                          &sci.scfcod[0..sci.total_bands as usize],
                          &mut sci.scf);

    for i in sci.stereo_bands .. sci.total_bands {
        let i = i as usize;
        sci.bitalloc[2*i + 1] = 0;
    }
}

fn L12_dequantize_granule<'a>(grbuf: &mut [f32],
                          bs: &mut Bs_t,
                          sci: &L12ScaleInfo,
                          group_size: u32)
    -> u32
{
    let mut choff: usize = 576;
    for j in 0 .. 4 {
        let offset = (group_size * j) as usize;
        let mut dst = &mut grbuf[offset..];

        for i in 0 .. 2 * sci.total_bands {
            match sci.bitalloc[i as usize] {
                0 => (),

                ba if ba < 17 => {
                    let half = 1 << (ba - 1) - 1;
                    for k in 0 .. group_size {
                        dst[k as usize] =
                            (bs.get_bits(ba as u32) - half) as f32;
                    }
                },

                ba => {
                    let mod_: u32 = 2 << (ba - 17) + 1; // 3, 5, 9
                    let mut code: u32 = bs.get_bits(mod_ + 2 - (mod_ >> 3)); // 5, 7, 10
                    for k in 0 .. group_size {
                        dst[k as usize] =
                            (code % mod_ - mod_ / 2) as u32 as f32;
                        code /= mod_;
                    }
                }
            }

            slice_advance_mut(&mut dst, choff);
            choff = 18 - choff;
        }
    }

    group_size * 4
}

fn L12_apply_scf_384(sci: &L12ScaleInfo,
                     mut scf: &[f32],
                     mut dst: &mut [f32])
{
    let sba18 = sci.stereo_bands as usize * 18;
    let diff18 = (sci.total_bands - sci.stereo_bands) as usize * 18;
    copy_forward_within_slice(dst, sba18, sba18 + 576, diff18);

    for _ in 0 .. sci.total_bands {
        for k in 0 .. 12 {
            dst[k + 0]   *= scf[0];
            dst[k + 576] *= scf[3];
        }

        slice_advance(&mut scf, 6);
        slice_advance_mut(&mut dst, 18);
    }
}

}} // #[only_mp3]

fn L3_read_side_info<'a>(
    bs: &mut Bs_t,
    gr: &mut [L3GrInfo_t],
    hdr: &Hdr,
) -> Option<u32> {
    static SFC_LONG: [[u8; 23]; 8] = [
        [
            6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28,
            32, 38, 46, 52, 60, 68, 58, 54, 0,
        ],
        [
            12, 12, 12, 12, 12, 12, 16, 20, 24, 28, 32, 40, 48,
            56, 64, 76, 90, 2, 2, 2, 2, 2, 0,
        ],
        [
            6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28,
            32, 38, 46, 52, 60, 68, 58, 54, 0,
        ],
        [
            6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 18, 22, 26,
            32, 38, 46, 54, 62, 70, 76, 36, 0,
        ],
        [
            6, 6, 6, 6, 6, 6, 8, 10, 12, 14, 16, 20, 24, 28,
            32, 38, 46, 52, 60, 68, 58, 54, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 6, 6, 8, 8, 10, 12, 16, 20, 24,
            28, 34, 42, 50, 54, 76, 158, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 10, 12, 16, 18, 22,
            28, 34, 40, 46, 54, 54, 192, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 6, 6, 8, 10, 12, 16, 20, 24, 30,
            38, 46, 56, 68, 84, 102, 26, 0,
        ],
    ];

    static SCF_SHORT: [[u8; 40]; 8] = [
        [
            4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8, 8, 10,
            10, 10, 12, 12, 12, 14, 14, 14, 18, 18, 18, 24, 24,
            24, 30, 30, 30, 40, 40, 40, 18, 18, 18, 0,
        ],
        [
            8, 8, 8, 8, 8, 8, 8, 8, 8, 12, 12, 12, 16, 16, 16,
            20, 20, 20, 24, 24, 24, 28, 28, 28, 36, 36, 36, 2,
            2, 2, 2, 2, 2, 2, 2, 2, 26, 26, 26, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 6, 6, 6, 8, 8,
            8, 10, 10, 10, 14, 14, 14, 18, 18, 18, 26, 26, 26,
            32, 32, 32, 42, 42, 42, 18, 18, 18, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8, 8, 10,
            10, 10, 12, 12, 12, 14, 14, 14, 18, 18, 18, 24, 24,
            24, 32, 32, 32, 44, 44, 44, 12, 12, 12, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8, 8, 10,
            10, 10, 12, 12, 12, 14, 14, 14, 18, 18, 18, 24, 24,
            24, 30, 30, 30, 40, 40, 40, 18, 18, 18, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8,
            8, 10, 10, 10, 12, 12, 12, 14, 14, 14, 18, 18, 18,
            22, 22, 22, 30, 30, 30, 56, 56, 56, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 6, 6,
            6, 10, 10, 10, 12, 12, 12, 14, 14, 14, 16, 16, 16,
            20, 20, 20, 26, 26, 26, 66, 66, 66, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 6, 6, 6, 8, 8,
            8, 12, 12, 12, 16, 16, 16, 20, 20, 20, 26, 26, 26,
            34, 34, 34, 42, 42, 42, 12, 12, 12, 0,
        ],
    ];

    static SCF_MIXED: [[u8; 40]; 8] = [
        [
            6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12,
            12, 12, 14, 14, 14, 18, 18, 18, 24, 24, 24, 30, 30,
            30, 40, 40, 40, 18, 18, 18, 0, 0, 0, 0,
        ],
        [
            12, 12, 12, 4, 4, 4, 8, 8, 8, 12, 12, 12, 16, 16,
            16, 20, 20, 20, 24, 24, 24, 28, 28, 28, 36, 36, 36,
            2, 2, 2, 2, 2, 2, 2, 2, 2, 26, 26, 26, 0,
        ],
        [
            6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 10,
            10, 10, 14, 14, 14, 18, 18, 18, 26, 26, 26, 32, 32,
            32, 42, 42, 42, 18, 18, 18, 0, 0, 0, 0,
        ],
        [
            6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12,
            12, 12, 14, 14, 14, 18, 18, 18, 24, 24, 24, 32, 32,
            32, 44, 44, 44, 12, 12, 12, 0, 0, 0, 0,
        ],
        [
            6, 6, 6, 6, 6, 6, 6, 6, 6, 8, 8, 8, 10, 10, 10, 12,
            12, 12, 14, 14, 14, 18, 18, 18, 24, 24, 24, 30, 30,
            30, 40, 40, 40, 18, 18, 18, 0, 0, 0, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 6, 6, 4, 4, 4, 6, 6, 6, 8, 8, 8,
            10, 10, 10, 12, 12, 12, 14, 14, 14, 18, 18, 18, 22,
            22, 22, 30, 30, 30, 56, 56, 56, 0, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 6, 6, 4, 4, 4, 6, 6, 6, 6, 6, 6,
            10, 10, 10, 12, 12, 12, 14, 14, 14, 16, 16, 16, 20,
            20, 20, 26, 26, 26, 66, 66, 66, 0, 0,
        ],
        [
            4, 4, 4, 4, 4, 4, 6, 6, 4, 4, 4, 6, 6, 6, 8, 8, 8,
            12, 12, 12, 16, 16, 16, 20, 20, 20, 26, 26, 26, 34,
            34, 34, 42, 42, 42, 12, 12, 12, 0, 0,
        ],
    ];

    let mut sr_idx = hdr.get_my_sample_rate() as usize;
    let mut gr_count = if hdr.is_mono() { 1 } else { 2 };
    let mut scfsi;
    let mut part_23_sum = 0;
    let main_data_begin;

    assert!(gr.len() >= gr_count);

    if sr_idx != 0 {
        sr_idx -= 1;
    }

    if hdr.test_mpeg1() != 0 {
        gr_count *= 2;
        main_data_begin = bs.get_bits(9);
        scfsi = bs.get_bits(gr_count as u32 + 7);
    } else {
        main_data_begin =
            bs.get_bits(gr_count as u32 + 8) >> gr_count;
        scfsi = 0;
    }

    // we don't need do-while loop here
    // because gr_count starts from 1 or bigger,
    // so the first iteration runs anyway
    for (_, gr) in (0..gr_count).zip(gr.iter_mut()) {
        if hdr.is_mono() {
            scfsi <<= 4;
        }

        gr.part_23_length = bs.get_bits(12) as u16;
        part_23_sum += gr.part_23_length;

        gr.big_values = bs.get_bits(9) as u16;
        if gr.big_values > 288 {
            return None;
        }

        gr.global_gain = bs.get_bits(8) as u8;
        gr.scalefac_compress =
            bs.get_bits(if hdr.test_mpeg1() != 0 {
                4
            } else {
                9
            }) as u16;
        gr.sfbtab = &SFC_LONG[sr_idx];
        gr.n_long_sfb = 22;
        gr.n_short_sfb = 0;

        let mut tables;

        if bs.get_bits(1) != 0 {
            gr.block_type = bs.get_bits(2) as u8;
            if gr.block_type == 0 {
                return None;
            }

            gr.mixed_block_flag = bs.get_bits(1) as u8;
            gr.region_count[0] = 7;
            gr.region_count[1] = 255;

            if gr.block_type == SHORT_BLOCK_TYPE {
                scfsi &= 0x0F0F;;

                if gr.mixed_block_flag == 0 {
                    gr.region_count[0] = 8;
                    gr.sfbtab = &SCF_SHORT[sr_idx];
                    gr.n_long_sfb = 0;
                    gr.n_short_sfb = 39;
                } else {
                    gr.sfbtab = &SCF_MIXED[sr_idx];
                    gr.n_long_sfb = if hdr.test_mpeg1() != 0 {
                        8
                    } else {
                        6
                    };
                    gr.n_short_sfb = 30;
                }
            }

            tables = bs.get_bits(10);
            tables <<= 5;
            gr.subblock_gain[0] = bs.get_bits(3) as u8;
            gr.subblock_gain[1] = bs.get_bits(3) as u8;
            gr.subblock_gain[2] = bs.get_bits(3) as u8;
        } else {
            gr.block_type = 0;
            gr.mixed_block_flag = 0;
            tables = bs.get_bits(15);
            gr.region_count[0] = bs.get_bits(4) as u8;
            gr.region_count[1] = bs.get_bits(3) as u8;
            gr.region_count[2] = 255;
        }

        gr.table_select[0] = (tables >> 10) as u8;
        gr.table_select[1] = ((tables >> 5) & 31) as u8;
        gr.table_select[2] = ((tables) & 31) as u8;

        gr.preflag = if hdr.test_mpeg1() != 0 {
            bs.get_bits(1) as u8
        } else {
            (gr.scalefac_compress >= 500) as u8
        };

        gr.scalefac_scale = bs.get_bits(1) as u8;
        gr.count1_table = bs.get_bits(1) as u8;
        gr.scfsi = ((scfsi >> 12) & 15) as u8;
        scfsi <<= 4;
    }

    if part_23_sum as u32 + bs.pos
        > bs.limit + main_data_begin * 8
    {
        return None;
    }

    Some(main_data_begin)
}

fn L3_read_scalefactors<'a>(
    mut scf: &mut [u8],
    mut ist_pos: &mut [u8],
    scf_size: &[u8],
    scf_count: &[u8],
    bitbuf: &mut Bs_t,
    mut scfsi: i32,
) {
    let mut i = 0;
    while i < 4 && scf_count[i] != 0 {
        let cnt = scf_count[i] as usize;

        if scfsi & 8 != 0 {
            slice_copy_n(ist_pos, scf, cnt);
        } else {
            let bits = scf_size[i];
            if bits == 0 {
                slice_fill(&mut scf[0..cnt], 0);
                slice_fill(&mut ist_pos[0..cnt], 0);
            } else {
                // FIXME: rewiev this
                let max_scf = if scfsi < 0 {
                    (1 << bits - 1) as i32
                } else {
                    -1
                };

                for k in 0..cnt {
                    let s = bitbuf.get_bits(bits as u32) as i32;
                    ist_pos[k] = if s == max_scf {
                        255
                    } else {
                        s as u8
                    };
                    scf[k] = s as u8;
                }
            }
        }

        slice_advance_mut(&mut ist_pos, cnt);
        slice_advance_mut(&mut scf, cnt);

        scfsi = scfsi.wrapping_mul(2);
        i += 1;
    }

    scf[0] = 0;
    scf[1] = 0;
    scf[2] = 0;
}

fn L3_ldexp_q2(mut y: f32, mut exp_q2: i32) -> f32 {
    static EXPFRAC: [f32; 4] = [
        9.31322575e-10,
        7.83145814e-10,
        6.58544508e-10,
        5.53767716e-10,
    ];

    let e = 0;

    loop {
        exp_q2.min(30 * 4);
        y *= EXPFRAC[e & 3] * (1 << 30 >> (e >> 2)) as f32;

        exp_q2 -= e as i32;
        if exp_q2 <= 0 {
            break;
        }
    }

    y
}

fn L3_decode_scalefactors(
    hdr: &Hdr,
    ist_pos: &mut [u8],
    bs: &mut Bs_t,
    gr: &L3GrInfo_t,
    scf: &mut [f32],
    ch: bool,
) {
    static SCF_PARTITIONS: [[u8; 28]; 3] = [
        [
            6, 5, 5, 5, 6, 5, 5, 5, 6, 5, 7, 3, 11, 10, 0, 0,
            7, 7, 7, 0, 6, 6, 6, 3, 8, 8, 5, 0,
        ],
        [
            8, 9, 6, 12, 6, 9, 9, 9, 6, 9, 12, 6, 15, 18, 0, 0,
            6, 15, 12, 0, 6, 12, 9, 6, 6, 18, 9, 0,
        ],
        [
            9, 9, 6, 12, 9, 9, 9, 9, 9, 9, 12, 6, 18, 18, 0, 0,
            12, 12, 12, 0, 12, 9, 9, 6, 15, 12, 9, 0,
        ],
    ];

    let scf_partition = &SCF_PARTITIONS
        [gr.n_short_sfb.bclamp() + gr.n_long_sfb.is0()];

    let mut scf_size: [u8; 4] = [0; 4];
    let mut iscf: [u8; 40] = [0; 40];

    let scf_shift = gr.scalefac_scale + 1;
    let mut scfsi = gr.scfsi as i32;

    if hdr.test_mpeg1() != 0 {
        static SCFC_DECODE: [u8; 16] = [
            0, 1, 2, 3, 12, 5, 6, 7, 9, 10, 11, 13, 14, 15, 18,
            19,
        ];

        let part = SCFC_DECODE[gr.scalefac_compress as usize];
        scf_size[0] = (part >> 2) as u8;
        scf_size[1] = (part >> 2) as u8;
        scf_size[2] = (part & 3) as u8;
        scf_size[3] = (part & 3) as u8;
    } else {
        static MOD: [u8; 6 * 4] = [
            5, 5, 4, 4, 5, 5, 4, 1, 4, 3, 1, 1, 5, 6, 6, 1, 4,
            4, 4, 1, 4, 3, 1, 1,
        ];

        let ist = hdr.test_i_stereo() != 0 && ch;
        let mut sfc = gr.scalefac_compress as i32 >> ist as u32;
        let mut k = ist as usize * 3 * 4;

        while sfc >= 0 {
            let mut modprod = 1;

            for i in (0..4).rev() {
                let val =
                    (sfc as u32 / modprod) % MOD[k + i] as u32;
                scf_size[i] = val as u8;
                modprod *= MOD[k + i] as u32;
            }

            sfc -= modprod as i32;
            k += 4;
        }

        slice_advance(&mut &scf_partition[..], k);
        scfsi = -16;
    }

    L3_read_scalefactors(
        &mut iscf,
        ist_pos,
        &mut scf_size,
        scf_partition,
        bs,
        scfsi,
    );

    if gr.n_short_sfb != 0 {
        let sh = 3 - scf_shift;
        let mut i = 0;
        while i < gr.n_short_sfb as usize {
            let n_long_sfb = gr.n_long_sfb as usize;
            iscf[n_long_sfb + i + 0] +=
                gr.subblock_gain[0] << sh;
            iscf[n_long_sfb + i + 1] +=
                gr.subblock_gain[1] << sh;
            iscf[n_long_sfb + i + 2] +=
                gr.subblock_gain[2] << sh;

            i += 3;
        }
    } else if gr.preflag != 0 {
        static PREAMP: [u8; 10] =
            [1, 1, 1, 1, 2, 2, 3, 3, 3, 2];
            
        for i in 0..10 {
            iscf[11 + i] += PREAMP[i];
        }
    }

    let gain_exp = gr.global_gain as isize
        + BITS_DEQUANTIZER_OUT * 4
        - 210
        - (if hdr.is_ms_stereo() { 2 } else { 0 });

    let gain = L3_ldexp_q2(
        (1 << (MAX_SCFI / 4)) as f32,
        (MAX_SCFI - gain_exp) as i32,
    );

    for i in
        0..(gr.n_long_sfb as usize + gr.n_short_sfb as usize)
    {
        scf[i] =
            L3_ldexp_q2(gain, (iscf[i] << scf_shift) as i32);
    }
}

static POW43: [f32; 129 + 16] = [
    0.0, -1.0, -2.519842, -4.326749, -6.349604, -8.549880,
    -10.902724, -13.390518, -16.000000, -18.720754, -21.544347,
    -24.463781, -27.473142, -30.567351, -33.741992, -36.993181,
    0.0, 1.0, 2.519842, 4.326749, 6.349604, 8.549880,
    10.902724, 13.390518, 16.000000, 18.720754, 21.544347,
    24.463781, 27.473142, 30.567351, 33.741992, 36.993181,
    40.317474, 43.711787, 47.173345, 50.699631, 54.288352,
    57.937408, 61.644865, 65.408941, 69.227979, 73.100443,
    77.024898, 81.000000, 85.024491, 89.097188, 93.216975,
    97.382800, 101.593667, 105.848633, 110.146801, 114.487321,
    118.869381, 123.292209, 127.755065, 132.257246, 136.798076,
    141.376907, 145.993119, 150.646117, 155.335327, 160.060199,
    164.820202, 169.614826, 174.443577, 179.305980, 184.201575,
    189.129918, 194.090580, 199.083145, 204.107210, 209.162385,
    214.248292, 219.364564, 224.510845, 229.686789, 234.892058,
    240.126328, 245.389280, 250.680604, 256.000000, 261.347174,
    266.721841, 272.123723, 277.552547, 283.008049, 288.489971,
    293.998060, 299.532071, 305.091761, 310.676898, 316.287249,
    321.922592, 327.582707, 333.267377, 338.976394, 344.709550,
    350.466646, 356.247482, 362.051866, 367.879608, 373.730522,
    379.604427, 385.501143, 391.420496, 397.362314, 403.326427,
    409.312672, 415.320884, 421.350905, 427.402579, 433.475750,
    439.570269, 445.685987, 451.822757, 457.980436, 464.158883,
    470.357960, 476.577530, 482.817459, 489.077615, 495.357868,
    501.658090, 507.978156, 514.317941, 520.677324, 527.056184,
    533.454404, 539.871867, 546.308458, 552.764065, 559.238575,
    565.731879, 572.243870, 578.774440, 585.323483, 591.890898,
    598.476581, 605.080431, 611.702349, 618.342238, 625.000000,
    631.675540, 638.368763, 645.079578,
];

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
