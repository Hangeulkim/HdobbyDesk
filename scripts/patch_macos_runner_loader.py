#!/usr/bin/env python3
"""Rebind a verified arm64 macOS runner to the renamed HdobbyDesk core.

This is a local fallback for machines that have the Command Line Tools but not
the full Xcode application. It patches only the weak core-library dependency
and its one undefined entry point; Flutter and plug-in binaries are unchanged.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile


MH_MAGIC_64 = 0xFEEDFACF
LC_SEGMENT_64 = 0x19
LC_SYMTAB = 0x2
LC_DYSYMTAB = 0xB
LC_DYLD_INFO = 0x22
LC_DYLD_INFO_ONLY = 0x80000022
LC_DYLD_CHAINED_FIXUPS = 0x80000034
LINKEDIT_DATA_COMMANDS = {
    0x1D,  # LC_CODE_SIGNATURE
    0x1E,  # LC_SEGMENT_SPLIT_INFO
    0x26,  # LC_FUNCTION_STARTS
    0x29,  # LC_DATA_IN_CODE
    0x2E,  # LC_LINKER_OPTIMIZATION_HINT
    0x80000033,  # LC_DYLD_EXPORTS_TRIE
    LC_DYLD_CHAINED_FIXUPS,
}
N_UNDF = 0x0
OLD_LIBRARY = "@rpath/liblibrustdesk.dylib"
NEW_LIBRARY = "@rpath/liblibhdobbydesk.dylib"
OLD_SYMBOL = b"_rustdesk_core_main"
NEW_SYMBOL = b"_hdobbydesk_core_main"


class PatchError(RuntimeError):
    pass


def run(*args: str) -> str:
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode:
        raise PatchError(f"{Path(args[0]).name} failed")
    return result.stdout


def parse_load_commands(data: bytearray) -> tuple[dict[str, int], dict[str, int], dict[str, int]]:
    if len(data) < 32 or struct.unpack_from("<I", data, 0)[0] != MH_MAGIC_64:
        raise PatchError("runner is not a little-endian 64-bit Mach-O")
    ncmds = struct.unpack_from("<I", data, 16)[0]
    offset = 32
    symtab: dict[str, int] | None = None
    fixups: dict[str, int] | None = None
    linkedit: dict[str, int] | None = None
    for _ in range(ncmds):
        if offset + 8 > len(data):
            raise PatchError("truncated Mach-O load commands")
        command, size = struct.unpack_from("<II", data, offset)
        if size < 8 or offset + size > len(data):
            raise PatchError("invalid Mach-O load command")
        if command == LC_SYMTAB:
            _, _, symoff, nsyms, stroff, strsize = struct.unpack_from(
                "<6I", data, offset
            )
            symtab = {
                "command": offset,
                "symoff": symoff,
                "nsyms": nsyms,
                "stroff": stroff,
                "strsize": strsize,
            }
        elif command == LC_DYLD_CHAINED_FIXUPS:
            _, _, dataoff, datasize = struct.unpack_from("<4I", data, offset)
            fixups = {
                "command": offset,
                "dataoff": dataoff,
                "datasize": datasize,
            }
        elif command == LC_SEGMENT_64:
            segment_name = bytes(data[offset + 8 : offset + 24]).rstrip(b"\0")
            if segment_name == b"__LINKEDIT":
                _, _, _, vmaddr, vmsize, fileoff, filesize = struct.unpack_from(
                    "<II16sQQQQ", data, offset
                )
                linkedit = {
                    "command": offset,
                    "vmaddr": vmaddr,
                    "vmsize": vmsize,
                    "fileoff": fileoff,
                    "filesize": filesize,
                }
        offset += size
    if symtab is None or fixups is None or linkedit is None:
        raise PatchError("runner is missing required Mach-O metadata")
    return symtab, fixups, linkedit


def patch_symbol(data: bytearray) -> None:
    symtab, fixups, linkedit = parse_load_commands(data)
    if symtab["stroff"] + symtab["strsize"] != len(data):
        raise PatchError("unsigned runner string table is not at end of file")

    header = struct.unpack_from("<7I", data, fixups["dataoff"])
    _, _, imports_offset, symbols_offset, imports_count, imports_format, symbols_format = header
    if imports_format != 1 or symbols_format != 0:
        raise PatchError("unsupported chained-import format")
    symbol_base = fixups["dataoff"] + symbols_offset
    import_base = fixups["dataoff"] + imports_offset

    imports: list[tuple[int, int, bytes]] = []
    for index in range(imports_count):
        entry_offset = import_base + index * 4
        entry = struct.unpack_from("<I", data, entry_offset)[0]
        name_offset = entry >> 9
        start = symbol_base + name_offset
        end = data.find(b"\0", start)
        if end < 0:
            raise PatchError("unterminated chained-import name")
        imports.append((entry_offset, name_offset, bytes(data[start:end])))
    chained_matches = [item for item in imports if item[2] == OLD_SYMBOL]
    if len(chained_matches) != 1:
        raise PatchError("expected exactly one legacy chained import")

    chained_entry_offset, old_chained_name_offset, _ = chained_matches[0]
    old_chained_start = symbol_base + old_chained_name_offset
    insertion_size = 8
    insertion_offset = old_chained_start + len(OLD_SYMBOL) + 1
    data[insertion_offset:insertion_offset] = b"\0" * insertion_size
    data[old_chained_start : old_chained_start + len(NEW_SYMBOL) + 1] = (
        NEW_SYMBOL + b"\0"
    )

    for entry_offset, name_offset, _ in imports:
        entry = struct.unpack_from("<I", data, entry_offset)[0]
        if name_offset > old_chained_name_offset:
            name_offset += insertion_size
        struct.pack_into(
            "<I", data, entry_offset, (name_offset << 9) | (entry & 0x1FF)
        )

    ncmds = struct.unpack_from("<I", data, 16)[0]
    command_offset = 32
    for _ in range(ncmds):
        command, size = struct.unpack_from("<II", data, command_offset)
        if command == LC_SYMTAB:
            for field_offset in (8, 16):
                value = struct.unpack_from("<I", data, command_offset + field_offset)[0]
                if value >= insertion_offset:
                    struct.pack_into(
                        "<I", data, command_offset + field_offset, value + insertion_size
                    )
        elif command == LC_DYSYMTAB:
            for field_offset in (32, 40, 48, 56, 64, 72):
                value = struct.unpack_from("<I", data, command_offset + field_offset)[0]
                if value >= insertion_offset:
                    struct.pack_into(
                        "<I", data, command_offset + field_offset, value + insertion_size
                    )
        elif command in (LC_DYLD_INFO, LC_DYLD_INFO_ONLY):
            for field_offset in (8, 16, 24, 32, 40):
                value = struct.unpack_from("<I", data, command_offset + field_offset)[0]
                if value >= insertion_offset:
                    struct.pack_into(
                        "<I", data, command_offset + field_offset, value + insertion_size
                    )
        elif command in LINKEDIT_DATA_COMMANDS:
            data_offset = struct.unpack_from("<I", data, command_offset + 8)[0]
            data_size = struct.unpack_from("<I", data, command_offset + 12)[0]
            if command == LC_DYLD_CHAINED_FIXUPS:
                struct.pack_into(
                    "<I", data, command_offset + 12, data_size + insertion_size
                )
            elif data_offset >= insertion_offset:
                struct.pack_into(
                    "<I", data, command_offset + 8, data_offset + insertion_size
                )
        command_offset += size

    struct.pack_into(
        "<Q",
        data,
        linkedit["command"] + 48,
        linkedit["filesize"] + insertion_size,
    )

    symtab, _, linkedit = parse_load_commands(data)

    old_string_start = data.find(
        OLD_SYMBOL + b"\0",
        symtab["stroff"],
        symtab["stroff"] + symtab["strsize"],
    )
    if old_string_start < 0:
        raise PatchError("legacy symbol is absent from Mach-O string table")
    old_string_index = old_string_start - symtab["stroff"]
    nlist_entries: list[tuple[int, int, int]] = []
    for index in range(symtab["nsyms"]):
        entry_offset = symtab["symoff"] + index * 16
        string_index, symbol_type = struct.unpack_from("<IB", data, entry_offset)
        nlist_entries.append((entry_offset, string_index, symbol_type))
    nlist_matches = [
        entry_offset
        for entry_offset, string_index, symbol_type in nlist_entries
        if string_index == old_string_index and symbol_type & 0x0E == N_UNDF
    ]
    if len(nlist_matches) != 1:
        raise PatchError("expected exactly one legacy undefined symbol")

    string_insertion_offset = old_string_start + len(OLD_SYMBOL) + 1
    data[string_insertion_offset:string_insertion_offset] = b"\0" * insertion_size
    data[old_string_start : old_string_start + len(NEW_SYMBOL) + 1] = NEW_SYMBOL + b"\0"
    for entry_offset, string_index, _ in nlist_entries:
        if string_index > old_string_index:
            struct.pack_into("<I", data, entry_offset, string_index + insertion_size)
    struct.pack_into(
        "<I",
        data,
        symtab["command"] + 20,
        symtab["strsize"] + insertion_size,
    )
    struct.pack_into(
        "<Q",
        data,
        linkedit["command"] + 48,
        linkedit["filesize"] + insertion_size,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    source = args.input.expanduser().resolve(strict=True)
    output = args.output.expanduser().absolute()
    if not source.is_file() or source.is_symlink():
        parser.error("input must be a regular runner file")
    if output.exists() or output.is_symlink() or not output.parent.is_dir():
        parser.error("output must be a new file in an existing directory")

    descriptor, temporary_name = tempfile.mkstemp(
        prefix=".hdobby-runner-", dir=output.parent
    )
    os.close(descriptor)
    temporary = Path(temporary_name)
    try:
        shutil.copy2(source, temporary)
        run("codesign", "--remove-signature", str(temporary))
        run(
            "install_name_tool",
            "-change",
            OLD_LIBRARY,
            NEW_LIBRARY,
            str(temporary),
        )
        data = bytearray(temporary.read_bytes())
        patch_symbol(data)
        temporary.write_bytes(data)
        os.chmod(temporary, source.stat().st_mode & 0o777)
        dependencies = run("otool", "-L", str(temporary))
        undefined = run("nm", "-u", str(temporary))
        if NEW_LIBRARY not in dependencies or OLD_LIBRARY in dependencies:
            raise PatchError("core library dependency was not rebound")
        if NEW_SYMBOL.decode() not in undefined or OLD_SYMBOL.decode() in undefined:
            raise PatchError("core entry point was not rebound")
        temporary.replace(output)
    finally:
        if temporary.exists():
            temporary.unlink()
    print(output)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except PatchError as error:
        print(f"Patch failed: {error}", file=os.sys.stderr)
        raise SystemExit(1)
