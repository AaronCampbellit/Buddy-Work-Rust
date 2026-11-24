//! Let the island dialog handle Tab in its multiline text fields. A native
//! multiline EDIT claims all key messages, and Rich Edit also claims Tab.
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{
        Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_TAB},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{DLGC_WANTALLKEYS, DLGC_WANTTAB, MSG, WM_GETDLGCODE, WM_KEYDOWN, WM_NCDESTROY},
    },
};

pub(super) fn attach(control: HWND) {
    unsafe { SetWindowSubclass(control, Some(editor_proc), 0xB064, 0); }
}

unsafe extern "system" fn editor_proc(
    hwnd: HWND, msg: u32, wp: usize, lp: isize, id: usize, _: usize,
) -> isize {
    if msg == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, Some(editor_proc), id);
    }
    let code = DefSubclassProc(hwnd, msg, wp, lp);
    if msg == WM_GETDLGCODE && lp != 0 {
        let key = &*(lp as *const MSG);
        if key.message == WM_KEYDOWN && key.wParam == VK_TAB as usize
            && GetKeyState(VK_CONTROL as i32) >= 0 && GetKeyState(VK_MENU as i32) >= 0
        {
            return code & !((DLGC_WANTALLKEYS | DLGC_WANTTAB) as isize);
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core, inline_images, island, ui::{wide, window_text}, workspace, Ui};
    use std::ptr::{null, null_mut};
    use windows::Win32::Foundation::HWND as TypedWindow;
    use windows_sys::Win32::{
        Foundation::{POINT,RECT}, System::LibraryLoader::GetModuleHandleW,
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    };

    // Post the same keyboard message that a Windows keyboard queue delivers,
    // including its scan code. Only this private desktop thread's modifier
    // state is changed; no input is sent to the interactive window station.
    unsafe fn key(root: HWND, vk: u16, modifier: Option<u16>) {
        let target = GetFocus();
        assert!(!target.is_null());
        let mut previous = [0u8; 256];
        assert_ne!(GetKeyboardState(previous.as_mut_ptr()), 0);
        let mut keys = [0u8; 256];
        if let Some(m) = modifier { keys[m as usize] = 0x80; }
        keys[vk as usize] = 0x80;
        assert_ne!(SetKeyboardState(keys.as_ptr()), 0);
        let scan = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC);
        PostMessageW(target, WM_KEYDOWN, vk as _, ((scan << 16) | 1) as _);
        let mut msg = MSG::default();
        assert_ne!(PeekMessageW(&mut msg, target, WM_KEYDOWN, WM_KEYDOWN, PM_REMOVE), 0);
        if !inline_images::editor_shortcut(&msg) && IsDialogMessageW(root, &msg) == 0 {
            TranslateMessage(&msg); DispatchMessageW(&msg);
        }
        while PeekMessageW(&mut msg, target, WM_CHAR, WM_CHAR, PM_REMOVE) != 0 { DispatchMessageW(&msg); }
        SetKeyboardState(previous.as_ptr());
    }
    unsafe fn code(edit:HWND,vk:u16)->isize{
        let query=MSG{hwnd:edit,message:WM_KEYDOWN,wParam:vk as _,pt:POINT{x:0,y:0},..Default::default()};
        SendMessageW(edit,WM_GETDLGCODE,vk as _,&query as *const _ as _)
    }
    unsafe fn traversal(root:HWND,edit:HWND,page:&str) {
        let prev=GetNextDlgTabItem(root,edit,1);
        let next=GetNextDlgTabItem(root,edit,0);
        assert!(!prev.is_null() && !next.is_null() && prev!=edit && next!=edit);
        for value in ["", "Two lines\r\nSecond line"]{
            SetWindowTextW(edit,wide(value).as_ptr());
            let before=window_text(edit,1024);
            // Query both directions after the text change; enabling action
            // buttons in Rewrite can change the nearby native tab stops.
            let prev=GetNextDlgTabItem(root,edit,1);let next=GetNextDlgTabItem(root,edit,0);
            SetFocus(prev);key(root,VK_TAB,None);assert_eq!(GetFocus(),edit,"{page} forward entry");
            key(root,VK_TAB,None);assert_eq!(GetFocus(),next,"{page} forward exit");
            key(root,VK_TAB,Some(VK_SHIFT));assert_eq!(GetFocus(),edit,"{page} reverse entry");
            key(root,VK_TAB,Some(VK_SHIFT));assert_eq!(GetFocus(),prev,"{page} reverse exit");
            assert_eq!(window_text(edit,1024),before,"{page} Tab altered text");
            println!("{page} id {} chars {} previous {} next {}",GetDlgCtrlID(edit),before.len(),GetDlgCtrlID(prev),GetDlgCtrlID(next));
        }
    }

    #[test]
    #[ignore="run alone on a private Windows window station"]
    fn expanded_workspace_tabs_enter_and_exit_multiline_editors(){
        workspace::with_private_test_desktop(||unsafe{
            let name=wide("BuddyKeyboardRegression");
            let instance=GetModuleHandleW(null());
            let class=WNDCLASSW{lpfnWndProc:Some(crate::window_proc),hInstance:instance,lpszClassName:name.as_ptr(),..Default::default()};
            RegisterClassW(&class);
            let root=CreateWindowExW(WS_EX_TOOLWINDOW,name.as_ptr(),wide("Synthetic keyboard navigation").as_ptr(),
                WS_POPUP|WS_CLIPCHILDREN,0,0,840,360,null_mut(),null_mut(),instance,null());
            assert!(!root.is_null());
            let profile=std::env::temp_dir().join(format!("Buddy-tab-{}-{}",std::process::id(),core::now()));
            std::fs::create_dir_all(&profile).unwrap();
            let mut ui=Ui::new(TypedWindow(root),profile.clone(),true).unwrap();
            ui.records.insert("notes".into(),vec![["synthetic","Keyboard check","Editable body","0","1","","1"].map(str::to_owned).to_vec()]);
            let layout=island::Layout::new(RECT{left:0,top:0,right:1920,bottom:1080},96,100,island::Placement::Left,360,&[]);
            island::configure(layout,true);island::request(true,true);ui.expanded=true;
            crate::WINDOW.with(|w|*w.borrow_mut()=Some(ui));
            ShowWindow(root,SW_SHOW);
            for (page,fields) in [("notes",&[inline_images::NOTE_EDITOR as i32][..]),("rewrite",&[3305,3306]),("ocr",&[103]),("media",&[103]),("diagnostics",&[103])]{
                crate::with_ui(|u|u.navigate(page));
                for &id in fields {
                    let edit=GetDlgItem(root,id);assert_ne!(GetWindowLongW(edit,GWL_STYLE) as u32 & WS_VISIBLE,0,"{page} visible");
                    traversal(root,edit,page);
                    assert_eq!(code(edit,VK_TAB)&((DLGC_WANTALLKEYS|DLGC_WANTTAB)as isize),0);
                    assert_ne!(code(edit,VK_RETURN)&DLGC_WANTALLKEYS as isize,0,"{page} multiline Enter retained");
                }
                if page=="notes"{
                    let body=GetDlgItem(root,inline_images::NOTE_EDITOR as i32);
                    use windows::Win32::UI::Controls::RichEdit::{CHARFORMATW,CFE_BOLD,EM_GETCHARFORMAT,SCF_SELECTION};
                    use windows_sys::Win32::System::{DataExchange::*,Memory::*};
                    SetFocus(body);SetWindowTextW(body,wide("").as_ptr());
                    let rtf=b"{\\rtf1\\ansi {\\b Bold} text}\0";
                    let format_id=RegisterClipboardFormatW(wide("Rich Text Format").as_ptr());
                    let memory=GlobalAlloc(GMEM_MOVEABLE,rtf.len());assert!(!memory.is_null());
                    let data=GlobalLock(memory);assert!(!data.is_null());
                    std::ptr::copy_nonoverlapping(rtf.as_ptr(),data.cast(),rtf.len());GlobalUnlock(memory);
                    assert_ne!(OpenClipboard(body),0);assert_ne!(EmptyClipboard(),0);
                    assert!(!SetClipboardData(format_id,memory).is_null());CloseClipboard();
                    key(root,b'V' as u16,Some(VK_CONTROL));
                    assert_eq!(window_text(body,128),"Bold text","Ctrl+V inserts the formatted clipboard payload once");
                    SendMessageW(body,0x00b1,0,4); // EM_SETSEL
                    let mut format=CHARFORMATW::default();format.cbSize=std::mem::size_of::<CHARFORMATW>() as _;
                    SendMessageW(body,EM_GETCHARFORMAT,SCF_SELECTION as _,&mut format as *mut _ as _);
                    assert_ne!(format.dwEffects.0&CFE_BOLD.0,0,"Ctrl+V retains native Rich Edit formatting");
                    SendMessageW(body,0x00b1,usize::MAX,-1);
                    key(root,VK_RETURN,None);
                    assert!(window_text(body,1024).ends_with(['\r','\n']),"Enter inserted a newline in Notes");
                } else if page=="rewrite"{
                    let source=GetDlgItem(root,3305);SetWindowTextW(source,wide("").as_ptr());SetFocus(source);
                    key(root,VK_TAB,Some(VK_CONTROL));assert_eq!(GetFocus(),source);
                    assert_eq!(window_text(source,32),"\t","standard multiline fields retain native Ctrl+Tab indentation");
                }
            }
            // Clipboard/formatting/image and pinned-widget keyboard workflows
            // have separate private-desktop tests; this hook is main-island only.
            DestroyWindow(root);
            crate::WINDOW.with(|w|{w.borrow_mut().take();});
            std::fs::remove_dir_all(profile).unwrap();
        });
    }
}
