import hashlib
import os
import subprocess
import tarfile
import tempfile
import urllib.request
from pathlib import Path


VERSION = "1.5.3"
SHA256 = "732010aa5ef461fa93355ed2c6c5fedb48ddc4b74e697eaabe8907eaeb943011"


def main():
    root = Path(tempfile.mkdtemp(prefix="kiln-dav1d-", dir=os.environ["RUNNER_TEMP"]))
    archive = root / "dav1d.tar.xz"
    urllib.request.urlretrieve(
        f"https://download.videolan.org/pub/videolan/dav1d/{VERSION}/dav1d-{VERSION}.tar.xz",
        archive,
    )
    with archive.open("rb") as contents:
        digest = hashlib.file_digest(contents, "sha256").hexdigest()
    if digest != SHA256:
        raise RuntimeError("dav1d source checksum mismatch")
    with tarfile.open(archive) as source:
        source.extractall(root, filter="data")

    prefix = root / "install"
    build = root / "build"
    subprocess.run(
        [
            "meson",
            "setup",
            str(build),
            str(root / f"dav1d-{VERSION}"),
            "--prefix",
            str(prefix),
            "--libdir=lib",
            "--buildtype=release",
            "--default-library=static",
            "-Denable_tools=false",
            "-Denable_tests=false",
            "-Db_vscrt=md",
        ],
        check=True,
    )
    subprocess.run(["meson", "compile", "-C", str(build)], check=True)
    subprocess.run(["meson", "install", "-C", str(build)], check=True)
    with open(os.environ["GITHUB_ENV"], "a", encoding="utf-8") as environment:
        environment.write(f"PKG_CONFIG_PATH={prefix / 'lib' / 'pkgconfig'}\n")
        environment.write("SYSTEM_DEPS_DAV1D_LINK=static\n")
        environment.write(f"DAV1D_PREFIX={prefix}\n")
        environment.write(f"DAV1D_LICENSE={root / f'dav1d-{VERSION}' / 'COPYING'}\n")


if __name__ == "__main__":
    main()
