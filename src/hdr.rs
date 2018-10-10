#![allow(non_camel_case_types)]

use std::ops::*;
use utils::*;


const HDR_SIZE: usize = 4;

pub struct Hdr([u8; HDR_SIZE]);

/// Note: some functions are renamed:
///   - ones starting with `hdr_` prefix => prefix is thrown away
///   - `hdr_valid` => `is_valid`
impl Hdr {
    pub fn new(data: &[u8]) -> Self {
        debug_assert!(data.len() >= 4);
        Hdr([data[0], data[1], data[2], data[3]])
    }

    pub fn is_mono(&self) -> bool {
        (self.0[3] & 0xC0) == 0xC0
    }

    pub fn is_ms_stereo(&self) -> bool {
        (self.0[3] & 0xE0) == 0x60
    }

    pub fn is_free_format(&self) -> bool {
        (self.0[2] & 0xF0) == 0
    }

    pub fn is_crc(&self) -> bool {
        self.0[1] & 1 == 0
    }

    pub fn test_padding(&self) -> u8 {
        self.0[2] & 0x2
    }

    pub fn test_mpeg1(&self) -> u8 {
        self.0[1] & 0x8
    }

    pub fn test_not_mpeg25(&self) -> u8 {
        self.0[1] & 0x10
    }

    pub fn test_i_stereo(&self) -> u8 {
        self.0[3] & 0x10
    }

    pub fn test_ms_stereo(&self) -> u8 {
        self.0[3] & 0x20
    }

    pub fn get_stereo_mode(&self) -> u8 {
        self.0[3].shr(6) & 3u8
    }

    pub fn get_stereo_mode_ext(&self) -> u8 {
        self.0[3].shr(4) & 3u8
    }

    pub fn get_layer(&self) -> u8 {
        self.0[1].shr(1) & 3u8
    }

    pub fn get_bitrate(&self) -> u8 {
        self.0[2].shr(4)
    }

    pub fn get_sample_rate(&self) -> u8 {
        self.0[2].shr(2) & 3u8
    }

    pub fn get_my_sample_rate(&self) -> u32 {
        let magic1 = self.0[1].shr(3) & 1u8;
        let magic2 = self.0[1].shr(4) & 1u8;
        let magic3 = (magic1 + magic2) as u32 * 3;
        self.get_sample_rate() as u32 + magic3
    }

    pub fn is_frame_576(&self) -> bool {
        (self.0[1] & 14) == 2
    }

    pub fn is_layer_1(&self) -> bool {
        (self.0[1] & 6) == 6
    }

    pub fn is_valid(&self) -> bool {
        let h = self;
        h[0] == 0xff &&
            (h[1] & 0xF0 == 0xf0 || h[1] & 0xFE == 0xe2) &&
            h.get_layer() != 0 &&
            h.get_bitrate() != 15 &&
            h.get_sample_rate() != 3
    }

    pub fn compare(&self, other: &Self) -> bool {
        let h1 = self;
        let h2 = other;

        h2.is_valid() &&
            (h1[1] ^ h2[1]) & 0xFE == 0 &&
            (h1[2] ^ h2[2]) & 0x0C == 0 &&
            !(h1.is_free_format() ^ h2.is_free_format())
    }

    pub fn bitrate_kbps(&self) -> u32 {
        static HALFRATE: [[[u8; 15]; 3]; 2] = [
            [ 
                [ 0,4,8,12,16,20,24,28,32,40,48,56,64,72,80 ], 
                [ 0,4,8,12,16,20,24,28,32,40,48,56,64,72,80 ], 
                [ 0,16,24,28,32,40,48,56,64,72,80,88,96,112,128 ] 
            ],
            [   
                [ 0,16,20,24,28,32,40,48,56,64,80,96,112,128,160 ],
                [ 0,16,24,28,32,40,48,56,64,80,96,112,128,160,192 ], 
                [ 0,16,32,48,64,80,96,112,128,144,160,176,192,208,224 ] 
            ],
        ];

        let h = self;
        let i1 = h.test_mpeg1().bclamp();
        let i2 = (h.get_layer() - 1) as usize;
        let i3 = h.get_bitrate() as usize;
        
        2 * HALFRATE[i1][i2][i3] as u32
    }

    pub fn sample_rate_hz(&self) -> u32 {
        static HZ: [u32; 3] = [44100, 48000, 32000]; 

        // FIXME: MAY BE INCORECT
        HZ[self.get_sample_rate() as usize] 
            >> self.test_mpeg1().is0()
            >> self.test_not_mpeg25().is0()
    }

    pub fn frame_samples(&self) -> u32 {
        if self.is_layer_1() {
            384
        }
        else {
            1152 >> self.is_frame_576() as u32
        }
    }

    pub fn frame_bytes(&self, free_format_size: u32) -> u32 {
        let mut frame_bytes = self.frame_samples() *
            self.bitrate_kbps() * 125 / self.sample_rate_hz();

        if self.is_layer_1() {
            frame_bytes &= !3;
        }

        if frame_bytes != 0 {
            frame_bytes
        }
        else {
            free_format_size
        }
    }

    pub fn padding(&self) -> u32 {
        if self.test_padding() != 0 {
            if self.is_layer_1() {
                4
            }
            else {
                1
            }
        }
        else {
            0
        }
    }
}

impl Index<usize> for Hdr {
    type Output = u8;

    fn index(&self, i: usize) -> &u8 {
        &self.0[i]
    }
}
