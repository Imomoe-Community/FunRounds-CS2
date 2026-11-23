; Copyright (C) 2026 Mai Ooizumi
; SPDX-License-Identifier: AGPL-3.0-or-later

option casemap:none

PUBLIC FunRoundsInvokeIgnite

.code
FunRoundsInvokeIgnite PROC FRAME
    sub rsp, 48h
    .allocstack 48h
    .endprolog

    ; Gamedata-provided vtable slot arrives in R9; mode remains in R8D.
    mov r10, rcx
    mov r11, qword ptr [r10]
    mov rax, qword ptr [r11+r9*8]

    ; Win64 shadow space plus four engine stack arguments.
    mov qword ptr [rsp+20h], 0
    lea rdx, [rsp+30h]
    mov qword ptr [rsp+28h], rdx
    mov r9, 07E37E43C8800759Fh
    mov qword ptr [rsp+30h], r9
    mov qword ptr [rsp+38h], 07F7FFFFFh
    movsd xmm0, qword ptr [rsp+30h]

    mov rcx, r10
    mov rdx, r11
    ; The caller supplies mode 14 in R8D and duration in XMM1.
    xor r9d, r9d
    xorps xmm3, xmm3
    call rax
    add rsp, 48h
    ret
FunRoundsInvokeIgnite ENDP
END
