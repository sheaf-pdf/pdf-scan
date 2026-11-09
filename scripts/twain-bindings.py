"""Generates twain-dsm's TWAIN constants from TWAINH.cs, each checked against twain.h.

TWAINH.cs (the TWAIN Working Group's twain-cs, under Kodak Alaris's MIT
notice) is the C# translation of twain.h; twain.h itself grants no licence,
so it is read here and never copied. The script writes every numeric constant
of twain.h that TWAINH.cs defines too, with TWAINH.cs's value, to
crates/twain-dsm/src/ffi/consts.rs, and fails if any value differs from
twain.h's; an alias (a #define of one name as another) as the name it
stands for. The constants TWAINH.cs lacks are written by hand in
crates/twain-dsm/src/ffi/by_hand.rs; the script checks those against twain.h
too, and fails if a numeric constant or an alias of twain.h is in neither
file.

Both files are fetched at pinned commits and their SHA-256 checked, unless
given with --twainh-cs and --twain-h. --check compares what it would write
with the file there, and changes nothing. --report writes what it found to a
file as well as stdout.

Usage: python -I scripts/twain-bindings.py [--check] [--report FILE]
                                           [--twainh-cs FILE --twain-h FILE]
"""

import argparse
import hashlib
import re
import sys
import tempfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates" / "twain-dsm" / "src" / "ffi" / "consts.rs"
BY_HAND = ROOT / "crates" / "twain-dsm" / "src" / "ffi" / "by_hand.rs"

PINS = {
    "TWAINH.cs": (
        "https://raw.githubusercontent.com/twain/twain-cs/"
        "74fe02ed78a689a8cc42b4d9e74cf1e47107e6a4/twaincs/source/TWAINH.cs",
        "699aaf9304bfa8f02647e9ddb0454f3262aa58ddd03180046c35f7acbe775ff6",
    ),
    "twain.h": (
        "https://raw.githubusercontent.com/twain/twain-dsm/"
        "a50f7326a191c786ee2d8cb8086aea24309c071f/TWAIN_DSM/src/twain.h",
        "dcae88f5212b8e625724f84dbc738386043dfeb4275db13edf659cd94704db6b",
    ),
}

# C#'s integer types as Rust's.
CS_TYPES = {"byte": "u8", "sbyte": "i8", "short": "i16", "ushort": "u16", "int": "i32", "uint": "u32"}
RANGES = {"u8": (0, 0xFF), "i8": (-0x80, 0x7F), "u16": (0, 0xFFFF), "i16": (-0x8000, 0x7FFF),
          "u32": (0, 0xFFFF_FFFF), "i32": (-0x8000_0000, 0x7FFF_FFFF)}

# TWAINH.cs's enums whose names aren't twain.h's prefixes, or whose types
# aren't what twain.h's structs hold them in. Each is said in the output.
ENUM_PREFIX = {
    "TWPFS": "TWPF",  # printer font styles: twain.h's TWPF_NORMAL..., beside the pixel flavors
}
ENUM_TYPE = {
    "TWMF": "u32",  # TW_MEMORY's Flags is a TW_UINT32; TWAINH.cs types the enum ushort
    "TWON_PROTOCOL": "u16",  # TW_IDENTITY's ProtocolMajor and ProtocolMinor are TW_UINT16
}
# Enums of TWAINH.cs's own, not twain.h's.
CS_ONLY_ENUMS = {"Platform", "Processor", "STATE"}
# The enums whose members are whole names (CAP_XFERCOUNT, ICAP_..., ACAP_...).
WHOLE_NAMES = {"CAP"}
# DG's supported-group flags are twain.h's DF_.
DG_FLAGS = {"DSM2", "APP2", "DS2"}

NUMBER = r"-?(?:0[xX][0-9A-Fa-f]+|\d+)"


def fetch(name, dest):
    url, sha = PINS[name]
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != sha:
        sys.exit(f"{name} from {url} has SHA-256 {digest}, not the pinned {sha}")
    path = dest / name
    path.write_bytes(data)
    return path


