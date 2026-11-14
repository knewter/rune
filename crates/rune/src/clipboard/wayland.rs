//! Linux's clipboard under Wayland: the window's own connection, through `wl_data_device`, which
//! every compositor offers (`device`). arboard reads Wayland only through a data-control protocol
//! (wlroots' or `ext-data-control`); GNOME offers neither, so arboard falls back to X11 there, and
//! finds no clipboard at all where no X display is reachable (a sandbox, a session without
//! Xwayland). Without a Wayland display, arboard is used as before.
//!
//! Every form Rune pastes and copies goes through Wayland: text, HTML, an image (PNG, or JPEG or
//! WebP turned into PNG) and files (`text/uri-list`).

use super::*;
use sctk::reexports::calloop::channel;
use sctk::reexports::client::Connection;
use sctk::reexports::client::backend::Backend as WaylandBackend;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

mod device;
use device::{Command, Reply};

/// The most a single form may be when read from another client.
const MAX_READ: usize = 64 * 1024 * 1024;
const TEXT: [&str; 5] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "TEXT", "STRING"];
const HTML: &str = "text/html";
const PNG: &str = "image/png";
const DECODED_IMAGES: [&str; 2] = ["image/jpeg", "image/webp"];
const URI_LIST: &str = "text/uri-list";

/// The `wl_display` of the event loop, once a window has started on Wayland.
static DISPLAY: AtomicUsize = AtomicUsize::new(0);

/// Record the event loop's Wayland display, before the clipboard is first used.
pub(crate) fn set_display(display: *mut c_void) { DISPLAY.store(display as usize, Ordering::Release); }

pub(super) enum Linux {
    Wayland(Wayland),
    Arboard(arboard::Clipboard),
}

impl Linux {
    pub(super) fn open() -> Option<Self> {
        match DISPLAY.load(Ordering::Acquire) {
            0 => arboard::Clipboard::new().ok().map(Self::Arboard),
            // SAFETY: the display is the event loop's, which outlives every window and so the
            // clipboard worker (shut down on the event loop's exit path).
            display => unsafe { Wayland::new(display as *mut c_void) }.map(Self::Wayland),
        }
    }
}

pub(super) struct Wayland {
    commands: channel::Sender<Command>,
    replies: mpsc::Receiver<Reply>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Wayland {
    /// # Safety
    /// `display` must be a valid `wl_display` that outlives the clipboard.
    unsafe fn new(display: *mut c_void) -> Option<Self> {
        // SAFETY: as the caller promises.
        let backend = unsafe { WaylandBackend::from_foreign_display(display.cast()) };
        let (commands, receiver) = channel::channel();
        let (reply, replies) = mpsc::channel();
        let thread = device::spawn(Connection::from_backend(backend), receiver, reply)?;
        Some(Self { commands, replies, thread: Some(thread) })
    }

    fn offered(&self) -> Vec<String> {
        let _ = self.commands.send(Command::Offered);
        match self.replies.recv() { Ok(Reply::Offered(offered)) => offered, _ => Vec::new() }
    }

    fn load(&self, preferred: &[&str]) -> Option<(String, Vec<u8>)> {
        let _ = self.commands.send(Command::Load(preferred.iter().map(|mime| (*mime).to_owned()).collect()));
        match self.replies.recv() { Ok(Reply::Loaded(Ok(loaded))) => Some(loaded), _ => None }
    }

    fn text(&self, offered: &[String]) -> Option<String> {
        let wanted: Vec<&str> = TEXT.into_iter().filter(|mime| offered.iter().any(|o| o == mime)).collect();
        let (mime, bytes) = self.load(&wanted)?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // `text/*` carries CRLF by RFC 2046; the platform's own line end is LF.
        Some(if mime.starts_with("text/") { text.replace("\r\n", "\n").replace('\r', "\n") } else { text })
    }

    fn image(&self, offered: &[String]) -> Option<ClipboardImage> {
        if offered.iter().any(|o| o == PNG) && let Some(image) = self.load(&[PNG]).and_then(|(_, png)| ClipboardImage::from_png(png)) {
            return Some(image);
        }
        let mime = DECODED_IMAGES.into_iter().find(|mime| offered.iter().any(|o| o == mime))?;
        let (_, bytes) = self.load(&[mime])?;
        let rgba = image::load_from_memory(&bytes).ok()?.to_rgba8();
        ClipboardImage::from_rgba(rgba.width(), rgba.height(), rgba.as_raw())
    }

    fn files(&self, offered: &[String]) -> Vec<PathBuf> {
        if !offered.iter().any(|o| o == URI_LIST) { return Vec::new(); }
        self.load(&[URI_LIST]).map(|(_, bytes)| uri_list(&String::from_utf8_lossy(&bytes))).unwrap_or_default()
    }
}

impl Drop for Wayland {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Exit);
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}

