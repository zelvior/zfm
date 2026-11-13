#!/usr/bin/env python3
"""Small repeatable GUI baseline; run under X11, e.g. xvfb-run -a python3 ... ZFM.
Reports startup to first visible window and process RSS and CPU over an idle
interval. Not a responsiveness benchmark, and not representative of all PCs.
"""
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

if len(sys.argv) != 2 or not os.environ.get('DISPLAY'):
    sys.exit('Usage under X11: python3 tests/benchmark-linux.py /path/to/zfm')
binary = os.path.abspath(sys.argv[1])
with tempfile.TemporaryDirectory(prefix='zfm-bench-') as directory:
    # Deterministic small-directory fixture. Timing begins after its creation.
    for i in range(1000):
        Path(directory, 'file-%04d.txt' % i).touch()
    started = time.monotonic()
    process = subprocess.Popen([binary, directory], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                               env=dict(os.environ, GDK_BACKEND='x11'))
    try:
        while True:
            if process.poll() is not None:
                sys.exit('ZFM exited: ' + process.stderr.read().decode('utf-8', 'replace'))
            result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(process.pid), '--name', '^ZFM'],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if result.returncode == 0:
                break
            if time.monotonic() - started > 15:
                sys.exit('Timed out waiting for window')
            time.sleep(0.02)
        startup = time.monotonic() - started
        time.sleep(2)
        status = Path('/proc/%d/status' % process.pid).read_text()
        values = dict(line.split(':', 1) for line in status.splitlines() if ':' in line)
        # /proc stat's command is parenthesized and may contain spaces.
        fields = Path('/proc/%d/stat' % process.pid).read_text().rsplit(')', 1)[1].split()
        cpu = (int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK')
        message = ('ZFM baseline: 1000 files; first visible window %.3fs; '
                   'RSS %s; peak RSS %s; process CPU %.3fs during startup + 2s idle' %
                   (startup, values.get('VmRSS', '?').strip(), values.get('VmHWM', '?').strip(), cpu))
        print(message)
        if os.environ.get('GITHUB_ACTIONS'):
            print('::notice::' + message)
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
