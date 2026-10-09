#!/usr/bin/env python3
"""Dump the WASM import section of a module: (module, name) pairs.
Evidence for C3/C4: the guest's entire ambient surface is what it imports."""
import json
import struct
import sys


def read_uleb(data, pos):
    result = 0
    shift = 0
    while True:
        byte = data[pos]
        pos += 1
        result |= (byte & 0x7F) << shift
        if not (byte & 0x80):
            return result, pos
        shift += 7


def read_name(data, pos):
    length, pos = read_uleb(data, pos)
    return data[pos:pos + length].decode("utf-8", "replace"), pos + length


def imports(path):
    with open(path, "rb") as f:
        data = f.read()
    if data[:4] != b"\x00asm":
        raise SystemExit("not a wasm file")
    pos = 8
    section_id = None
    while pos < len(data):
        section_id = data[pos]
        pos += 1
        size, pos = read_uleb(data, pos)
        end = pos + size
        if section_id == 2:  # import section
            count, pos = read_uleb(data, pos)
            out = []
            for _ in range(count):
                module, pos = read_name(data, pos)
                name, pos = read_name(data, pos)
                kind = data[pos]
                pos += 1
                out.append({"module": module, "name": name, "kind": kind})
            return out
        pos = end
    return []


if __name__ == "__main__":
    path = sys.argv[1]
    result = imports(path)
    print(json.dumps({"module": path, "imports": result}, indent=2))