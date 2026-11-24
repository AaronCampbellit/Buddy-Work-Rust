use crate::{core,devices,island,side_widgets,ui::wide,Ui};
use std::cell::RefCell;
use std::{ptr::{null,null_mut},time::{Duration,Instant}};
use windows_sys::Win32::{Foundation::*,Graphics::Gdi::*,System::{DataExchange::*,LibraryLoader::GetModuleHandleW,Memory::*,Ole::CF_UNICODETEXT,Registry::*,SystemInformation::GetLocalTime,Threading::*},UI::{Accessibility::{HIGHCONTRASTW,HCF_HIGHCONTRASTON},HiDpi::*,Input::KeyboardAndMouse::*,Shell::*,WindowsAndMessaging::*}};

pub const CALLBACK:u32=WM_APP+25;
pub const REVEAL:u32=WM_APP+26;
pub fn focus(h:HWND){unsafe{SetFocus(h);}}
pub fn dpi(h:HWND)->u32{unsafe{GetDpiForWindow(h).max(96)}}
pub fn confirm(ui:&Ui,s:&str)->bool{unsafe{MessageBoxW(ui.hwnd.0,wide(s).as_ptr(),wide("Buddy Work").as_ptr(),MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2)==IDYES}}
pub fn clock(mode: &str, custom: &str) -> String {
    if mode=="custom"{return custom.to_owned()}
    #[link(name="kernel32")]
    extern "system" {
        fn GetTimeFormatEx(locale:*const u16,flags:u32,time:*const SYSTEMTIME,format:*const u16,out:*mut u16,size:i32)->i32;
        fn GetDateFormatEx(locale:*const u16,flags:u32,time:*const SYSTEMTIME,format:*const u16,out:*mut u16,size:i32,calendar:*const u16)->i32;
        fn GetLocaleInfoEx(locale:*const u16,key:u32,out:*mut u16,size:i32)->i32;
    }
    let mut local=SYSTEMTIME::default();
    let mut out=[0u16;128];
    unsafe{GetLocalTime(&mut local);}
    let n=unsafe{GetTimeFormatEx(null(),2,&local,null(),out.as_mut_ptr(),128)};
    let time=if n>1{String::from_utf16_lossy(&out[..n as usize-1])}else{format!("{:02}:{:02}",local.wHour,local.wMinute)};
    if mode!="date" && mode!="time-date"{return time}
    let n=unsafe{GetLocaleInfoEx(null(),0x0078,out.as_mut_ptr(),128)};
    let pattern=if n>1 {String::from_utf16_lossy(&out[..n as usize-1]).replace("MMMM","MMM")}else{"MMM d".to_owned()};
    let n=unsafe{GetDateFormatEx(null(),0,&local,wide(&pattern).as_ptr(),out.as_mut_ptr(),128,null())};
    let day=if n>1{String::from_utf16_lossy(&out[..n as usize-1])}else{format!("{:04}-{:02}-{:02}",local.wYear,local.wMonth,local.wDay)};
    if mode=="date"{day}else{format!("{}  {}",time,day)}
}
unsafe extern "system" fn enum_monitor(h:HMONITOR,_:HDC,_:*mut RECT,ctx:isize)->i32{
    let list=&mut *(ctx as *mut Vec<(String,RECT,bool,String)>);let mut i=MONITORINFOEXW::default();i.monitorInfo.cbSize=std::mem::size_of_val(&i) as u32;
    if GetMonitorInfoW(h,&mut i.monitorInfo)!=0{let n=i.szDevice.iter().position(|&p|p==0).unwrap_or(i.szDevice.len());let mut device=DISPLAY_DEVICEW::default();device.cb=std::mem::size_of_val(&device) as u32;
        let hardware=if EnumDisplayDevicesW(i.szDevice.as_ptr(),0,&mut device,EDD_GET_DEVICE_INTERFACE_NAME)!=0{
            let n=device.DeviceID.iter().position(|&c|c==0).unwrap_or(device.DeviceID.len());String::from_utf16_lossy(&device.DeviceID[..n])
        }else{String::new()};
        list.push((String::from_utf16_lossy(&i.szDevice[..n]),i.monitorInfo.rcWork,i.monitorInfo.dwFlags&1!=0,hardware));}1
}
pub fn screens()->Vec<(String,RECT,bool,String)>{let mut monitors=Vec::new();unsafe{EnumDisplayMonitors(null_mut(),null(),Some(enum_monitor),&mut monitors as *mut _ as isize);}monitors}
pub fn remember_display(ui:&mut Ui,name:&str){
    let matched=screens().into_iter().find(|d|d.0==name);
    core::set(ui,"display",name);
    let (hardware,bounds)=matched.map(|m|(m.3,format!("{},{},{},{}",m.1.left,m.1.top,m.1.right,m.1.bottom))).unwrap_or_default();
    core::set(ui,"displayHardware",&hardware);core::set(ui,"displayBounds",&bounds);
}
pub fn color_value(text:&str)->Option<u32>{
    let hex=text.strip_prefix('#')?;
    if hex.len()!=6 || !hex.bytes().all(|c|c.is_ascii_hexdigit()){return None}
    let rgb=u32::from_str_radix(hex,16).ok()?;
    Some(((rgb&0xff)<<16)|(rgb&0xff00)|((rgb>>16)&0xff))
}
pub fn palette(ui:&Ui)->(u32,u32,u32,u32){
    unsafe{
        let mut contrast=HIGHCONTRASTW::default();contrast.cbSize=std::mem::size_of_val(&contrast) as u32;
        if SystemParametersInfoW(SPI_GETHIGHCONTRAST,contrast.cbSize,(&mut contrast as *mut HIGHCONTRASTW).cast(),0)!=0 && (contrast.dwFlags&HCF_HIGHCONTRASTON)!=0{return (GetSysColor(COLOR_BTNFACE),GetSysColor(COLOR_WINDOW),GetSysColor(COLOR_WINDOWTEXT),GetSysColor(COLOR_WINDOWTEXT));}
        let mut light:u32=1;let mut size=4;
        RegGetValueW(HKEY_CURRENT_USER,wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize").as_ptr(),wide("AppsUseLightTheme").as_ptr(),RRF_RT_REG_DWORD,null_mut(),(&mut light as *mut u32).cast(),&mut size);
        let dark=match core::setting(ui,"theme","system"){"dark"=>true,"light"=>false,_=>light==0};
        let (dark_accent,light_accent,hue)=match core::setting(ui,"accent","cyan"){
            "ocean"=>("#74b5ff","#124f9e",Some(218.0)),"violet"=>("#bd9aff","#61349f",Some(265.0)),"rose"=>("#ff9dbb","#982247",Some(338.0)),
            "forest"=>("#73dfb1","#096342",Some(157.0)),"amber"=>("#f5c16b","#744707",Some(36.0)),_=>("#25d8f5","#007f9c",None)
        };
        let hsl=|h:f64,s:f64,l:f64|{
            let channel=|n:f64|{let k=(n+h/30.0)%12.0;let a=s*l.min(1.0-l);
                ((l-a*(k-3.0).min(9.0-k).clamp(-1.0,1.0))*255.0).round() as u32};
            channel(0.0)|(channel(8.0)<<8)|(channel(4.0)<<16)
        };
        let (bg,field,text)=match (dark,hue){
            (true,Some(h))=>(hsl(h,0.62,0.18),hsl(h,0.56,0.24),0xffffff),
            (false,Some(h))=>(hsl(h,0.82,0.88),hsl(h,0.76,0.94),hsl(h,0.65,0.13)),
            (true,None)=>(0x1b1712,0x27221b,0xfaf8f6),(false,None)=>(0xfbf9f7,0xf4f1ed,0x272017)
        };
        let selected=|key,default|color_value(core::setting(ui,key,"")).unwrap_or(default);
        (selected("customMain",bg),selected("customField",field),selected("customTextColor",text),selected("customAccent",color_value(if dark{dark_accent}else{light_accent}).unwrap()))

    }
}
pub const MOTION_TIMER:usize=9;
const GRACE:Duration=Duration::from_millis(300);
const REOPEN_DELAY:Duration=Duration::from_millis(1_200);

#[derive(Default)]
struct Interaction {
    pinned:bool,
    hover_blocked_until:Option<Instant>,
    left_at:Option<Instant>,
    pointer_seen:bool,
    inside_body:bool,
    inside_union:bool,
    side_open_hold:bool,
    drag:Option<(i32,i32)>,
    resized_height:Option<i32>,
    last_widgets:Vec<island::Widget>,
}
thread_local! {static INTERACTION:RefCell<Interaction>=RefCell::new(Interaction::default());}

fn widget_list(ui:&Ui)->Vec<island::Widget>{
    use island::Widget::*;
    if core::setting(ui,"widgets","on")!="on"{return Vec::new()}
    let mut cards=Vec::new();
    if ui.call_state.is_some(){cards.push(Camera);}
    if core::widget_enabled(ui,"pomWidget","pomEnd"){cards.push(Focus);}
    if core::widget_enabled(ui,"watchWidget","watchStart"){cards.push(Stopwatch);}
    let pinned=core::setting(ui,"noteWidget","");
    if !pinned.is_empty() && ui.records.get("notes").is_some_and(|notes|notes.iter().any(|n|n.first().is_some_and(|id|id==pinned))){cards.push(Note);}
    if !crate::workspace::widget_text(ui).is_empty(){cards.push(Clipboard);}
    cards
}

pub fn position(ui:&mut Ui){
    let displays=screens();
    let target=core::setting(ui,"display","primary");
    let hardware=core::setting(ui,"displayHardware","");
    let bounds=core::setting(ui,"displayBounds","");
    let requested=if target=="primary"{None}else if !hardware.is_empty(){
        displays.iter().find(|m|m.3==hardware)
    }else{
        displays.iter().find(|m|m.0==target).or_else(||displays.iter().find(|m|
            !bounds.is_empty() && format!("{},{},{},{}",m.1.left,m.1.top,m.1.right,m.1.bottom)==bounds))
    };
    let work=requested.or_else(||displays.iter().find(|m|m.2)).or_else(||displays.first()).map(|m|m.1)
        .unwrap_or(RECT{left:0,top:0,right:1024,bottom:768});
    let center=POINT{x:work.left+(work.right-work.left)/2,y:work.top+(work.bottom-work.top)/2};
    let monitor=unsafe{MonitorFromPoint(center,MONITOR_DEFAULTTONEAREST)};
    let (mut x_dpi,mut y_dpi)=(96u32,96u32);
    if unsafe{GetDpiForMonitor(monitor,MDT_EFFECTIVE_DPI,&mut x_dpi,&mut y_dpi)}<0{x_dpi=dpi(ui.hwnd.0)}
    ui.zoom=core::setting(ui,"scale","100").parse::<i32>().unwrap_or(100).clamp(75,175);
    let placement=match core::setting(ui,"position","top-center"){
        "top-left"=>island::Placement::Left,"top-right"=>island::Placement::Right,_=>island::Placement::Center
    };
    let height=INTERACTION.with(|i|i.borrow().resized_height)
        .unwrap_or_else(||core::setting(ui,"expandedHeight","300").parse().unwrap_or(300));
    let widgets=widget_list(ui);
    let layout=island::Layout::new(work,x_dpi,ui.zoom,placement,height,&widgets);
    let host=layout.host;
    island::configure(layout,ui.expanded);
    INTERACTION.with(|i|i.borrow_mut().last_widgets=widgets);
    unsafe{
        let mut old=RECT::default();GetWindowRect(ui.hwnd.0,&mut old);
        if !island::same_rect(old,host){SetWindowPos(ui.hwnd.0,HWND_TOPMOST,host.left,host.top,host.right-host.left,
            host.bottom-host.top,SWP_NOACTIVATE|SWP_NOOWNERZORDER);}
    }
    ui.apply_appearance();crate::note_preview::validate(ui);
    if island::needs_tick() && reduce_motion(ui){
        island::request(ui.expanded,true);
        unsafe{KillTimer(ui.hwnd.0,MOTION_TIMER);}
    }
    render_frame(ui,true);
}
pub const COMMIT_PAGE:u32=WM_APP+27;
pub const OPEN_WIDGET_NOTE:u32=WM_APP+28;

pub fn finish_navigation(ui:&mut Ui){
    if island::frame().animating || !crate::workspace_masked(){return}
    crate::mask_workspace_for_animation(ui.hwnd.0,false);
    if let Some(page)=ui.pending_page.take(){
        if ui.expanded && !ui.suppressed && !ui.locked && !ui.paused{ui.navigate(page);crate::present_frame(ui.hwnd.0);return}
    }
    ui.layout();crate::present_frame(ui.hwnd.0);
}
fn render_frame(ui:&mut Ui,layout_changed:bool){
    let frame=island::frame();
    if frame.expanded && !frame.animating && ui.pending_widget_note.is_some(){
        if crate::note_preview::active(){
            // Keep the real, full-size note visible during the live HWND swap.
            ui.layout_motion_header();crate::present_frame(ui.hwnd.0);
            unsafe{PostMessageW(ui.hwnd.0,OPEN_WIDGET_NOTE,0,0);}
            return
        }
        side_widgets::finish_widget_open(ui);
    }
    // Keep the last full-width snapshot on screen until the requested page is
    // ready. Laying out and unmasking the obsolete live page first makes the
    // user wait for two complete workspace swaps.
    if !frame.animating && frame.expanded && ui.pending_page.is_some() && crate::workspace_masked(){
        ui.layout_motion_header();
        crate::present_frame(ui.hwnd.0);
        unsafe{PostMessageW(ui.hwnd.0,COMMIT_PAGE,0,0);}
        return
    }
    let full_layout=layout_changed || !frame.animating;
    if full_layout{
        crate::mask_workspace_for_animation(ui.hwnd.0,frame.animating);
    }
    if full_layout{
        ui.layout();
        if !frame.expanded && !frame.animating && unsafe{GetFocus()}==ui.hwnd.0{
            unsafe{SetFocus(GetDlgItem(ui.hwnd.0,crate::ui::COMPACT as i32));}
        }
        side_widgets::sync(ui);
    }else{
        // The workspace was laid out at its full size when the transition began.
        // Moving/repainting every live editor each frame causes missed first frames.
        ui.layout_motion_header();
    }
    crate::present_frame(ui.hwnd.0);

}

#[cfg(test)]
pub fn capture_transition_at(ui:&mut Ui,elapsed:Duration){
    island::sample_after(elapsed);
    render_frame(ui,false);
}
pub fn animate(ui:&mut Ui){
    let moving=island::frame().animating;
    island::sample();
    if moving{
        render_frame(ui,false);
        if ui.pending_widget_note.is_some() && !crate::note_preview::active() && island::frame().progress>0.0{
            unsafe{PostMessageW(ui.hwnd.0,OPEN_WIDGET_NOTE,0,0);}
        }
        // The first reveal can create an editor and several HWNDs. Its CSS
        // opacity clock begins once the strip is actually on screen.
        if !island::frame().animating{island::cards_presented();}
    }else{side_widgets::set_opacity(island::frame().widgets_alpha);}
    if !island::needs_tick(){unsafe{KillTimer(ui.hwnd.0,MOTION_TIMER);}}
}

pub fn motion_setting_changed(ui:&mut Ui){
    if island::needs_tick() && reduce_motion(ui){
        island::request(ui.expanded,true);
        render_frame(ui,true);
        unsafe{KillTimer(ui.hwnd.0,MOTION_TIMER);}
    }
}
pub fn reduce_motion(ui:&Ui)->bool{
    let mut enabled=1i32;
    let known=unsafe{SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION,0,(&mut enabled as *mut i32).cast(),0)}!=0;
    core::setting(ui,"motion","off")=="on" || (known && enabled==0)
}