/// The local files a `text/uri-list` names, skipping its comments and anything not `file://`.
fn uri_list(list: &str) -> Vec<PathBuf> {
    list.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.strip_prefix("file://"))
        // `file://host/path`: only this host's (an empty host, or localhost).
        .filter_map(|rest| rest.strip_prefix("localhost").or(Some(rest)).filter(|path| path.starts_with('/')))
        .map(|path| PathBuf::from(percent_decode(path)))
        .collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |at: usize| bytes.get(at).and_then(|b| (*b as char).to_digit(16));
        match (bytes[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(high), Some(low)) => { out.push((high * 16 + low) as u8); i += 3; }
            (byte, _, _) => { out.push(byte); i += 1; }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode_path(path: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().iter().map(|&byte| match byte {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
        _ => format!("%{byte:02X}"),
    }).collect()
}

/// The forms a copy is offered in, by MIME type.
fn forms(content: &ClipboardContent, rich: bool) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    if rich && !content.files().is_empty() {
        let list: String = content.files().iter().map(|path| format!("file://{}\r\n", percent_encode_path(path))).collect();
        out.push((URI_LIST.to_owned(), list.into_bytes()));
    }
    if rich && let Some(image) = content.image() { out.push((PNG.to_owned(), image.png().to_vec())); }
    if rich && let Some(html) = content.html() { out.push((HTML.to_owned(), html.as_bytes().to_vec())); }
    if !content.text().is_empty() || out.is_empty() {
        out.extend(TEXT.into_iter().map(|mime| (mime.to_owned(), content.text().as_bytes().to_vec())));
    }
    out
}

impl Backend for Linux {
    fn write(&mut self, copy: &ClipboardWrite) -> Result<(), String> {
        match self {
            Self::Arboard(arboard) => super::native::write(arboard, copy),
            Self::Wayland(wayland) => {
                // A secret is offered as text alone, as arboard's write does.
                let rich = copy.clear_after.is_none();
                let _ = wayland.commands.send(Command::Store(forms(&copy.content, rich)));
                Ok(())
            }
        }
    }
    fn text(&mut self) -> Option<Zeroizing<String>> {
        match self {
            Self::Arboard(arboard) => Backend::text(arboard),
            Self::Wayland(wayland) => { let offered = wayland.offered(); wayland.text(&offered).map(Zeroizing::new) }
        }
    }
    fn content(&mut self, all: bool) -> Option<ClipboardContent> {
        let wayland = match self {
            Self::Arboard(arboard) => return arboard.content(all),
            Self::Wayland(wayland) => wayland,
        };
        let offered = wayland.offered();
        let text = wayland.text(&offered);
        let html = offered.iter().any(|o| o == HTML).then(|| wayland.load(&[HTML])).flatten()
            .map(|(_, bytes)| String::from_utf8_lossy(&bytes).into_owned())
            .filter(|html| !html.is_empty() && html.len() <= html::MAX_RICH_BYTES);
        let image = if all { wayland.image(&offered) } else { None };
        let files = if all { wayland.files(&offered) } else { Vec::new() };
        if text.is_none() && html.is_none() && image.is_none() && files.is_empty() { return None; }
        let mut content = ClipboardContent::from(text.unwrap_or_default()).with_files(files);
        if let Some(html) = html { content = content.with_html(html); }
        if let Some(image) = image { content = content.with_image(image); }
        Some(content)
    }
    fn revision(&self) -> Option<u64> { super::native::revision() }
    fn clear(&mut self) {
        match self {
            Self::Arboard(arboard) => Backend::clear(arboard),
            Self::Wayland(wayland) => { let _ = wayland.commands.send(Command::Store(forms(&ClipboardContent::from(String::new()), false))); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uri_list_names_this_hosts_files_decoded() {
        let list = "# copied\r\nfile:///home/a/My%20Photo.png\r\nfile://localhost/tmp/b.txt\r\nhttps://example.com/x\r\nfile://elsewhere/c\r\n";
        assert_eq!(uri_list(list), vec![PathBuf::from("/home/a/My Photo.png"), PathBuf::from("/tmp/b.txt")]);
    }

    #[test]
    fn a_copy_is_offered_in_every_form_it_holds_and_a_secret_as_text_alone() {
        let png = [&b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"[..], &[0, 0, 0, 2, 0, 0, 0, 3]].concat();
        let content = ClipboardContent::from("hi".to_owned()).with_html("<b>hi</b>")
            .with_image(ClipboardImage::from_png(png).unwrap()).with_files([PathBuf::from("/tmp/a b")]);
        let mimes: Vec<String> = forms(&content, true).into_iter().map(|(mime, _)| mime).collect();
        assert_eq!(&mimes[..3], [URI_LIST, PNG, HTML]);
        assert!(TEXT.iter().all(|text| mimes.iter().any(|m| m == text)));
        assert_eq!(forms(&content, true)[0].1, b"file:///tmp/a%20b\r\n");
        assert!(forms(&content, false).iter().all(|(mime, bytes)| TEXT.contains(&mime.as_str()) && bytes == b"hi"));
    }

    #[test]
    fn percent_decoding_keeps_what_is_not_an_escape() {
        assert_eq!(percent_decode("/a%2Fb%zz%4"), "/a/b%zz%4");
    }
}
