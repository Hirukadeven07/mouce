"""Builds mouce's offline dictionary (oewn.dict) from Open English WordNet.

Usage:
    python build_dictionary.py english-wordnet-2025.xml oewn.dict

The input is the WN-LMF XML release from
https://github.com/globalwordnet/english-wordnet/releases (unzipped). Only the Python
standard library is used, and the output is the same for the same input.

What is kept: single words (no phrases), each with its spelling, part of speech,
pronunciation, its first MAX_SENSES meanings per part of speech (in WordNet's order,
most common first) and the first example of each, plus the irregular forms that point
back to a word ("ran" -> "run").

File layout (all integers little-endian u32):
    b"MOUCEDICT1"
    block count
    per block: first key (u32 length + UTF-8), offset from the start of the data, length
    the blocks: each is a zlib-compressed JSON object {key: record}, keys sorted

A key is a word in lower case. A record is {"e": [entry...]} and/or {"f": [word...]}
where "f" lists the words the key is a form of. An entry is
{"w": spelling, "p": "n"|"v"|"a"|"r", "i": pronunciation (optional), "s": [[meaning,
example (optional)]...]}.
"""

import json
import struct
import sys
import xml.etree.ElementTree as ET
import zlib

MAX_SENSES = 6
BLOCK_BYTES = 48 * 1024
MAGIC = b"MOUCEDICT1"
# Adjective satellites are adjectives to a reader.
POS = {"n": "n", "v": "v", "a": "a", "s": "a", "r": "r"}


def is_word(text):
    return " " not in text and any(c.isalpha() for c in text)


def read(path):
    synsets = {}
    entries = []
    for _, el in ET.iterparse(path, events=("end",)):
        if el.tag == "LexicalEntry":
            lemma = el.find("Lemma")
            written = lemma.get("writtenForm")
            pos = POS.get(lemma.get("partOfSpeech"))
            if pos and is_word(written):
                ipa = [p.text for p in lemma.findall("Pronunciation") if p.text]
                us = [p.text for p in lemma.findall("Pronunciation")
                      if p.text and p.get("variety") == "US"]
                forms = [f.get("writtenForm") for f in el.findall("Form")]
                senses = [s.get("synset") for s in el.findall("Sense")]
                entries.append((written, pos, (us or ipa or [None])[0], forms, senses))
            el.clear()
        elif el.tag == "Synset":
            definition = el.findtext("Definition")
            example = el.findtext("Example")
            if definition:
                synsets[el.get("id")] = (definition.strip(), example.strip() if example else None)
            el.clear()
    return synsets, entries


def records(synsets, entries):
    out = {}
    for written, pos, ipa, forms, senses in entries:
        meanings = []
        for synset in senses:
            if synset in synsets and len(meanings) < MAX_SENSES:
                definition, example = synsets[synset]
                meanings.append([definition, example] if example else [definition])
        if not meanings:
            continue
        entry = {"w": written, "p": pos, "s": meanings}
        if ipa:
            entry["i"] = ipa
        key = written.lower()
        out.setdefault(key, {}).setdefault("e", []).append(entry)
        for form in forms:
            form_key = form.lower()
            if form_key != key and is_word(form):
                targets = out.setdefault(form_key, {}).setdefault("f", [])
                if key not in targets:
                    targets.append(key)
    return out


def blocks(recs):
    current, size = {}, 0
    for key in sorted(recs):
        rec = recs[key]
        encoded = len(json.dumps({key: rec}, ensure_ascii=False).encode("utf-8"))
        if current and size + encoded > BLOCK_BYTES:
            yield current
            current, size = {}, 0
        current[key] = rec
        size += encoded
    if current:
        yield current


def write(recs, path):
    index, data = [], bytearray()
    for block in blocks(recs):
        raw = json.dumps(block, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        packed = zlib.compress(raw, 9)
        index.append((next(iter(block)), len(data), len(packed)))
        data += packed
    head = bytearray(MAGIC)
    head += struct.pack("<I", len(index))
    for first, offset, length in index:
        key = first.encode("utf-8")
        head += struct.pack("<I", len(key)) + key + struct.pack("<II", offset, length)
    with open(path, "wb") as f:
        f.write(head + data)
    return len(index), len(head) + len(data)


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    synsets, entries = read(sys.argv[1])
    recs = records(synsets, entries)
    count, size = write(recs, sys.argv[2])
    print(f"{len(recs)} keys in {count} blocks, {size / 1e6:.1f} MB")


if __name__ == "__main__":
    main()