fn change(ui:&mut Ui,open:bool,explicit:bool){
    let frame=island::frame();
    if !open && !explicit && INTERACTION.with(|i|i.borrow().pinned){return}
    if !open && ui.dirty_note && ui.page=="notes"{
        if let Err(reason)=core::save_note(ui){ui.status(&reason);return}
    }
    if !open{ui.pending_page=None;ui.pending_widget_note=None;crate::note_preview::clear();}
    if !open && explicit{
        INTERACTION.with(|i|{let mut i=i.borrow_mut();i.pinned=false;i.hover_blocked_until=Some(Instant::now()+REOPEN_DELAY);});
    }
    if open==ui.expanded{
        if open && explicit{
            ui.hidden_until=None;ui.suppressed=false;
            unsafe{ShowWindow(ui.hwnd.0,SW_SHOWNOACTIVATE);SetForegroundWindow(ui.hwnd.0);}
        }
        return
    }
    let opening=open && !ui.expanded;
    if opening && ui.pending_widget_note.is_none() {if let Err(reason)=side_widgets::flush(ui){ui.status(&reason);balloon(ui,"Buddy note",&reason);return}}
    if !open && ui.expanded {devices::leave(ui.page);ui.collapsed_at=Some(Instant::now());}
    if !open{
        unsafe{if IsChild(ui.hwnd.0,GetFocus())!=0{SetFocus(ui.hwnd.0);}}
    }
    ui.expanded=open;ui.opened_by_hover=open && !explicit;
    ui.hidden_until=None;ui.suppressed=false;
    INTERACTION.with(|i|{let mut i=i.borrow_mut();i.left_at=None;if opening{i.pointer_seen=i.inside_union;}if !open{i.pointer_seen=false;i.side_open_hold=false;}});
    if opening && ui.pending_widget_note.is_none(){
        let restore_home=ui.collapsed_at.is_some_and(|t|t.elapsed()>=Duration::from_secs(core::setting(ui,"retention","15").parse().unwrap_or(15)));
        if restore_home{ui.navigate("home");}
        if !ui.isolated && !ui.paused && ui.page=="home"{let _=crate::workspace::open_fresh(ui);}
    }
    island::request(open,reduce_motion(ui));
    if frame.expanded!=open || frame.animating {
        // The live workspace already occupies its expanded bounds on both legs
        // of a reversal. Keep its snapshot and move only the persistent header.
        let moving=island::frame().animating;
        if moving && !frame.animating && (!open || ui.pending_widget_note.is_some()){
            // Closing already has the expanded workspace laid out. For a pinned
            // note, its destination is prepared after the first moving frame.
            crate::mask_workspace_for_animation(ui.hwnd.0,true);
            if open{
                // The compact island is already on screen. Count time spent
                // taking its card away toward the first visible movement.
                island::first_frame_presented();
                side_widgets::sync(ui);
                island::sample();
            }
            ui.layout_motion_header();
            crate::present_frame(ui.hwnd.0);
        }else{render_frame(ui,!frame.animating || !moving);}
    }
    unsafe{
        if open{SetTimer(ui.hwnd.0,2,50,None);}else{KillTimer(ui.hwnd.0,2);}
        if island::needs_tick(){SetTimer(ui.hwnd.0,MOTION_TIMER,15,None);}else{KillTimer(ui.hwnd.0,MOTION_TIMER);}
        ShowWindow(ui.hwnd.0,SW_SHOWNOACTIVATE);
        if open && explicit{SetForegroundWindow(ui.hwnd.0);}
    }
}
pub fn expand(ui:&mut Ui,value:bool){change(ui,value,true);}
pub fn expand_from_side(ui:&mut Ui){
    expand(ui,true);
    if ui.expanded{INTERACTION.with(|i|i.borrow_mut().side_open_hold=true);}
}
pub fn model_tick(ui:&mut Ui){
    let widgets=widget_list(ui);
    if INTERACTION.with(|i|i.borrow().last_widgets!=widgets){position(ui);}
    else{side_widgets::sync(ui);}
}
pub fn step_height(ui:&mut Ui,dips:i32){
    if !ui.expanded{return}
    let (scale,limit)=island::scale_and_limit();
    let current=(island::frame().expanded_body.bottom as f32/scale).round() as i32;
    let cap=(limit as f32/scale).round() as i32;
    core::set(ui,"expandedHeight",&(current+dips).clamp(300,cap.max(300)).to_string());
    position(ui);if let Err(e)=ui.save(){ui.status(&e);}
}
pub fn resize_grip(x:i32,y:i32)->bool{
    let f=island::frame();let (scale,_)=island::scale_and_limit();
    if !f.expanded || f.animating{return false}
    let pad=(30.0*scale).round() as i32;
    let h=(12.0*scale).round() as i32;
    x>=f.body.left+pad && x<f.body.right-pad && y>=f.body.bottom-h && y<f.body.bottom
}
pub fn resize_down(ui:&Ui,x:i32,y:i32)->bool{
    if !resize_grip(x,y){return false}
    let mut at=POINT{x,y};unsafe{ClientToScreen(ui.hwnd.0,&mut at);SetCapture(ui.hwnd.0);}
    INTERACTION.with(|i|i.borrow_mut().drag=Some((at.y,island::frame().expanded_body.bottom)));
    true
}
pub fn resize_move(ui:&mut Ui,x:i32,y:i32)->bool{
    let Some((start,height))=INTERACTION.with(|i|i.borrow().drag) else{return false};
    let mut at=POINT{x,y};unsafe{ClientToScreen(ui.hwnd.0,&mut at);}
    let (scale,limit)=island::scale_and_limit();
    let new=((height+at.y-start).min(limit) as f32/scale).round() as i32;
    INTERACTION.with(|i|i.borrow_mut().resized_height=Some(new.max(300)));
    position(ui);true
}
pub fn resize_up(ui:&mut Ui){
    let height=INTERACTION.with(|i|{let mut i=i.borrow_mut();i.drag=None;i.resized_height.take()});
    if unsafe{GetCapture()}==ui.hwnd.0{unsafe{ReleaseCapture();}}
    if let Some(h)=height{core::set(ui,"expandedHeight",&h.to_string());if let Err(e)=ui.save(){ui.status(&e);}}
}
pub fn hide_temporarily(ui:&mut Ui){expand(ui,false);if ui.expanded{return}ui.hidden_until=Some(Instant::now()+Duration::from_secs(core::setting(ui,"hide","5").parse::<u64>().unwrap_or(5).clamp(1,600)));unsafe{ShowWindow(ui.hwnd.0,SW_HIDE);}side_widgets::sync(ui);}
pub fn autostart(ui:&Ui,enabled:bool)->Result<(),String>{
    if ui.isolated{return Err("Start at sign-in is disabled for isolated test profiles.".into())}
    unsafe{let mut key=null_mut();let name=wide("Buddy Work Rust");
        let result=RegCreateKeyExW(HKEY_CURRENT_USER,wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run").as_ptr(),0,null(),0,KEY_SET_VALUE,null(),&mut key,null_mut());
        if result!=0{return Err(format!("Could not open the startup settings ({result})."))}
        let val=std::env::current_exe().map_err(|e|e.to_string()).map(|p|wide(&format!("\"{}\"",p.display())));
        let res=match val{Ok(v) if enabled=>RegSetValueExW(key,name.as_ptr(),0,REG_SZ,v.as_ptr().cast(),(v.len()*2) as _),Ok(_)=>RegDeleteValueW(key,name.as_ptr()),Err(e)=>{RegCloseKey(key);return Err(e)}};
        RegCloseKey(key);if res==0 || (!enabled && res==ERROR_FILE_NOT_FOUND){Ok(())}else{Err(format!("Could not change startup settings ({res})."))}
    }
}
fn tray_data(ui:&Ui)->NOTIFYICONDATAW{let mut n=NOTIFYICONDATAW::default();n.cbSize=std::mem::size_of_val(&n) as u32;n.hWnd=ui.hwnd.0;n.uID=1;n}
pub fn tray(ui:&Ui,create:bool){if ui.isolated{return}let mut n=tray_data(ui);unsafe{
    if create{n.uFlags=NIF_ICON|NIF_MESSAGE|NIF_TIP;n.uCallbackMessage=CALLBACK;
        n.hIcon=LoadIconW(GetModuleHandleW(null()),101usize as _);if n.hIcon.is_null(){n.hIcon=LoadIconW(null_mut(),IDI_APPLICATION);}
        let tip=wide("Buddy Work");n.szTip[..tip.len()].copy_from_slice(&tip);Shell_NotifyIconW(NIM_ADD,&n);}else{Shell_NotifyIconW(NIM_DELETE,&n);}
}}
pub fn balloon(ui:&Ui,title:&str,body:&str){if ui.isolated{return}let mut n=tray_data(ui);n.uFlags=NIF_INFO;n.dwInfoFlags=NIIF_INFO;
    for (slot,ch) in n.szInfoTitle.iter_mut().take(63).zip(title.encode_utf16()){*slot=ch;}for (slot,ch) in n.szInfo.iter_mut().take(255).zip(body.encode_utf16()){*slot=ch;}unsafe{Shell_NotifyIconW(NIM_MODIFY,&n);}}
pub fn menu(ui:&mut Ui){
    let mut point=POINT::default();
    unsafe { GetCursorPos(&mut point); }
    show_menu(ui,point);
}

pub fn context_menu(ui:&mut Ui, target:HWND, position:isize){
    let point=if position != -1 {
        POINT{x:position as i16 as i32,y:(position >> 16) as i16 as i32}
    }else{unsafe{
        let focus=GetFocus();
        let anchor=if IsChild(ui.hwnd.0,focus)!=0 {focus} else {target};
        let mut bounds=RECT::default();
        if IsChild(ui.hwnd.0,anchor)!=0 && IsWindowVisible(anchor)!=0 && GetWindowRect(anchor,&mut bounds)!=0 {
            POINT{x:bounds.left,y:bounds.bottom}
        }else{
            let body=island::frame().body;
            let header_height=(40.0*island::scale_and_limit().0).round() as i32;
            let mut point=POINT{x:(body.left+body.right)/2,y:body.bottom.min(header_height)};
            ClientToScreen(ui.hwnd.0,&mut point);
            point
        }
    }};
    show_menu(ui,point);
}

fn show_menu(ui:&mut Ui,mouse:POINT){unsafe{
    const FIRST_WORKSPACE: i32 = 100;
    let m=CreatePopupMenu();
    if m.is_null(){return}
    let item=|id, label:&str| { AppendMenuW(m,MF_STRING,id,wide(label).as_ptr()); };
    let divider=|| { AppendMenuW(m,MF_SEPARATOR,0,null()); };
    item(1,"Open Buddy");
    if !ui.paused && !ui.locked && !ui.suppressed {
        let pages = CreatePopupMenu();
        if !pages.is_null() {
            for (index, &(page, label)) in crate::ui::PAGES.iter().enumerate() {
                let checked = if ui.expanded && ui.page == page { MF_CHECKED } else { 0 };
                AppendMenuW(pages, MF_STRING | checked, FIRST_WORKSPACE as usize + index, wide(label).as_ptr());
            }
            if AppendMenuW(m, MF_POPUP, pages as usize, wide("Workspaces").as_ptr()) == 0 {
                DestroyMenu(pages);
            }
        }
    }
    item(2,"Settings");
    item(6,"Updates and diagnostics");
    divider();
    let pinned=INTERACTION.with(|i|i.borrow().pinned);
    AppendMenuW(m,MF_STRING|if pinned{MF_CHECKED}else{0},8,wide("Pin island open").as_ptr());
    item(4,"Hide temporarily");
    item(5,"Next display");

    let mut shortcuts=Vec::new();
    if core::due_title(ui).is_some(){
        shortcuts.extend([(30,"Open reminder"),(31,"Snooze reminder 5 minutes"),(32,"Dismiss reminder")]);
    }
    if core::setting(ui,"widgets","on")=="on" {
        if core::widget_enabled(ui,"pomWidget","pomEnd"){shortcuts.push((20,"Start / pause focus timer"));}
        if core::widget_enabled(ui,"watchWidget","watchStart"){
            shortcuts.extend([(21,"Start / pause stopwatch"),(22,"Record stopwatch lap")]);
        }
        if !core::setting(ui,"noteWidget","").is_empty(){shortcuts.push((23,"Open pinned note"));}
        if !crate::workspace::widget_text(ui).is_empty(){
            shortcuts.extend([(24,"Open pinned clipboard item"),(25,"Copy pinned clipboard item")]);
        }
    }
    if ui.call_state.is_some() && !ui.paused && !ui.suppressed && !ui.locked {
        shortcuts.extend([(10,"Freeze camera"),(11,"Resume camera"),(12,"Standby camera")]);
    }
    if !shortcuts.is_empty(){
        divider();
        for (id,label) in shortcuts { item(id,label); }
    }
    divider();
    item(3,if ui.paused{"Resume services"}else{"Pause services"});
    item(7,"Quit Buddy");
    SetForegroundWindow(ui.hwnd.0);
    let cmd=crate::chrome::menu::track(ui.hwnd.0,m,TPM_RETURNCMD|TPM_RIGHTBUTTON,mouse.x,mouse.y,ui.appearance);DestroyMenu(m);
    match cmd{1=>{ui.paused=false;expand(ui,true);},2=>{ui.paused=false;expand(ui,true);ui.navigate("settings");},3=>{ui.paused=!ui.paused;if ui.paused{devices::stop();crate::activity::stop();ShowWindow(ui.hwnd.0,SW_HIDE);}else{if !ui.isolated{devices::restore(&ui.profile);}ShowWindow(ui.hwnd.0,SW_SHOWNOACTIVATE);}side_widgets::sync(ui);core::tick(ui);},
        4=>hide_temporarily(ui),5=>{let _=core::action(ui,"settings","display");},6=>{expand(ui,true);ui.navigate("diagnostics")},7=>{ui.quitting=true;PostMessageW(ui.hwnd.0,WM_CLOSE,0,0);},8=>{let pin=INTERACTION.with(|i|{let mut i=i.borrow_mut();i.pinned=!i.pinned;i.pinned});if pin{expand(ui,true);}},10..=12 if !ui.paused && !ui.suppressed && !ui.locked=>{
            let key=match cmd{10=>"freeze",11=>"resume",_=>"standby"};
            let msg=devices::call_control(key).unwrap_or_else(|e|e);
            ui.status(&msg);
        },
        20..=22=>{let key=match cmd{20=>"pom",21=>"watch",_=>"lap"};match core::action(ui,"widgets",key){Ok(s)=>ui.status(&s),Err(s)=>ui.status(&s)}core::tick(ui);},
        23=>{let id=core::setting(ui,"noteWidget","").to_owned();expand(ui,true);ui.navigate("notes");if ui.page=="notes" && !ui.dirty_note{if let Some(i)=ui.note_ids.iter().position(|s|s==&id){ui.select_row(i);ui.run_action("select");}}},
        24=>{expand(ui,true);if let Err(e)=crate::workspace::open_widget(ui){ui.status(&e);}},
        25=>{if let Err(e)=crate::workspace::copy_widget(ui){ui.status(&e);balloon(ui,"Buddy clipboard",&e);}},
        30=>{if let Some(id)=ui.due_note.clone(){expand(ui,true);ui.navigate("notes");if ui.page=="notes"&&!ui.dirty_note{if let Some(i)=ui.note_ids.iter().position(|s|s==&id){ui.select_row(i);ui.run_action("select");}}}},
        31|32=>{if let Err(e)=core::acknowledge_reminder(ui,cmd==31){ui.status(&e);balloon(ui,"Buddy reminder",&e);}core::tick(ui);},
        id if id >= FIRST_WORKSPACE && !ui.paused && !ui.locked && !ui.suppressed => {
            if let Some(&(page, _)) = crate::ui::PAGES.get((id - FIRST_WORKSPACE) as usize) {
                expand(ui, true);
                ui.select_workspace(page);
            }
        }
        _=>{}}
}}
fn foreground_hidden(ui:&Ui)->bool{unsafe{
    let foreground=GetForegroundWindow();if foreground.is_null()||foreground==ui.hwnd.0{return false}
    let mut pid=0;GetWindowThreadProcessId(foreground,&mut pid);if pid==0||pid==GetCurrentProcessId(){return false}
    let mut executable=String::new();let process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);
    if !process.is_null(){let mut buf=vec![0u16;1024];let mut n=buf.len() as u32;if QueryFullProcessImageNameW(process,0,buf.as_mut_ptr(),&mut n)!=0{executable=String::from_utf16_lossy(&buf[..n as usize]).rsplit('\\').next().unwrap_or("").to_ascii_lowercase();}CloseHandle(process);}
    if core::setting(ui,"exclude","").split(',').map(str::trim).any(|s|!s.is_empty()&&s.eq_ignore_ascii_case(&executable)){return true}
    if core::setting(ui,"fullscreen","on")!="on"||matches!(executable.as_str(),"explorer.exe"|"chrome.exe"|"msedge.exe"|"firefox.exe"|"obs64.exe"|"vlc.exe"|"wmplayer.exe"){return false}
    let mut rc=RECT::default();if GetWindowRect(foreground,&mut rc)==0{return false}let mut info=MONITORINFO::default();info.cbSize=std::mem::size_of_val(&info) as u32;
    if GetMonitorInfoW(MonitorFromWindow(foreground,MONITOR_DEFAULTTONEAREST),&mut info)==0{return false}let r=info.rcMonitor;
    rc.left==r.left && rc.right==r.right && rc.top==r.top && rc.bottom==r.bottom
}}
fn focus_blocks(ui:&Ui)->bool{unsafe{
    if ui.dirty_note || ui.page=="rewrite" || ui.read_error.is_some() || GetCapture()==ui.hwnd.0{return true}
    let mut thread=GUITHREADINFO::default();thread.cbSize=std::mem::size_of_val(&thread) as u32;
    if GetGUIThreadInfo(0,&mut thread)!=0 &&
        (thread.flags&(GUI_INMENUMODE|GUI_POPUPMENUMODE|GUI_INMOVESIZE))!=0 && thread.hwndActive==ui.hwnd.0{return true}
    let foreground=GetForegroundWindow();
    // Windows can briefly report no foreground window during an activation
    // change. A private desktop also has no system foreground. Preserve an
    // actual keyboard focus on our active editor in either case.
    let active=if foreground.is_null(){GetActiveWindow()}else{foreground};
    if active.is_null(){return false}
    if active!=ui.hwnd.0 {
        // File/confirmation/date pickers and Studio/markup windows are owned by
        // Buddy, including windows on its dedicated editor thread.
        let mut pid=0;GetWindowThreadProcessId(active,&mut pid);
        return pid==GetCurrentProcessId();
    }
    let focused=GetFocus();
    if focused.is_null(){return false}
    let mut text=[0u16;64];let n=GetClassNameW(focused,text.as_mut_ptr(),64);
    let name=String::from_utf16_lossy(&text[..n.max(0) as usize]).to_ascii_lowercase();
    name=="edit" || name.starts_with("richedit") || name=="combobox" || name=="combolbox" || name=="sysdatetimepick32"
}}
impl Interaction {
    fn side_hold(&mut self,body:bool,settled_open:bool)->bool{
        // A side shortcut can move the animated header under a stationary
        // pointer. Only a fresh entry into the settled panel releases its hold.
        if body && !self.inside_body && settled_open{self.side_open_hold=false;}
        self.side_open_hold
    }
    fn pointer(&mut self,now:Instant,expanded:bool,body:bool,inside:bool,blocked:bool)->(bool,bool){
        let entered=body && !self.inside_body;
        if inside{self.pointer_seen=true;self.left_at=None;}
        else if self.inside_union && expanded{self.left_at=Some(now);}
        if !inside && expanded && self.pointer_seen && !blocked && !self.pinned && self.left_at.is_none(){self.left_at=Some(now);}
        if blocked || self.pinned{self.left_at=None;}
        let open=entered && !expanded && self.hover_blocked_until.is_none_or(|d|now>=d);
        let close=!inside && !self.pinned && !blocked && self.left_at.is_some_and(|t|now.duration_since(t)>=GRACE);
        self.inside_body=body;self.inside_union=inside;
        if close{self.left_at=None;self.pointer_seen=false;}
        (open,close)
    }
}
pub fn pointer_at(ui:&mut Ui,body:bool,inside:bool){
    if ui.locked || ui.suppressed || ui.paused || ui.hidden_until.is_some(){return}
    let now=Instant::now();
    let held=INTERACTION.with(|state|state.borrow_mut().side_hold(body,ui.expanded && !island::frame().animating));
    let blocked=held || focus_blocks(ui);
    let (open,close)=INTERACTION.with(|i|i.borrow_mut().pointer(now,ui.expanded,body,inside,blocked));
    if open && core::setting(ui,"open","hover")=="hover"{change(ui,true,false);}
    else if close && ui.expanded{change(ui,false,false);}
}
pub fn deactivated(ui:&Ui,next:HWND){
    if !ui.expanded || ui.isolated{return}
    let mut pid=0;
    if !next.is_null(){unsafe{GetWindowThreadProcessId(next,&mut pid);}}
    if pid!=unsafe{GetCurrentProcessId()}{
        INTERACTION.with(|i|{let mut i=i.borrow_mut();i.pointer_seen=true;i.left_at=Some(Instant::now());});
    }
}
pub fn visibility(ui:&mut Ui,check_foreground:bool){
    if ui.locked{return}
    if ui.isolated {
        // Exercise private message-driven hover without reading the user's cursor.
        let (body,union)=INTERACTION.with(|i|{let i=i.borrow();(i.inside_body,i.inside_union)});
        pointer_at(ui,body,union);
        return
    }
    if let Some(t)=ui.hidden_until{
        if t>Instant::now(){return}
        ui.hidden_until=None;unsafe{ShowWindow(ui.hwnd.0,SW_SHOWNOACTIVATE);}
    }
    if check_foreground {
        let hide=foreground_hidden(ui);
        if hide!=ui.suppressed{
            if hide{devices::leave(ui.page);}
            ui.suppressed=hide;unsafe{ShowWindow(ui.hwnd.0,if hide{SW_HIDE}else{SW_SHOWNOACTIVATE});}
            side_widgets::sync(ui);
        }
    }
    if ui.suppressed||ui.paused{return}
    let mut screen=POINT::default();
    if unsafe{GetCursorPos(&mut screen)}==0{return}
    let mut local=screen;unsafe{ScreenToClient(ui.hwnd.0,&mut local);}
    let body=island::contains(&island::frame(),local.x,local.y);
    let union=body || side_widgets::contains_screen_point(screen.x,screen.y);
    pointer_at(ui,body,union);
}
pub fn copy_text(ui:&Ui,text:&str)->Result<(),String>{unsafe{
    let buf=wide(text);let mem=GlobalAlloc(GMEM_MOVEABLE,buf.len()*2);if mem.is_null(){return Err("Insufficient clipboard memory.".into())}let p=GlobalLock(mem);if p.is_null(){GlobalFree(mem);return Err("Could not prepare the clipboard.".into())}
    std::ptr::copy_nonoverlapping(buf.as_ptr(),p.cast(),buf.len());GlobalUnlock(mem);
    if OpenClipboard(ui.hwnd.0)==0{GlobalFree(mem);return Err("The clipboard is busy.".into())}let ok=EmptyClipboard()!=0&&!SetClipboardData(CF_UNICODETEXT as u32,mem).is_null();CloseClipboard();if !ok{GlobalFree(mem);return Err("Could not copy the note.".into())}Ok(())
}}
unsafe extern "system" fn dialog_proc(hwnd:HWND,msg:u32,wp:usize,lp:isize)->isize{
    let outcome=GetWindowLongPtrW(hwnd,GWLP_USERDATA) as *const std::cell::Cell<i32>;
    let selected=if msg==WM_CLOSE{IDCANCEL}else if msg==WM_COMMAND{(wp&0xffff) as i32}else{0};
    if !outcome.is_null() && (selected==IDOK || selected==IDCANCEL){(*outcome).set(selected);return 0}
    DefWindowProcW(hwnd,msg,wp,lp)
}
fn dialog<T>(ui:&Ui,title:&str,caption:&str,build:impl FnOnce(HWND)->HWND,accept:impl Fn(HWND)->Result<T,String>)->Option<T>{unsafe{
    let class=wide("BuddyWorkRustDialog");
    let descriptor=WNDCLASSW{lpfnWndProc:Some(dialog_proc),hInstance:GetModuleHandleW(null()),lpszClassName:class.as_ptr(),hCursor:LoadCursorW(null_mut(),IDC_ARROW),hbrBackground:GetSysColorBrush(COLOR_BTNFACE),..Default::default()};
    RegisterClassW(&descriptor);
    let mut parent=RECT::default();GetWindowRect(ui.hwnd.0,&mut parent);
    let popup=CreateWindowExW(WS_EX_DLGMODALFRAME,class.as_ptr(),wide(title).as_ptr(),WS_POPUP|WS_CAPTION|WS_SYSMENU,
        parent.left+35,parent.top+70,400,156,ui.hwnd.0,null_mut(),descriptor.hInstance,null());if popup.is_null(){return None}
    let outcome=std::cell::Cell::new(0i32);
    SetWindowLongPtrW(popup,GWLP_USERDATA,&outcome as *const _ as isize);
    let child=|class:&str,name:&str,id:usize,x,y,w,h,flags|CreateWindowExW(0,wide(class).as_ptr(),wide(name).as_ptr(),WS_CHILD|WS_VISIBLE|flags,x,y,w,h,popup,id as _,null_mut(),null());
    let label=child("STATIC",caption,0,12,9,360,22,0);let control=build(popup);
    let ok=child("BUTTON","OK",IDOK as _,192,75,80,29,WS_TABSTOP|BS_DEFPUSHBUTTON as u32);let cancel=child("BUTTON","Cancel",IDCANCEL as _,280,75,80,29,WS_TABSTOP);
    let font=GetStockObject(DEFAULT_GUI_FONT);for h in [label,control,ok,cancel]{SendMessageW(h,WM_SETFONT,font as _,1);}
    EnableWindow(ui.hwnd.0,0);ShowWindow(popup,SW_SHOW);SetFocus(control);
    let mut value=None;let mut msg=MSG::default();
    while IsWindow(popup)!=0{
        match outcome.replace(0){
            IDOK=>match accept(control){Ok(v)=>{value=Some(v);break},Err(reason)=>{MessageBoxW(popup,wide(&reason).as_ptr(),wide(title).as_ptr(),MB_ICONINFORMATION);}},
            IDCANCEL=>break,_=>{}
        }
        let status=GetMessageW(&mut msg,null_mut(),0,0);if status<=0{if status==0{PostQuitMessage(msg.wParam as i32)}break}
        if IsDialogMessageW(popup,&msg)==0{TranslateMessage(&msg);DispatchMessageW(&msg);}
    }
    EnableWindow(ui.hwnd.0,1);
    if IsWindow(popup)!=0{SetWindowLongPtrW(popup,GWLP_USERDATA,0);DestroyWindow(popup);}
    SetForegroundWindow(ui.hwnd.0);value
}}
pub fn prompt(ui:&Ui,title:&str,caption:&str,initial:&str)->Option<String>{
    dialog(ui,title,caption,|owner|unsafe{
        let edit=CreateWindowExW(WS_EX_CLIENTEDGE,wide("EDIT").as_ptr(),wide(initial).as_ptr(),WS_CHILD|WS_VISIBLE|WS_TABSTOP|ES_AUTOHSCROLL as u32,12,36,348,25,owner,10usize as _,null_mut(),null());
        crate::chrome::edit_menu::attach(edit);
        edit
    },
        |edit|Ok(crate::ui::window_text(edit,32768)))
}
pub fn choice(ui:&Ui,title:&str,caption:&str,items:&[String])->Option<usize>{
    dialog(ui,title,caption,|owner|unsafe{
        let choices=CreateWindowExW(0,wide("COMBOBOX").as_ptr(),null(),WS_CHILD|WS_VISIBLE|WS_TABSTOP|WS_VSCROLL|CBS_DROPDOWNLIST as u32,12,36,348,200,owner,10usize as _,null_mut(),null());
        crate::chrome::scroll::attach(choices,WS_VSCROLL,true);
        for item in items{SendMessageW(choices,CB_ADDSTRING,0,wide(item).as_ptr() as isize);}
        SendMessageW(choices,CB_SETCURSEL,0,0);choices
    },|choices|{
        let index=unsafe{SendMessageW(choices,CB_GETCURSEL,0,0)};
        usize::try_from(index).ok().filter(|n|*n<items.len()).ok_or_else(||"Select an attached image.".into())
    })
}
pub fn schedule_reminder(ui:&Ui)->Option<u64>{
    use windows_sys::Win32::{UI::Controls::*,System::Time::*};
    dialog(ui,"Schedule reminder","Date and time on this computer",|owner|unsafe{
        let init=INITCOMMONCONTROLSEX{dwSize:std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,dwICC:ICC_DATE_CLASSES};
        InitCommonControlsEx(&init);
        let picker=CreateWindowExW(WS_EX_CLIENTEDGE,DATETIMEPICK_CLASSW,null(),WS_CHILD|WS_VISIBLE|WS_TABSTOP,12,36,348,28,owner,10usize as _,null_mut(),null());
        SendMessageW(picker,DTM_SETFORMATW,0,wide("yyyy-MM-dd  HH:mm").as_ptr() as isize);
        picker
    },|picker|unsafe{
        let mut local=SYSTEMTIME::default();let mut universal=SYSTEMTIME::default();let mut file=FILETIME::default();
        if SendMessageW(picker,DTM_GETSYSTEMTIME,0,&mut local as *mut _ as isize)!=GDT_VALID as isize ||
            TzSpecificLocalTimeToSystemTime(null(),&local,&mut universal)==0 || SystemTimeToFileTime(&universal,&mut file)==0{return Err("Choose a valid local date and time.".into())}
        let ticks=(u64::from(file.dwHighDateTime)<<32)|u64::from(file.dwLowDateTime);
        let millis=(ticks/10000).saturating_sub(11644473600000);
        if millis<=core::now(){Err("Choose a date and time in the future.".into())}else{Ok(millis)}
    })
}

