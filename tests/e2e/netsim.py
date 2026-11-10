import asyncio
from contextlib import contextmanager
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import time

from wp2apps import h4, link, paths

from . import harness

NETSIMD = Path(os.environ.get("WP2_NETSIMD") or shutil.which("netsimd") or "~/Library/Android/sdk/emulator/netsimd")
PAIRED = harness.RUNS / "paired-nor.bin"
INSTANCE = os.environ.get("WP2_NETSIM_INSTANCE", "71")  # one per agent: two netsimd on one instance exit


@contextmanager
def netsim():
    """netsimd with a free H4 port; yields the port."""
    port = harness.free_port()
    proc = subprocess.Popen([str(NETSIMD.expanduser()), "--instance", INSTANCE, "--hci-port", str(port),
                             "--no-cli-ui", "--no-web-ui"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if proc.poll() is not None:
                raise RuntimeError(f"netsimd exited with {proc.returncode}")
            # netsimd quits when its first client disconnects, so probing the port by connecting would kill it
            if subprocess.run(["lsof", "-nP", "-a", "-p", str(proc.pid), f"-iTCP:{port}", "-sTCP:LISTEN"],
                              capture_output=True).returncode == 0:
                break
            time.sleep(0.1)
        else:
            raise TimeoutError("netsimd did not open its HCI port")
        yield port
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()


async def provision(port: int) -> None:
    async with h4.gatt_over_h4(port) as gatt:
        session = link.Session()
        await session.attach(gatt)
        secret = secrets.token_hex(16)
        reply = await session.at(f"AT SETSECRET {secret}")
        if b"SETSECRET,OK" not in reply or secret.encode() not in await session.at("AT GETSECRET"):
            raise RuntimeError(f"stock did not keep the synthetic secret: {reply!r}")


def paired_nor() -> bytes:
    """Stock NOR with a synthetic pairing secret, made once over our own H4 client (the cmf-watch-utils
    pair-fixture command timed out against the same guest)."""
    if not PAIRED.is_file():
        PAIRED.parent.mkdir(parents=True, exist_ok=True)
        size = len(harness.stock_nor())
        with netsim() as port, harness.Emu("pair", nor=harness.stock_nor(), hci_port=port) as emu:
            emu.wait_uart("launcher enter", 90)
            emu.keep_awake()
            asyncio.run(provision(port))
            emu.qmp.request("stop")
            partial = PAIRED.with_suffix(".part")
            emu.qmp.request("pmemsave", val=0x11000000, size=size, filename=str(partial))
            for _ in range(100):
                if partial.is_file() and partial.stat().st_size == size:
                    break
                time.sleep(0.1)
            partial.rename(PAIRED)
    return PAIRED.read_bytes()
