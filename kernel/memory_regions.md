# user memory (lower half)
0x0000_0000_0000_0000 - 0x0000_0000_000b_8000 : empty
0x0000_0000_000b_8000 - 0x0000_0000_000b_8fa0 : vga buffer (~4KiB)
0x0000_0000_000b_8fa0 - 0x0000_0000_0020_0000 : empty
0x0000_0000_0020_0000 - 0x0000_0000_4000_0000 : userspace ELF LOAD segments (~1GiB)
0x0000_0000_4000_0000 - 0x0000_0000_8000_0000 : user heap (1 GiB)
0x0000_0000_8000_0000 - 0x0000_7fff_ffef_e000 : empty
0x0000_7fff_ffef_e000 - 0x0000_7fff_ffef_f000 : guard page (4 KiB, unmapped)
0x0000_7fff_ffef_f000 - 0x0000_7fff_ffff_f000 : user process stack (1MiB)
0x0000_7fff_ffff_f000 - 0x0000_8000_0000_0000 : empty

# non-canonical hole

0x0000_8000_0000_0000 - 0xffff_8000_0000_0000 : invalid, can't use

# kernel memory (higher half)
0xffff_8000_0000_0000 - 0xffff_9000_0000_0000 : kernel process stacks (16TiB)
0xffff_9000_0000_0000 - 0xffff_9000_0200_0000 : kernel heap (32 MiB)
0xffff_9000_0200_0000 - 0xffff_fe00_0000_0000 : empty
0xffff_fe00_0000_0000 - 0xffff_ff00_0000_0000 : physical-memory direct map (512GiB reserved)
0xffff_ff00_0000_0000 - 0xffff_ffff_7fcf_f000 : empty
0xffff_ffff_7fcf_f000 - 0xffff_ffff_7fd0_0000 : original kernel guard page (4KiB)
0xffff_ffff_7fd0_0000 - 0xffff_ffff_7ff0_0000 : original kernel stack (2MiB)
0xffff_ffff_7ff0_0000 - 0xffff_ffff_8000_0000 : boot infos (1MiB)
0xffff_ffff_8000_0000 - 0xffff_ffff_ffff_ffff : kernel image (~2GiB)