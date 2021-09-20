#[inline]
pub fn round_up(num: u64, multiple_of: u64) -> u64 {
    ((num + multiple_of - 1) / multiple_of) * multiple_of
}

#[inline]
fn flsu(x: u32) -> u32 {
    let mut result: u32;
    unsafe {
        asm!("bsr {:e}, {:e}", out(reg) result, in(reg) x);
    }
    return result;
}

#[inline]
pub fn next_pow2_sqrt(x: u32) -> u32 {
    let y = if (x != 1) { 1 } else { 0 };
    return y + flsu(x - y);
}
