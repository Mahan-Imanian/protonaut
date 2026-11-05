#!/usr/bin/env python3
"""Rewrites every frame delay in a GIF. Usage: gif-speed.py FILE FACTOR"""
import sys


def skip_sub_blocks(data, i):
    while data[i]:
        i += data[i] + 1
    return i + 1


def speed_up(data, factor):
    data = bytearray(data)
    packed = data[10]
    i = 13 + (3 << ((packed & 7) + 1) if packed & 0x80 else 0)
    delays = []
    while data[i] != 0x3B:
        if data[i] == 0x21:
            if data[i + 1] == 0xF9:
                delay = int.from_bytes(data[i + 4 : i + 6], "little")
                new = max(2, round(delay / factor))
                data[i + 4 : i + 6] = new.to_bytes(2, "little")
                delays.append((delay, new))
            i = skip_sub_blocks(data, i + 2)
        elif data[i] == 0x2C:
            packed = data[i + 9]
            i += 10 + (3 << ((packed & 7) + 1) if packed & 0x80 else 0) + 1
            i = skip_sub_blocks(data, i)
        else:
            raise ValueError(f"unexpected GIF block {data[i]:#x} at {i}")
    return bytes(data), delays


if __name__ == "__main__":
    path, factor = sys.argv[1], float(sys.argv[2])
    with open(path, "rb") as f:
        out, delays = speed_up(f.read(), factor)
    assert delays, "no frames found"
    with open(path, "wb") as f:
        f.write(out)
    print("delays in centiseconds:", delays)
