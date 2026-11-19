"""A windowed emulator to look at: paired watch on Netsim, new loader and the apps installed, left running.

  WP2_NETSIM_INSTANCE=75 WP2_QEMU=... uv run --locked python -m tests.e2e.show loader maps chats assistant nav pixview
"""
import os, sys, time

from tests.e2e import harness
from tests.e2e.netsim import netsim
from wp2apps import paths

os.environ["WP2_GUI"] = "1"
names = sys.argv[1:] or ["loader"]
nor = harness.patched_paired_nor()
with netsim() as port, harness.Emu("show", nor=nor, hci_port=port, seconds=12 * 3600) as emu:
    emu.wait_uart("launcher enter", 120)
    emu.keep_awake()
    for n in names:
        harness.cli_out(["install", paths.ROOT / "target" / f"{n}.wp2app", "--hci-port", port])
        print("installed", n, flush=True)
    print(f"READY hci-port {port} run {emu.run}", flush=True)
    while emu.vm.poll() is None:
        time.sleep(5)
