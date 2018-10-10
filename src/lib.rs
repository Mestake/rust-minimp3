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
    bitrate_kbps: i32
}

pub struct Mp3Dec_t {
    mdct_overlap: [[f32; 9*32]; 2],
    qmf_state: [f32; 15*2*32],
    reserv: i32,
    free_format_bytes: i32,
    header: [u8; 4],
    reserv_buf: [u8; 511]
}


#[cfg(feature="float_output")]
type Mp3dSample_t = f32;

#[cfg(feature="float_output")]
pub fn mp3dec_f32_to_16(input: &mut [f32], out: &[i16]) {
    unimplemented!()
}

#[cfg(not(feature="float_output"))]
type Mp3dSample_t = i16;


pub fn mp3dec_init(dec: &Mp3Dec_t) {
    unimplemented!()
}

pub fn mp3dec_decode_frame(dec: &Mp3Dec_t, mp3: &u8, mp3_bytes: i32, pcm: &Mp3dSample_t, info: &Mp3DecFrameInfo_t) -> i32 {
    unimplemented!()
}

// *PUBLIC API END*
/////////////////////////////////////////////////

const MAX_FREE_FORMAT_FRAME_SIZE: usize = 2304; // more than ISO spec's

// TODO: make it conditional
const MAX_FRAME_SYNC_MATCHES: usize = 10;
const MAX_L3_FRAME_PAYLOAD_BYTES: usize = MAX_FREE_FORMAT_FRAME_SIZE; //  MUST be >= 320000/8/32000*1152 = 1440 
const MAX_BITRESERVOIR_BYTES: usize = 511;
const SHORT_BLOCK_TYPE: usize = 2;
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
    scf: [f32; 3*64],
    total_bands: u8,
    stereo_bands: u8,
    bitalloc: [u8; 64],
    scfcod: [u8; 64]
}

struct L12SubbandAlloc_t {
    tab_offset: u8,
    code_tab_width: u8,
    band_count: u8,
}

struct L3GrInfo_t<'a> {
    sfbtab: &'a [u8],
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
    scfsi: u8
}

struct Mp3DecScratch_t<'a> {
    bs: Bs_t<'a>,
    maindata: [u8; MAX_BITRESERVOIR_BYTES + MAX_L3_FRAME_PAYLOAD_BYTES],
    gr_info: L3GrInfo_t<'a>,
    grbuf: [[f32; 576]; 2],
    scf: [f32; 40],
    syn: [[f32; 2*32]; 18+15],
    ist_pos: [[u8; 39]; 2],
}

impl<'a> Bs_t<'a> {
    fn new(data: &'a [u8], bytes: u32) -> Self {
        Bs_t {
            buf: data,
            pos: 0,
            limit: bytes * 8
        }
    }

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
fn L12_read_scalefactors<'a>(bs: &mut Bs_t<'a>, 
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
        advance_slice(&mut pba, 1);

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
                           bs: &mut Bs_t<'a>,
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
            advance_slice(&mut subband_alloc, 1);
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
                          bs: &mut Bs_t<'a>,
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

            advance_slice_mut(&mut dst, choff);
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

        advance_slice(&mut scf, 6);
        advance_slice_mut(&mut dst, 18);
    }
}

}} // #[only_mp3]




#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
