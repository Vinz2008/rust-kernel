use crate::syscall::syscall_get_random;

// TODO : instead, use the kernel random only as a seed, then use a normal algorithm (use the same chacha20 ? maybe a smaller chacha ? another algorithm ?)

pub fn random_u64() -> u64 {
    let mut buf = [0; 8];
    syscall_get_random(&mut buf);
    u64::from_ne_bytes(buf)
}