# PlayStation 3 import

Keep PS3 disc images in the shared ROM library when RPCS3 can read them directly. Install firmware, PKG content, and sequential updates into RPCS3-owned storage under `RetroBat\saves\ps3\rpcs3` or the configured RPCS3 data path.

Do not decrypt or rebuild an ISO speculatively. First perform a read-only RPCS3 readability check. Only extract, decrypt, or install an update when the emulator reports that it is required, and record the original image hash before doing so.

Updates are applied in version order. Keep the original ISO and update packages in the material library; the installed result belongs to RPCS3's managed data directory.
