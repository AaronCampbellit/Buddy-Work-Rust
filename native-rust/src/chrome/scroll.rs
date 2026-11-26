// Keep Windows in charge of scrollbar geometry, accessibility and dragging.
use std::{cell::{Cell, RefCell}, mem::size_of, ptr::null};
use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    Graphics::Gdi::ScreenToClient,
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        Controls::{GetComboBoxInfo, SetWindowTheme, COMBOBOXINFO},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};

thread_local! {
    static DARK: Cell<bool> = const { Cell::new(false) };
    static WINDOWS: RefCell<Vec<HWND>> = const { RefCell::new(Vec::new()) };
}

unsafe fn theme(window: HWND, dark: bool) {
    let class = if dark { windows_sys::w!("DarkMode_Explorer") } else { null() };
    SetWindowTheme(window, class, null());
}

pub(crate) fn set_palette(background: u32) {
    let brightness = 299 * (background & 255) + 587 * ((background >> 8) & 255)
        + 114 * ((background >> 16) & 255);
    let mut contrast = HIGHCONTRASTW { cbSize: size_of::<HIGHCONTRASTW>() as _, ..Default::default() };
    unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, contrast.cbSize, (&mut contrast as *mut HIGHCONTRASTW).cast(), 0); }
    let dark = brightness < 128000 && contrast.dwFlags & HCF_HIGHCONTRASTON == 0;
    if DARK.replace(dark) != dark {
        // SetWindowTheme sends messages synchronously; release the registry borrow first.
        let windows = WINDOWS.with_borrow(Clone::clone);
        for window in windows { unsafe { theme(window, dark) }; }
    }
}

unsafe extern "system" fn lifetime(window: HWND, msg: u32, wp: usize, lp: isize, id: usize, _: usize) -> isize {
    if msg == WM_NCDESTROY {
        WINDOWS.with_borrow_mut(|windows| windows.retain(|&h| h != window));
        RemoveWindowSubclass(window, Some(lifetime), id);
    }
    DefSubclassProc(window, msg, wp, lp)
}

pub(crate) unsafe fn register_themed_window(window: HWND) {
    if window.is_null() || WINDOWS.with_borrow(|windows| windows.contains(&window)) { return; }
    if SetWindowSubclass(window, Some(lifetime), 1, 0) == 0 { return; }
    WINDOWS.with_borrow_mut(|windows| windows.push(window));
    if DARK.get() { theme(window, true); }
}

pub(crate) unsafe fn attach(window: HWND, style: u32, combo: bool) {
    if style & (WS_VSCROLL | WS_HSCROLL) == 0 || window.is_null() { return; }
    register_themed_window(window);
    if combo {
        let mut info = COMBOBOXINFO { cbSize: size_of::<COMBOBOXINFO>() as _, ..Default::default() };
        if GetComboBoxInfo(window, &mut info) != 0 { register_themed_window(info.hwndList); }
    }
}

unsafe fn scrolls(window: HWND) -> bool {
    let style = GetWindowLongW(window, GWL_STYLE) as u32;
    if style & WS_VSCROLL == 0 { return false; }
    let mut name = [0u16; 16];
    let n = GetClassNameW(window, name.as_mut_ptr(), name.len() as i32);
    // A closed picker should let its surrounding pane scroll without changing the selection.
    if n == 8 && name[..8].iter().zip(b"combobox").all(|(&a, &b)| a | 32 == u16::from(b)) { return false; }
    let mut info = SCROLLINFO { cbSize: size_of::<SCROLLINFO>() as _, fMask: SIF_RANGE | SIF_PAGE, ..Default::default() };
    GetScrollInfo(window, SB_VERT, &mut info) != 0 &&
        i64::from(info.nMax) - i64::from(info.nMin) + 1 > i64::from(info.nPage.max(1))
}

// The wheel can be delivered to the focused control even when the pointer is over another
// pane. Resolve the pointer against the real child windows without moving keyboard focus.
pub(crate) unsafe fn route_wheel(msg: &mut MSG) {
    if msg.message != WM_MOUSEWHEEL || msg.wParam & 0x000c != 0 || msg.hwnd.is_null() { return; }
    let point = POINT { x: msg.lParam as i16 as i32, y: (msg.lParam >> 16) as i16 as i32 };
    let root = GetAncestor(msg.hwnd, GA_ROOT);
    if root.is_null() { return; }
    let hit = WindowFromPoint(point);
    if !hit.is_null() && GetAncestor(hit, GA_ROOT) != root { return; }
    let mut window = root;
    loop {
        let mut local = point;
        ScreenToClient(window, &mut local);
        let child = ChildWindowFromPointEx(window, local, CWP_SKIPINVISIBLE | CWP_SKIPDISABLED | CWP_SKIPTRANSPARENT);
        if child.is_null() || child == window { break; }
        window = child;
    }
    while window != root && !window.is_null() {
        if scrolls(window) { msg.hwnd = window; return; }
        window = GetParent(window);
    }
}
