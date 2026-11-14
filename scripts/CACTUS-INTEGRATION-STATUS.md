# Cactus model integration — not implemented

ZFM does **not** currently include or run Needle 3, Whistle or Mirage. The
asset staging script is groundwork only: it does not modify package builds or
application features, and is not a user-facing installer. Do not advertise it
as a working AI feature.

`scripts/prepare-cactus.py TARGET OUTPUT` downloads the official Cactus 4.0.0
native runner and all three `.cact` files to a new build staging directory,
checks exact lengths and SHA-256 before publishing the directory, and leaves
no assets there on a failure. It permits HTTPS redirects only to Hugging Face
hosts. The output path should be in `artifacts/` or another build-only location,
not committed to the repository. Asset metadata is pinned in the script and
verified by unit tests using mock bytes; **the real downloads, binary behavior,
compatibility and packaging have not been tested** in this environment. The
runner plus models total roughly 75 MB. This staging process accesses the
network at build time; model inference behavior and network isolation are not
established by checksum verification.

Only the published Linux x86-64 and Windows x64 runners are pinned. The current
release also targets Linux i686 and Windows i686, for which this script fails
explicitly rather than substituting a different or cloud model. Whether the
64-bit runners work on the project's oldest supported OS versions and CPUs is
unverified. Upstream Cactus Python tooling has optional telemetry and automatic
network downloads; the plan is not to silently use that tooling at runtime.

Upstream describes Needle 3 as tool calling, extraction and text embeddings;
Whistle as speech transcription/embeddings; and Mirage as image understanding
and structured extraction. They are not interchangeable, and their output may
be incorrect. Review proposed filesystem operations explicitly and never treat
model output as trusted code or silently delete/overwrite user files.

Before this can ship as built-in functionality, the project still needs:

- Executable or library integration with checked protocol/ABI, bounded input,
  output, runtime, memory and cancellation; safe handling of untrusted images,
  audio and text; and explicit offline/privacy verification.
- Real end-to-end tests against all three pinned model files, with Linux and
  Windows x64, supported media, error handling, subprocess lifecycle and CPU/
  OS compatibility checks; a supported solution or explicit unavailability on
  both x86 builds.
- Native file-manager interactions for text, audio, image and reviewed file
  actions, including accessibility, cancellation and user-visible errors.
- Installer/Debian package integration, reproducible fetching and verification,
  upstream license/NOTICE inventory and SBOM updates, plus distribution review
  for every included runtime and model. An upstream model card's stated license
  is not legal clearance.
- Review of the Linux x64 runner's provenance and security: Hugging Face's
  current metadata reports one antivirus engine flagged the binary. This alone
  does not establish a vulnerability or a false positive; investigate before
  distribution.
