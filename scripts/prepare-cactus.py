#!/usr/bin/env python3
"""Stage pinned Cactus assets for research; not yet wired to ZFM packaging.

Run only in a trusted build directory. The staged binaries and models are not
included in ZFM releases until the runtime integration and packaging land.
"""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import ssl
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request

# Immutable Hugging Face revisions and LFS SHA-256 values. Keep in sync with
# upstream model cards, licensing review and tests before deliberately updating.
ROOT = 'https://huggingface.co/'
RUNTIME_REV = '1a028f5a3972b67dd8d8752f882113d475e7ec4c'
MODELS = (
    ('needle3.cact', 'Cactus-Compute/needle3/resolve/c95d25b4efa1e9601e785d92438e4ade0dbddbf8/needle3.cact', 35335380, 'c9d915eca282ed42d1a09b143b592adb4cc6744ffe2d294adf5cfc5548170c38'),
    ('whistle.cact', 'Cactus-Compute/whistle/resolve/ad73f1468dd5203791e0cc81d31544c553563645/whistle.cact', 16919407, 'b6e02f048568ac5d01a2042556c658061e699acbc0aa2a1439f52f3d461dffeb'),
    ('mirage.cact', 'Cactus-Compute/mirage/resolve/3d03cee88c0acbe4e71b94b567ab14e6e55f40c9/mirage.cact', 21611043, '4249a0f7515272ff76b9414bec17d7e7daf84b15b66c7fb4f17fce1e66f9f237'),
)
RUNTIMES = {
    'x86_64-unknown-linux-gnu': ('cactus', 'Cactus-Compute/cactus/resolve/' + RUNTIME_REV + '/releases/4.0.0/linux-x86_64/cactus', 1602448, '1233a27b65f2557ade0c965f17ba9de70a47e9f10c139539daca78814f946045'),
    'x86_64-pc-windows-msvc': ('cactus.exe', 'Cactus-Compute/cactus/resolve/' + RUNTIME_REV + '/releases/4.0.0/windows-x86_64/cactus.exe', 1618944, '3727bc70f4ad4f5dcf3719b90ec01a3a5cbf7a79da9db8424b8ea64c7c4db1b2'),
}


def permitted_url(url):
    parts = urllib.parse.urlsplit(url)
    host = (parts.hostname or '').lower()
    return (parts.scheme == 'https' and parts.username is None and parts.password is None
            and parts.port in (None, 443)
            and (host == 'huggingface.co' or host.endswith('.huggingface.co') or host.endswith('.hf.co')))


class HfRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not permitted_url(newurl):
            raise ValueError('Download redirected outside permitted Hugging Face HTTPS hosts')
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch(asset, directory, opener):
    name, path, size, digest = asset
    url = ROOT + path
    if not permitted_url(url):
        raise ValueError('Invalid asset URL')
    checksum = hashlib.sha256()
    remaining = size
    request = urllib.request.Request(url, headers={'User-Agent': 'ZFM-build-assets/1', 'Accept-Encoding': 'identity'})
    with opener.open(request, timeout=60) as response, (directory / name).open('xb') as output:
        while True:
            chunk = response.read(min(65536, remaining + 1))
            if not chunk:
                break
            if len(chunk) > remaining:
                raise ValueError('Asset exceeds pinned size: ' + name)
            output.write(chunk)
            checksum.update(chunk)
            remaining -= len(chunk)
        if remaining or checksum.hexdigest() != digest:
            raise ValueError('Asset size or SHA-256 mismatch: ' + name)


def stage(target, output, opener=None):
    if target not in RUNTIMES:
        raise ValueError('No pinned Cactus runtime for this target; 32-bit builds are not supported')
    output = Path(output)
    if output.exists() or output.is_symlink():
        raise ValueError('Output directory already exists')
    output.parent.mkdir(parents=True, exist_ok=True)
    if opener is None:
        opener = urllib.request.build_opener(HfRedirect(), urllib.request.HTTPSHandler(context=ssl.create_default_context()))
    # Build into a temporary sibling, then publish the directory only after all
    # four checks pass. This is not a defense against hostile parent directories.
    temp = Path(tempfile.mkdtemp(prefix='.cactus-stage-', dir=output.parent))
    try:
        for asset in (RUNTIMES[target],) + MODELS:
            fetch(asset, temp, opener)
        if target.endswith('linux-gnu'):
            (temp / 'cactus').chmod(0o755)
        if output.exists() or output.is_symlink():
            raise ValueError('Output directory appeared while staging')
        temp.rename(output)
    finally:
        if temp.exists():
            shutil.rmtree(temp)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('target', choices=sorted(RUNTIMES))
    parser.add_argument('output', help='new staging directory (must not exist)')
    args = parser.parse_args()
    try:
        stage(args.target, args.output)
    except (OSError, ValueError, urllib.error.URLError) as error:
        # Do not print redirected URLs: CDN query strings can contain credentials.
        status = 'HTTP ' + str(error.code) if isinstance(error, urllib.error.HTTPError) else type(error).__name__
        print('Could not stage Cactus assets (' + status + '). Check network access and pinned hashes.', file=sys.stderr)
        return 1
    print('Verified assets staged at', args.output, '(not yet integrated into ZFM)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
