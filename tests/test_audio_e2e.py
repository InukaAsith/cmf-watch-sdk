"""The speaker through the loader's AUDIO record (fw/wp2-loader/src/audio.rs) and the Speaker app.

  WP2_E2E=1 uv run --locked python -m unittest tests.test_audio_e2e -v

Streams 1 s of 440 Hz then 1 s of 660 Hz as IMA ADPCM (wp2apps.play) to sdk/examples/speaker and checks the
emulator's recorded speaker (audio.wav): both tones, in that order. Then the button closes the app: the track is
destroyed with it and the watch keeps running.
"""
import asyncio, math, struct, unittest, wave

from tests.e2e.harness import E2E, STUB_RAM, installed, open_app
from tests.test_throughput_e2e import bind
from wp2apps import play
from wp2apps.channel import Channel


def tone(hz: float, seconds: float) -> list[int]:
    return [int(9000 * math.sin(2 * math.pi * hz * i / play.RATE)) for i in range(int(seconds * play.RATE))]


def bursts(path):
    """(rate, mono samples) of the recording."""
    with wave.open(str(path)) as w:
        rate, ch = w.getframerate(), w.getnchannels()
        raw = w.readframes(w.getnframes())
    return rate, struct.unpack(f"<{len(raw) // 2}h", raw)[::ch]


def pitch(s, rate) -> float:
    zc = sum(1 for k in range(1, len(s)) if (s[k - 1] < 0) != (s[k] < 0))
    return zc / 2 / (len(s) / rate)


@unittest.skipUnless(E2E, "WP2_E2E=1")
class AudioTest(unittest.TestCase):
    def test_tones(self):
        with installed(["loader", "speaker"], name="audio", wav=True) as (emu, port):
            key = asyncio.run(bind(port))
            open_app(emu, 0)

            async def run():
                ch = Channel(hci_port=port)
                ch.key = key
                async with ch:
                    session = await ch.connect()
                    return await play.play(session, tone(440, 1.0) + tone(660, 1.0), "Two tones", volume=100,
                                           log=lambda *a, **k: None)

            result = asyncio.run(run())
            print(f"\n{result}", flush=True)
            emu.send("c 1", "c 0", pause=2)  # the button: the app exits
            self.assertEqual(emu.peek(STUB_RAM + 6, 1), b"\xff", "the app still runs")
            uart = emu.uart()
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
            self.assertNotIn("audio track init failed", uart)

        rate, s = bursts(emu.run / "audio.wav")
        # 20 ms windows with sound in them (an underrun leaves silence between), in order
        n = rate // 50
        windows = [s[i:i + n] for i in range(0, len(s) - n, n) if max(abs(x) for x in s[i:i + n]) > 1500]
        played = len(windows) / 50
        print(f"speaker: {played:.2f} s with sound of {len(s) / rate:.2f} s recorded", flush=True)
        self.assertGreater(played, 1.5, "most of the 2 s should play")
        # the first and last 0.4 s of sound: 440 Hz, then 660 Hz
        first = sum(pitch(w, rate) for w in windows[1:21]) / 20
        last = sum(pitch(w, rate) for w in windows[-21:-1]) / 20
        print(f"pitch: {first:.0f} Hz then {last:.0f} Hz", flush=True)
        self.assertAlmostEqual(first, 440, delta=25)
        self.assertAlmostEqual(last, 660, delta=35)
