# PlayStation 3 import

Package and update installation is manual; there is no scripted PS3 package installer in this skill.

Keep standalone PS3 disc images in the shared ROM library when RPCS3 can read them directly. Use `-Platform ps3` for the helper's copy route. Install user-supplied firmware, PKG content, and sequential updates through RPCS3 into `RetroBat\saves\ps3\rpcs3` or its configured data path.

Do not decrypt or rebuild an ISO speculatively. First perform a read-only RPCS3 readability check. Only extract, decrypt, or install an update when the emulator reports that it is required, and record the original image hash before doing so.

Updates are applied in version order. Keep the original ISO and update packages in the material library; the installed result belongs to RPCS3's managed data directory.
