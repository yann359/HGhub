#!/usr/bin/env python3
"""Validate an actual device Release archive. No certificates or signing are added."""
import argparse
import hashlib
import json
import plistlib
import struct
import subprocess
import zipfile
from pathlib import Path


def validate(path, bundle_id, version):
    with zipfile.ZipFile(path) as archive:
        if archive.testzip():
            raise ValueError("ZIP CRC verification failed")
        names = archive.namelist()
        if any(n.startswith("/") or ".." in Path(n).parts for n in names):
            raise ValueError("Unsafe archive paths")
        roots = {n.split("/")[1] for n in names if n.startswith("Payload/") and len(n.split("/")) > 2}
        if len(roots) != 1 or not next(iter(roots)).endswith(".app"):
            raise ValueError("Expected exactly one Payload/*.app")
        root = "Payload/" + next(iter(roots)) + "/"
        info = plistlib.loads(archive.read(root + "Info.plist"))
        assert info["CFBundleIdentifier"] == bundle_id, "Unexpected Bundle ID"
        assert info["CFBundleShortVersionString"] == version, "Unexpected app version"
        assert info.get("CFBundlePackageType") == "APPL", "Not an application bundle"
        assert info.get("DTPlatformName") == "iphoneos", "Not built for physical iPhone"
        assert tuple(map(int, info["MinimumOSVersion"].split("."))) >= (16, 4), "Deployment target mismatch"
        assert not info.get("NSAppTransportSecurity", {}).get("NSAllowsArbitraryLoads"), "Broad ATS exception"
        for key in ["UISupportedInterfaceOrientations", "UISupportedInterfaceOrientations~ipad"]:
            assert {"UIInterfaceOrientationPortrait", "UIInterfaceOrientationLandscapeLeft", "UIInterfaceOrientationLandscapeRight"} <= set(info.get(key, [])), "Missing orientations"
        executable = root + info["CFBundleExecutable"]
        entry = archive.getinfo(executable)
        assert entry.external_attr >> 16 & 0o111, "Executable permission missing"
        binary = archive.read(executable)
        magic, cpu, _, filetype, commands, _, flags, _ = struct.unpack_from("<8I", binary)
        assert magic == 0xFEEDFACF and cpu == 0x0100000C and filetype == 2, "Expected ARM64 Mach-O executable"
        assert flags & 0x200000, "Release executable must use PIE"
        offset, device_build, encrypted = 32, False, False
        for _ in range(commands):
            command, size = struct.unpack_from("<II", binary, offset)
            assert size >= 8 and offset + size <= len(binary), "Invalid Mach-O commands"
            if command == 0x32:  # LC_BUILD_VERSION
                platform = struct.unpack_from("<I", binary, offset + 8)[0]
                device_build = platform == 2
            if command == 0x2C:  # LC_ENCRYPTION_INFO_64
                encrypted = struct.unpack_from("<I", binary, offset + 16)[0] != 0
            offset += size
        assert device_build, "Mach-O is not built for device iOS"
        assert not encrypted, "Encrypted App Store executable cannot be personally re-signed"
        assert b"localhost:1420" not in binary and b"127.0.0.1:1420" not in binary, "Development URL embedded"
        assert not any(n.endswith(".dSYM/") for n in names), "Unexpected debug symbols in payload"
        assert not any(n.startswith(root + "_CodeSignature/") for n in names), "Expected an unsigned app"
        assert root + "embedded.mobileprovision" not in names, "Unexpected provisioning profile"
        with open(path, "rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        result = {"bundleId": bundle_id, "version": version, "minimumOS": info["MinimumOSVersion"],
                  "executable": info["CFBundleExecutable"], "architecture": "arm64", "platform": "iphoneos",
                  "configuration": "release", "encrypted": False, "signature": "unsigned",
                  "files": len(names), "bytes": Path(path).stat().st_size,
                  "sha256": digest,
                  "deviceTested": False, "checks": "ZIP CRC, Payload, plist, executable mode, Mach-O, device platform, ATS, orientations, dev URL, encryption, signature"}
        try:
            result["sourceCommit"] = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        except (OSError, subprocess.CalledProcessError):
            result["sourceCommit"] = None
        return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("ipa")
    parser.add_argument("--bundle-id", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--manifest", required=True)
    args = parser.parse_args()
    result = validate(args.ipa, args.bundle_id, args.version)
    Path(args.manifest).write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result, indent=2))
