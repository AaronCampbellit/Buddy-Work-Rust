use super::*;
use windows_sys::Win32::UI::Controls::{EM_GETSEL, EM_SETSEL};

thread_local! {
    static SAMPLE: Cell<Option<(u32, u32, u32)>> = const { Cell::new(None) };
    static TICKS: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "system" fn inspect(_: HWND, _: u32, _: usize, _: u32) {
    if TICKS.replace(TICKS.get() + 1) > 80 {
        EndMenu();
        return;
    }
    if SAMPLE.get().is_some() { return; }
    let Some(popup) = menu_windows().into_iter().next() else { return; };
    let menu = SendMessageW(popup, MN_GETHMENU, 0, 0) as HMENU;
    let mut rect = RECT::default();
    GetWindowRect(popup, &mut rect);
    let shot = Surface::new(rect.right - rect.left, rect.bottom - rect.top);
    SendMessageW(popup, WM_PRINT, shot.dc as _, (PRF_NONCLIENT | PRF_CLIENT | PRF_ERASEBKGND) as _);
    let background = GetPixel(shot.dc, shot.width - 10, 15);
    SAMPLE.set(Some((background, GetMenuState(menu, WM_COPY, MF_BYCOMMAND), GetMenuState(menu, WM_CUT, MF_BYCOMMAND))));
    // Use a native keyboard shortcut while the original edit menu is active.
    PostMessageW(popup, WM_KEYDOWN, VK_ESCAPE as _, 1);
}

#[test]
#[ignore = "real Windows edit menu on a private desktop"]
fn readonly_edit_keeps_native_commands_and_uses_buddy_palette() {
    crate::workspace::with_private_test_desktop(|| unsafe {
        let owner = CreateWindowExW(WS_EX_TOOLWINDOW, wide("STATIC").as_ptr(), null(), WS_POPUP,
            10, 10, 360, 160, null_mut(), null_mut(), null_mut(), null());
        let edit = CreateWindowExW(0, wide("EDIT").as_ptr(), wide("Sample text").as_ptr(),
            WS_CHILD | WS_VISIBLE | ES_READONLY as u32, 10, 10, 220, 35, owner, null_mut(), null_mut(), null());
        assert!(!owner.is_null() && !edit.is_null());
        ShowWindow(owner, SW_SHOWNOACTIVATE);
        let palette = (0x1b1712, 0x27221b, 0xfaf8f6, 0xf5d825);
        set_colors(palette);
        crate::chrome::edit_menu::attach(edit);
        SendMessageW(edit, EM_SETSEL, 0, 6);
        TICKS.set(0);
        SAMPLE.set(None);
        SetTimer(owner, 89, 25, Some(inspect));
        SendMessageW(edit, WM_CONTEXTMENU, edit as _, -1);
        KillTimer(owner, 89);
        let selection = SendMessageW(edit, EM_GETSEL, 0, 0);
        DestroyWindow(owner);

        let (color, copy, cut) = SAMPLE.get().expect("the stock edit popup opened");
        assert_eq!(color, Paint::new(palette).surface);
        assert_ne!(copy, u32::MAX);
        assert_ne!(cut, u32::MAX);
        assert_eq!(copy & (MF_DISABLED | MF_GRAYED), 0);
        assert_ne!(cut & (MF_DISABLED | MF_GRAYED), 0);
        assert_eq!(selection, 6 << 16, "Escape preserves the selected text");
    });
}
