# Common import rules

## Plan output

Every plan should include:

- selected drive root, source path, and whether the source is a file or directory;
- original filename, resolved display name, proposed target filename, and naming evidence;
- detected platform and confidence;
- title ID or serial when available;
- source file count, total bytes, and selected hashes;
- exact destination under the selected drive's `roms` or emulator storage;
- backup and staging paths;
- whether a conversion or emulator-owned installation is required;
- final validation checks.

## Naming

Resolve the English display name from the existing library, game metadata, or the user's supplied name. Preserve known region, revision, and language tags. Clarify uncertain identities; do not invent titles or edition details.

Pass the resolved filename stem through `-DisplayName`, without the ROM extension. For example, `-DisplayName "Game Title (USA) (Rev 1)"` produces `Game Title (USA) (Rev 1).nsp` for an `.nsp` source. Vita launch entries use `<Display Name> [<TITLE_ID>].psvita`. The script preserves ROM extensions and makes names Windows-safe; Codex resolves the title.

Show original and proposed names before applying them. Rename imported files and launch entries while preserving source names and emulator-owned title-ID directories. Update references together when handling linked assets.

## Staging and rollback

Stage on the target volume and back up conflicting targets before replacement. On failure, preserve partial output and restore backups after confirming the emulator has stopped. Report recovery paths and unresolved failures. Never delete sources or saves as cleanup.

## Completion evidence

Distinguish these results:

- **Copied:** final SHA-256 matches the source.
- **Installed:** the emulator reports completion and its required layout checks pass.
- **Runtime verified:** a requested RetroBat test reaches the game's title screen or another observable running state.

A matching header or license-file size is a structural check; it does not establish decryption, license validity, or playability. Report failed checks as failures.
