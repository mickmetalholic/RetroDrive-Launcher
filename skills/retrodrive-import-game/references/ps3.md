# PlayStation 3 import

Package and update installation is manual; there is no scripted PS3 package installer in this skill.

Keep standalone PS3 disc images in the shared ROM library when RPCS3 can read them directly. Use `-Platform ps3` for the helper's copy route. Install user-supplied firmware, PKG content, and sequential updates through RPCS3 into `RetroBat\saves\ps3\rpcs3` or its configured data path.

Do not decrypt or rebuild an ISO speculatively. First perform a read-only RPCS3 readability check. Only extract, decrypt, or install an update when the emulator reports that it is required, and record the original image hash before doing so.

Updates are applied in version order. Keep the original ISO and update packages in the material library; the installed result belongs to RPCS3's managed data directory.

## RetroBat launch entry for an installed game

Confirm the base game and its title ID in RPCS3. For the standard RetroDrive layout, verify that `RetroBat\saves\ps3\rpcs3\dev_hdd0\game\<TITLE_ID>\USRDIR\EBOOT.BIN` exists. Create `roms\ps3\<Display Name>.m3u` with one line using the verified title ID.

```text
SAVESPATH\dev_hdd0\game\<TITLE_ID>\USRDIR\EBOOT.BIN
```

For a custom RPCS3 data path, verify the game in RPCS3 and use `GAMEID:<TITLE_ID>` as the single line. Verify the launch entry shows up in RetroBat after refreshing the game list. [RetroBat PS3 documentation](https://wiki.retrobat.org/systems-and-emulators/supported-game-systems/game-consoles/sony/playstation-3)
