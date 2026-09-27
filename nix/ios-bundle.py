#!/usr/bin/env python3
"""Makes an iOS .app self-contained, then signs it.

Every Mach-O in it names its libraries by @rpath and finds them in Frameworks/;
no load command keeps a /nix/store path, and every image is built for the
simulator. A library the app does not ship fails the build here, not in dyld.
"""
import os
import subprocess
import sys

MACHO = {b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf", b"\xca\xfe\xba\xbe"}
LOADS = {"LC_LOAD_DYLIB", "LC_LOAD_WEAK_DYLIB", "LC_REEXPORT_DYLIB", "LC_LAZY_LOAD_DYLIB"}
IOS_SIMULATOR = "7"


def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout


def is_macho(path):
    with open(path, "rb") as f:
        return f.read(4) in MACHO


def commands(path):
    """(cmd, value) for each dylib id, dylib load, rpath and build platform."""
    found, cmd = [], None
    for line in run("otool", "-l", path).splitlines():
        words = line.split()
        if len(words) >= 2 and words[0] == "cmd":
            cmd = words[1]
        elif cmd in LOADS | {"LC_ID_DYLIB"} and words[:1] == ["name"]:
            found.append((cmd, words[1]))
        elif cmd == "LC_RPATH" and words[:1] == ["path"]:
            found.append((cmd, words[1]))
        elif cmd == "LC_BUILD_VERSION" and words[:1] == ["platform"]:
            found.append((cmd, words[1]))
    return found


def main():
    app = os.path.abspath(sys.argv[1])
    frameworks = os.path.join(app, "Frameworks")
    shipped = set(os.listdir(frameworks))
    images = [
        os.path.join(root, name)
        for root, _, names in os.walk(app)
        for name in names
        if not os.path.islink(os.path.join(root, name)) and is_macho(os.path.join(root, name))
    ]
    for image in images:
        rpath = "@loader_path/" + os.path.relpath(frameworks, os.path.dirname(image))
        args = []
        for cmd, value in commands(image):
            if cmd == "LC_ID_DYLIB":
                args += ["-id", "@rpath/" + os.path.basename(image)]
            elif cmd in LOADS and not value.startswith(("/usr/lib/", "/System/")):
                base = os.path.basename(value)
                if base not in shipped:
                    sys.exit(f"{os.path.relpath(image, app)} loads {value}, which the app does not ship")
                if value != "@rpath/" + base:
                    args += ["-change", value, "@rpath/" + base]
            elif cmd == "LC_RPATH" and value != rpath:
                args += ["-delete_rpath", value]
        if ("LC_RPATH", rpath) not in commands(image):
            args += ["-add_rpath", rpath]
        if args:
            run("install_name_tool", *args, image)

    bad = []
    for image in images:
        found = commands(image)
        where = os.path.relpath(image, app)
        bad += [f"{where}: {cmd} {value}" for cmd, value in found if "/nix/store/" in value]
        platforms = {value for cmd, value in found if cmd == "LC_BUILD_VERSION"}
        if platforms != {IOS_SIMULATOR}:
            bad.append(f"{where}: built for platform {sorted(platforms)}, not the iOS simulator")
    if bad:
        sys.exit("not self-contained:\n" + "\n".join(bad))

    # Nested code first, the bundle (and its executable) last.
    for image in images:
        if image.endswith(".dylib"):
            run("codesign", "--force", "--sign", "-", "--timestamp=none", image)
    run("codesign", "--force", "--sign", "-", "--timestamp=none", app)
    print(f"{len(images)} images in {os.path.basename(app)}, all @rpath and signed")


if __name__ == "__main__":
    main()
