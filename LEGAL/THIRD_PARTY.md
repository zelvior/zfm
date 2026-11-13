<!-- Modified: record prospective upstream archive-engine integration and its unresolved obligations. -->
# Third-party inventory and obligations

**ACTION REQUIRED.** `Cargo.lock` contains only local workspace packages `zfm` and `zfm-installer` 0.1.0. This is not a complete executable SBOM: Rust's standard library and its own dependencies are outside this lockfile; Linux system libraries and Windows runtime are also outside it.

| Component | Relationship / evidence | License and action |
|---|---|---|
| Rust 1.77.2 standard library and bundled components | release workflow pins rustc 1.77.2; Rust code may be statically linked into both binaries | [std manifest](https://github.com/rust-lang/rust/blob/1.77.2/library/std/Cargo.toml) declares MIT OR Apache-2.0, but transitive/in-tree exceptions vary. Upstream versioned COPYRIGHT and license texts included in `LICENSES/rust-1.77.2`, and appended to installed README. **NEEDS REVIEW**: trace included objects and per-component notices for each target; upstream COPYRIGHT alone may not cover all vendored dependencies. |
| Microsoft CRT / Windows SDK | release workflow uses MSVC `+crt-static` | **UNVERIFIED** exact libraries, versions, applicable redistribution terms and required notices. Review actual toolchain agreement/redist list and linked output for each architecture. Do not substitute an invented license. |
| GTK 3/GIO | Linux loads system libraries at runtime | GTK LGPL-2.1-or-later; dependencies/versions vary; not bundled by current deb. See [GTK](https://github.com/GNOME/gtk). If redistributed or modified, assess all applicable LGPL and other obligations. |
| libarchive | Linux loads system `libarchive.so.13` | [COPYING](https://github.com/libarchive/libarchive/blob/master/COPYING) lists BSD-family and per-file exceptions; system version varies. Inventory applicable components if bundled. |
| libc, OS libraries, xdg-utils | system-provided Linux runtime / `xdg-open` | Versions and licenses depend on distribution. Review actual binary and packaged dependencies, especially if bundled. |
| Windows system APIs | provided by Windows | Microsoft terms apply; verify SDK/runtime separately. |
| rustup, compiler, build tools and GitHub Actions | build-time tooling; network access in scripts | Not all become binary contents. Check versions, provenance and terms independently. |

Upstream Rust notice files are unchanged from [Rust 1.77.2](https://github.com/rust-lang/rust/tree/1.77.2). Inclusion is a conservative reference, not a claim all 1.77.2 components are in ZFM or that every required attribution is resolved. Custom toolchains/binaries need their own inventory; release managers must verify actual contents and distribute all applicable notices alongside every binary. See [SBOM](SBOM.md).

## Proposed archive engine: not integrated or distributed

The requested archive engine is [7-Zip](https://github.com/ip7z/7zip), reviewed at upstream commit `d3296e39e1f1f92c7b41c0c3848e65640428d42d`. ZFM currently does **not** include, invoke, or ship this project; merely recording it here does not add archive support. The upstream [license file](https://github.com/ip7z/7zip/blob/d3296e39e1f1f92c7b41c0c3848e65640428d42d/DOC/License.txt) identifies LGPL-2.1-or-later as the default, BSD-licensed and public-domain components, and RAR decoder files subject to an additional unRAR restriction. The exact obligations depend on the files and binaries actually included. Before distributing any build, inventory the pinned source and binary components and transitive dependencies, retain applicable notices, provide required license texts and source/relinking rights as applicable, review the unRAR restrictions and trademark use, and verify provenance and security updates. Rebranding the ZFM interface or executable does not remove these obligations. This note is not legal clearance.