def number(text):
    return int(text.strip().rstrip("uUlL"), 0)


def header_constants(h):
    """twain.h's numeric #defines, and its aliases (a #define of one name as another), resolved."""
    numeric = {}
    for name, value in re.findall(rf"^\s*#\s*define\s+(\w+)\s+\(?\s*({NUMBER})[uUlL]*\s*\)?\s*(?:/[*/].*)?$", h, re.M):
        if name in numeric and numeric[name] != number(value):
            sys.exit(f"twain.h defines {name} twice, differently")
        numeric[name] = number(value)
    aliases = {}
    for name, target in re.findall(r"^\s*#\s*define\s+(\w+)\s+([A-Z]\w*)\s*(?:/[*/].*)?$", h, re.M):
        if target in numeric:
            aliases[name] = target
    return numeric, aliases


def strip_comments(text):
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def cs_enums(cs):
    """TWAINH.cs's enums: [(name, rust type or None, [(member, expression, literal)])]."""
    enums = []
    for m in re.finditer(r"public enum (\w+)\s*(?::\s*(\w+))?\s*\{(.*?)\}", cs, re.S):
        name, cs_type, body = m.group(1), m.group(2) or "int", strip_comments(m.group(3))
        body = re.sub(r"\[[^\]]*\]", "", body)  # attributes
        members = []
        for item in body.split(","):
            item = item.strip()
            if not item:
                continue
            mm = re.fullmatch(r"(\w+)\s*=\s*(.+)", item, re.S)
            members.append((mm.group(1), mm.group(2).strip()) if mm else (item, None))
        enums.append((name, CS_TYPES.get(cs_type), members))
    return enums


def cs_consts(cs):
    return [(n, CS_TYPES[t], v) for t, n, v in re.findall(rf"public const (\w+) (\w+)\s*=\s*({NUMBER})\s*;", cs)]


def evaluate(expr, enum_values, stscc):
    """An enum member's value: a literal, STSCC + n, or another member (bare or Enum.Member)."""
    if re.fullmatch(NUMBER, expr):
        return number(expr)
    m = re.fullmatch(r"STSCC\s*\+\s*(\d+)", expr)
    if m:
        return stscc + int(m.group(1))
    m = re.fullmatch(r"(?:\w+\.)?(\w+)", expr)
    if m and m.group(1) in enum_values:
        return enum_values[m.group(1)]
    sys.exit(f"TWAINH.cs: can't read the value {expr!r}")


def twain_names(enum, member):
    """The twain.h names a TWAINH.cs member stands for."""
    if enum in WHOLE_NAMES:
        return [member]
    if enum == "DG" and member in DG_FLAGS:
        return [f"DF_{member}"]
    if enum == "TWON_PROTOCOL":
        return [f"TWON_PROTOCOL{member}"]  # twain.h's TWON_PROTOCOLMAJOR, TWON_PROTOCOLMINOR
    prefix = ENUM_PREFIX.get(enum, enum)
    # TWAINH.cs writes a name that starts with a digit with an X before it
    # ("X3OF9 = 0, // 3OF9 in TWAIN.H").
    if re.fullmatch(r"X\d\w*", member):
        member = member[1:]
    return [f"{prefix}_{member}"]


def rust_literal(value, ty, literal):
    if literal is not None and re.fullmatch(r"0[xX][0-9A-Fa-f]+", literal):
        return "0x" + literal[2:].lower()
    if literal is not None and re.fullmatch(r"-?\d+", literal):
        return literal
    return str(value)


