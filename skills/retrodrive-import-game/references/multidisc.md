# Multi-disc import

Prepare linked images and playlists manually. The single-file helper rejects `.m3u`, `.cue`, `.gdi`, `.ccd`, and `.mds` inputs because their referenced assets must be handled together.

Keep disc images beside one another under the platform's ROM directory and create one `.m3u` launcher file. Each line must be a trimmed, relative filename that exists beside the playlist. Reject absolute paths, parent traversal, blank lines, and lines containing control characters.

For `.cue`, `.gdi`, and similar descriptors, retain every required track file and update descriptor filenames when renaming tracks. Verify all references before claiming the set is complete.

For Vita3K installed content, use a `.psvita` stub instead of an `.m3u` title-ID file.
