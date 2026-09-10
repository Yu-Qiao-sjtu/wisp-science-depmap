# Clipboard file context

On Windows, copy one or more files in Explorer (Ctrl+C), focus the chat composer,
and press Ctrl+V. Each file appears as a removable **File path** card. Hover over
the card to see its full path. Repeated pastes do not duplicate the same path.
Directories can also be referenced. On macOS, copy files in Finder with
Command+C and paste into the composer with Command+V. Wisp reads Finder's
native `NSPasteboardTypeFileURL` items with the same path-only behavior.

Sending includes the paths as ordinary message context. Wisp does not copy the
files into the project, upload their bytes, or automatically load referenced
images. The agent can subsequently use its normal tools and access rules to
inspect a referenced file. Paths refer to the local computer; a remote execution
context does not automatically receive those files.

Screenshot/image-data paste keeps the existing image upload behavior. Plain text
paste is unchanged. File-URL paste (`text/uri-list`) is supported where the
WebView exposes it (including other desktop environments).

Manual Windows smoke check: copy two files (including an image and a filename
with spaces/Chinese characters), paste in chat, verify the card names and full
path tooltips, paste again, remove one card, then send with no typed message.
Verify only the remaining path is sent and no project upload is created. Also
check ordinary text and a Snipping Tool screenshot still paste normally.
