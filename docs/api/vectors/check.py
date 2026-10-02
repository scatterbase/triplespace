#!/usr/bin/env python3
"""Regenerates and checks the derived values in log-v1.json, independently of the Rust crate.

    python3 docs/api/vectors/check.py            # check: exit 1 on any mismatch
    python3 docs/api/vectors/check.py --write    # regenerate the `expect` sections

The canonical CBOR encoder here is written from RFC 8949 §4.2.1 and payloads.md §1, not
taken from a library, so that it and crates/scatter-log are two readings of the same
text. If `cbor2` is installed, every encoding is also decoded with it and compared.
"""

import hashlib
import json
import math
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
VECTORS = HERE / "log-v1.json"

TAG_LEAF, TAG_NODE, TAG_COMMITMENT, TAG_CONTENT, TAG_PART, TAG_SIGNATURE, TAG_INSTANCE = range(7)


# --- canonical CBOR -----------------------------------------------------------


def head(major, arg):
    if arg < 24:
        return bytes([major << 5 | arg])
    if arg <= 0xFF:
        return bytes([major << 5 | 24, arg])
    if arg <= 0xFFFF:
        return bytes([major << 5 | 25]) + arg.to_bytes(2, "big")
    if arg <= 0xFFFF_FFFF:
        return bytes([major << 5 | 26]) + arg.to_bytes(4, "big")
    if arg <= 0xFFFF_FFFF_FFFF_FFFF:
        return bytes([major << 5 | 27]) + arg.to_bytes(8, "big")
    raise ValueError(f"integer out of range: {arg}")


def encode_float(x):
    if math.isnan(x):
        return b"\xf9\x7e\x00"
    for fmt, prefix in (("e", b"\xf9"), ("f", b"\xfa")):
        try:
            packed = struct.pack(">" + fmt, x)
        except (OverflowError, struct.error):
            continue
        back = struct.unpack(">" + fmt, packed)[0]
        # Same value and same sign of zero: compare the double's bits.
        if struct.pack(">d", back) == struct.pack(">d", x):
            return prefix + packed
    return b"\xfb" + struct.pack(">d", x)


def encode(v):
    if v is None:
        return b"\xf6"
    if v is True:
        return b"\xf5"
    if v is False:
        return b"\xf4"
    if isinstance(v, int):
        return head(0, v) if v >= 0 else head(1, -1 - v)
    if isinstance(v, float):
        return encode_float(v)
    if isinstance(v, bytes):
        return head(2, len(v)) + v
    if isinstance(v, str):
        b = v.encode("utf-8")
        return head(3, len(b)) + b
    if isinstance(v, list):
        return head(4, len(v)) + b"".join(encode(x) for x in v)
    if isinstance(v, dict):
        pairs = sorted((encode(k), encode(val)) for k, val in v.items())
        for (a, _), (b, _) in zip(pairs, pairs[1:]):
            if a == b:
                raise ValueError("duplicate key")
        return head(5, len(pairs)) + b"".join(k + val for k, val in pairs)
    raise TypeError(type(v))


# --- hashes --------------------------------------------------------------------


def h(tag, *parts):
    d = hashlib.sha256(bytes([tag]))
    for p in parts:
        d.update(p)
    return d.digest()


def header_cbor(hd, commitment):
    return encode(
        [
            hd["version"],
            hd["partition"],
            hd["offset"],
            hd["appended_at"],
            hd["payload_type"],
            hd["key"],
            commitment,
            hd["revid"],
            hd["logid"],
            hd["page_id"],
        ]
    )


def derive_record(rec):
    parts = [(bytes.fromhex(p["salt"]), encode(p["json"])) for p in rec["parts"]]
    leaves = [h(TAG_PART, bytes([i]), salt, b) for i, (salt, b) in enumerate(parts)]
    commitment = h(TAG_COMMITMENT, *leaves)
    hdr = header_cbor(rec["header"], commitment)
    content, comment = parts[0][1], parts[1][1]
    body = [[salt, b] for salt, b in parts]
    erased = [
        [None, leaves[i]] if i in rec.get("erase", []) else [salt, b]
        for i, (salt, b) in enumerate(parts)
    ]
    return {
        "part_cbor": [b.hex() for _, b in parts],
        "part_leaves": [l.hex() for l in leaves],
        "commitment": commitment.hex(),
        "header_cbor": hdr.hex(),
        "leaf": h(TAG_LEAF, hdr).hex(),
        "content_hash": h(TAG_CONTENT, content).hex(),
        "signature_preimage": h(TAG_SIGNATURE, h(TAG_CONTENT, content), h(TAG_CONTENT, comment)).hex(),
        "record_cbor": encode([json_to_cbor_header(rec["header"], commitment), body]).hex(),
        "record_cbor_erased": encode([json_to_cbor_header(rec["header"], commitment), erased]).hex(),
    }


