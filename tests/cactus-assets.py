#!/usr/bin/env python3
"""Offline tests for Cactus staging; no models or runtime are included."""
import hashlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('prepare_cactus', ROOT / 'scripts/prepare-cactus.py')
assets = importlib.util.module_from_spec(spec)
spec.loader.exec_module(assets)


class Opener:
    def __init__(self, data):
        self.data = data
        self.requests = []

    def open(self, request, timeout):
        self.requests.append(request.full_url)
        return io.BytesIO(self.data)


class AssetTests(unittest.TestCase):
    def setUp(self):
        self.old_models, self.old_runtimes = assets.MODELS, assets.RUNTIMES
        self.data = b'fixture-asset'
        digest = hashlib.sha256(self.data).hexdigest()
        assets.MODELS = tuple((n + '.cact', 'model/resolve/revision/' + n, len(self.data), digest)
                              for n in ('needle3', 'whistle', 'mirage'))
        assets.RUNTIMES = {'x86_64-unknown-linux-gnu': ('cactus', 'runtime/resolve/revision/cactus', len(self.data), digest)}

    def tearDown(self):
        assets.MODELS, assets.RUNTIMES = self.old_models, self.old_runtimes

    def test_manifest_is_pinned_and_bounded(self):
        self.assertEqual([asset[0] for asset in self.old_models], ['needle3.cact', 'whistle.cact', 'mirage.cact'])
        self.assertEqual(set(self.old_runtimes), {'x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc'})
        for name, path, size, digest in self.old_models + tuple(self.old_runtimes.values()):
            self.assertNotIn('..', name)
            self.assertTrue(assets.permitted_url(assets.ROOT + path))
            self.assertGreater(size, 0)
            self.assertLess(size, 40 * 1024 * 1024)
            self.assertEqual(len(digest), 64)
            self.assertEqual(len(bytes.fromhex(digest)), 32)

    def test_stage_verifies_and_refuses_replacement(self):
        with tempfile.TemporaryDirectory() as root:
            output = Path(root) / 'assets'
            opener = Opener(self.data)
            assets.stage('x86_64-unknown-linux-gnu', output, opener)
            self.assertEqual(sorted(p.name for p in output.iterdir()), ['cactus', 'mirage.cact', 'needle3.cact', 'whistle.cact'])
            self.assertEqual((output / 'whistle.cact').read_bytes(), self.data)
            self.assertEqual(len(opener.requests), 4)
            with self.assertRaises(ValueError):
                assets.stage('x86_64-unknown-linux-gnu', output, opener)
            self.assertEqual((output / 'whistle.cact').read_bytes(), self.data)

    def test_corruption_short_and_oversize_leave_no_stage(self):
        with tempfile.TemporaryDirectory() as root:
            output = Path(root) / 'assets'
            for data in (b'x' * len(self.data), b'x', self.data + b'x'):
                with self.assertRaises(ValueError):
                    assets.stage('x86_64-unknown-linux-gnu', output, Opener(data))
                self.assertFalse(output.exists())
                self.assertEqual(list(Path(root).iterdir()), [])

    def test_failure_after_some_downloads_is_not_published(self):
        class LaterFailure(Opener):
            def open(inner, request, timeout):
                inner.requests.append(request.full_url)
                return io.BytesIO(inner.data if len(inner.requests) < 3 else b'corrupt')
        with tempfile.TemporaryDirectory() as root:
            output = Path(root) / 'assets'
            opener = LaterFailure(self.data)
            with self.assertRaises(ValueError):
                assets.stage('x86_64-unknown-linux-gnu', output, opener)
            self.assertEqual(len(opener.requests), 3)
            self.assertFalse(output.exists())
            self.assertEqual(list(Path(root).iterdir()), [])

    def test_unknown_platform_and_redirects_fail_closed(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(ValueError):
                assets.stage('i686-pc-windows-msvc', Path(root) / 'assets', Opener(self.data))
        for url in ('http://huggingface.co/a', 'https://evil.example/a', 'https://huggingface.co.evil.example/a', 'https://user@huggingface.co/a'):
            self.assertFalse(assets.permitted_url(url), url)
        self.assertTrue(assets.permitted_url('https://huggingface.co/a'))
        self.assertTrue(assets.permitted_url('https://cas-bridge.xethub.hf.co/a'))
        with self.assertRaises(ValueError):
            assets.HfRedirect().redirect_request(None, None, 302, '', {}, 'https://evil.example/a')


if __name__ == '__main__':
    unittest.main()
