from contextlib import contextmanager
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time

from wp2apps import paths

from . import harness

NETSIMD = Path(os.environ.get("WP2_NETSIMD") or shutil.which("netsimd") or "~/Library/Android/sdk/emulator/netsimd")
PAIRED = harness.RUNS / "paired-nor.bin"


@contextmanager
def netsim():
    """netsimd with a free H4 port; yields the port."""
    port = harness.free_port()
    proc = subprocess.Popen([str(NETSIMD.expanduser()), "--instance", "71", "--hci-port", str(port),
                             "--no-cli-ui", "--no-web-ui"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(100):
            if proc.poll() is not None:
                raise RuntimeError(f"netsimd exited with {proc.returncode}")
            try:
                socket.create_connection(("127.0.0.1", port), timeout=0.2).close()
                break
            except OSError:
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


def paired_nor() -> bytes:
    """Stock NOR with a synthetic pairing secret; made once with the external cmf-watch-utils CLI."""
    if not PAIRED.is_file():
        PAIRED.parent.mkdir(parents=True, exist_ok=True)
        with netsim() as port, harness.Emu("pair", nor=harness.stock_nor(), hci_port=port) as emu:
            subprocess.run(["uv", "run", "--locked", "cmf-watch-utils", "emu", "pair-fixture",
                            "--hci-port", str(port), "--qmp-port", str(emu.qmp_port), "--output", str(PAIRED)],
                           cwd=paths.ROOT, check=True)
    return PAIRED.read_bytes()