def json_to_cbor_header(hd, commitment):
    return [
        hd["version"],
        hd["partition"],
        hd["offset"],
        hd["appended_at"],
        hd["payload_type"],
        hd["key"],
        commitment,
        hd["revid"],
        hd["logid"],
        hd["page_id"],
    ]


# --- the Merkle tree (RFC 6962 §2.1) ---------------------------------------------


def mth(leaves):
    if not leaves:
        return hashlib.sha256(b"").digest()
    if len(leaves) == 1:
        return leaves[0]
    k = 1
    while k * 2 < len(leaves):
        k *= 2
    return h(TAG_NODE, mth(leaves[:k]), mth(leaves[k:]))


def derive_tree(t):
    leaves = [h(TAG_LEAF, bytes.fromhex(d)) for d in t["leaf_data"]]
    return {
        "leaves": [l.hex() for l in leaves],
        "roots": [mth(leaves[:n]).hex() for n in range(len(leaves) + 1)],
        "segment_roots": {
            str(k): [mth(leaves[i : i + 2**k]).hex() for i in range(0, len(leaves) - 2**k + 1, 2**k)]
            for k in t["segment_exponents"]
        },
    }


def derive(doc):
    for c in doc["cbor"]:
        c["expect"] = {"hex": encode(c["json"]).hex()}
    for r in doc["records"]:
        r["expect"] = derive_record(r)
    doc["tree"]["expect"] = derive_tree(doc["tree"])


# --- cross-check with cbor2, when present --------------------------------------


def cbor2_check(doc):
    try:
        import cbor2
    except ImportError:
        print("cbor2 not installed; skipping the decode cross-check")
        return
    for c in doc["cbor"]:
        raw = bytes.fromhex(c["expect"]["hex"])
        back = cbor2.loads(raw)
        if normalise(back) != normalise(c["json"]):
            raise SystemExit(f"cbor2 decodes {c['name']!r} differently: {back!r}")
        if cbor2.dumps(c["json"], canonical=True) != raw:
            # cbor2's canonical order is RFC 7049's length-first; for text keys and
            # everything in these vectors it should agree.
            raise SystemExit(f"cbor2's canonical encoding of {c['name']!r} differs")
    for r in doc["records"]:
        for key in ("record_cbor", "record_cbor_erased"):
            hdr, body = cbor2.loads(bytes.fromhex(r["expect"][key]))
            assert hdr[4] == r["header"]["payload_type"]
            assert len(body) == len(r["parts"])
    print("cbor2 agrees")


def normalise(v):
    """Compare JSON-ish values exactly: 1 and 1.0 and -0.0 are different here."""
    if isinstance(v, bool) or v is None:
        return ("s", v)
    if isinstance(v, int):
        return ("i", v)
    if isinstance(v, float):
        return ("f", struct.pack(">d", v))
    if isinstance(v, str):
        return ("t", v)
    if isinstance(v, list):
        return ("a", tuple(normalise(x) for x in v))
    if isinstance(v, dict):
        return ("m", tuple(sorted((k, normalise(x)) for k, x in v.items())))
    raise TypeError(type(v))


def main():
    doc = json.loads(VECTORS.read_text(encoding="utf-8"))
    before = json.dumps(doc, sort_keys=True)
    derive(doc)
    cbor2_check(doc)
    if "--write" in sys.argv:
        VECTORS.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"wrote {VECTORS}")
        return
    if json.dumps(doc, sort_keys=True) != before:
        raise SystemExit("log-v1.json is out of date: run check.py --write and review the diff")
    print(f"{len(doc['cbor'])} CBOR vectors, {len(doc['records'])} records and the tree check out")


if __name__ == "__main__":
    main()
