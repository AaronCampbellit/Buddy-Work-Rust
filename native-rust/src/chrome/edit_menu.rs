//! Theme the system edit menu while Windows handles its commands and selection.
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{WM_CONTEXTMENU, WM_NCDESTROY},
    },
};

pub(crate) fn attach(edit: HWND) {
    unsafe { SetWindowSubclass(edit, Some(edit_proc), 0xB063, 0); }
}

unsafe extern "system" fn edit_proc(
    hwnd: HWND, message: u32, wparam: usize, lparam: isize, id: usize, _: usize,
) -> isize {
    if message == WM_CONTEXTMENU {
        return super::menu::scope(hwnd, super::menu::colors(), || {
            DefSubclassProc(hwnd, message, wparam, lparam)
        });
    }
    if message == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, Some(edit_proc), id);
    }
    DefSubclassProc(hwnd, message, wparam, lparam)
}
