#!/usr/bin/env python3
"""Basic drift checks, not a legal or complete SPDX validation."""
import hashlib
import json
import pathlib
import re

root = pathlib.Path(__file__).resolve().parent.parent

def read(path):
    return (root / path).read_text(encoding="utf-8")

def require(condition, message):
    if not condition:
        raise SystemExit(message)

# Avoid accidental changes to the standard terms and upstream reference texts.
expected = {
    'LICENSE': 'c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4',
    'LICENSES/rust-1.77.2/COPYRIGHT': 'bd0581fef622b3d8b25836cf70feb7e1a7a6171ea9e440b46dfda74c46a0ed0d',
    'LICENSES/rust-1.77.2/LICENSE-MIT': '23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3',
    'LICENSES/rust-1.77.2/LICENSE-APACHE': '62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a',
}
for path, digest in expected.items():
    require(hashlib.sha256((root / path).read_bytes()).hexdigest() == digest,
            f'{path} changed; review license terms and update check deliberately')
sbom = json.loads(read('SBOM.spdx.json'))
require(sbom['spdxVersion'] == 'SPDX-2.3' and sbom['dataLicense'] == 'CC0-1.0', 'unexpected SPDX metadata')
# Python 3.8 is the legacy container baseline: parse this simple Cargo.lock format
# without relying on tomllib or downloading a TOML package.
lock = read('Cargo.lock')
blocks = re.findall(r'\[\[package\]\]\s*\n(.*?)(?=\n\[\[|\Z)', lock, re.S)
locked = set()
for block in blocks:
    name = re.search(r'^name = "([^"]+)"$', block, re.M)
    version = re.search(r'^version = "([^"]+)"$', block, re.M)
    require(name and version, 'unrecognized Cargo.lock package')
    require(not re.search(r'^source\s*=', block, re.M), 'external Cargo dependency: review third-party inventory and SBOM')
    locked.add((name.group(1), version.group(1)))
packages = sbom['packages']
require(locked == {(p['name'], p['versionInfo']) for p in packages}, 'Cargo.lock and source SBOM drifted')
for path in ('Cargo.toml', 'installer/Cargo.toml'):
    manifest = read(path)
    package = re.search(r'(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)', manifest)
    require(package, f'missing package section in {path}')
    fields = dict(re.findall(r'^(name|version|license)\s*=\s*"([^"]+)"', package.group(1), re.M))
    require(fields.get('license') == 'Apache-2.0' and
            (fields.get('name'), fields.get('version')) in locked,
            f'{path} declaration drifted; review inventory')
require(all(p['licenseDeclared'] == 'Apache-2.0' and p['filesAnalyzed'] is False for p in packages), 'review SPDX package declarations')
for path in ('SECURITY.md', 'LEGAL/RELEASE_READINESS.md', 'LEGAL/THIRD_PARTY.md', 'LEGAL/PRIVACY.md'):
    require((root / path).is_file(), f'missing {path}')
print('Legal drift checks passed (not legal approval or a full SPDX validation).')
