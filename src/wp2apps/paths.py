import hashlib
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FIRMWARE = Path("dumps/binaries/firmware/JX402_V1.0.0.73.bin")


def cmf_watch_utils() -> Path:
    path = Path(os.environ.get("CMF_WATCH_UTILS") or ROOT.parent / "cmf-watch-utils").expanduser().resolve()
    if not path.is_dir():
        raise FileNotFoundError(f"cmf-watch-utils checkout not found at {path}; set CMF_WATCH_UTILS")
    return path


def firmware() -> Path:
    path = cmf_watch_utils() / FIRMWARE
    if not path.is_file() or path.stat().st_size < 1 << 20:
        raise FileNotFoundError(f"{path} is missing or a Git LFS pointer; run `git lfs pull` in cmf-watch-utils")
    return path


def fwkit(*args) -> str:
    try:
        return subprocess.run(["uv", "run", "--locked", "fwkit", *map(str, args)], cwd=ROOT, check=True,
                              capture_output=True, text=True).stdout
    except subprocess.CalledProcessError as error:
        raise ValueError(f"fwkit {args[0]} failed: {error.stderr.strip() or error.stdout.strip() or error}") from None


def app_from(image: Path) -> bytes:
    """app.bin from a full firmware image (outer AOTA -> TEMP.bin -> app.bin)."""
    with tempfile.TemporaryDirectory() as tmp:
        outer, inner = Path(tmp) / "image", Path(tmp) / "temp"
        fwkit("unpack", image, outer)
        fwkit("unpack", "--no-raw", outer / "raw/TEMP.bin.bin", inner)
        return (inner / "parts/app.bin.part").read_bytes()


def stock_app() -> bytes:
    data = app_from(firmware())
    if not hashlib.sha256(data).hexdigest().startswith("30e7978c39d8fb79"):
        raise ValueError("app.bin does not match JX402 1.0.0.73")
    return data
