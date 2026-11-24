#![windows_subsystem="windows"]
mod activity;
mod chrome;
mod inline_images;
mod island;
mod motion_surface;
#[cfg(test)]
mod island_capture_test;
mod side_widgets;
mod core;
mod devices;
mod shell;
pub mod storage;
mod ui;
mod workspace;
pub use ui::Ui;
use std::{cell::{Cell,RefCell},ffi::OsString,os::windows::ffi::OsStringExt,path::{Path,PathBuf},ptr::{null,null_mut}};
use windows::Win32::Foundation::HWND as TypedWindow;
use windows_sys::Win32::{Foundation::*,Graphics::Gdi::*,System::{Com::*,RemoteDesktop::*,LibraryLoader::GetModuleHandleW,Threading::*},UI::{Controls::WM_MOUSELEAVE,HiDpi::*,Input::KeyboardAndMouse::*,Shell::*,WindowsAndMessaging::*}};
use ui::wide;

thread_local!{static WINDOW:RefCell<Option<Ui>>=const{RefCell::new(None)};}
thread_local!{static FRAME_TO_PRESENT:Cell<HWND>=const{Cell::new(null_mut())};}
fn read_ui<T>(read:impl FnOnce(&Ui)->T)->Option<T>{
    WINDOW.with(|cell|{let slot=cell.try_borrow().ok()?;slot.as_ref().map(read)})
}
thread_local!{static DEFERRED_PAINT:Cell<HWND>=const{Cell::new(null_mut())};}
fn repaint_after_update(hwnd: HWND) {
    if WINDOW.with(|cell| cell.try_borrow().is_err()) {
        DEFERRED_PAINT.set(hwnd);
    }
}
fn with_ui(f:impl FnOnce(&mut Ui)){
    let completed=WINDOW.with(|cell|{
        let Ok(mut slot)=cell.try_borrow_mut() else{return false};
        if let Some(ui)=slot.as_mut(){f(ui);}
        true
    });
    if completed{
        let repair=DEFERRED_PAINT.replace(null_mut());
        let hwnd=FRAME_TO_PRESENT.replace(null_mut());
        if hwnd.is_null() && !repair.is_null(){unsafe{
            RedrawWindow(repair,null(),null_mut(),RDW_INVALIDATE|RDW_UPDATENOW|RDW_ALLCHILDREN);
        }}
        if !hwnd.is_null(){unsafe{
            // Moving children or changing their regions can synchronously paint
            // while layout owns Ui. Present the complete new geometry only
            // after its owner-drawn children can paint from the new state.
            let frame=island::frame();
            if (frame.animating || workspace_masked()) && motion_surface::present(hwnd,&frame){
                island::apply_region(hwnd,&frame);
                return
            }
            let _handoff=motion_surface::hold_last_frame(hwnd);
            motion_surface::end(hwnd);
            island::apply_region(hwnd,&frame);
            RedrawWindow(hwnd,null(),null_mut(),RDW_INVALIDATE|RDW_UPDATENOW|RDW_ALLCHILDREN);
        }}
    }
}
fn present_frame(hwnd:HWND){
    if WINDOW.with(|s|s.try_borrow().is_err()){FRAME_TO_PRESENT.set(hwnd);}
    else{island::apply_region(hwnd,&island::frame());unsafe{InvalidateRect(hwnd,null(),0);}}
}
// While the workspace is fading, its native controls are clipped. Remember
// navigation presses on the visible snapshot until the matching release.
thread_local!{static NAV_PRESS:Cell<HWND>=const{Cell::new(null_mut())};}
const CLASS:&str="BuddyRustWorkIsland";
unsafe extern "system" fn editor_proc(hwnd: HWND, message: u32, wp: usize, lp: isize, id: usize, _: usize) -> isize {
    if message==WM_NCDESTROY { RemoveWindowSubclass(hwnd,Some(editor_proc),id); }
    if message==WM_PASTE {
        let mut handled=false;
        with_ui(|ui| {
            if ui.page!="notes" || !ui.expanded || ui.locked || ui.active_note.is_none(){return}
            use windows_sys::Win32::System::{DataExchange::{IsClipboardFormatAvailable,RegisterClipboardFormatW}, Ole::{CF_BITMAP,CF_DIB,CF_DIBV5}};
            let png=RegisterClipboardFormatW(wide("PNG").as_ptr());
            let image=[CF_DIBV5 as u32,CF_DIB as u32,CF_BITMAP as u32,png].into_iter().any(|format|format!=0&&IsClipboardFormatAvailable(format)!=0);
            if image {handled=true;ui.run_action("attach");}
        });
        if handled{return 0}
    }
    DefSubclassProc(hwnd,message,wp,lp)
}
unsafe extern "system" fn compact_proc(hwnd:HWND,msg:u32,wp:usize,lp:isize,id:usize,_:usize)->isize{
    match msg {
        WM_MOUSEMOVE=>{
            let(x,y)=local_point(lp);let mut p=POINT{x,y};
            let parent=GetParent(hwnd);ClientToScreen(hwnd,&mut p);ScreenToClient(parent,&mut p);
            let mapped=((p.y as u16 as usize)<<16)|(p.x as u16 as usize);
            PostMessageW(parent,WM_MOUSEMOVE,wp,mapped as isize);
        }
        WM_MOUSELEAVE=>{PostMessageW(GetParent(hwnd),WM_MOUSELEAVE,0,0);}
        WM_NCDESTROY=>{RemoveWindowSubclass(hwnd,Some(compact_proc),id);}
        _=>{}
    }
    DefSubclassProc(hwnd,msg,wp,lp)
}
fn local_point(value:isize)->(i32,i32){
    ((value as u16 as i16) as i32,((value as usize >>16) as u16 as i16) as i32)
}