def generate(cs, h):
    numeric, aliases = header_constants(h)

    def header_value(name):
        return numeric.get(name, numeric.get(aliases.get(name)))
    stscc = number(re.search(rf"public const int STSCC\s*=\s*({NUMBER})", cs).group(1))
    lines, table, report, emitted = [], [], [], {}
    only_cs, renamed, mismatched = [], [], []

    def emit(name, ty, value, text):
        lo, hi = RANGES[ty]
        if not lo <= value <= hi:
            sys.exit(f"{name} = {value} doesn't fit {ty}")
        if name in emitted:
            sys.exit(f"{name} would be written twice")
        expected = header_value(name)
        if expected is None:
            return False
        if expected != value:
            mismatched.append(f"{name}: TWAINH.cs {value:#x}, twain.h {expected:#x}")
            return False
        emitted[name] = value
        lines.append(f"pub const {name}: {ty} = {text};")
        table.append(name)
        return True

    for enum, ty, members in cs_enums(cs):
        if enum in CS_ONLY_ENUMS:
            report.append(f"skipped: enum {enum}, TWAINH.cs's own")
            continue
        values = {}
        for member, expr in members:
            values[member] = evaluate(expr, values, stscc) if expr is not None else None
        lines.append("")
        lines.append(f"// enum {enum}" + (f", as {ENUM_PREFIX[enum]}" if enum in ENUM_PREFIX else "")
                     + (f" (TWAINH.cs: {ty}; here {ENUM_TYPE[enum]})" if enum in ENUM_TYPE else ""))
        rust_type = ENUM_TYPE.get(enum, ty)
        for member, expr in members:
            value = values[member]
            if enum == "STS":
                # Return codes below STSCC; condition codes STSCC + n, as n.
                # SUCCESS and CUSTOMBASE are both.
                if value >= stscc:
                    named = [(f"TWCC_{member}", value - stscc, None)]
                else:
                    named = [(f"TWRC_{member}", value, expr)]
                    if member in ("SUCCESS", "CUSTOMBASE"):
                        named.append((f"TWCC_{member}", value, expr))
                for name, v, literal in named:
                    if not emit(name, "u16", v, rust_literal(v, "u16", literal)):
                        only_cs.append(f"STS.{member} ({name})")
                continue
            for name in twain_names(enum, member):
                alias = re.fullmatch(r"(?:\w+\.)?(\w+)", expr or "")
                target = None
                if alias and not re.fullmatch(NUMBER, expr):
                    target = twain_names(enum, alias.group(1))[0]
                text = target if target in emitted else rust_literal(value, rust_type, expr)
                if emit(name, rust_type, value, text):
                    if enum not in WHOLE_NAMES and name != f"{enum}_{member}":
                        renamed.append(f"{name} from {enum}.{member}")
                elif name not in numeric and name not in aliases:
                    only_cs.append(f"{enum}.{member} ({name})")
    lines.append("")
    lines.append("// public const")
    for name, ty, literal in cs_consts(cs):
        if name == "STSCC":
            continue
        if not emit(name, ty, number(literal), rust_literal(number(literal), ty, literal)):
            only_cs.append(name)

    if mismatched:
        sys.exit("TWAINH.cs and twain.h differ:\n  " + "\n  ".join(mismatched))

    by_hand = {}
    hand_text = BY_HAND.read_text(encoding="utf-8") if BY_HAND.exists() else ""
    for name, value in re.findall(rf"^pub const (\w+): \w+ = ({NUMBER}|[A-Z]\w*);", hand_text, re.M):
        if re.fullmatch(NUMBER, value):
            by_hand[name] = number(value)
        elif value in emitted or value in by_hand:
            by_hand[name] = emitted.get(value, by_hand.get(value))
        else:
            sys.exit(f"by_hand.rs: {name} = {value}, which isn't defined before it")
    problems = []
    listed = re.findall(r'^\s*\("(\w+)", (\w+) as i64\),$', hand_text, re.M)
    if [n for n, _ in listed] != list(by_hand) or any(n != c for n, c in listed):
        problems.append("by_hand.rs's BY_HAND doesn't list its constants, each once, in order")
    for name, value in by_hand.items():
        if name in emitted:
            problems.append(f"{name} is in by_hand.rs and generated from TWAINH.cs too")
        elif header_value(name) != value:
            problems.append(f"{name} is {value:#x} in by_hand.rs and {header_value(name)!r} in twain.h")
    unbound = sorted(n for n in [*numeric, *aliases] if n not in emitted and n not in by_hand)
    for name in unbound:
        problems.append(f"{name} ({header_value(name):#x}) is in twain.h, and neither generated nor in by_hand.rs")

    report[:0] = [
        f"twain.h: {len(numeric)} numeric #defines, {len(aliases)} aliases of them",
        f"generated from TWAINH.cs: {len(emitted)}",
        f"by hand: {len(by_hand)}",
    ]
    report += [f"renamed: {r}" for r in renamed]
    report += [f"only in TWAINH.cs: {n}" for n in only_cs]
    report += [f"by hand: {n} = {v:#x}" for n, v in sorted(by_hand.items())]
    report += [f"problem: {p}" for p in problems]

    out = [HEADER]
    out += lines
    out.append("")
    out.append("/// Every constant above, by name, its value widened to `i64`: for")
    out.append("/// names, and for checking against `twain.h`.")
    out.append("pub(crate) const GENERATED: &[(&str, i64)] = &[")
    out += [f"    (\"{n}\", {n} as i64)," for n in table]
    out.append("];")
    return "\n".join(out) + "\n", report, problems


