//! Read file references from the OS clipboard without opening or importing files.

#[tauri::command]
pub(crate) fn read_clipboard_file_paths() -> Result<Vec<String>, String> {
    read_paths()
}

#[cfg(windows)]
fn read_paths() -> Result<Vec<String>, String> {
    use windows::Win32::{
        System::DataExchange::{
            CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        },
        UI::Shell::HDROP,
    };
    const CF_HDROP: u32 = 15;
    struct ClipboardGuard;
    impl Drop for ClipboardGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }
    // Clipboard ownership stays with the OS: never DragFinish/free this handle.
    unsafe {
        if IsClipboardFormatAvailable(CF_HDROP).is_err() {
            return Ok(vec![]);
        }
        OpenClipboard(None).map_err(|e| e.to_string())?;
        let _guard = ClipboardGuard;
        let handle = GetClipboardData(CF_HDROP).map_err(|e| e.to_string())?;
        read_drop_paths(HDROP(handle.0))
    }
}

#[cfg(windows)]
unsafe fn read_drop_paths(drop: windows::Win32::UI::Shell::HDROP) -> Result<Vec<String>, String> {
    use windows::Win32::UI::Shell::DragQueryFileW;
    let count = DragQueryFileW(drop, u32::MAX, None);
    let mut paths = Vec::new();
    for index in 0..count {
        let len = DragQueryFileW(drop, index, None) as usize;
        let mut buffer = vec![0; len + 1];
        let read = DragQueryFileW(drop, index, Some(&mut buffer)) as usize;
        if read > 0 {
            paths.push(String::from_utf16(&buffer[..read]).map_err(|e| e.to_string())?);
        }
    }
    Ok(paths)
}

#[cfg(target_os = "macos")]
fn read_paths() -> Result<Vec<String>, String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL};
    use objc2_foundation::NSURL;

    let clipboard = NSPasteboard::generalPasteboard();
    let Some(items) = clipboard.pasteboardItems() else {
        return Ok(vec![]);
    };
    let mut paths = Vec::new();
    for index in 0..items.count() {
        let item = items.objectAtIndex(index);
        // Read only Finder's file URL flavor; screenshots and copied text do
        // not become file references. NSURL decodes Unicode/escaped paths.
        let Some(value) = item.stringForType(unsafe { NSPasteboardTypeFileURL }) else {
            continue;
        };
        if let Some(url) = NSURL::URLWithString(&value) {
            if url.isFileURL() {
                if let Some(path) = url.path() {
                    paths.push(path.to_string());
                }
            }
        }
    }
    Ok(paths)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn read_paths() -> Result<Vec<String>, String> {
    // Other platforms expose file URLs through the browser paste event.
    Ok(vec![])
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn native_drop_list_preserves_unicode_spaces_and_unc_paths() {
        use windows::Win32::{
            Foundation::GlobalFree,
            System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            UI::Shell::HDROP,
        };
        let expected = [r"C:\研究 data\mcp.html", r"\\server\folder\photo.png"];
        let names: Vec<u16> = expected
            .iter()
            .flat_map(|path| path.encode_utf16().chain([0]))
            .chain([0])
            .collect();
        // A real CF_HDROP-format allocation, without replacing the user's clipboard.
        let mut bytes = vec![0u8; 20];
        bytes[..4].copy_from_slice(&20u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend(names.iter().flat_map(|unit| unit.to_le_bytes()));
        unsafe {
            let allocation = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).unwrap();
            let pointer = GlobalLock(allocation);
            assert!(!pointer.is_null());
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(allocation);
            let result = read_drop_paths(HDROP(allocation.0));
            let _ = GlobalFree(Some(allocation));
            assert_eq!(result.unwrap(), expected);
        }
    }
}