// During a transition Windows keeps the real controls and their keyboard focus.
// Empty child regions remove them from desktop painting and hit testing. WM_PRINT
// still draws their client surfaces into the offscreen workspace.
struct FadeMask { hwnd:HWND, previous:HRGN }
struct WorkspaceBitmap { dc:HDC, image:HBITMAP, original:HGDIOBJ, width:i32, height:i32, page:&'static str }
impl Drop for WorkspaceBitmap {
    fn drop(&mut self){unsafe{SelectObject(self.dc,self.original);DeleteObject(self.image);DeleteDC(self.dc);}}
}
#[derive(Default)]
struct Fade { active:bool, masks:Vec<FadeMask>, bitmap:Option<WorkspaceBitmap>, frame_buffer:Option<WorkspaceBitmap> }
thread_local! {static FADE:RefCell<Fade>=RefCell::new(Fade::default());}

fn workspace_masked()->bool{FADE.with(|state|state.borrow().active)}
pub(crate) fn workspace_layout_changed(hwnd:HWND){
    if FADE.with(|f|f.borrow().active){
        mask_workspace_for_animation(hwnd,true);
    }
}
pub(crate) fn mask_workspace_for_animation(hwnd:HWND,on:bool){
    FADE.with(|state|unsafe{
        let mut f=state.borrow_mut();
        if !on && !f.active{return}
        f.active=on;f.bitmap=None;
        if !on{
            f.frame_buffer=None;
            for mask in f.masks.drain(..){
                if (IsWindow(mask.hwnd)==0 || SetWindowRgn(mask.hwnd,mask.previous,0)==0) && !mask.previous.is_null(){DeleteObject(mask.previous);}
            }
            return
        }
        let mut child=GetTopWindow(hwnd);
        while !child.is_null(){
            if GetWindowLongW(child,GWL_STYLE) as u32 & WS_VISIBLE!=0
                && ui::workspace_child(GetDlgCtrlID(child)) && !f.masks.iter().any(|m|m.hwnd==child){
                let mut old=CreateRectRgn(0,0,0,0);
                if !old.is_null() && GetWindowRgn(child,old)==0{DeleteObject(old);old=null_mut();}
                let empty=CreateRectRgn(0,0,0,0);
                if !empty.is_null() && SetWindowRgn(child,empty,0)!=0{f.masks.push(FadeMask{hwnd:child,previous:old});}
                else{if !empty.is_null(){DeleteObject(empty);}if !old.is_null(){DeleteObject(old);}}
            }
            child=GetWindow(child,GW_HWNDNEXT);
        }
    })
}
unsafe fn print_children(hwnd:HWND,dc:HDC,workspace:Option<bool>){
    // Enumerating the controls outside the Ui borrow lets WM_DRAWITEM and
    // WM_CTLCOLOR enter the owner normally when the children print themselves.
    let mut windows=Vec::new();
    let mut child=GetTopWindow(hwnd);
    while !child.is_null(){
        // The private print target may have a hidden ancestor (e.g. signed out).
        // Print the children the layout marked visible on their own HWND.
        if GetWindowLongW(child,GWL_STYLE) as u32 & WS_VISIBLE!=0 && workspace.is_none_or(|want|want==ui::workspace_child(GetDlgCtrlID(child))){
            windows.push(child);
        }
        child=GetWindow(child,GW_HWNDNEXT);
    }
    for child in windows.into_iter().rev(){
        let mut bounds=RECT::default();
        if GetWindowRect(child,&mut bounds)==0{continue}
        let mut origin=POINT{x:bounds.left,y:bounds.top};
        ScreenToClient(hwnd,&mut origin);
        let state=SaveDC(dc);
        IntersectClipRect(dc,origin.x,origin.y,origin.x+bounds.right-bounds.left,origin.y+bounds.bottom-bounds.top);
        OffsetViewportOrgEx(dc,origin.x,origin.y,null_mut());
        if workspace==Some(true) {
            // Bypass DefWindowProc's nonclient clipping of the masked HWND.
            SendMessageW(child,WM_PRINTCLIENT,dc as usize,(PRF_CLIENT|PRF_ERASEBKGND) as isize);
            // WM_PRINTCLIENT only paints this control's client area. Some
            // compound pages delegate child printing to DefWindowProc(WM_PRINT).
            if !GetTopWindow(child).is_null(){print_children(child,dc,None);}
        }else{
            SendMessageW(child,WM_PRINT,dc as usize,(PRF_CLIENT|PRF_NONCLIENT|PRF_ERASEBKGND|PRF_CHILDREN) as isize);
        }
        RestoreDC(dc,state);
    }
}
impl WorkspaceBitmap{
    unsafe fn allocate(reference:HDC,width:i32,height:i32,page:&'static str)->Option<Self>{
        let dc=CreateCompatibleDC(reference);
        if dc.is_null(){return None}
        let image=CreateCompatibleBitmap(reference,width,height);
        if image.is_null(){DeleteDC(dc);return None}
        let original=SelectObject(dc,image);
        if original.is_null(){DeleteObject(image);DeleteDC(dc);return None}
        Some(Self{dc,image,original,width,height,page})
    }
    unsafe fn capture(hwnd:HWND,reference:HDC,width:i32,height:i32,page:&'static str)->Option<Self>{
        let bitmap=Self::allocate(reference,width,height,page)?;
        let dc=bitmap.dc;
        let painted=read_ui(|u|{
            SetDCBrushColor(dc,u.appearance.0);
            FillRect(dc,&RECT{left:0,top:0,right:width,bottom:height},GetStockObject(DC_BRUSH));
            u.paint_workspace(dc);
        });
        painted?;
        print_children(hwnd,dc,Some(true));
        Some(bitmap)
    }
    unsafe fn blend(&self,dest:HDC,mut bounds:RECT,alpha:u8,offset_x:i32){
        bounds.left=bounds.left.max(offset_x);bounds.right=bounds.right.min(offset_x+self.width);
        let w=bounds.right-bounds.left;let h=bounds.bottom-bounds.top;
        if w<=0 || h<=0 || alpha==0{return}
        AlphaBlend(dest,bounds.left,bounds.top,w,h,self.dc,bounds.left-offset_x,bounds.top,w,h,
            BLENDFUNCTION{BlendOp:AC_SRC_OVER as u8,BlendFlags:0,SourceConstantAlpha:alpha,AlphaFormat:0});
    }
}
unsafe fn composite_workspace(hwnd:HWND,dc:HDC,frame:&island::Frame){
    let alpha=(frame.body_alpha.clamp(0.0,1.0)*255.0).round() as u8;
    if alpha==0{return}
    let mut bounds=RECT::default();
    if GetClientRect(hwnd,&mut bounds)==0{return}
    let Some(page)=read_ui(|u|u.page) else{return};
    let fresh=FADE.with(|f|{
        let s=f.borrow();!s.bitmap.as_ref().is_some_and(|b|b.width==bounds.right&&b.height==bounds.bottom&&b.page==page)
    });
    if fresh {
        let bitmap=WorkspaceBitmap::capture(hwnd,dc,bounds.right,bounds.bottom,page);
        FADE.with(|f|f.borrow_mut().bitmap=bitmap);
    }
    FADE.with(|f|if let Some(bitmap)=&f.borrow().bitmap{
        // The shared header, border and resize indicator are painted by the host.
        let scale=island::scale_and_limit().0;
        let r=frame.visible_workspace(scale);
        // CSS centers the expanded-width content under the moving header, even
        // for left/right alignment or a compact header with attached cards.
        bitmap.blend(dc,r,alpha,frame.workspace_offset());
    });
}
unsafe fn paint_to(hwnd:HWND,dc:HDC,children:bool){
    // Synchronous WM_PAINT during child layout cannot obtain a coherent
    // surface or enter owner-draw handlers. The outer layout invalidates again.
    if WINDOW.with(|s|s.try_borrow().is_err()){DEFERRED_PAINT.set(hwnd);return}
    let saved=SaveDC(dc);
    let mask=CreateRectRgn(0,0,0,0);
    if !mask.is_null(){if GetWindowRgn(hwnd,mask)>0{ExtSelectClipRgn(dc,mask,RGN_AND);}DeleteObject(mask);}
    let frame=island::frame();
    if frame.animating || workspace_masked() {
        // Paint the surface and translucent workspace together. Painting them
        // separately to the window briefly exposes the empty background.
        let mut client=RECT::default();GetClientRect(hwnd,&mut client);
        let buffer=FADE.with(|f|f.borrow_mut().frame_buffer.take())
            .filter(|b|b.width==client.right && b.height==client.bottom)
            .or_else(||WorkspaceBitmap::allocate(dc,client.right,client.bottom,""));
        let target=buffer.as_ref().map_or(dc,|b|b.dc);
        read_ui(|u|u.paint_surface(target));
        composite_workspace(hwnd,target,&frame);
        if children{print_children(hwnd,target,Some(false));}
        if let Some(buffer)=buffer{
            let b=frame.body;
            BitBlt(dc,b.left,b.top,b.right-b.left,b.bottom-b.top,target,b.left,b.top,SRCCOPY);
            FADE.with(|f|f.borrow_mut().frame_buffer=Some(buffer));
        }
    }else{
        read_ui(|u|u.paint(dc));
        if children{print_children(hwnd,dc,None);}
    }
    RestoreDC(dc,saved);
}
unsafe extern "system" fn window_proc(hwnd:HWND,msg:u32,wp:usize,lp:isize)->isize{
    match msg{
        WM_NOTIFY if wp==inline_images::NOTE_EDITOR as usize=>inline_images::link_notification(hwnd,lp),
        side_widgets::MESSAGE=>{with_ui(|u|side_widgets::command(u,wp));0},
        shell::COMMIT_PAGE=>{with_ui(shell::finish_navigation);0},
        devices::OCR_DONE=>{with_ui(devices::finish_scan);0},
        WM_HSCROLL=>{with_ui(|u|u.home_seek_scroll(lp as _,wp as u16));0},
        WM_COMMAND=>{with_ui(|u|match (wp&0xffff) as u16{
            188=>shell::step_height(u,-20),189=>shell::step_height(u,20),id=>u.command(id,(wp>>16) as u16)
        });0},
        WM_TIMER=>{
            with_ui(|u|match wp {
                shell::MOTION_TIMER=>shell::animate(u),
                1=>{shell::visibility(u,true);if !u.paused && !u.locked{
                    core::tick(u);devices::finish_scan(u);activity::tick(u);
                    if !u.isolated{workspace::tick(u);}
                    shell::model_tick(u);
                }},
                2=>shell::visibility(u,false),_=>{}
            });
            0
        },
        WM_PAINT=>{let mut ps=PAINTSTRUCT::default();let dc=BeginPaint(hwnd,&mut ps);if !dc.is_null() && !motion_surface::active(hwnd){paint_to(hwnd,dc,false);}EndPaint(hwnd,&ps);0},
        WM_PRINT|WM_PRINTCLIENT=>{if wp!=0{paint_to(hwnd,wp as HDC,lp as i32&PRF_CHILDREN!=0);}0},
        WM_DRAWITEM=>match read_ui(|u|u.draw_item(lp)){
            Some(true)=>1,
            None if lp!=0=>{
                // The native button may synchronously request a draw while
                // navigation owns Ui. Repaint after the outer operation.
                DEFERRED_PAINT.set(hwnd);1
            }
            _=>DefWindowProcW(hwnd,msg,wp,lp)
        },
        WM_CTLCOLORSTATIC|WM_CTLCOLORBTN|WM_CTLCOLORLISTBOX|WM_CTLCOLOREDIT=>{
            let brush=ui::control_brush(hwnd,msg,wp as _,lp as _);
            if brush!=0{brush}else{DefWindowProcW(hwnd,msg,wp,lp)}
        },
        WM_ERASEBKGND=>1,
        WM_SETTINGCHANGE|WM_THEMECHANGED=>{with_ui(|u|{
            u.apply_appearance();
            shell::motion_setting_changed(u);
            InvalidateRect(hwnd,null(),0);
        });0},
        WM_LBUTTONDOWN=>{
            let(x,y)=local_point(lp);let mut handled=false;
            with_ui(|u|{
                handled=shell::resize_down(u,x,y);
                if !handled && island::frame().animating{
                    if let Some(child)=u.visible_navigation_at(x,y){
                        SetCapture(hwnd);NAV_PRESS.set(child);handled=true;
                    }
                }
            });
            if handled{0}else{DefWindowProcW(hwnd,msg,wp,lp)}
        },
        WM_MOUSELEAVE=>{with_ui(|u|if u.isolated{shell::pointer_at(u,false,false)}else{shell::visibility(u,false)});0},
        WM_MOUSEMOVE=>{let(x,y)=local_point(lp);
            let mut track=TRACKMOUSEEVENT{cbSize:std::mem::size_of::<TRACKMOUSEEVENT>() as _,dwFlags:TME_LEAVE,hwndTrack:hwnd,dwHoverTime:0};
            TrackMouseEvent(&mut track);
            with_ui(|u|{if !shell::resize_move(u,x,y){let body=island::contains(&island::frame(),x,y);shell::pointer_at(u,body,body);}});0},
        WM_LBUTTONUP=>{
            let(x,y)=local_point(lp);let pressed=NAV_PRESS.replace(null_mut());
            if !pressed.is_null(){
                if GetCapture()==hwnd{ReleaseCapture();}
                let mut activate=false;
                with_ui(|u|activate=u.visible_navigation_at(x,y)==Some(pressed));
                if activate{SetFocus(pressed);SendMessageW(pressed,BM_CLICK,0,0);}
            }else{
                with_ui(|u|{if GetCapture()==hwnd{shell::resize_up(u);}else if !u.expanded && island::contains(&island::frame(),x,y){shell::expand(u,true);}});
            }
            0
        },
        WM_CAPTURECHANGED=>{NAV_PRESS.set(null_mut());with_ui(shell::resize_up);0},
        WM_SETCURSOR=>{let mut p=POINT::default();GetCursorPos(&mut p);ScreenToClient(hwnd,&mut p);if wp==hwnd as usize && shell::resize_grip(p.x,p.y){SetCursor(LoadCursorW(null_mut(),IDC_SIZENS));1}else{DefWindowProcW(hwnd,msg,wp,lp)}},
        WM_ACTIVATE=>{if (wp as u16)as u32==WA_INACTIVE{with_ui(|u|shell::deactivated(u,lp as _));}DefWindowProcW(hwnd,msg,wp,lp)},
        WM_DPICHANGED|WM_DISPLAYCHANGE=>{with_ui(shell::position);0},
        WM_CONTEXTMENU=>{with_ui(|ui|shell::context_menu(ui,wp as HWND,lp));0},
        shell::CALLBACK=>{if lp as u32==WM_RBUTTONUP{with_ui(shell::menu);}else if lp as u32==WM_LBUTTONUP{with_ui(|u|shell::expand(u,true));}else if lp as u32==NIN_BALLOONUSERCLICK{with_ui(|u|{if let Some(id)=u.due_note.clone(){u.active_note=Some(id);shell::expand(u,true);u.navigate("notes");}});}0},
        shell::REVEAL|WM_HOTKEY=>{with_ui(|u|shell::expand(u,if msg==shell::REVEAL{true}else{!u.expanded}));0},
        WM_DROPFILES=>{let h=wp as HDROP;let count=DragQueryFileW(h,0xffffffff,null_mut(),0).min(128);let mut paths=Vec::new();for i in 0..count{let n=DragQueryFileW(h,i,null_mut(),0);if n>32767{continue}let mut buffer=vec![0;n as usize+1];let len=DragQueryFileW(h,i,buffer.as_mut_ptr(),buffer.len() as _);if len>0{paths.push(PathBuf::from(OsString::from_wide(&buffer[..len as usize])));}}DragFinish(h);with_ui(|u|{if u.dirty_note{if let Err(reason)=core::save_note(u){u.status(&reason);return}}shell::expand(u,true);let message=workspace::drop_files(u,paths).unwrap_or_else(|e|e);u.status(&message);});0},
        WM_CLOSE=>{let mut quit=false;with_ui(|u|{if u.quitting||u.isolated{
            if let Err(reason)=side_widgets::flush(u){
                u.status(&reason);
                if !shell::confirm(u,&format!("{reason}\r\n\r\nDiscard these side note edits and quit Buddy?")){u.quitting=false;return;}
            }
            if u.dirty_note && !u.isolated {
                if let Err(reason)=core::save_note(u){
                    u.status(&reason);
                    if !shell::confirm(u,&format!("{reason}\r\n\r\nDiscard these edits and quit Buddy?")){u.quitting=false;return;}
                }
            }
            quit=true;
        }else{shell::expand(u,false);}});if quit{DestroyWindow(hwnd);}0},
        WM_WTSSESSION_CHANGE=>{with_ui(|u|match wp as u32{
            WTS_SESSION_LOCK|WTS_SESSION_LOGOFF|WTS_REMOTE_DISCONNECT=>{u.locked=true;devices::leave(u.page);devices::stop();activity::stop();ShowWindow(hwnd,SW_HIDE);side_widgets::sync(u);},
            WTS_SESSION_UNLOCK|WTS_SESSION_LOGON|WTS_REMOTE_CONNECT=>{u.locked=false;if !u.paused && !u.isolated{devices::restore(&u.profile);if !u.suppressed && u.hidden_until.is_none(){ShowWindow(hwnd,SW_SHOWNOACTIVATE);}side_widgets::sync(u);}},_=>{}
        });0},
        WM_QUERYENDSESSION=>1,
        WM_ENDSESSION if wp!=0=>{with_ui(|u|{let _=side_widgets::flush(u);if u.dirty_note{let _=core::save_note(u);}devices::stop();activity::stop();});0},
        WM_DESTROY=>{motion_surface::end(hwnd);mask_workspace_for_animation(hwnd,false);side_widgets::close();with_ui(|u|{shell::tray(u,false);if !u.isolated{WTSUnRegisterSessionNotification(hwnd);}UnregisterHotKey(hwnd,1);devices::stop();activity::stop();});PostQuitMessage(0);0},
        _=>DefWindowProcW(hwnd,msg,wp,lp)
    }
}
fn profile()->Result<(PathBuf,bool),String>{
    let args:Vec<OsString>=std::env::args_os().skip(1).collect();if args.is_empty(){let local=std::env::var_os("LOCALAPPDATA").ok_or("Windows did not provide a local application-data folder.")?;return Ok((PathBuf::from(local).join("Buddy Work Rust"),false))}
    if args.len()==2 && args[0]=="--benchmark-profile"{let path=Path::new(&args[1]);if path.is_absolute(){return Ok((path.to_owned(),true))}}
    Err("Use --benchmark-profile with an absolute path for an isolated test.".into())
}
fn run()->Result<(),String>{unsafe{
    let (profile,isolated)=profile()?;std::fs::create_dir_all(&profile).map_err(|e|e.to_string())?;
    let class=wide(CLASS);let instance=GetModuleHandleW(null());
    // Create each normal instance only once within the logged-in Windows session.
    let mutex=if isolated{null_mut()}else{let h=CreateMutexW(null(),0,wide("Local\\BuddyWorkRust.SingleInstance").as_ptr());if h.is_null(){return Err("Windows could not establish the Buddy session.".into())}if GetLastError()==ERROR_ALREADY_EXISTS{let existing=FindWindowW(class.as_ptr(),null());if !existing.is_null(){PostMessageW(existing,shell::REVEAL,0,0);}CloseHandle(h);return Ok(())}h};
    SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    let com=CoInitializeEx(null(),COINIT_APARTMENTTHREADED as u32);if com<0{return Err(format!("Windows COM initialization failed ({com:#x})."))}
    let icon=LoadIconW(instance,101usize as _);
    let descriptor=WNDCLASSW{lpfnWndProc:Some(window_proc),hInstance:instance,lpszClassName:class.as_ptr(),hCursor:LoadCursorW(null_mut(),IDC_ARROW),hIcon:if icon.is_null(){LoadIconW(null_mut(),IDI_APPLICATION)}else{icon},hbrBackground:null_mut(),..Default::default()};
    if RegisterClassW(&descriptor)==0{return Err("Windows could not register the Buddy window.".into())}
    // Present the parent background and its GDI/Rich Edit child windows as a
    // single paint during workspace swaps and the hand-off from a fade.
    let hwnd=CreateWindowExW(WS_EX_TOOLWINDOW|WS_EX_TOPMOST|WS_EX_COMPOSITED,class.as_ptr(),wide("Buddy Work Rust").as_ptr(),WS_POPUP|WS_CLIPCHILDREN,
        0,0,760,520,null_mut(),null_mut(),instance,null());if hwnd.is_null(){return Err("Windows could not create the Buddy window.".into())}
    let mut ui=Ui::new(TypedWindow(hwnd),profile,isolated)?;SetWindowSubclass(ui.detail_box,Some(editor_proc),1,0);
    for id in [ui::COMPACT,ui::COMPACT_TOOLS]{SetWindowSubclass(GetDlgItem(hwnd,id as i32),Some(compact_proc),2,0);}
    ui.reorder_navigation();ui.navigate("home");core::tick(&mut ui);shell::position(&mut ui);
    if !isolated{WTSRegisterSessionNotification(hwnd,0);shell::tray(&ui,true);if RegisterHotKey(hwnd,1,MOD_CONTROL|MOD_ALT|MOD_NOREPEAT,VK_SPACE as u32)==0{ui.status("Ctrl+Alt+Space is already in use. Open Buddy from its tray icon.");}devices::restore(&ui.profile);}
    DragAcceptFiles(hwnd,1);SetTimer(hwnd,1,1000,None);if ui.expanded{SetTimer(hwnd,2,50,None);}
    WINDOW.with(|s|*s.borrow_mut()=Some(ui));ShowWindow(hwnd,SW_SHOWNOACTIVATE);
    let mut msg=MSG::default();
    while GetMessageW(&mut msg,null_mut(),0,0)>0{
        chrome::scroll::route_wheel(&mut msg);
        if inline_images::editor_shortcut(&msg)||side_widgets::dialog(&msg){continue}
        if msg.message==WM_KEYDOWN && matches!(msg.wParam as u16,VK_TAB|VK_UP|VK_DOWN|VK_LEFT|VK_RIGHT){chrome::keyboard_navigation(true);}
        if matches!(msg.message,WM_LBUTTONDOWN|WM_RBUTTONDOWN){chrome::keyboard_navigation(false);}
        if msg.message==WM_KEYDOWN && (msg.hwnd==hwnd || IsChild(hwnd,msg.hwnd)!=0){
            if msg.wParam==VK_ESCAPE as usize{with_ui(|u|{if !u.editor_escape(){shell::expand(u,false)}});continue}
            let frame=island::frame();
            if !frame.expanded && msg.hwnd==hwnd{
                if msg.wParam==VK_RETURN as usize || msg.wParam==VK_SPACE as usize{
                    with_ui(|u|shell::expand(u,true));continue
                }
                // The closing body is inert until the compact controls take focus.
                if frame.animating && msg.wParam==VK_TAB as usize{continue}
            }
        }
        if IsDialogMessageW(hwnd,&msg)==0{TranslateMessage(&msg);DispatchMessageW(&msg);}
    }
    WINDOW.with(|s|{s.borrow_mut().take();});CoUninitialize();if !mutex.is_null(){CloseHandle(mutex);}Ok(())
}}
fn main(){if let Err(e)=run(){unsafe{MessageBoxW(null_mut(),wide(&e).as_ptr(),wide("Buddy Work Rust").as_ptr(),MB_ICONERROR);}}}

#[cfg(test)]
mod fade_tests {
    use super::*;
    // A private, offscreen window verifies Windows' actual control rendering.
    // No desktop input, capture, clipboard, user profile or camera is touched.
    #[test]
    fn masked_native_child_prints_into_fade_and_restores_its_hit_target() {unsafe{
        let class=wide("STATIC");
        let hwnd=CreateWindowExW(WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE,class.as_ptr(),null(),WS_POPUP,
            -30000,-30000,100,100,null_mut(),null_mut(),null_mut(),null());
        assert!(!hwnd.is_null());
        let child=CreateWindowExW(0,class.as_ptr(),null(),WS_CHILD|WS_VISIBLE|4,12,20,33,31,
            hwnd,301usize as _,null_mut(),null());
        assert!(!child.is_null());
        ShowWindow(hwnd,SW_SHOWNOACTIVATE);
        mask_workspace_for_animation(hwnd,true);
        let mask=CreateRectRgn(0,0,0,0);
        assert_eq!(GetWindowRgn(child,mask),NULLREGION);
        let reference=GetDC(hwnd);
        let make=||{
            let dc=CreateCompatibleDC(reference);let image=CreateCompatibleBitmap(reference,100,100);
            let original=SelectObject(dc,image);
            let empty=RECT{left:0,top:0,right:100,bottom:100};
            FillRect(dc,&empty,GetStockObject(WHITE_BRUSH));
            WorkspaceBitmap{dc,image,original,width:100,height:100,page:"home"}
        };
        let source=make();let dest=make();let direct=make();
        SendMessageW(child,WM_PRINTCLIENT,direct.dc as usize,(PRF_CLIENT|PRF_ERASEBKGND) as isize);
        let native=GetPixel(direct.dc,8,8);assert_ne!(native,0xffffff);
        print_children(hwnd,source.dc,Some(true));
        assert_eq!(GetPixel(source.dc,22,30),native,"native static rectangle printed through empty region");
        assert_eq!(GetPixel(source.dc,2,3),0xffffff);
        source.blend(dest.dc,RECT{left:0,top:0,right:100,bottom:100},128,0);
        let mid=GetPixel(dest.dc,22,30);
        for shift in [0,8,16]{
            let expected=(((native>>shift)&255) as i32+255)/2;
            assert!((((mid>>shift)&255) as i32-expected).abs()<=2,"half opacity {mid:#x}");
        }
        assert_eq!(GetPixel(dest.dc,2,3),0xffffff);
        let added=CreateWindowExW(0,class.as_ptr(),null(),WS_CHILD|WS_VISIBLE,2,2,10,10,
            hwnd,302usize as _,null_mut(),null());
        workspace_layout_changed(hwnd);
        assert_eq!(GetWindowRgn(added,mask),NULLREGION,"a lazily created panel joins the same fade");
        mask_workspace_for_animation(hwnd,false);
        assert_eq!(GetWindowRgn(added,mask),0);
        assert_eq!(GetWindowRgn(child,mask),0,"original rectangular hit target restored");
        DeleteObject(mask);drop(direct);drop(dest);drop(source);ReleaseDC(hwnd,reference);DestroyWindow(hwnd);
    }}
}