HEADER = """\
// @generated by scripts/twain-bindings.py: don't edit; run it again.
//
// TWAIN's constants, from `TWAINH.cs` in the TWAIN Working Group's twain-cs
// (commit 74fe02ed78a6, the C# translation of `twain.h`): each numeric
// `#define` of `twain.h` 2.5 (twain-dsm commit a50f7326a191) that `TWAINH.cs`
// defines too, with `TWAINH.cs`'s value, which the script checked is
// `twain.h`'s. Changed in the port: written as Rust constants with `twain.h`'s
// names, from `TWAINH.cs`'s enums and their members (`TWPT.RGB` as
// `TWPT_RGB`; `STS` as `TWRC_` and `TWCC_`, without `TWAINH.cs`'s offset of
// the condition codes; `X3OF9` as `TWBT_3OF9`); typed as `TWAINH.cs` types
// each enum, but where a comment says otherwise. The rest are in
// `by_hand.rs`.
//
// The notice TWAINH.cs carries:
//
//  Copyright (C) 2013-2021 Kodak Alaris Inc.
//
//  Permission is hereby granted, free of charge, to any person obtaining a
//  copy of this software and associated documentation files (the "Software"),
//  to deal in the Software without restriction, including without limitation
//  the rights to use, copy, modify, merge, publish, distribute, sublicense,
//  and/or sell copies of the Software, and to permit persons to whom the
//  Software is furnished to do so, subject to the following conditions:
//
//  The above copyright notice and this permission notice shall be included in
//  all copies or substantial portions of the Software.
//
//  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
//  THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//  LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
//  FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//  DEALINGS IN THE SOFTWARE."""


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--twainh-cs", type=Path)
    parser.add_argument("--twain-h", type=Path)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if (args.twainh_cs is None) != (args.twain_h is None):
        parser.error("give both --twainh-cs and --twain-h, or neither")
    with tempfile.TemporaryDirectory() as temp:
        cs_path = args.twainh_cs or fetch("TWAINH.cs", Path(temp))
        h_path = args.twain_h or fetch("twain.h", Path(temp))
        cs = cs_path.read_text(encoding="utf-8-sig")
        h = h_path.read_text(encoding="latin-1")
    text, report, problems = generate(cs, h)
    print("\n".join(report))
    if args.report:
        args.report.write_text("\n".join(report) + "\n", encoding="utf-8")
    if args.check:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current.replace("\r\n", "\n") != text:
            sys.exit(f"{OUT.relative_to(ROOT)} isn't what the script writes: run it again")
        print(f"{OUT.relative_to(ROOT)} is what the script writes")
    else:
        OUT.parent.mkdir(parents=True, exist_ok=True)
        OUT.write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {OUT.relative_to(ROOT)}")
    if problems:
        sys.exit(f"{len(problems)} problems")


if __name__ == "__main__":
    main()
