use super::*;
use std::{cell::Cell,path::PathBuf};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_ESCAPE,VK_RIGHT,VK_RETURN};
mod stock_edit;

#[test]
fn literal_ampersands_and_shortcuts_have_native_accessible_names(){
    assert_eq!(label_parts("R&&D && &Notes"),("R&D & Notes".into(),Some('N')));
    assert_eq!(label_parts("&日本語"),("日本語".into(),Some('日')));
}

struct Surface {dc:HDC,bmp:HBITMAP,old:HGDIOBJ,bits:*mut u8,width:i32,height:i32}
impl Surface{
    unsafe fn new(width:i32,height:i32)->Self{
        let dc=CreateCompatibleDC(null_mut());
        let mut info=BITMAPINFO::default();info.bmiHeader=BITMAPINFOHEADER{biSize:size_of::<BITMAPINFOHEADER>()as _,biWidth:width,
            biHeight:-height,biPlanes:1,biBitCount:32,biCompression:BI_RGB,..Default::default()};
        let mut bits=null_mut();let bmp=CreateDIBSection(dc,&info,DIB_RGB_COLORS,&mut bits,null_mut(),0);let old=SelectObject(dc,bmp);
        assert!(!bits.is_null());
        Self{dc,bmp,old,bits:bits.cast(),width,height}
    }
    fn save(&self,name:&str){
        let Some(folder)=std::env::var_os("BUDDY_MENU_CAPTURE_DIR").map(PathBuf::from) else{return};
        unsafe{GdiFlush();let px=std::slice::from_raw_parts(self.bits,(self.width*self.height*4)as usize);
            let im=image::RgbImage::from_fn(self.width as u32,self.height as u32,|x,y|{let at=((y*self.width as u32+x)*4) as usize;
                image::Rgb([px[at+2],px[at+1],px[at]])});
            std::fs::create_dir_all(&folder).unwrap();im.save(folder.join(format!("{name}.png"))).unwrap();
        }
    }
}
impl Drop for Surface{fn drop(&mut self){unsafe{SelectObject(self.dc,self.old);DeleteObject(self.bmp);DeleteDC(self.dc);}}}
thread_local! {static STAGE:Cell<u32>=const{Cell::new(0)};static SHOULD_CANCEL:Cell<bool>=const{Cell::new(false)};
    static REPORT:RefCell<Vec<String>>=const{RefCell::new(Vec::new())};}
