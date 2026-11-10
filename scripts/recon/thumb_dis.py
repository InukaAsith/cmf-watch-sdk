"""Disassemble Thumb code of runs/recon/app.bin: thumb_dis.py ADDR [COUNT]."""
import sys
from pathlib import Path

import capstone

B = 0x10000000
img = (Path(__file__).resolve().parents[2] / "runs/recon/app.bin").read_bytes()
md = capstone.Cs(capstone.CS_ARCH_ARM, capstone.CS_MODE_THUMB | capstone.CS_MODE_MCLASS)
addr, count = int(sys.argv[1], 16) & ~1, int(sys.argv[2]) if len(sys.argv) > 2 else 30
for ins in list(md.disasm(img[addr - B: addr - B + count * 4], addr))[:count]:
    print(f"{ins.address:#x} {ins.mnemonic} {ins.op_str}")
