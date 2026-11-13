# Archive fixtures

`viewer-safe.7z` contains two small original text files and one directory.
`viewer-compressed.7z` contains the same files using LZMA2. `viewer-link.7z`
contains a symbolic link, `viewer-traversal.7z` has a `../escape` entry, and
`viewer-encrypted.7z` uses encrypted metadata. The last three must be rejected.

These fixtures were generated using the upstream 7-Zip console built from
commit `d3296e39e1f1f92c7b41c0c3848e65640428d42d`:

- stored, link and traversal: `a -t7z -m0=Copy -mhc=off`, with `-snl` for
  the link and `-si../escape` for the traversal input;
- compressed: `a -t7z -m0=LZMA2 -mhc=off`;
- encrypted: `a -t7z -pfixture -mhe=on` (the password is a public test value).

The fixtures contain no upstream source or binary code. ZFM's release packages
bundle and invoke a separately built, renamed 7-Zip console engine; see
`vendor/7zip/README.md` and the project legal notices.
