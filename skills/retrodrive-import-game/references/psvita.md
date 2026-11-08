# PlayStation Vita import

Vita3K owns installed Vita content. Do not place the full Vita game payload under `roms\psvita`.

## Source recognition

The helper accepts one unpacked game containing:

```text
app\<TITLE_ID>\sce_sys\param.sfo
```

Read the title ID from the content and directory structure. Split sources containing multiple titles into separate imports. Handle `.vpk`/PKG packages, standalone patches, and DLC through Vita3K's own installation workflow. An accompanying `app\<TITLE_ID>\sce_sys\package\work.bin` may supply license material; let Vita3K manage installation and decryption.

## Runtime layout

```text
RetroBat\saves\psvita\vita3k\ux0\app\<TITLE_ID>
RetroBat\saves\psvita\vita3k\ux0\license\<TITLE_ID>\*.rif
roms\psvita\<Display Name> [<TITLE_ID>].psvita
```

Paths are relative to the drive root. Keep internal title-ID directories unchanged. Create a zero-length `.psvita` launch entry with the title ID in brackets only after installation checks pass; use this entry instead of a title-ID `.m3u` file.

## Installation checks

- Vita3K reports successful content installation and exits normally.
- No `ux0\app\<TITLE_ID>_dec` remains after completion.
- `ux0\app\<TITLE_ID>\eboot.bin` begins with `53 43 45 00` (`SCE\0`).
- At least one 512-byte `.rif` file exists below `ux0\license\<TITLE_ID>`.

The header and file-size checks do not prove successful decryption or a usable license. When runtime verification is requested, launch through RetroBat, capture the `-r <TITLE_ID>` invocation, and observe the game reaching a title screen or equivalent running state.

## Failure handling

Do not terminate an active installation or move its files while Vita3K is still running. On a timeout, preserve the process, current files, and backup workspace; report the unresolved installation. Once the process has stopped, preserve failed content and restore the previous installation. An encrypted SELF or license error is an import failure, even when structural checks passed.
