import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from html.parser import HTMLParser
from pathlib import Path
from textwrap import dedent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("target")
    parser.add_argument("license", type=Path)
    args = parser.parse_args()
    output = Path("dist")
    output.mkdir(exist_ok=True)
    name = "kiln.exe" if os.name == "nt" else "kiln"
    archive = output / f"kiln-{args.target}"
    if os.name == "nt":
        archive = archive.with_suffix(".zip")
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as package:
            package.write(args.binary, name)
            package.write(args.license, "LICENSE")
            package.write(os.environ["DAV1D_LICENSE"], "dav1d-LICENSE")
    else:
        archive = archive.with_suffix(".tar.gz")
        with tarfile.open(archive, "w:gz") as package:
            package.add(args.binary, arcname=name)
            package.add(args.license, arcname="LICENSE")
            package.add(os.environ["DAV1D_LICENSE"], arcname="dav1d-LICENSE")

    with tempfile.TemporaryDirectory(prefix="kiln-archive-") as directory:
        root = Path(directory)
        shutil.unpack_archive(archive, root)
        binary = root / name
        check_dependencies(binary)
        subprocess.run([binary, "--version"], check=True)
        smoke_build(binary, root / "site")

    with archive.open("rb") as contents:
        digest = hashlib.file_digest(contents, "sha256").hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(
        f"{digest}  {archive.name}\n", encoding="utf-8"
    )


def check_dependencies(binary):
    if sys.platform == "darwin":
        dependencies = subprocess.check_output(["otool", "-L", binary], text=True)
        libraries = [
            line.strip().split(" ")[0] for line in dependencies.splitlines()[1:]
        ]
        unexpected = [
            path
            for path in libraries
            if not path.startswith(("/usr/lib/", "/System/Library/"))
        ]
    elif sys.platform == "linux":
        dependencies = subprocess.check_output(["readelf", "-d", binary], text=True)
        libraries = re.findall(r"\(NEEDED\).*\[(.*?)\]", dependencies)
        allowed = {
            "libc.so.6",
            "libm.so.6",
            "libgcc_s.so.1",
            "libpthread.so.0",
            "libdl.so.2",
            "librt.so.1",
            "ld-linux-x86-64.so.2",
        }
        unexpected = sorted(set(libraries) - allowed)
    else:
        dependencies = subprocess.check_output(
            ["dumpbin", "/dependents", binary], text=True
        )
        unexpected = [
            line.strip()
            for line in dependencies.splitlines()
            if "dav1d" in line.lower()
        ]
    print(dependencies)
    if unexpected:
        raise RuntimeError(f"Unbundled native dependencies: {unexpected}")


def smoke_build(binary, root):
    (root / "content").mkdir(parents=True)
    (root / "templates").mkdir()
    (root / "static").mkdir()
    (root / "config.toml").write_text(
        'base_url = "https://example.com"\n', encoding="utf-8"
    )
    (root / "templates" / "post.html").write_text(
        "{{ content | safe }}", encoding="utf-8"
    )
    (root / "content" / "example.md").write_text(
        dedent("""\
            +++
            title = "Release smoke test"
            +++
            ![Example](/example.avif)
            """),
        encoding="utf-8",
    )
    shutil.copyfile(
        Path(__file__).resolve().parents[2] / "crates/kiln/tests/fixtures/example.avif",
        root / "static" / "example.avif",
    )
    subprocess.run([binary, "build", "--root", root], check=True)
    html = (root / "public" / "example" / "index.html").read_text(encoding="utf-8")
    images = Images()
    images.feed(html)
    if not any(
        image.get("width") == "8" and image.get("height") == "6"
        for image in images.items
    ):
        raise RuntimeError("AVIF dimensions were not decoded")
    if "data:image/webp;base64," not in html:
        raise RuntimeError("AVIF pixels were not decoded into a placeholder")


class Images(HTMLParser):
    def __init__(self):
        super().__init__()
        self.items = []

    def handle_starttag(self, tag, attrs):
        if tag == "img":
            self.items.append(dict(attrs))


if __name__ == "__main__":
    main()