unsafe fn menu_windows()->Vec<HWND>{
    unsafe extern "system" fn add(h:HWND,p:isize)->i32{
        let mut name=[0u16;16];let n=GetClassNameW(h,name.as_mut_ptr(),16);
        if n==6 && &name[..6]==[35,51,50,55,54,56] && GetWindowLongW(h,GWL_STYLE)as u32&WS_VISIBLE!=0{
            (&mut *(p as *mut Vec<HWND>)).push(h);
        }1
    }
    let mut out=Vec::new();EnumThreadWindows(GetCurrentThreadId(),Some(add),&mut out as *mut _ as _);out
}
unsafe extern "system" fn drive(owner:HWND,_:u32,_:usize,_:u32){
    let tick=STAGE.with(|n|n.replace(n.get()+1));
    if tick>80{REPORT.with(|v|v.borrow_mut().push("timeout".into()));EndMenu();return}
    let menus=menu_windows();
    let Some(&popup)=menus.first() else{return};
    if tick==1{
        let mut r=RECT::default();GetWindowRect(popup,&mut r);
        let shot=Surface::new(r.right-r.left,r.bottom-r.top);
        SendMessageW(popup,WM_PRINT,shot.dc as _,(PRF_NONCLIENT|PRF_CLIENT|PRF_ERASEBKGND)as _);
        let hmenu=SendMessageW(popup,MN_GETHMENU,0,0) as HMENU;
        let state=GetMenuState(hmenu,14,MF_BYCOMMAND);
        let mut row=RECT::default();GetMenuItemRect(null_mut(),hmenu,1,&mut row);
        let hit=MenuItemFromPoint(null_mut(),hmenu,POINT{x:(row.left+row.right)/2,y:(row.top+row.bottom)/2});
        let mut monitor=MONITORINFO::default();monitor.cbSize=size_of::<MONITORINFO>() as _;
        GetMonitorInfoW(MonitorFromWindow(popup,MONITOR_DEFAULTTONEAREST),&mut monitor);
        let work=monitor.rcWork;
        let bounded=r.left>=work.left && r.right<=work.right && r.top>=work.top && r.bottom<=work.bottom;
        let brush_color=GetPixel(shot.dc,r.right-r.left-8,15);
        let border_color=GetPixel(shot.dc,1,(r.bottom-r.top)/2);
        let mask=CreateRectRgn(0,0,0,0);let region=GetWindowRgn(popup,mask);let corner=PtInRegion(mask,0,0)!=0;DeleteObject(mask);
        let cancel=SHOULD_CANCEL.with(Cell::get);
        shot.save(if cancel{"native-dark-cancel"}else{"native-light-select"});
        REPORT.with(|v|v.borrow_mut().push(format!("root size={}x{} hit={hit} bounded={bounded} state={state:x} body={brush_color:06x} edge={border_color:06x} region={region} corner={corner}",r.right-r.left,r.bottom-r.top)));
        if cancel{PostMessageW(popup,WM_KEYDOWN,VK_ESCAPE as _,0);return}
        PostMessageW(popup,WM_KEYDOWN,b'W' as usize,0);
    }
    if !SHOULD_CANCEL.with(Cell::get) && menus.len()>=2 && tick>=2{
        for &open in &menus{
            let mut r=RECT::default();GetWindowRect(open,&mut r);
            let shot=Surface::new(r.right-r.left,r.bottom-r.top);
            SendMessageW(open,WM_PRINT,shot.dc as _,(PRF_CLIENT|PRF_NONCLIENT|PRF_ERASEBKGND)as _);
            let current=SendMessageW(open,MN_GETHMENU,0,0) as HMENU;
            shot.save(if GetMenuItemCount(current)==2{"native-light-submenu"}else{"native-light-highlight"});
        }
        REPORT.with(|v|v.borrow_mut().push("submenu opened".into()));
        PostMessageW(popup,WM_CHAR,'n' as usize,0);
        PostMessageW(popup,WM_KEYDOWN,VK_RETURN as _,0);
    }else if tick==8 && !SHOULD_CANCEL.with(Cell::get){PostMessageW(popup,WM_KEYDOWN,VK_RIGHT as _,0);}
    let _=owner;
}
#[test]
#[ignore="run alone on its own private Windows window station"]
fn native_menu_draws_round_frame_and_dispatches_submenu_mnemonics(){
    crate::workspace::with_private_test_desktop(||unsafe{
        let owner=CreateWindowExW(WS_EX_TOOLWINDOW,wide("STATIC").as_ptr(),wide("Buddy synthetic popup owner").as_ptr(),WS_POPUP,
            10,10,420,300,null_mut(),null_mut(),null_mut(),null());
        assert!(!owner.is_null());ShowWindow(owner,SW_SHOWNOACTIVATE);
        for (cancel,expected,colors) in [(false,51,(0xfbf9f7,0xf4f1ed,0x272017,0x9c7f00)),(true,0,(0x1b1712,0x27221b,0xfaf8f6,0xf5d825))]{
            let menu=CreatePopupMenu();let child=CreatePopupMenu();
            for (id,label) in [(50,"&Home"),(51,"&Notes")]{AppendMenuW(child,MF_STRING,id,wide(label).as_ptr());}
            AppendMenuW(menu,MF_STRING,7,wide("&Open Buddy").as_ptr());AppendMenuW(menu,MF_POPUP,child as usize,wide("&Workspaces").as_ptr());
            AppendMenuW(menu,MF_STRING|MF_GRAYED,14,wide("Unavailable").as_ptr());AppendMenuW(menu,MF_SEPARATOR,0,null());
            AppendMenuW(menu,MF_STRING|MF_CHECKED,15,wide("R&&D pinned").as_ptr());
            let mut monitor=MONITORINFO::default();monitor.cbSize=size_of::<MONITORINFO>() as _;
            GetMonitorInfoW(MonitorFromWindow(owner,MONITOR_DEFAULTTONEAREST),&mut monitor);
            let pos=if cancel{POINT{x:monitor.rcWork.right-1,y:monitor.rcWork.bottom-1}}else{POINT{x:62,y:64}};
            // Native accessible item metadata and shortcut routes survive the
            // same owner-draw transformation used by the modal tracking loop.
            {
                let p=Presentation::new(owner,pos,colors);p.prepare(menu);
                let mut item=MENUITEMINFOW{cbSize:size_of::<MENUITEMINFOW>()as _,fMask:MIIM_DATA|MIIM_STATE|MIIM_SUBMENU,..Default::default()};
                assert_ne!(GetMenuItemInfoW(menu,1,1,&mut item),0);assert_eq!(item.hSubMenu,child);
                let mut label=[0u16;64];
                let mut original=MENUITEMINFOW{cbSize:size_of::<MENUITEMINFOW>()as _,fMask:MIIM_STRING,dwTypeData:label.as_mut_ptr(),cch:64,..Default::default()};
                assert_ne!(GetMenuItemInfoW(menu,1,1,&mut original),0);
                assert_eq!(String::from_utf16_lossy(&label[..original.cch as usize]),"&Workspaces");
                let accessible=&*(item.dwItemData as *const MSAAMENUINFO);
                assert_eq!(accessible.dwMSAASignature,MSAA_MENU_SIG as u32);
                let text=String::from_utf16_lossy(std::slice::from_raw_parts(accessible.pszWText,accessible.cchWText as usize));
                assert_eq!(text,"Workspaces");assert_eq!(p.mnemonic(child,'n'),Some((MNC_EXECUTE as isize)<<16|1));
                assert_eq!(p.mnemonic(menu,'u'),None);
            }
            assert_eq!(GetMenuState(menu,0,MF_BYPOSITION)&MF_OWNERDRAW,0);
            STAGE.with(|s|s.set(0));SHOULD_CANCEL.with(|s|s.set(cancel));REPORT.with(|v|v.borrow_mut().clear());
            SetTimer(owner,88,25,Some(drive));
            let actual=track(owner,menu,TPM_RETURNCMD|TPM_RIGHTBUTTON,pos.x,pos.y,colors);
            KillTimer(owner,88);let log=REPORT.with(|v|v.borrow().join("; "));
            println!("cancel={cancel} command={actual} {log}");assert_eq!(actual,expected,"{log}");
            assert!(log.contains("corner=false") && !log.contains("region=0") && !log.contains("timeout"),"{log}");
            assert!(log.contains("hit=1 bounded=true"),"native hit test and work-area placement: {log}");
            assert!(cancel||log.contains("submenu opened"),"{log}");DestroyMenu(menu);
        }
        DestroyWindow(owner);
    });
}

