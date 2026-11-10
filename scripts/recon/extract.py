import hashlib
from pathlib import Path

from fwkit import aota
from fwkit.image import Image

from wp2apps import paths

out = paths.ROOT / "runs" / "recon"
out.mkdir(parents=True, exist_ok=True)
temp = Image.open(str(paths.firmware())).decompressed("TEMP.bin")
_, entries = aota.parse(temp)
for entry in entries:
    if entry.name in ("app.bin", "sdfs.bin"):
        data = entry.stored(temp)
        (out / entry.name).write_bytes(data)
        print(entry.name, len(data), hashlib.sha256(data).hexdigest()[:16])
