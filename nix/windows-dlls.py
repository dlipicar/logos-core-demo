#!/usr/bin/env python3
"""Copies every DLL the given PE files import, transitively, into one directory.

Windows resolves an import by its base name from the loading program's
directory, so a portable app keeps them all in bin/. System DLLs are left out;
an import that is neither found under --search nor a known system DLL fails the
build, instead of the app failing on the user's machine with no message.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys

SYSTEM = {
    "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "cfgmgr32.dll", "combase.dll",
    "comctl32.dll", "comdlg32.dll",
    "crypt32.dll", "d2d1.dll", "d3d11.dll", "d3d12.dll", "dbghelp.dll", "dnsapi.dll",
    "dwmapi.dll", "dwrite.dll", "dxgi.dll", "gdi32.dll", "gdiplus.dll", "hid.dll",
    "imm32.dll", "iphlpapi.dll", "kernel32.dll", "msimg32.dll", "msvcrt.dll",
    "mswsock.dll", "ncrypt.dll", "netapi32.dll", "ntdll.dll", "ole32.dll", "oleacc.dll",
    "oleaut32.dll", "opengl32.dll", "powrprof.dll", "propsys.dll", "psapi.dll",
    "rpcrt4.dll", "secur32.dll", "setupapi.dll", "shcore.dll", "shell32.dll",
    "shlwapi.dll", "synchronization.dll", "ucrtbase.dll", "uiautomationcore.dll",
    "user32.dll", "userenv.dll", "uxtheme.dll", "version.dll", "windowscodecs.dll",
    "winmm.dll", "winspool.drv", "ws2_32.dll", "wsock32.dll",
}


def is_system(name):
    lower = name.lower()
    return lower in SYSTEM or lower.startswith(("api-ms-win-", "ext-ms-"))


def imports(objdump, path):
    out = subprocess.run([objdump, "-p", path], capture_output=True, text=True, check=True).stdout
    return re.findall(r"DLL Name:\s*(\S+)", out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--objdump", required=True)
    ap.add_argument("--into", required=True)
    ap.add_argument("--search", nargs="+", required=True)
    ap.add_argument("files", nargs="+")
    args = ap.parse_args()

    found = {}
    for root in args.search:
        for dirpath, _, names in os.walk(root, followlinks=True):
            for name in names:
                if name.lower().endswith(".dll"):
                    found.setdefault(name.lower(), os.path.join(dirpath, name))

    # By lower-cased name: an import table's case need not match the file's.
    present = {n.lower(): n for n in os.listdir(args.into)}
    pending, seen, missing = list(args.files), set(), set()
    while pending:
        path = pending.pop()
        for name in imports(args.objdump, path):
            key = name.lower()
            if key in seen or is_system(name):
                continue
            seen.add(key)
            if key not in present:
                if key not in found:
                    missing.add(name)
                    continue
                present[key] = os.path.basename(found[key])
                shutil.copyfile(found[key], os.path.join(args.into, present[key]))
            pending.append(os.path.join(args.into, present[key]))
    if missing:
        sys.exit("imports found nowhere: " + ", ".join(sorted(missing)))
    print(f"{len(seen)} DLLs resolved into {args.into}")


if __name__ == "__main__":
    main()
