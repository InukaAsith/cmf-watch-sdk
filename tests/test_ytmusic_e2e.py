"""YouTube Music (sdk/examples/ytmusic) with its host (hosts/ytmhost.py) on songs from files, in the emulator.

  WP2_E2E=1 uv run --locked python -m unittest tests.test_ytmusic_e2e -v

As a user would: the search tool, "song" typed on the keyboard, Go, the first result. The host streams it and the
next one after it (up next); the recorded speaker (audio.wav) has 440 Hz, then 660 Hz. Then the first again and
the next button while it plays: 660 Hz comes sooner than the song's end. Each stream starts once the watch's
~7 s buffer is full (player.rs FULL). Frames in runs/ytmusic/.
"""
import asyncio, math, os, pathlib, sys, tempfile, threading, time, unittest, wave

from tests.e2e.harness import E2E, installed, open_app
from tests.test_audio_e2e import bursts, pitch
from tests.test_demo_e2e import save_png
from tests.test_throughput_e2e import bind
from wp2apps import paths, play
from wp2apps.channel import Channel

sys.path.insert(0, str(paths.ROOT / "hosts"))
import ytmhost  # noqa: E402

# WP2_YTM_APP=spplayer: the same app as Spotify (examples/spplayer)
APP = os.environ.get("WP2_YTM_APP", "ytmusic")
OUT = paths.ROOT / "runs" / APP
SEARCH, NEXT, PLAY = (193, 392), (329, 312), (233, 312)
ROW0 = (233, 135)
# the QWERTY keys' centres (ui.rs ROW_Y, each_key): s o n g, then Go
KEYS = {"s": (101, 271), "o": (387, 213), "n": (292, 329), "g": (233, 271)}
GO = (349, 387)
SECONDS = 7


def song(path: pathlib.Path, hz: float):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1), w.setsampwidth(2), w.setframerate(play.RATE)
        w.writeframes(b"".join(int(9000 * math.sin(2 * math.pi * hz * i / play.RATE)).to_bytes(2, "little", signed=True)
                               for i in range(SECONDS * play.RATE)))


class Slow(ytmhost.Files):
    """Songs that take 3 s to prepare, as a download would: the watch shows Preparing, its yellow circle."""

    def pcm(self, song, pos):
        time.sleep(3)
        return super().pcm(song, pos)


def tap(emu, xy, pause=0.6):
    emu.send(f"t 1 {xy[0]} {xy[1]}", f"t 0 {xy[0]} {xy[1]}", pause=pause)


def shot(emu, name):
    OUT.mkdir(parents=True, exist_ok=True)
    save_png(OUT / f"{name}.png", *emu.frame())


def ring_fraction(emu) -> float:
    """The red part of the progress ring (radius 226, clockwise from the top)."""
    w, h, rgb = emu.frame()
    red = 0
    for k in range(360):
        a = math.radians(k)
        x, y = int(233 + 226 * math.sin(a)), int(233 - 226 * math.cos(a))
        r, g, b = rgb[(y * w + x) * 3:(y * w + x) * 3 + 3]
        red += r > 100 and r > 3 * g + 40  # the frame is dimmed: red reads ~(139, 15, 20)
    return red / 360


@unittest.skipUnless(E2E, "WP2_E2E=1")
class YtMusicTest(unittest.TestCase):
    def test_search_play_next(self):
        music = pathlib.Path(tempfile.mkdtemp())
        song(music / "Tester - Alpha Song.wav", 440)
        song(music / "Tester - Beta Song.wav", 660)
        song(music / "Else - Gamma.wav", 1000)
        log: list[str] = []
        with installed(["loader", APP], name=APP, wav=True) as (emu, port):
            key = asyncio.run(bind(port))
            open_app(emu, 0)

            def host():
                async def run():
                    ch = Channel(hci_port=port)
                    ch.key = key
                    async with ch:
                        session = await ch.connect()
                        await ytmhost.Host(session, Slow(music), log=lambda *a, **k: log.append(" ".join(map(str, a)))).run()
                try:
                    asyncio.run(run())
                except Exception as e:  # noqa: BLE001 (the emulator going away ends it)
                    log.append(f"host ended: {e!r}")

            threading.Thread(target=host, daemon=True).start()
            emu.sleep(3)
            shot(emu, "1-idle")
            tap(emu, SEARCH)
            for ch in "song":
                tap(emu, KEYS[ch], pause=0.2)
            shot(emu, "2-typed")
            tap(emu, GO, pause=2)
            shot(emu, "3-results")
            self.assertTrue(any("search 'song'" in l for l in log), log)
            tap(emu, ROW0, pause=1)
            shot(emu, "4a-preparing")  # the mark's ring yellow: the phone prepares the song
            emu.sleep(3)
            shot(emu, "4b-buffering")  # red: the watch's buffer filling
            shot(emu, "4-playing")
            # the buffer filling (~6 s at the link's speed), both songs and the end of the queue
            emu.sleep(2 * SECONDS + 16)
            shot(emu, "5-ended")
            # again, and the next button 2 s into it
            tap(emu, SEARCH)
            for ch in "song":
                tap(emu, KEYS[ch], pause=0.2)
            tap(emu, GO, pause=2)
            tap(emu, ROW0, pause=1)
            for _ in range(30):  # until the buffer is full and it plays: the progress ring moves
                emu.sleep(0.5)
                if ring_fraction(emu) > 0:
                    break
            emu.sleep(1)
            t_next = time.monotonic()
            tap(emu, NEXT, pause=0)
            fracs = []
            for k in range(16):
                emu.sleep(1)
                fracs.append(round(ring_fraction(emu), 2))
                if k == 3:
                    shot(emu, "6-next")
            print("ring after next, each guest second:", fracs, flush=True)
            shot(emu, "7-ended")
            uart = emu.uart()
            self.assertNotIn("ZEPHYR FATAL ERROR", uart)
            self.assertEqual(uart.count("*** Booting Zephyr"), 1, "guest rebooted")
        print("\n" + "\n".join(log), flush=True)
        rate, s = bursts(emu.run / "audio.wav")
        n = rate // 5
        tones = []
        for i in range(0, len(s) - n, n):
            w = s[i:i + n]
            if max(abs(x) for x in w) > 1500:
                hz = pitch(w, rate)
                tones.append(min((440, 660, 1000), key=lambda t: abs(t - hz)))
            else:
                tones.append(0)
        runs = []
        for t in tones:
            if runs and runs[-1][0] == t:
                runs[-1][1] += 1
            else:
                runs.append([t, 1])
        runs = [(t, k / 5) for t, k in runs if k >= 3]  # 0.6 s or more
        print("speaker:", runs, flush=True)
        heard = [t for t, _ in runs if t]
        self.assertEqual(heard[:4], [440, 660, 440, 660], runs)
        played = dict(enumerate(d for t, d in runs if t))
        self.assertGreater(played[0], SECONDS - 2, "the first song played to its end")
        self.assertLess(played[2], SECONDS - 2, "next cut the song short")
