# Release readiness and audit scope

**ACTION REQUIRED — not approved for worldwide distribution.** Audit date: 2026-11-12. Evidence covers the working tree based on commit `93cfdc7` and its available Git history, not a legal opinion, binary reverse engineering, formal security assessment or jurisdiction-by-jurisdiction certification. The v0.0.8 tag points to older source; changes here do not amend published artifacts.

| Priority | Finding | Owner action / evidence |
|---|---|---|
| P0 ACTION REQUIRED | Ownership and contribution rights, AI/asset provenance unknown | Resolve [ownership](OWNERSHIP.md) and [assets](ASSET_PROVENANCE.md), retain written evidence privately. |
| P0 ACTION REQUIRED | Exact distributed runtime and Microsoft CRT attribution/redistribution not resolved | Review actual x86 and x64 binaries and MSVC terms, produce release-specific notices and SBOM; [third-party](THIRD_PARTY.md). Rust 1.77.2 upstream reference notices now included, not an exhaustive conclusion. |
| P0 ACTION REQUIRED | No private vulnerability reporting endpoint enabled | Enable and verify a private channel before inviting sensitive reports; [SECURITY](../SECURITY.md). |
| P1 NEEDS REVIEW | Name conflicts and legal entity | Search registers; verify entity/authority before branding or applications. [trademarks](TRADEMARKS.md), [Pakistan](PAKISTAN.md). |
| P1 NEEDS REVIEW | Privacy, terms, and consumer law | Identify actual operator and review [privacy](PRIVACY.md), [terms](TERMS.md) with appropriate counsel. |
| P1 ACTION REQUIRED | Signing and platform testing | [distribution](DISTRIBUTION.md); unsigned Windows builds; Mint 19 and Windows 7 are not verified. |

Statuses mean: VERIFIED = evidence observed, not legal certification; IMPLEMENTED = repository change made; NEEDS REVIEW = expert/owner decision; ACTION REQUIRED = unfinished action; UNVERIFIED = evidence insufficient; NOT APPLICABLE = out of current scope, subject to change. Review after changes and before each release. No dates, signatures or approval are implied.
