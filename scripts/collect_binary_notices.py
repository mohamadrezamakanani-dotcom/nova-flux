#!/usr/bin/env python3
"""Collect exact, target-selected Rust/font notices; no build or implicit Cargo fetch.

Python 3.11+. See docs/RUST_NOTICES.md for the evidence boundary and usage.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tomllib
import urllib.parse
import urllib.request

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
TARGETS = ("aarch64-apple-darwin", "x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu")
# Reviewed declared expressions in the locked release graph. OR chooses a permissive
# branch; all supplied license/NOTICE files are nevertheless retained verbatim.
LICENSE_CHOICES = {
    "MIT": ["MIT"], "Apache-2.0": ["Apache-2.0"], "Zlib": ["Zlib"],
    "ISC": ["ISC"], "BSL-1.0": ["BSL-1.0"], "Unicode-3.0": ["Unicode-3.0"],
    "MIT OR Apache-2.0": ["MIT"], "Apache-2.0 OR MIT": ["MIT"],
    "MIT/Apache-2.0": ["MIT"], "Apache-2.0/MIT": ["MIT"],
    "MIT / Apache-2.0": ["MIT"], "Unlicense OR MIT": ["MIT"],
    "Unlicense/MIT": ["MIT"], "Zlib OR Apache-2.0 OR MIT": ["MIT"],
    "MIT OR Apache-2.0 OR Zlib": ["MIT"], "MIT OR Zlib OR Apache-2.0": ["MIT"],
    "BSD-3-Clause OR Apache-2.0": ["BSD-3-Clause"],
    "BSD-3-Clause OR MIT OR Apache-2.0": ["MIT"],
    "BSD-2-Clause OR Apache-2.0 OR MIT": ["MIT"],
    "0BSD OR MIT OR Apache-2.0": ["MIT"],
    "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT": ["MIT"],
    "MIT OR Apache-2.0 OR LGPL-2.1-or-later": ["MIT"],
    "Apache-2.0 OR GPL-2.0-only": ["Apache-2.0"],
    "Apache-2.0 AND MIT": ["Apache-2.0", "MIT"],
    "(MIT OR Apache-2.0) AND Unicode-3.0": ["MIT", "Unicode-3.0"],
    "(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0": ["MIT", "OFL-1.1", "Ubuntu-font-1.0"],
}
# These exact published objc2-family archives explicitly grant MIT by link but
# omit its permission text in their upstream LICENSE.md. No future version is
# silently covered by this reviewed list.
MIT_TEXT_OMISSIONS = {('block2', '0.6.2'): 'cdeb9d870516001442e364c5220d3574d2da8dc765554b4a617230d33fa58ef5', ('dispatch2', '0.3.1'): '1e0e367e4e7da84520dedcac1901e4da967309406d1e51017ae1abfb97adbd38', ('objc2', '0.6.5'): '08849bbd4767dfae9457696856ae1c84fe4e0281bbe4a7abff2d0e06fb7981f8', ('objc2-app-kit', '0.3.2'): 'd49e936b501e5c5bf01fda3a9452ff86dc3ea98ad5f283e1455153142d97518c', ('objc2-core-foundation', '0.3.2'): '2a180dd8642fa45cdb7dd721cd4c11b1cadd4929ce112ebd8b9f5803cc79d536', ('objc2-core-graphics', '0.3.2'): 'e022c9d066895efa1345f8e33e584b9f958da2fd4cd116792e15e07e4720a807', ('objc2-encode', '4.1.0'): 'ef25abbcd74fb2609453eb695bd2f860d389e457f67dc17cafc8b8cbc89d0c33', ('objc2-foundation', '0.3.2'): 'e3e0adef53c21f888deb4fa59fc59f7eb17404926ee8a6f59f5df0fd7f9f3272', ('objc2-metal', '0.3.2'): 'a0125f776a10d00af4152d74616409f0d4a2053a6f57fa5b7d6aa2854ac04794', ('objc2-quartz-core', '0.3.2'): '96c1358452b371bf9f104e21ec536d37a650eb10f7ee379fff67d2e08d537f1f'}

FONT_NOTICES = ("fonts/Hack-Regular.txt", "fonts/OFL.txt", "fonts/UFL.txt", "fonts/emoji-icon-font-mit-license.txt")
TEXT_SUFFIXES = {".rs", ".c", ".cc", ".cpp", ".h", ".hpp", ".xml", ".idl", ".py"}


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def is_notice(name):
    return bool(re.search(r"(?:^|[._-])(?:licen[cs]e|notice|copying|copyright|unlicense)(?:$|[._-])", PurePosixPath(name).name, re.I)) or PurePosixPath(name).name.lower() in {"ofl.txt", "ufl.txt", "hack-regular.txt"}


def checked_path(name):
    path = PurePosixPath(name)
    if path.is_absolute() or not name or any(part in ("", ".", "..") for part in name.split("/")) or "\\" in name or ":" in name:
        raise ValueError(f"Unsafe notice path: {name}")
    return path


def inline_header(data):
    """Retain an initial comment block, byte for byte, when it has attribution.

    Covers Rust/C line/block comments and XML declarations/comments. These are
    supplementary; the crate still must supply a standalone full license.
    """
    end = 0
    block = None
    for line in data.splitlines(keepends=True):
        stripped = line.lstrip()
        if block:
            end += len(line)
            if block in line:
                block = None
            continue
        if not stripped.strip() or stripped.startswith((b"//", b"#!")):
            end += len(line)
        elif stripped.startswith((b"/*", b"<!--")):
            marker = b"*/" if stripped.startswith(b"/*") else b"-->"
            block = None if marker in stripped else marker
            end += len(line)
        elif stripped.strip() == b"<registry>":
            end += len(line)
        elif stripped.startswith(b"<comment>"):
            block = None if b"</comment>" in stripped else b"</comment>"
            end += len(line)
        elif stripped.startswith(b"<?xml"):
            end += len(line)
        else:
            break
    header = data[:end]
    return header if re.search(rb"copyright|spdx-license-identifier|permission is hereby granted", header, re.I) else b""


def read_archive(package, checksum):
    base = Path(package["manifest_path"]).parent
    archive = base.parents[2] / "cache" / base.parent.name / f"{package['name']}-{package['version']}.crate"
    if not archive.is_file():
        raise ValueError(f"Missing cached crate archive: {archive}; run cargo fetch --locked explicitly")
    if sha256(archive.read_bytes()) != checksum:
        raise ValueError(f"Crate checksum mismatch: {package['name']} {package['version']}")
    expected = f"{package['name']}-{package['version']}/"
    contents = {}
    with tarfile.open(archive, "r:gz") as source:
        for member in source:
            if member.isdir():
                continue
            if not member.isfile() or not member.name.startswith(expected):
                raise ValueError(f"Unsafe crate member: {member.name}")
            relative = member.name[len(expected):]
            checked_path(relative)
            if relative in contents:
                raise ValueError(f"Duplicate crate member: {relative}")
            contents[relative] = source.extractfile(member).read()
    declared = tomllib.loads(contents["Cargo.toml"].decode())["package"]
    if (declared["name"], declared["version"], declared.get("license")) != (package["name"], package["version"], package["license"]):
        raise ValueError(f"Metadata/archive disagreement: {package['name']}")
    return contents


def verify_git_blob(data, expected):
    actual = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
    if actual != expected:
        raise ValueError("Git blob checksum mismatch in upstream notice")


class UpstreamNotices:
    """Small public GitHub texts at the exact commit recorded inside the crate."""
    def __init__(self, cache, fetch=False):
        self.cache, self.fetch = Path(cache), fetch
        manifest_path = self.cache / "cache-manifest.json"
        self.inventory = None
        if manifest_path.exists():
            manifest = json.loads(manifest_path.read_text())
            if manifest.get("schema_version") != 1:
                raise ValueError("Retained upstream cache has an unsupported manifest")
            self.inventory = {item["url"]: item for item in manifest["files"]}

    def read(self, url):
        cached = self.cache / sha256(url.encode())
        if cached.exists():
            data = cached.read_bytes()
            if self.inventory is not None:
                record = self.inventory.get(url)
                if not record or record["path"] != cached.name or record["sha256"] != sha256(data) or record["bytes"] != len(data):
                    raise ValueError(f"Retained upstream cache integrity mismatch: {url}")
            return data
        if not self.fetch:
            raise ValueError(f"Missing upstream notice cache; rerun with --fetch-upstream: {url}")
        request = urllib.request.Request(url, headers={"User-Agent": "NovaFlux-Notice-Collector", "Accept": "application/vnd.github+json"})
        with urllib.request.urlopen(request, timeout=30) as response:
            data = response.read(4 * 1024 * 1024 + 1)
        if len(data) > 4 * 1024 * 1024:
            raise ValueError(f"Upstream notice/tree exceeds 4 MiB: {url}")
        self.cache.mkdir(parents=True, exist_ok=True)
        cached.write_bytes(data)
        return data

    def collect(self, package, contents):
        if ".cargo_vcs_info.json" not in contents:
            raise ValueError(f"Missing exact upstream VCS revision: {package['name']}")
        vcs = json.loads(contents[".cargo_vcs_info.json"])
        commit = vcs["git"]["sha1"]
        if not re.fullmatch(r"[0-9a-f]{40}", commit) or vcs["git"].get("dirty"):
            raise ValueError(f"Unusable upstream revision: {package['name']}")
        repository = package.get("repository") or ""
        match = re.match(r"^https?://github\.com/([^/]+)/([^/#]+)", repository)
        if not match:
            raise ValueError(f"Unreviewed upstream repository: {repository}")
        owner, repo = match.groups()
        repo = repo.removesuffix(".git")
        crate_path = vcs.get("path_in_vcs", "").strip("/")
        if crate_path:
            checked_path(crate_path)
        # Older gl-rs crates omit path_in_vcs. These exact crate names live in
        # those repository subdirectories; root licenses are inherited too.
        if not crate_path and package["name"] in {"gl_generator", "khronos_api"}:
            crate_path = package["name"]
        tree_url = f"https://api.github.com/repos/{owner}/{repo}/git/trees/{commit}?recursive=1"
        tree = json.loads(self.read(tree_url))
        if tree.get("truncated") or not isinstance(tree.get("tree"), list):
            raise ValueError(f"Incomplete upstream tree: {repository} {commit}")
        ancestors = {""}
        if crate_path:
            ancestors.update(str(parent) if str(parent) != "." else "" for parent in PurePosixPath(crate_path).parents)
            ancestors.add(crate_path)
        files, provenance = {}, []
        for entry in tree["tree"]:
            name = entry["path"]
            if entry["type"] != "blob" or not is_notice(name):
                continue
            parent = str(PurePosixPath(name).parent)
            parent = "" if parent == "." else parent
            if parent not in ancestors and not (crate_path and name.startswith(crate_path + "/")):
                continue
            checked_path(name)
            url = f"https://raw.githubusercontent.com/{owner}/{repo}/{commit}/{urllib.parse.quote(name)}"
            data = self.read(url)
            verify_git_blob(data, entry["sha"])
            if not data.strip():
                raise ValueError(f"Empty upstream notice: {url}")
            dest = "upstream/" + name
            files[dest] = data
            provenance.append({"file": dest, "url": url, "git_commit": commit, "git_blob_sha1": entry["sha"], "sha256": sha256(data)})
        if package["name"] == "khronos_api":
            # This exact crate embeds XML from a pinned ANGLE submodule whose
            # BSD notice is not included in the .crate. Preserve its real text.
            angle_commit = "7403dd2cd3764fe96660fe09892e764e9ae1dbca"
            if not any(item["path"] == "khronos_api/api_angle" and item["type"] == "commit" and item["sha"] == angle_commit for item in tree["tree"]):
                raise ValueError("Unreviewed khronos_api ANGLE submodule revision")
            url = f"https://raw.githubusercontent.com/google/angle/{angle_commit}/LICENSE"
            data = self.read(url)
            blob = "0f65fd60fd68d465a2283378849f16532f5ee4f1"
            verify_git_blob(data, blob)
            dest = "upstream/submodules/angle/LICENSE"
            files[dest] = data
            provenance.append({"file": dest, "url": url, "git_commit": angle_commit, "git_blob_sha1": blob, "sha256": sha256(data), "parent_git_commit": commit})
        if not files:
            raise ValueError(f"Missing license at exact upstream revision: {package['name']} {commit}")
        return files, provenance


def collect_crate(package, checksum, upstream=None):
    expression = package.get("license")
    if expression not in LICENSE_CHOICES:
        raise ValueError(f"Unreviewed license expression: {package['name']}: {expression}")
    if package.get("source") != REGISTRY:
        raise ValueError(f"Unreviewed package source: {package['source']}")
    contents = read_archive(package, checksum)
    files = {name: data for name, data in contents.items() if is_notice(name)}
    declared_file = package.get("license_file")
    if declared_file and declared_file in contents:
        files[declared_file] = contents[declared_file]
    root_license = any("/" not in name and re.match(r"^(?:licen[cs]e|copying|unlicense)", name, re.I) for name in files)
    provenance = []
    omission = None
    if not root_license and (package["name"], package["version"]) == ("dispatch", "0.2.0"):
        extra, omission = dispatch_supplement(package["name"], package["version"], checksum, contents)
        files.update(extra)
        root_license = True
    if not root_license:
        if upstream is None:
            raise ValueError(f"Missing license text in crate: {package['name']} {package['version']}")
        extra, provenance = upstream.collect(package, contents)
        files.update(extra)
    if package["name"] == "epaint_default_fonts":
        for required in FONT_NOTICES:
            if required not in files:
                raise ValueError(f"Missing font notice: {required}")
    if package["name"] == "khronos_api":
        files.update({"original-source/" + name: data for name, data in contents.items()})
    if "MIT" in LICENSE_CHOICES[expression] and not any(b"permission is hereby granted" in data.lower() for data in files.values()):
        if MIT_TEXT_OMISSIONS.get((package["name"], package["version"])) != checksum:
            raise ValueError(f"Missing selected MIT permission text: {package['name']} {package['version']}")
        files.update({"original-source/" + name: data for name, data in contents.items()})
        files["supplement/MIT-permission.txt"] = MIT_PERMISSION
        files["supplement/UPSTREAM-NOTICE-OMISSION.txt"] = ("The exact published crate and upstream LICENSE.md explicitly license this\ncrate under MIT but omit the full permission text. Original source, manifest,\nREADME, and all upstream notices are preserved. The separately labeled MIT\npermission, conditions and disclaimer are supplied from canonical SPDX text.\nNo author copyright notice or date has been invented. This exception is\nreviewed for this exact crate version and archive checksum only.\n").encode()
        omission = {"status": "reviewed-upstream-notice-omission", "canonical_license": "MIT",
                    "canonical_source_url": "https://raw.githubusercontent.com/spdx/license-list-data/31ba1a50e5397e00a304dbadc76531740e89ee48/text/MIT.txt",
                    "canonical_source_sha256": "b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5", "supplement_sha256": sha256(MIT_PERMISSION),
                    "reason": "Exact upstream LICENSE.md grants MIT by reference but omits its permission text; all original sources retained"}
    for name, data in contents.items():
        if PurePosixPath(name).suffix in TEXT_SUFFIXES:
            header = inline_header(data)
            if header:
                files[f"inline/{name}.txt"] = header
    if any(not data.strip() for data in files.values()):
        raise ValueError(f"Empty notice: {package['name']}")
    result = {"name": package["name"], "version": package["version"], "source": package["source"], "checksum_sha256": checksum,
              "license_expression": expression, "selected_license_branches": LICENSE_CHOICES[expression], "repository": package.get("repository"),
              "upstream_supplements": provenance, "files": [{"path": name, "sha256": sha256(data), "bytes": len(data)} for name, data in sorted(files.items())]}
    if omission:
        result["notice_omission"] = omission
    if package["name"] == "epaint_default_fonts":
        result["font_notices"] = list(FONT_NOTICES)
        result["font_license_note"] = "Hack includes its MIT and Bitstream Vera notices; do not infer all font licenses from the crate SPDX expression."
    return result, files


# Canonical SPDX MIT permission, conditions and disclaimer, verbatim. The generic
# copyright template line is deliberately not attributed to the upstream author.
# Source: spdx/license-list-data commit 31ba1a50e5397e00a304dbadc76531740e89ee48,
# text/MIT.txt; complete source SHA256 b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5.
MIT_PERMISSION = b'Permission is hereby granted, free of charge, to any person obtaining a copy of this software and\nassociated documentation files (the "Software"), to deal in the Software without restriction, including\nwithout limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell\ncopies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the\nfollowing conditions:\n\nThe above copyright notice and this permission notice shall be included in all copies or substantial\nportions of the Software.\n\nTHE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT\nLIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO\nEVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER\nIN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE\nUSE OR OTHER DEALINGS IN THE SOFTWARE.\n'


def dispatch_supplement(name, version, checksum, contents):
    """A reviewed omission, not a generic fallback for future missing licenses."""
    expected = ("dispatch", "0.2.0", "bd0c93bb4b0c6d9b77f4435b0ae98c24d17f1c45b2ff844c6151a07256ca923b")
    if (name, version, checksum) != expected:
        raise ValueError(f"Missing license text; no reviewed omission: {name} {version}")
    vcs = json.loads(contents.get(".cargo_vcs_info.json", b"{}"))
    if vcs.get("git", {}).get("sha1") != "82d6c7a5b75dc0c71c3f46f87bb6c16a476f7748":
        raise ValueError("Reviewed dispatch upstream revision changed")
    original = tomllib.loads(contents["Cargo.toml"].decode())["package"]
    if original.get("license") != "MIT" or original.get("authors") != ["Steven Sheldon"]:
        raise ValueError("Reviewed dispatch MIT declaration/authorship changed")
    files = {"original-source/" + name: data for name, data in contents.items()}
    files["supplement/MIT-permission.txt"] = MIT_PERMISSION
    explanation = ("dispatch 0.2.0 upstream notice omission\n\n"
                   "The exact published crate declares license = MIT and authors = Steven Sheldon\n"
                   "in its own Cargo.toml. Its archive and recorded upstream Git revision contain\n"
                   "no separate LICENSE or copyright notice. All original crate files, including\n"
                   "the original license declaration, README and complete source, are retained.\n"
                   "The supplemental MIT permission, conditions and warranty disclaimer come\n"
                   "from the canonical SPDX MIT text. No author copyright date or copyright\n"
                   "notice has been invented. This is a documented, checksum-pinned exception,\n"
                   "not permission to silently substitute canonical text for other crates.\n")
    files["supplement/UPSTREAM-NOTICE-OMISSION.txt"] = explanation.encode()
    return files, {"status": "reviewed-upstream-notice-omission", "canonical_license": "MIT",
                   "canonical_source_url": "https://raw.githubusercontent.com/spdx/license-list-data/31ba1a50e5397e00a304dbadc76531740e89ee48/text/MIT.txt",
                   "canonical_source_sha256": "b05785f9f18e6716bab63424b11454513b9943a222595b70411009202fc592b5",
                   "supplement_sha256": sha256(MIT_PERMISSION), "original_copyright_notice": "No separate notice present; original files retained without invented attribution"}


def toolchain_notices(sysroot, version):
    doc = Path(sysroot) / "share/doc/rust"
    required = ["COPYRIGHT.html", "COPYRIGHT-library.html", "licenses/MIT.txt", "licenses/Apache-2.0.txt"]
    for name in required:
        if not (doc / name).is_file():
            raise ValueError(f"Missing Rust toolchain notice: {doc / name}; install matching rust-docs component")
    names = {"COPYRIGHT.html", "COPYRIGHT-library.html"}
    names.update(str(item.relative_to(doc)).replace("\\", "/") for item in (doc / "licenses").rglob("*") if item.is_file())
    files = {name: (doc / name).read_bytes() for name in sorted(names)}
    return {"rustc_version_verbose": version, "scope": "Complete installed toolchain copyright inventory, standard-library inventory and license texts, conservatively retained",
            "files": [{"path": name, "sha256": sha256(data), "bytes": len(data)} for name, data in files.items()]}, files


def select_packages(metadata, tree, root_name):
    available = {}
    for package in metadata["packages"]:
        available.setdefault((package["name"], package["version"]), []).append(package)
    selected = {}
    for line in tree.splitlines():
        if not line.strip():
            continue
        match = re.match(r"^(\S+) v(\S+)(?:\s|$)", line)
        if not match:
            raise ValueError(f"Unrecognized Cargo tree line: {line}")
        name, version = match.groups()
        if name == root_name:
            continue
        candidates = available.get((name, version), [])
        if len(candidates) != 1:
            raise ValueError(f"Ambiguous or missing Cargo package: {name} {version}")
        package = candidates[0]
        selected[package["id"]] = package
    if not selected:
        raise ValueError("Empty target dependency tree")
    return sorted(selected.values(), key=lambda p: (p["name"], p["version"]))


def public_tree(tree, root):
    # Cargo's generated root-package location is local build metadata, not an
    # upstream attribution. Keep versions/edges intact while making it public.
    forms = {str(root), str(root).replace("\\", "/"), str(root).replace("\\", "\\\\")}
    for form in sorted(forms, key=len, reverse=True):
        tree = tree.replace(form, "{SOURCE}")
    return tree


def run_cargo(root, *arguments, toolchain=None):
    return subprocess.run(["cargo", *(["+" + toolchain] if toolchain else []), *arguments], cwd=root, check=True, capture_output=True, text=True).stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=TARGETS, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument("--upstream-cache", type=Path, default=Path(__file__).resolve().parent / "notices-upstream")
    parser.add_argument("--toolchain", help="Use the same rustup toolchain as the release build")
    parser.add_argument("--fetch-upstream", action="store_true", help="Allow small exact-commit license/tree downloads; never fetch crate archives")
    args = parser.parse_args()
    root, output = args.root.resolve(), args.output.resolve()
    if output.exists():
        raise ValueError(f"Refusing to overwrite existing notice output: {output}")
    lock_data = (root / "Cargo.lock").read_bytes()
    lock = tomllib.loads(lock_data.decode())
    project = tomllib.loads((root / "Cargo.toml").read_text())["package"]
    checksums = {(p["name"], p["version"], p.get("source")): p.get("checksum") for p in lock["package"]}
    metadata = json.loads(run_cargo(root, "metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", args.target, toolchain=args.toolchain))
    tree = run_cargo(root, "tree", "--locked", "--offline", "--target", args.target, "--edges", "normal,build", "--prefix", "none", "--format", "{p}", toolchain=args.toolchain)
    tree = public_tree(tree, root)
    packages = select_packages(metadata, tree, project["name"])
    rustc = ["rustup", "run", args.toolchain, "rustc"] if args.toolchain else ["rustc"]
    sysroot = subprocess.check_output([*rustc, "--print", "sysroot"], text=True).strip()
    rustc_version = subprocess.check_output([*rustc, "-vV"], text=True)
    toolchain_record, toolchain_files = toolchain_notices(sysroot, rustc_version)
    upstream = UpstreamNotices(args.upstream_cache, args.fetch_upstream)
    collected, all_files = [], {"toolchain/" + name: data for name, data in toolchain_files.items()}
    for package in packages:
        checksum = checksums.get((package["name"], package["version"], package["source"]))
        if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise ValueError(f"Missing locked registry checksum: {package['name']}")
        result, files = collect_crate(package, checksum, upstream)
        node = next(node for node in metadata["resolve"]["nodes"] if node["id"] == package["id"])
        result["resolved_features"] = sorted(node["features"])
        collected.append(result)
        all_files.update({f"crates/{package['name']}-{package['version']}/{name}": data for name, data in files.items()})
    if (root / "Cargo.lock").read_bytes() != lock_data:
        raise ValueError("Cargo.lock changed during notice collection")
    # Write only after every crate passes. A failed run cannot leave a manifest
    # which appears to approve an incomplete notice bundle.
    output.mkdir(parents=True)
    for name, data in sorted(all_files.items()):
        destination = output / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    (output / "cargo-tree.txt").write_bytes(tree.encode("utf-8"))
    manifest = {"schema_version": 1, "application": {"name": project["name"], "version": project["version"]}, "target": args.target,
                "cargo_lock_sha256": sha256(lock_data), "dependency_edges": ["normal", "build"], "dev_dependencies": False,
                "cargo_tree_sha256": sha256(tree.encode()), "crate_count": len(collected), "crates": collected, "toolchain": toolchain_record,
                "scope": "Target-selected normal/build Rust crates and bundled epaint fonts. Includes build tools and installed toolchain notices conservatively; not a proof of every linked native/system library."}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    (output / "README.txt").write_text("Nova Flux Rust and embedded font notices\n\nOriginal copyright/license/NOTICE files are retained byte-for-byte under crates/.\nThe manifest records target selection, exact locked crate archive checksums,\nper-file SHA-256 hashes, and exact-commit upstream supplements.\nBuild tools and the installed Rust toolchain notice inventory are conservatively\nincluded. FFmpeg, native/system libraries and OS font files need separate review.\nSee docs/RUST_NOTICES.md in the source release for the release checklist.\n")
    print(json.dumps({"target": args.target, "crates": len(collected), "notice_files": len(all_files), "output": str(output)}))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"Notice collection failed: {error}")
