use std::mem;

pub fn advance_slice<T>(slice: &mut &[T], i: usize) {
    let res = unsafe {
        let (_, res) = slice.split_at(i);
        mem::transmute(res)
    };

    *slice = res;
}

pub fn advance_slice_mut<T>(slice: &mut &mut [T], i: usize) {
    let res = unsafe {
        let (_, res) = slice.split_at_mut(i);
        mem::transmute(res)
    };

    *slice = res;
}

pub fn copy_forward_within_slice<T: Copy>(slice: &mut [T], 
                                  src: usize, 
                                  dst: usize,
                                  count: usize) {
    assert!(src < dst);
    assert!(dst + count <= slice.len());

    let src = &mut slice[src..].as_mut_ptr();
    let dst = &mut slice[dst..].as_mut_ptr();

    for i in 0 .. count {
        unsafe {
            let val = src.add(i).read();
            dst.add(i).write(val);
        }
    }
}