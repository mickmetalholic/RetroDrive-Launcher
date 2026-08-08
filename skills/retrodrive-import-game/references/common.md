# Common import rules

## Plan output

Every plan should include:

- source path and whether it is a file or directory;
- detected platform and confidence;
- title ID or serial when available;
- source file count, total bytes, and selected hashes;
- exact destination under `D:\roms` or `D:\RetroBat\saves`;
- backup and staging paths;
- whether a conversion or emulator-owned installation is required;
- final validation checks.

## Naming

Use the existing library's English display name for the launch entry. Preserve region, revision, and language information in metadata when it is known, but do not invent a serial or version. Keep filesystem names Windows-safe and avoid trailing spaces, reserved device names, and control characters.

## Staging and rollback

Stage on the same volume as the final target so a final rename is atomic where possible. Move an existing conflicting target to a dated backup before replacing it. If an operation fails, move the partial target aside, restore the backup, and report both paths. Keep the source untouched.

## Completion evidence

A successful import must have at least one content-specific invariant beyond process exit code: a valid header, a matching hash, a required license, a completed emulator log line, or a successful RetroBat launch. Record failures as failures instead of downgrading them to warnings.
