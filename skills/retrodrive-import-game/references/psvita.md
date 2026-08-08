# PlayStation Vita import

Vita3K owns installed Vita content. Do not place the full Vita game payload under `roms\psvita`.

## Source recognition

An unpacked NoNpDrm source normally contains one or more of:

```text
app\<TITLE_ID>\sce_sys\param.sfo
app\<TITLE_ID>\sce_sys\package\work.bin
patch\<TITLE_ID>\sce_sys\param.sfo
```

Use the title ID from the directory and `param.sfo`, not from a guessed display name. Vita3K must be allowed to copy, decrypt, install its license, and finish its temporary `<TITLE_ID>_dec` replacement.

## Runtime layout

```text
D:\RetroBat\saves\psvita\vita3k\ux0\app\<TITLE_ID>
D:\RetroBat\saves\psvita\vita3k\ux0\license\<TITLE_ID>\*.rif
D:\roms\psvita\<Display Name> [<TITLE_ID>].psvita
```

The `.psvita` file is a zero-length launch stub. Its bracketed title ID is read by RetroBat's Vita3K generator. A title-ID `.m3u` containing a newline can be interpreted as an invalid Windows path.

## Verification

- No `ux0\app\<TITLE_ID>_dec` remains after completion.
- `ux0\app\<TITLE_ID>\eboot.bin` begins with `53 43 45 00` (`SCE\0`).
- At least one 512-byte license exists below `ux0\license\<TITLE_ID>`.
- The RetroBat log contains a Vita3K launch with `-r <TITLE_ID>`.
- The game window reaches its title or an equivalent running state.

If the emulator reports an encrypted SELF or a missing/corrupt license, classify the import as failed and preserve the failed directory for diagnosis.
