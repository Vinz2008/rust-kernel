.section .boot, "awx"
.code16

# ControllerInfo struct
.set VBE_INFO_SIGNATURE,          0
.set VBE_INFO_VERSION,            4
.set VBE_INFO_OEM_PTR,            6
.set VBE_INFO_CAPABILITIES,      10
.set VBE_INFO_MODES_OFFSET,      14
.set VBE_INFO_MODES_SEGMENT,     16
.set VBE_INFO_TOTAL_MEMORY,      18
.set VBE_INFO_SIZE,             512

# ModeInfo struct
.set VBE_MODE_ATTRIBUTES,         0
.set VBE_MODE_PITCH,             16
.set VBE_MODE_WIDTH,             18
.set VBE_MODE_HEIGHT,            20
.set VBE_MODE_BPP,               25
.set VBE_MODE_MEMORY_MODEL,      27
.set VBE_MODE_RED_MASK_SIZE,     31
.set VBE_MODE_RED_POSITION,      32
.set VBE_MODE_GREEN_MASK_SIZE,   33
.set VBE_MODE_GREEN_POSITION,    34
.set VBE_MODE_BLUE_MASK_SIZE,    35
.set VBE_MODE_BLUE_POSITION,     36
.set VBE_MODE_PHYS_BASE,         40
.set VBE_MODE_INFO_SIZE,        256
    
# TODO : need to get it to the rust part (the vbe infos like width, height, bpp, etc)

config_video_mode:
    # get ControllerInfo struct
    mov dword ptr [vbe_info + VBE_INFO_SIGNATURE], 0x32454256

    mov ax, 0
    mov es, ax
    mov di, offset vbe_info
    
    mov ax, 0x4f00
    int 0x10
    
    cmp ax, 0x004f
    jne .vbe_failed


    # get ModeInfo struct
    # TODO : need cx and es
    mov si, word ptr [vbe_info + VBE_INFO_MODES_OFFSET]
    mov ax, word ptr [vbe_info + VBE_INFO_MODES_SEGMENT]
    mov fs, ax

.find_mode_loop:
    # fs:si points to array of u16 mode numbers

    mov cx, word ptr fs:[si]
    add si, 2

    cmp cx, 0xffff # no modes
    je .vbe_failed

    mov word ptr [vbe_current_mode], cx

    mov ax, 0
    mov es, ax
    mov di, offset vbe_mode_info

    push si
    push fs

    mov ax, 0x4f01
    int 0x10

    pop fs
    pop si

    cmp ax, 0x004f
    jne .find_mode_loop

    # Mode must be supported
    test word ptr [vbe_mode_info + VBE_MODE_ATTRIBUTES], 0x0001
    jz .find_mode_loop

    # Graphics mode (not text mode)
    test word ptr [vbe_mode_info + VBE_MODE_ATTRIBUTES], 0x0010
    jz .find_mode_loop

    # Linear framebuffer available
    test word ptr [vbe_mode_info + VBE_MODE_ATTRIBUTES], 0x0080
    jz .find_mode_loop

    # For now, just look for exactly 1024x768x32. (TODO ?)
    cmp word ptr [vbe_mode_info + VBE_MODE_WIDTH], 1024
    jne .find_mode_loop
    cmp word ptr [vbe_mode_info + VBE_MODE_HEIGHT], 768
    jne .find_mode_loop
    cmp byte ptr [vbe_mode_info + VBE_MODE_BPP], 32
    jne .find_mode_loop

    # Direct-color mode. (TODO  WHAT IS THIS)
    cmp byte ptr [vbe_mode_info + VBE_MODE_MEMORY_MODEL], 6
    jne .find_mode_loop

    mov bx, word ptr [vbe_current_mode]
    or bx, 0x4000

    mov ax, 0x4f02
    int 0x10

    cmp ax, 0x004f
    jne .vbe_failed

    ret

    .vbe_failed:
        # TODO could use fallback to vga char mode instead
        jmp .vbe_failed

.align 16
vbe_info:
    .space VBE_INFO_SIZE

vbe_mode_info:
    .space VBE_MODE_INFO_SIZE

vbe_current_mode:
    .word 0

.code32

# TODO : vga_println (use the BIOS writing chars in real mode, like in vga_320x200, no need for a font here)
# print a string and a newline
# IN
#   esi: points at zero-terminated String
vga_println:
    ret

vga_map_frame_buffer:
# need nothing, will just use the physmap
    ret