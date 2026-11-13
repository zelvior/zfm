// ZFM Office renderer bridge. The Node executable, this script, and its
// pinned dependencies must be installed together in a trusted directory.
// Input is received through stdin; only a bounded PDF is emitted on stdout.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { createConverter } from '@deepseek-ai/libreoffice-kit';

const MAX_INPUT = 64 * 1024 * 1024;
const MAX_OUTPUT = 64 * 1024 * 1024;
const allowed = new Set(['doc', 'docx', 'xls', 'xlsx', 'ppt', 'pptx', 'odt', 'ods', 'odp']);
const extension = process.argv[2];
if (!allowed.has(extension)) throw new Error('Unsupported document type');
const data = fs.readFileSync(0);
if (data.length === 0 || data.length > MAX_INPUT) throw new Error('Invalid document size');

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const resolveEnginePackage = name => {
  // Never discover an unbundled native engine through Node's resolution path.
  if (name !== '@deepseek-ai/libreoffice-kit-wasm') {
    const error = new Error('Native engine not bundled');
    error.code = 'MODULE_NOT_FOUND';
    throw error;
  }
  return require.resolve('@deepseek-ai/libreoffice-kit-wasm/package.json', { paths: [here] });
};
let directory;
let converter;
try {
  directory = fs.mkdtempSync(path.join(os.tmpdir(), 'zfm-office-'));
  // mkdtempSync uses mode 0700 on Unix; the Windows temp directory must be
  // private to the user. A forced termination can leave this directory behind.
  const input = path.join(directory, `input.${extension}`);
  const output = path.join(directory, 'output.pdf');
  fs.writeFileSync(input, data, { mode: 0o600, flag: 'wx' });
  converter = await createConverter({
    timeoutMs: 90000,
    maxInputBytes: MAX_INPUT,
    maxOutputBytes: MAX_OUTPUT,
    maxArchiveEntries: 10000,
    maxUncompressedBytes: 256 * 1024 * 1024,
    fontMetadataCacheDirectory: false,
    resolveEnginePackage,
  });
  await converter.render({ inputPath: input, outputPath: output });
  const stat = fs.statSync(output);
  if (!stat.isFile() || stat.size === 0 || stat.size > MAX_OUTPUT) throw new Error('Invalid PDF output');
  const pdf = fs.readFileSync(output);
  if (!pdf.subarray(0, 5).equals(Buffer.from('%PDF-'))) throw new Error('Invalid PDF output');
  let offset = 0;
  while (offset < pdf.length) offset += fs.writeSync(1, pdf, offset, pdf.length - offset);
} finally {
  if (converter) await converter.dispose();
  if (directory) fs.rmSync(directory, { recursive: true, force: true });
}
