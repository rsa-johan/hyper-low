//! CAN bus constraints applied to frame definitions.

pub const MAX_CLASSIC_LEN: usize = 8;
pub const FD_LENGTHS: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64];

pub fn check_size(size: usize, fd: bool) -> Result<(), String> {
    if fd {
        if !FD_LENGTHS.contains(&size) {
            return Err(format!("CAN FD payload size must be one of {FD_LENGTHS:?}, got {size}"));
        }
    } else if size > MAX_CLASSIC_LEN {
        return Err(format!("classic CAN payload is at most {MAX_CLASSIC_LEN} bytes, got {size} (add `.fd` for CAN FD)"));
    }
    Ok(())
}

/// Smallest valid CAN FD payload length that holds `n` bytes.
pub fn fd_len(n: usize) -> Option<usize> {
    FD_LENGTHS.iter().copied().find(|l| *l >= n)
}
