"""Takes the digits 0-9 out of a TrueType font's character map, so text
set in it shows numbers in the fallback font. Usage: strip_digits.py in.ttf out.ttf"""
import struct, sys

def checksum(data):
    data += b"\0" * (-len(data) % 4)
    return sum(struct.unpack(">%dI" % (len(data) // 4), data)) & 0xFFFFFFFF

def read_cmap(cmap):
    count = struct.unpack(">H", cmap[2:4])[0]
    best = None
    for i in range(count):
        plat, enc, off = struct.unpack(">HHI", cmap[4 + 8 * i:12 + 8 * i])
        fmt = struct.unpack(">H", cmap[off:off + 2])[0]
        rank = {(3, 10, 12): 3, (0, 4, 12): 3, (0, 6, 12): 3, (3, 1, 4): 2, (0, 3, 4): 2}.get((plat, enc, fmt), 0)
        if rank and (best is None or rank > best[0]):
            best = (rank, fmt, off)
    assert best, "no usable character map"
    _, fmt, off = best
    chars = {}
    if fmt == 12:
        groups = struct.unpack(">I", cmap[off + 12:off + 16])[0]
        for g in range(groups):
            start, end, gid = struct.unpack(">III", cmap[off + 16 + 12 * g:off + 28 + 12 * g])
            for cp in range(start, end + 1):
                chars[cp] = gid + cp - start
    else:
        segx2 = struct.unpack(">H", cmap[off + 6:off + 8])[0]
        n = segx2 // 2
        ends = struct.unpack(">%dH" % n, cmap[off + 14:off + 14 + segx2])
        base = off + 16 + segx2
        starts = struct.unpack(">%dH" % n, cmap[base:base + segx2])
        deltas = struct.unpack(">%dh" % n, cmap[base + segx2:base + 2 * segx2])
        ro_at = base + 2 * segx2
        offsets = struct.unpack(">%dH" % n, cmap[ro_at:ro_at + segx2])
        for s in range(n):
            for cp in range(starts[s], ends[s] + 1):
                if cp == 0xFFFF:
                    continue
                if offsets[s] == 0:
                    gid = (cp + deltas[s]) & 0xFFFF
                else:
                    at = ro_at + 2 * s + offsets[s] + 2 * (cp - starts[s])
                    gid = struct.unpack(">H", cmap[at:at + 2])[0]
                    if gid:
                        gid = (gid + deltas[s]) & 0xFFFF
                if gid:
                    chars[cp] = gid
    return chars

def runs(chars):
    """Stretches of code points in a row whose glyphs are in a row too."""
    out = []
    for cp in sorted(chars):
        if out and cp == out[-1][1] + 1 and chars[cp] == out[-1][2] + cp - out[-1][0]:
            out[-1][1] = cp
        else:
            out.append([cp, cp, chars[cp]])
    return out

def write_cmap(chars):
    bmp = runs({cp: g for cp, g in chars.items() if cp < 0xFFFF})
    segs = bmp + [[0xFFFF, 0xFFFF, 1]]
    n = len(segs)
    search = 2 * (1 << (n.bit_length() - 1))
    f4 = struct.pack(">HHHHHHH", 4, 16 + 8 * n, 0, 2 * n, search, (n.bit_length() - 1), 2 * n - search)
    f4 += struct.pack(">%dH" % n, *[s[1] for s in segs]) + b"\0\0"
    f4 += struct.pack(">%dH" % n, *[s[0] for s in segs])
    f4 += struct.pack(">%dH" % n, *[(s[2] - s[0]) & 0xFFFF for s in segs])
    f4 += struct.pack(">%dH" % n, *[0] * n)
    tables = [(0, 3, f4), (3, 1, f4)]
    if any(cp > 0xFFFF for cp in chars):
        groups = runs(chars)
        f12 = struct.pack(">HHIII", 12, 0, 16 + 12 * len(groups), 0, len(groups))
        f12 += b"".join(struct.pack(">III", s, e, g) for s, e, g in groups)
        tables += [(0, 4, f12), (3, 10, f12)]
    tables.sort(key=lambda t: (t[0], t[1]))
    head = struct.pack(">HH", 0, len(tables))
    at, body, seen = 4 + 8 * len(tables), b"", {}
    for plat, enc, data in tables:
        if id(data) not in seen:
            seen[id(data)] = at + len(body)
            body += data
        head += struct.pack(">HHI", plat, enc, seen[id(data)])
    return head + body

def main(src, dst):
    font = open(src, "rb").read()
    version, count = struct.unpack(">IH", font[:6])
    tables = []
    for i in range(count):
        tag, _, off, length = struct.unpack(">4sIII", font[12 + 16 * i:28 + 16 * i])
        tables.append([tag, font[off:off + length]])
    for table in tables:
        if table[0] == b"cmap":
            chars = read_cmap(table[1])
            before = len(chars)
            for cp in range(0x30, 0x3A):
                chars.pop(cp, None)
            table[1] = write_cmap(chars)
            assert read_cmap(table[1]) == chars
        if table[0] == b"head":
            table[1] = table[1][:8] + b"\0\0\0\0" + table[1][12:]
    n = len(tables)
    search = 16 * (1 << (n.bit_length() - 1))
    out = struct.pack(">IHHHH", version, n, search, n.bit_length() - 1, 16 * n - search)
    at, records, body = 12 + 16 * n, b"", b""
    head_at = None
    for tag, data in tables:
        if tag == b"head":
            head_at = at + len(body)
        records += struct.pack(">4sIII", tag, checksum(data), at + len(body), len(data))
        body += data + b"\0" * (-len(data) % 4)
    out = out + records + body
    adjust = (0xB1B0AFBA - checksum(out)) & 0xFFFFFFFF
    out = out[:head_at + 8] + struct.pack(">I", adjust) + out[head_at + 12:]
    open(dst, "wb").write(out)
    print(dst.split("/")[-1], "characters", before, "->", len(chars))

main(sys.argv[1], sys.argv[2])
