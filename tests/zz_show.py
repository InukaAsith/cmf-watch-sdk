"""Live emulator window (WP2_GUI=1): patched paired watch, loader and apps installed, runs until the window closes."""
import os, sys, time
os.environ["WP2_GUI"] = "1"
from tests.e2e import harness
names = sys.argv[1:] or ["loader", "pixview", "spotify", "photos"]
from tests.e2e.netsim import netsim
from tests.e2e.harness import Emu, patched_paired_nor, cli_out
from wp2apps import paths
with netsim() as port, Emu("show", nor=patched_paired_nor(), hci_port=port, seconds=3600) as emu:
    emu.wait_uart("launcher enter", 120)
    emu.keep_awake()
    for n in names:
        print("installing", n, flush=True)
        cli_out(["install", paths.ROOT / "target" / f"{n}.wp2app", "--hci-port", port])
    print("READY hci port", port, "run", emu.run, flush=True)
    emu.vm.wait()
