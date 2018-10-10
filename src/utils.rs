use std::mem;

/// returns an Iterator that repeats `n` times
// pub fn repeat_n(n: usize) -> impl Iterator<Item=()> {
//     struct Iter(usize);

//     impl Iterator for Iter {
//         type Item = ();

//         fn next(&mut self) -> Option<()> {
//             if self.0 != 0 {
//                 self.0 -= 1;
//                 Some(())
//             }
//             else {
//                 None
//             }
//         }
//     }

//     Iter(n)
// }

pub fn slice_advance<T>(slice: &mut &[T], i: usize) {
    let res = unsafe {
        let (_, res) = slice.split_at(i);
        mem::transmute(res)
    };

    *slice = res;
}

pub fn slice_advance_mut<T>(slice: &mut &mut [T], i: usize) {
    let res = unsafe {
        let (_, res) = slice.split_at_mut(i);
        mem::transmute(res)
    };

    *slice = res;
}

pub fn copy_forward_within_slice<T: Copy>(
    slice: &mut [T],
    src: usize,
    dst: usize,
    count: usize,
) {
    assert!(src < dst);
    assert!(dst + count <= slice.len());

    let src = &mut slice[src..].as_mut_ptr();
    let dst = &mut slice[dst..].as_mut_ptr();

    for i in 0..count {
        unsafe {
            let val = src.add(i).read();
            dst.add(i).write(val);
        }
    }
}

pub fn slice_copy_n<T: Copy>(
    src: &[T],
    dst: &mut [T],
    n: usize,
) {
    assert!(src.len() >= n);
    assert!(dst.len() >= n);

    let src = &src[0..n];
    let dst = &mut dst[0..n];

    dst.copy_from_slice(src);
}

pub fn slice_fill<T: Copy>(dst: &mut [T], val: T) {
    for spot in dst.iter_mut() {
        *spot = val;
    }
}

/// A set of some useful things surprisengly missing in std
pub trait Number: Copy {
    /// "clamp" value between lower and highter bounds
    fn clamp(self, l: Self, h: Self) -> Self;

    /// clamp between 0 and 1
    /// usize because I see only indexing usages
    fn bclamp(self) -> usize;

    /// return 1 if x == 0; 0 otherwise
    fn is0(self) -> usize;
}

macro_rules! impl_Number {
    ($($t:ty),*) => ($(

impl Number for $t {
    fn clamp(self, l: Self, h: Self) -> Self {
        self.min(h).max(l)
    }

    fn bclamp(self) -> usize {
        self.clamp(0, 1) as usize
    }

    fn is0(self) -> usize {
        (self != 0) as usize
    }
}
)*)}

impl_Number! {
    i8, i16, i32, i64, isize,
    u8, u16, u32, u64, usize
}
