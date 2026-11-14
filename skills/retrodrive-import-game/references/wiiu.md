# Wii U import

For a game dumped as a folder with `code`, `content`, and `meta`, preserve its internal layout. RetroBat can detect the game's `.rpx` when the folder is placed under `roms\wiiu`. To create a named launch entry for that layout, put `<Display Name>.m3u` in `roms\wiiu` and use the actual folder and executable names.

```text
.\<Game Folder>\code\<Game>.rpx
```

For a game installed in Cemu, use **Game directory** in Cemu to find the base game's executable. Compute the relative path from the playlist directory as seen by RetroBat. For example, if the playlist is in the drive root's `roms\wiiu`, use this path to the verified `.rpx` on the same drive.

```text
.\..\..\RetroBat\saves\wiiu\cemu\mlc01\usr\title\00050000\<Title ID Low>\code\<Game>.rpx
```

Use one line per launch entry and verify the resolved `.rpx` before writing it. Check for an existing RetroBat entry for the same game before adding an alias. Refresh the game list to verify the entry. [RetroBat Wii U documentation](https://wiki.retrobat.org/systems-and-emulators/supported-game-systems/game-consoles/nintendo-game-consoles/wiiu)
