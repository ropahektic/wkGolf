#!/usr/bin/env python3
"""Pack wkGolf test zip → /var/lib/wormnet-share/wkGolf.zip + Tailscale URL on :8777."""
from __future__ import annotations

import shutil
import subprocess
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
INCOMING = ROOT / "incoming" / "golf_map.wkgolf.json"
TARGET = ROOT / "target" / "i686-pc-windows-gnu" / "release" / "wkGolf.dll"
SHARE = Path("/var/lib/wormnet-share")
ZIP_NAME = "wkGolf.zip"
SERVE_PORT = 8777


def tailscale_url(path: str) -> str | None:
    try:
        ip = subprocess.check_output(["tailscale", "ip", "-4"], text=True).strip()
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
    return f"http://{ip}:{SERVE_PORT}/{path}" if ip else None


def ensure_map() -> None:
    if INCOMING.is_file():
        subprocess.run(
            [
                "python3",
                str(ROOT / "scripts" / "export_wkgolf_project.py"),
                str(INCOMING),
                "--out-dir",
                str(DIST),
                "--grass-fill",
            ],
            check=True,
        )
        return
    raw = ROOT / "incoming" / "GolfTest1_raw.png"
    script = "prepare_golf_map.py" if raw.is_file() else "make_golf_map.py"
    subprocess.run(["python3", str(ROOT / "scripts" / script)], check=True)


def pack_zip() -> Path:
    if not TARGET.is_file():
        raise SystemExit(f"missing {TARGET} — run cargo build first")
    DIST.mkdir(parents=True, exist_ok=True)
    shutil.copy2(TARGET, DIST / "wkGolf.dll")
    map_png = DIST / "golf_map.png"
    if not map_png.is_file():
        map_png = DIST / "GolfTest1.png"
    if not (DIST / "golf.toml").is_file():
        raise SystemExit("missing golf.toml in dist/")
    install = DIST / "INSTALL.txt"
    if not install.is_file():
        install = ROOT / "README.txt"

    stubs = ROOT / "wine_stubs"
    stub_files = [
        ("api-ms-win-core-synch-l1-2-0.dll", stubs / "api-ms-win-core-synch-l1-2-0.dll"),
        ("bcryptprimitives.dll", stubs / "bcryptprimitives.dll"),
    ]
    for _, src in stub_files:
        if not src.is_file():
            raise SystemExit(f"missing Wine stub {src} — build wine_stubs/*.c first")

    zpath = SHARE / ZIP_NAME
    with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED) as z:
        for name, src in (
            ("wkGolf.dll", DIST / "wkGolf.dll"),
            ("golf.toml", DIST / "golf.toml"),
            (map_png.name, map_png),
            ("INSTALL.txt", install),
            *stub_files,
        ):
            z.write(src, name)
            print(f"zip+ {name}")
    shutil.copy2(zpath, DIST / ZIP_NAME)
    return zpath


def ensure_http_server() -> None:
    try:
        out = subprocess.check_output(["pgrep", "-af", f"http.server {SERVE_PORT}"], text=True)
        if "wormnet-share" in out or str(SHARE) in out:
            return
    except subprocess.CalledProcessError:
        pass
    subprocess.Popen(
        ["python3", "-m", "http.server", str(SERVE_PORT), "--bind", "0.0.0.0"],
        cwd=str(SHARE),
        stdout=open("/tmp/wkGolf-http.log", "w"),
        stderr=subprocess.STDOUT,
    )


def main() -> None:
    ensure_map()
    zpath = pack_zip()
    ensure_http_server()
    print(f"OK {zpath} ({zpath.stat().st_size} bytes)")
    url = tailscale_url(ZIP_NAME)
    if url:
        print(f"DOWNLOAD={url}")


if __name__ == "__main__":
    main()
