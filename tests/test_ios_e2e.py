"""The iPhone app's WireKit against the workout app in the emulator (docs/specs/2026-11-13-workout-ios-design.md):
`wp2apps bridge` turns the K channel into hex lines over TCP and `swift test --filter syncOverTcp` syncs through it
(no bind). Needs target/{loader,workout}.wp2app and Xcode's swift. WP2_NETSIM_INSTANCE=62."""
import asyncio
import os
import struct
import subprocess
import threading
import time
import unittest
from unittest import skipUnless

from tests.e2e import harness
from tests.test_back_e2e import tap_row
from tests.test_e2e import START
from tests.test_workout_e2e import ACKED, ELF
from wp2apps import bridge, paths, workout
from tests import ios_vectors

PORT = 7731


@skipUnless(harness.E2E, "set WP2_E2E=1 (needs the emulator, netsimd, gdb and llvm-nm)")
class IosSyncTest(unittest.TestCase):
    def test_sync_over_k(self):
        with harness.installed(["loader", "workout"], name="ios") as (emu, port):
            harness.ready(emu)
            harness.open_apps(emu)
            threading.Thread(target=lambda: asyncio.run(bridge.serve(PORT, hci_port=port)), daemon=True).start()
            time.sleep(1)
            # a clean environment: nix-shell's SDKROOT and compilers break Xcode's swift
            swift = subprocess.Popen(["/usr/bin/swift", "test", "--filter", "syncOverTcp"], cwd=paths.ROOT / "ios" / "WireKit",
                                     env={"HOME": os.environ["HOME"], "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
                                          "WP2_TCP_PORT": str(PORT)}, stdout=subprocess.PIPE,
                                     stderr=subprocess.STDOUT, text=True)
            emu.sleep(20)  # swift builds the tests first
            tap_row(emu, 0)  # open the workout app: it asks with R
            for _ in range(240):
                if swift.poll() is not None:
                    break
                emu.sleep(0.5)
            out = swift.communicate(timeout=30)[0] if swift.poll() is not None else (swift.kill() or "timeout")
            self.assertEqual(swift.returncode, 0, out[-3000:])
            self.assertIn("syncOverTcp() passed", out)
            days = [workout.Day(d["name"], tuple(workout.Exercise(e["name"], e["sets"], (e["repsMin"], e["repsMax"]),
                                                                  e["weight"], e["restS"]) for e in d["exercises"]))
                    for d in ios_vectors.vectors()["days"]]
            stats = harness.u32(emu, START + harness.APP_BLOCK_OFF) + harness.symbol(ELF, "STATS")
            acked = struct.unpack("<5i", emu.peek(stats, 20))[ACKED] & 0xFFFFFFFF
            self.assertEqual(acked, workout.version(workout.encode(days)))