#[test]
fn supplied_high_contrast_colors_survive_selection_and_disabled_ink(){
    unsafe{
        let menu=CreatePopupMenu();AppendMenuW(menu,MF_STRING,1,wide("Unavailable").as_ptr());
        let mut style=Presentation::new(null_mut(),POINT{x:0,y:0},(0,0,0,0));
        style.paint=Paint::contrast(|id|match id{COLOR_MENU=>0x020201,COLOR_MENUTEXT=>0xfcfcfc,COLOR_GRAYTEXT=>0x777777,
            COLOR_HIGHLIGHT=>0x99ecff,COLOR_HIGHLIGHTTEXT=>0x010102,COLOR_WINDOWTEXT=>0xffff90,_=>0});
        style.prepare(menu);
        let data={let entries=style.entries.borrow();entries[0].as_ref() as *const Entry as usize};
        let width=250;let height=style.row_height as i32;let shot=Surface::new(width,height*2);
        for (row,mask) in [(0,ODS_SELECTED),(1,ODS_DISABLED)]{
            let di=DRAWITEMSTRUCT{CtlType:ODT_MENU,hDC:shot.dc,itemData:data,itemState:mask,rcItem:RECT{left:0,top:row*height,right:width,bottom:(row+1)*height},..Default::default()};
            assert!(style.draw(&di));
        }
        assert_eq!(GetPixel(shot.dc,225,height/2),style.paint.selected);
        assert_eq!(GetPixel(shot.dc,225,height+height/2),style.paint.surface);
        let count=|color| (2..width-2).map(|x|(height+3..2*height-3).filter(|&y|GetPixel(shot.dc,x,y)==color).count()).sum::<usize>();
        assert!(count(style.paint.muted)>2,"disabled native text uses the supplied system disabled color");
        shot.save("high-contrast-renderer");drop(style);DestroyMenu(menu);
    }
}