#[cfg(test)]
mod pointer_tests {
    use super::*;
    #[test]
    fn side_cards_hold_the_surface_without_opening_it_and_edits_pause_the_leave_timer(){
        let now=Instant::now();
        let ms=|n|now+Duration::from_millis(n);
        let mut p=Interaction::default();
        assert_eq!(p.pointer(ms(0),false,false,true,false),(false,false));
        assert_eq!(p.pointer(ms(1),false,true,true,false),(true,false));
        assert_eq!(p.pointer(ms(10),true,false,false,false),(false,false));
        assert_eq!(p.pointer(ms(309),true,false,false,false),(false,false));
        assert_eq!(p.pointer(ms(310),true,false,false,false),(false,true));
        p.pointer(ms(330),true,true,true,false);
        p.pointer(ms(340),true,false,false,true);
        assert_eq!(p.pointer(ms(1000),true,false,false,true),(false,false));
        p.pointer(ms(1100),true,false,false,false);
        assert_eq!(p.pointer(ms(1399),true,false,false,false),(false,false));
        assert_eq!(p.pointer(ms(1400),true,false,false,false),(false,true));
    }
    #[test]
    fn shortcut_open_holds_until_a_real_entry_into_the_settled_workspace(){
        let now=Instant::now();
        let mut p=Interaction{side_open_hold:true,..Interaction::default()};
        assert!(p.side_hold(true,false));
        p.pointer(now,true,true,true,true);
        assert!(p.side_hold(true,true));
        p.pointer(now,true,false,false,true);
        assert!(!p.side_hold(true,true));
        p.pointer(now,true,true,true,false);
        p.pointer(now,true,false,false,false);
        assert_eq!(p.pointer(now+GRACE,true,false,false,false),(false,true));
    }
    #[test]
    fn pin_and_explicit_collapse_do_not_trap_focus_or_immediately_reopen(){
        let now=Instant::now();
        let mut p=Interaction::default();
        p.pinned=true;
        p.pointer(now,true,true,true,false);
        p.pointer(now,true,false,false,false);
        assert_eq!(p.pointer(now+Duration::from_secs(1),true,false,false,false),(false,false));
        p.pinned=false;p.hover_blocked_until=Some(now+REOPEN_DELAY);
        assert_eq!(p.pointer(now+Duration::from_secs(1),false,true,true,false),(false,false));
        p.pointer(now+REOPEN_DELAY,false,false,false,false);
        assert_eq!(p.pointer(now+REOPEN_DELAY,false,true,true,false),(true,false));
    }
}
