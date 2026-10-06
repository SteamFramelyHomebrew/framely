# Terminal and files

[简体中文](../zh-CN/user-guide/terminal-and-files.md)

[Documentation](../README.md) · [Network panel](network-panel.md)

## Manager navigation

The top switcher contains **Plugin / APK / Terminal / Files / Settings**. Opening the device IP without a fragment starts in Plugin. Each section remembers its last subpage in the browser. Existing `#settings`, `#launcher-settings`, `#about` and `#updates` links continue to work.

Plugin contains installed plugins, the plugin library, sources and notification settings, including Framely's notifications. Plugin-specific settings remain in each plugin's management dialog. APK keeps its apps, containers and retained-data cleanup. Settings contains General, Launcher, Updates and About.

## Terminal

Create a terminal to open the configured Steam session user's login shell in their home directory, normally `steamos`. This is a real PTY: Ctrl-C, Tab completion, arrows, Unicode input, window resizing and full-screen programs such as Vim and top work. Use `sudo` inside the terminal when administrator privileges are needed; the panel password is separate from the user's sudo password.

You can create up to 12 sessions, rename them, reconnect or explicitly close them. Changing pages, closing a browser or losing the network leaves the shell running. Restarting the user-session service or Framely ends the sessions; processes are not restored after restart. Closing a session requires confirmation and releases the shell, its session's child processes and PTY.

Only one connection can type in a session. Other connections are read-only; **Take control** transfers input ownership. While disconnected, input is disabled. Reconnection never replays queued keystrokes and restores only the retained output. Each session has a 2 MiB output buffer; a notice appears when older output was discarded. Replayed raw terminal output can lose earlier screen context after truncation.

Terminal input, sudo passwords and output are not written to Framely logs. Output is rendered as terminal characters, not HTML; terminal clipboard-write escape sequences are ignored. xterm.js and its fit addon are included with the offline UI, without CDN requests.

Use **Ctrl+Shift+C** to copy selected terminal text and **Ctrl+Shift+V** to paste. **Ctrl+C** still interrupts the running program; it is not the copy shortcut. The footer shows these shortcuts.

## Browse and organize files

Files opens the user's home directory and accesses the filesystem with that user's permissions. It never automatically elevates privileges. Permission changes are limited to ordinary Unix permission bits; use Terminal and `sudo` for administrator operations.

Use the address bar, breadcrumbs, parent button or expandable folder tree. Quick access includes Home, Downloads, mounted storage, the filesystem root, the recycle bin and your saved folders. Add the current folder using the star button; rename, reorder or remove its shortcut with the adjacent controls. Unavailable folders keep their saved shortcut and display the access error when opened.

Drag the divider between the sidebar and file list to resize it; the width is remembered in this browser and limited automatically on narrow windows. Quick access stays in a fixed-height upper section. The folder tree fills the remaining sidebar height; both lists scroll independently, with their headings kept visible. Opening a folder in the file list or address bar expands the tree to its location and highlights it.

The file list provides hidden-file visibility, sorting, checkboxes for multiple selection, current-folder filtering and cancellable recursive filename search. Recursive search does not follow directory links. Lists are paged in groups of 200; recursive results are limited to 5,000 matches and 500,000 visited entries. Media contents are never scanned to create thumbnails.

**Actions** provides copy, cut, paste, rename, delete, properties, permission editing, compression, extraction and opening files. Right-click a file or folder to open a menu at the pointer; right-clicking a selected item preserves the multi-selection. Right-click the empty list area for new file/folder, paste, upload and refresh actions. Escape or clicking elsewhere closes the menu. Symbolic links have an arrow marker and their target in Properties. Copy preserves links; deleting a link does not delete its target. Special filesystem objects cannot be copied or archived.

Copy, move, upload and extraction let you skip conflicts, keep both names or confirm replacement for the batch. Replacement of a same-name folder replaces that whole folder; it does not merge its contents. Long tasks show actual bytes or completed entries, can be cancelled, and report individual failures. Completed items remain completed when a later item fails or the task is cancelled. Switching manager sections keeps active file work and editor state in the current page.

## Recycle bin

Delete defaults to **Move to recycle bin (30 days)**. Permanent deletion requires an explicit choice and confirmation. Framely keeps trash on the source filesystem: home-directory trash or a private `.framely-trash-UID` directory at the storage mount. If that storage cannot support trash, the operation reports an error and does not fall back to permanent deletion.

The recycle bin supports restoring with an unused name or keeping both copies, permanent removal and clearing all tracked items. Restore needs the original parent directory to exist. Framely cleans expired records about hourly while the session service runs, and only removes items recorded in its own recycle bin. Unavailable storage is retained until accessible again. Other applications' trash is unaffected.

## Transfer and edit

Upload multiple files or a folder from your browser. Transfers use 1 MiB chunks, byte progress and private temporary files; only a completed upload receives its final filename. Cancelled uploads are removed and hourly maintenance removes transfers idle for at least an hour. A stopped service may leave clearly named `.framely-upload-*` temporary files, never a falsely complete final file. Uploads are limited to 64 GiB per file and 64 simultaneous transfer records.

Download a single file directly, or select multiple files/folders to download a ZIP. Download links expire after an hour. Large files stream without loading the entire file into memory; single-byte ranges support video seeking and resuming downloads.

Opening an ordinary file attempts the UTF-8 editor, limited to 5 MiB. Invalid UTF-8, binary data and larger files cannot be edited; use Download instead. Saving checks a content revision. External changes require reloading the file or explicitly confirming overwrite. Unsaved changes require confirmation before closing the editor.

## Archives and media

Create ZIP, TAR or TAR.GZ archives; the output filename selects the format. Choose a destination and conflict behavior when extracting. Archive operations reject absolute paths, parent traversal, symbolic/hard links and special files; extracted contents are never executed. Archive limits are 50,000 entries and 64 GiB expanded data. Cancelling removes staging files and leaves already committed items intact.

Select an image or video to preview it. PNG, JPEG, GIF, WebP, AVIF, BMP, MP4, WebM and MOV use the browser's native decoder; actual codec support depends on the browser. Video controls provide playback, seeking and fullscreen. Unsupported formats can be downloaded; there is no server-side transcoding. HTML and SVG are not opened as same-origin pages.

## Access and privacy

Terminal, file and transfer routes use the existing authenticated manager connection and port. Mutating routes and the terminal WebSocket require an exact matching Origin; plugin sandboxes cannot use them. The service runs as the configured Steam user, independently of the root daemon. Preview/download content is read only after explicit selection. Test media are generated fixtures; Framely does not automatically read or upload Arcturus recordings.
