#!/usr/bin/env python3
"""Boot Wana OS under QEMU + OVMF with a real display device, take a
screenshot when a log line appears, and check pixel colors in it.

This is the graphical counterpart of qemu-boot-test.sh: the serial log is
checked for --expect patterns as usual, and in addition the *displayed
frame* is read back from QEMU (monitor `screendump`) and verified, so a
test passes only if the pixels on the virtual screen are right.

Example:
  qemu-graphics-test.py --kernel bzImage --initrd rootfs.cpio.zst \\
      --append "wana.run=/usr/bin/wana-kms,--hold,5 wana.test=poweroff" \\
      --gpu virtio --screendump-on 'holding frame' --screendump shot.ppm \\
      --pixel 0.5,0.5=4f8cff --expect '\\[DRM\\] info: modeset done'
"""

import argparse
import os
import re
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
import zlib

OVMF_CANDIDATES = ["/usr/share/OVMF/OVMF_CODE_4M.fd", "/usr/share/OVMF/OVMF_CODE.fd",
                   "/usr/share/edk2/x64/OVMF_CODE.4m.fd"]


def log(level, msg, stream=sys.stdout):
    print(f"[BOOT] {level}: {msg}", file=stream, flush=True)


def read_ppm(path):
    """Minimal binary PPM (P6, maxval 255) reader -> (w, h, bytes)."""
    with open(path, "rb") as f:
        data = f.read()
    fields, pos = [], 0
    while len(fields) < 4:
        while data[pos:pos + 1].isspace():
            pos += 1
        if data[pos:pos + 1] == b"#":
            pos = data.index(b"\n", pos) + 1
            continue
        end = pos
        while not data[end:end + 1].isspace():
            end += 1
        fields.append(data[pos:end])
        pos = end
    if fields[0] != b"P6" or int(fields[3]) != 255:
        raise ValueError(f"unsupported PPM header {fields}")
    w, h = int(fields[1]), int(fields[2])
    return w, h, data[pos + 1:pos + 1 + w * h * 3]


def write_png(path, w, h, rgb):
    """Write RGB bytes as PNG (for humans and CI artifacts)."""
    raw = b"".join(b"\x00" + rgb[y * w * 3:(y + 1) * w * 3] for y in range(h))
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


def parse_pixel(spec):
    """'0.5,0.5=4f8cff' -> (fx, fy, (r, g, b)); fractions of width/height."""
    pos, color = spec.split("=")
    fx, fy = (float(v) for v in pos.split(","))
    c = int(color, 16)
    return fx, fy, ((c >> 16) & 255, (c >> 8) & 255, c & 255)


