import os
from pathlib import Path

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
