"""The mic through the loader's MIC records (fw/wp2-loader/src/mic.rs) and the Mic lab app.

  WP2_E2E=1 uv run --locked python -m unittest tests.test_mic_e2e -v

The emulator has no mic (it gives 16 kHz mono silence; a real watch gives two channels, which Mic lab sends to
hosts/miclisten.py): this checks that opening the stock audio_record, reading it and closing it neither faults
nor reboots the watch, records or not, and that the button then closes the app.
"""
import unittest

from tests.e2e.harness import E2E, STUB_RAM, installed, open_app


@unittest.skipUnless(E2E, "WP2_E2E=1")
class MicTest(unittest.TestCase):
    def test_record_and_exit(self):
        with installed(["loader", "miclab"], name="mic") as (emu, port):
            open_app(emu, 0)
            emu.send("t 1 233 233", "t 0 233 233", pause=0)  # record 3 s, then play it back
            emu.sleep(14)
            w, h, px = emu.frame()
            (emu.run / "miclab.ppm").write_bytes(f"P6\n{w} {h}\n255\n".encode() + px)
            emu.send("c 1", "c 0", pause=2)  # the button: the app exits
            self.assertEqual(emu.peek(STUB_RAM + 6, 1), b"\xff", "the app still runs")
            uart = emu.uart()
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
            print("\n" + "\n".join(l for l in uart.splitlines() if "record" in l.lower() or "mic" in l.lower())[-3000:])
            print("frame:", emu.run / "miclab.ppm")


if __name__ == "__main__":
    unittest.main()