def monitor(sock_path, command, timeout=10):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect(sock_path)
    s.recv(4096)  # banner
    s.sendall(command.encode() + b"\n")
    time.sleep(0.5)
    try:
        s.recv(4096)
    except socket.timeout:
        pass
    s.close()


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    src = ap.add_mutually_exclusive_group(required=True)
    src.add_argument("--kernel")
    src.add_argument("--disk")
    ap.add_argument("--initrd")
    ap.add_argument("--append", default="")
    ap.add_argument("--gpu", choices=["virtio", "std"], default="virtio")
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--log", default="qemu-graphics.log")
    ap.add_argument("--expect", action="append", default=[])
    ap.add_argument("--screendump-on", help="regex; take the screenshot when a log line matches")
    ap.add_argument("--screendump", help="output .ppm path (a .png is written next to it)")
    ap.add_argument("--pixel", action="append", default=[], help="fx,fy=RRGGBB (repeatable)")
    ap.add_argument("--tolerance", type=int, default=8)
    args = ap.parse_args()

    ovmf = next((p for p in OVMF_CANDIDATES if os.path.isfile(p)), None)
    if not ovmf:
        log("error", "OVMF firmware not found (install ovmf)", sys.stderr)
        return 2
    tmp = tempfile.mkdtemp(prefix="wana-gfx-")
    vars_fd = os.path.join(tmp, "vars.fd")
    shutil.copy(ovmf.replace("CODE", "VARS"), vars_fd)
    mon = os.path.join(tmp, "monitor.sock")
    accel = "kvm" if os.access("/dev/kvm", os.W_OK) else "tcg"

    cmd = ["qemu-system-x86_64", "-machine", f"q35,accel={accel}", "-m", "1024", "-smp", "2",
           "-no-reboot", "-display", "none", "-serial", "stdio",
           "-monitor", f"unix:{mon},server,nowait",
           "-drive", f"if=pflash,format=raw,readonly=on,file={ovmf}",
           "-drive", f"if=pflash,format=raw,file={vars_fd}"]
    cmd += ["-vga", "none", "-device", "virtio-gpu-pci"] if args.gpu == "virtio" else ["-vga", "std"]
    if args.disk:
        cmd += ["-drive", f"file={args.disk},if=virtio,format=raw,snapshot=on"]
    else:
        cmd += ["-kernel", args.kernel, "-append", f"console=ttyS0 panic=-1 {args.append}"]
        if args.initrd:
            cmd += ["-initrd", args.initrd]

    log("info", f"qemu accel={accel} gpu={args.gpu} firmware={ovmf} timeout={args.timeout}s")
    proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT)
    os.set_blocking(proc.stdout.fileno(), False)
    deadline = time.time() + args.timeout
    text, pending, shot_taken = b"", b"", False
    trigger = re.compile(args.screendump_on.encode()) if args.screendump_on else None
    with open(args.log, "wb") as logf:
        while proc.poll() is None and time.time() < deadline:
            chunk = proc.stdout.read() or b""
            if not chunk:
                time.sleep(0.05)
                continue
            logf.write(chunk)
            logf.flush()
            text += chunk
            pending += chunk
            lines = pending.split(b"\n")
            pending = lines.pop()
            for line in lines:
                if trigger and not shot_taken and trigger.search(line):
                    monitor(mon, f"screendump {os.path.abspath(args.screendump)}")
                    shot_taken = True
                    log("info", f"screendump taken -> {args.screendump}")
        if proc.poll() is None:
            log("info", f"timeout reached after {args.timeout}s")
            proc.kill()
        rest = proc.stdout.read() or b""
        logf.write(rest)
        text += rest
    shutil.rmtree(tmp, ignore_errors=True)

    fail = False
    out = text.decode(errors="replace")
    for pattern in args.expect:
        if re.search(pattern, out, re.M):
            log("info", f"found: {pattern}")
        else:
            log("error", f"missing: {pattern}", sys.stderr)
            fail = True

    if args.screendump:
        if not shot_taken or not os.path.isfile(args.screendump):
            log("error", "no screenshot was taken (trigger line never appeared)", sys.stderr)
            fail = True
        else:
            w, h, rgb = read_ppm(args.screendump)
            png = os.path.splitext(args.screendump)[0] + ".png"
            write_png(png, w, h, rgb)
            log("info", f"screenshot {w}x{h} -> {png}")
            for spec in args.pixel:
                fx, fy, want = parse_pixel(spec)
                x, y = min(int(fx * w), w - 1), min(int(fy * h), h - 1)
                got = tuple(rgb[(y * w + x) * 3:(y * w + x) * 3 + 3])
                ok = all(abs(a - b) <= args.tolerance for a, b in zip(got, want))
                msg = f"pixel ({x},{y}) = #{bytes(got).hex()} expected #{bytes(want).hex()}"
                if ok:
                    log("info", msg)
                else:
                    log("error", msg, sys.stderr)
                    fail = True

    if fail:
        print("[BOOT] last 25 console lines:", file=sys.stderr)
        print("\n".join(out.splitlines()[-25:]), file=sys.stderr)
        print(f"[BOOT] graphics test: FAIL (log: {args.log})", file=sys.stderr)
        return 1
    print(f"[BOOT] graphics test: PASS (log: {args.log})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
