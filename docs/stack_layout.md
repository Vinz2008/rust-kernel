# Stack Layout

-- stack top (higher address)
Arg string data (bytes)
u64 0
Args infos (pair 
    ptr u64
    len u64)
Args len u64
random seed (32 bytes)
stack_canary u64
-- real stack start
.. rest of stack
-- stack bottom (lower addresss)