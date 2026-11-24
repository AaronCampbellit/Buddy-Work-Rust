//! Paint a native popup menu while Windows continues to own its navigation,
//! submenu placement, focus, scrolling, cancellation and accessibility tree.
use super::{Colors, divider, fill, inset, mix, panel};
use crate::ui::wide;
use std::{cell::{Cell,RefCell}, mem::size_of, ptr::{null,null_mut}};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, System::Threading::GetCurrentThreadId,
    UI::{Accessibility::{HIGHCONTRASTW,HCF_HIGHCONTRASTON,MSAAMENUINFO,MSAA_MENU_SIG},
        Controls::{DRAWITEMSTRUCT,MEASUREITEMSTRUCT,ODT_MENU,ODS_CHECKED,ODS_DISABLED,ODS_FOCUS,ODS_GRAYED,ODS_NOACCEL,ODS_SELECTED},
        HiDpi::*, Shell::{DefSubclassProc,RemoveWindowSubclass,SetWindowSubclass}, WindowsAndMessaging::*}};

thread_local! {
    static PALETTE:Cell<Colors>=const{Cell::new((0xfbf9f7,0xf4f1ed,0x272017,0x9c7f00))};
    static ACTIVE:Cell<*const Presentation>=const{Cell::new(null())};
}
pub(crate) fn colors()->Colors{PALETTE.with(Cell::get)}
pub(crate) fn set_colors(p:Colors){PALETTE.with(|v|v.set(p));}

#[derive(Clone,Copy)]
struct Paint { surface:u32,ink:u32,muted:u32,selected:u32,selected_ink:u32,accent:u32,edge:u32 }
impl Paint {
    unsafe fn new(colors:Colors)->Self {
        let mut hc=HIGHCONTRASTW::default();hc.cbSize=size_of::<HIGHCONTRASTW>() as u32;
        if SystemParametersInfoW(SPI_GETHIGHCONTRAST,hc.cbSize,(&mut hc as *mut HIGHCONTRASTW).cast(),0)!=0 && hc.dwFlags&HCF_HIGHCONTRASTON!=0 {
            return Self::contrast(|index|GetSysColor(index));
        }
        let (bg,field,ink,accent)=colors;
        let surface=mix(bg,field,96);
        Self{surface,ink,muted:mix(surface,ink,130),selected:mix(surface,accent,36),selected_ink:ink,accent,edge:divider(colors)}
    }
    fn contrast(color:impl Fn(SYS_COLOR_INDEX)->u32)->Self{
        Self{surface:color(COLOR_MENU),ink:color(COLOR_MENUTEXT),muted:color(COLOR_GRAYTEXT),
            selected:color(COLOR_HIGHLIGHT),selected_ink:color(COLOR_HIGHLIGHTTEXT),
            accent:color(COLOR_MENUTEXT),edge:color(COLOR_WINDOWTEXT)}
    }
}

// MSAA requires this prefix at the start of owner-drawn menu item data. USER
// keeps the ordinary menu roles, states, keyboard events and submenu hierarchy.
#[repr(C)]
struct Entry {
    msaa:MSAAMENUINFO,
    name:Vec<u16>,
    caption:String,
    shortcut:String,
    key:Option<char>,
    menu:HMENU,
    index:u32,
    original_kind:u32,
    original_data:usize,
    submenu:bool,
    width:u32,
    height:u32,
}
fn label_parts(s:&str)->(String,Option<char>) {
    let mut out=String::with_capacity(s.len());let mut key=None;
    let mut chars=s.chars().peekable();
    while let Some(c)=chars.next(){
        if c!='&'{out.push(c);continue}
        if chars.peek()==Some(&'&'){chars.next();out.push('&');}
        else if let Some(&next)=chars.peek(){if key.is_none(){key=Some(next);}}
    }
    (out,key)
}

struct MenuRecord { menu:HMENU, previous:MENUINFO }
struct Presentation {
    paint:Paint,
    dpi:u32,
    width_limit:i32,
    font:HFONT,
    dc:HDC,
    previous_font:HGDIOBJ,
    row_height:u32,
    brush:HBRUSH,
    menus:RefCell<Vec<MenuRecord>>,
    entries:RefCell<Vec<Box<Entry>>>,
    windows:RefCell<Vec<HWND>>,
}
impl Presentation{
    unsafe fn new(owner:HWND,point:POINT,colors:Colors)->Self{
        let monitor=MonitorFromPoint(point,MONITOR_DEFAULTTONEAREST);
        let mut mon=MONITORINFO::default();mon.cbSize=size_of::<MONITORINFO>() as _;
        let width_limit=if GetMonitorInfoW(monitor,&mut mon)!=0{mon.rcWork.right-mon.rcWork.left-16}else{800};
        let (mut dpi,mut y)=(96,96);
        if GetDpiForMonitor(monitor,MDT_EFFECTIVE_DPI,&mut dpi,&mut y)<0{dpi=GetDpiForWindow(owner).max(96);}
        let mut metrics=NONCLIENTMETRICSW::default();metrics.cbSize=size_of::<NONCLIENTMETRICSW>() as _;
        let font=if SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS,metrics.cbSize,(&mut metrics as *mut NONCLIENTMETRICSW).cast(),0,dpi)!=0{
            CreateFontIndirectW(&metrics.lfMenuFont)
        }else{null_mut()};
        let dc=CreateCompatibleDC(null_mut());
        let previous_font=SelectObject(dc,if font.is_null(){GetStockObject(DEFAULT_GUI_FONT)}else{font});
        let mut tm=TEXTMETRICW::default();GetTextMetricsW(dc,&mut tm);
        let scale=|v:i32|((v*dpi as i32+48)/96).max(1);
        let row_height=scale(31).max(tm.tmHeight+scale(10)) as u32;
        let paint=Paint::new(colors);
        Self{paint,dpi,width_limit,font,dc,previous_font,row_height,brush:CreateSolidBrush(paint.surface),
            menus:RefCell::new(Vec::new()),entries:RefCell::new(Vec::new()),windows:RefCell::new(Vec::new())}
    }
    fn px(&self,v:i32)->i32{((v*self.dpi as i32+48)/96).max(1)}
    unsafe fn text_width(&self,s:&str)->i32{
        let s=wide(s);let mut sz=SIZE::default();GetTextExtentPoint32W(self.dc,s.as_ptr(),(s.len()-1) as _,&mut sz);sz.cx
    }
    unsafe fn prepare(&self,menu:HMENU){
        if menu.is_null() || IsMenu(menu)==0{return}
        if !self.menus.borrow().iter().any(|r|r.menu==menu) {
            let mut info=MENUINFO{cbSize:size_of::<MENUINFO>() as _,fMask:MIM_BACKGROUND|MIM_STYLE,..Default::default()};
            if GetMenuInfo(menu,&mut info)==0{return}
            let updated=MENUINFO{hbrBack:self.brush,dwStyle:info.dwStyle|MNS_NOCHECK,..info};
            if SetMenuInfo(menu,&updated)==0{return}
            self.menus.borrow_mut().push(MenuRecord{menu,previous:info});
        }
        for pos in 0..GetMenuItemCount(menu).max(0) as u32{
            let mut item=MENUITEMINFOW{cbSize:size_of::<MENUITEMINFOW>() as _,fMask:MIIM_FTYPE|MIIM_DATA|MIIM_SUBMENU|MIIM_STRING,..Default::default()};
            if GetMenuItemInfoW(menu,pos,1,&mut item)==0{continue}
            if !item.hSubMenu.is_null(){self.prepare(item.hSubMenu);}
            if item.fType&(MFT_OWNERDRAW|MFT_BITMAP)!=0{continue}
            let mut caption=vec![0u16;item.cch.min(32_768) as usize+1];
            item.dwTypeData=caption.as_mut_ptr();item.cch=caption.len() as _;
            if GetMenuItemInfoW(menu,pos,1,&mut item)==0{continue}
            let raw=String::from_utf16_lossy(&caption[..(item.cch as usize).min(caption.len().saturating_sub(1))]);
            let (label,shortcut)=raw.split_once('\t').unwrap_or((&raw,""));
            let (plain,key)=label_parts(label);
            let separator=item.fType&MFT_SEPARATOR!=0;
            let width=(self.px(58)+self.text_width(&plain)+if shortcut.is_empty(){0}else{self.px(24)+self.text_width(shortcut)})
                .max(self.px(150)).min(self.width_limit.max(self.px(80))) as u32;
            let mut name=wide(&plain);
            let msaa=MSAAMENUINFO{dwMSAASignature:MSAA_MENU_SIG as u32,cchWText:(name.len()-1) as _,pszWText:name.as_mut_ptr()};
            let mut entry=Box::new(Entry{msaa,name,caption:label.to_owned(),shortcut:shortcut.to_owned(),key,menu,index:pos,original_kind:item.fType,
                original_data:item.dwItemData,submenu:!item.hSubMenu.is_null(),width,height:if separator{self.px(9) as u32}else{self.row_height}});
            let info=MENUITEMINFOW{cbSize:size_of::<MENUITEMINFOW>() as _,fMask:MIIM_FTYPE|MIIM_DATA,fType:item.fType|MFT_OWNERDRAW,
                dwItemData:entry.as_mut() as *mut Entry as usize,..Default::default()};
            if SetMenuItemInfoW(menu,pos,1,&info)!=0{self.entries.borrow_mut().push(entry);}
        }
    }
    unsafe fn with_entry(&self,data:usize,f:impl FnOnce(&Entry))->bool{
        let entries=self.entries.borrow();
        let Some(entry)=entries.iter().find(|e|e.as_ref() as *const Entry as usize==data) else{return false};
        f(entry);true
    }
    unsafe fn measure(&self,m:&mut MEASUREITEMSTRUCT)->bool {
        m.CtlType==ODT_MENU && self.with_entry(m.itemData,|e|{m.itemWidth=e.width;m.itemHeight=e.height;})
    }
    unsafe fn draw(&self,d:&DRAWITEMSTRUCT)->bool{
        if d.CtlType!=ODT_MENU{return false}
        self.with_entry(d.itemData,|e|{
            let dc=d.hDC;let r=d.rcItem;let p=self.paint;let state=SaveDC(dc);
            fill(dc,r,p.surface);
            if e.original_kind&MFT_SEPARATOR!=0{
                let y=(r.top+r.bottom)/2;fill(dc,RECT{left:r.left+self.px(10),right:r.right-self.px(10),top:y,bottom:y+self.px(1)},p.edge);
            }else{
                let disabled=d.itemState&(ODS_GRAYED|ODS_DISABLED)!=0;
                let selected=d.itemState&(ODS_SELECTED|ODS_FOCUS)!=0;
                if selected{panel(dc,inset(r,self.px(2)),self.px(6),p.selected,p.selected);}
                let fg=if disabled{p.muted}else if selected{p.selected_ink}else{p.ink};
                let cy=(r.top+r.bottom)/2;
                if d.itemState&ODS_CHECKED!=0{self.stroke(dc,r.left+self.px(15),cy,&[(-4,0),(-1,3),(5,-4)],if selected{fg}else{p.accent});}
                if e.submenu{self.stroke(dc,r.right-self.px(14),cy,&[(-2,-4),(2,0),(-2,4)],fg);}
                SelectObject(dc,if self.font.is_null(){GetStockObject(DEFAULT_GUI_FONT)}else{self.font});
                SetBkMode(dc,TRANSPARENT as i32);SetTextColor(dc,fg);
                let mut tr=RECT{left:r.left+self.px(31),right:r.right-self.px(27),..r};
                let mut flags=DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS;
                if d.itemState&ODS_NOACCEL!=0{flags|=DT_HIDEPREFIX;}
                if !e.shortcut.is_empty(){
                    let short=wide(&e.shortcut);let mut sr=tr;SetTextColor(dc,if selected{fg}else{p.muted});
                    DrawTextW(dc,short.as_ptr(),(short.len()-1) as _,&mut sr,DT_RIGHT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX);
                    tr.right-=self.text_width(&e.shortcut)+self.px(14);SetTextColor(dc,fg);
                }
                let label=wide(&e.caption);DrawTextW(dc,label.as_ptr(),(label.len()-1) as _,&mut tr,flags);
            }
            RestoreDC(dc,state);
        })
    }
    unsafe fn stroke(&self,dc:HDC,x:i32,y:i32,offsets:&[(i32,i32);3],color:u32){
        let pen=CreatePen(PS_SOLID,self.px(2),color);let old=SelectObject(dc,pen);
        let points=offsets.map(|(dx,dy)|POINT{x:x+dx*self.dpi as i32/96,y:y+dy*self.dpi as i32/96});
        Polyline(dc,points.as_ptr(),3);SelectObject(dc,old);DeleteObject(pen);
    }
    unsafe fn submenu_tail(&self,h:HWND,dc:HDC){
        // USER32 adds its own system-colored triangle after WM_DRAWITEM. Keep
        // the native submenu and its accessibility, replacing only that strip.
        let menu=SendMessageW(h,MN_GETHMENU,0,0) as HMENU;
        if menu.is_null(){return}
        let entries=self.entries.borrow();
        for e in entries.iter().filter(|e|e.menu==menu && e.submenu){
            let mut r=RECT::default();
            if GetMenuItemRect(null_mut(),menu,e.index,&mut r)==0{continue}
            let mut at=POINT::default();ClientToScreen(h,&mut at);
            OffsetRect(&mut r,-at.x,-at.y);
            let flags=GetMenuState(menu,e.index,MF_BYPOSITION);
            let selected=flags&MF_HILITE!=0;
            let mut cut=r;cut.left=(r.right-self.px(28)).max(r.left);
            let state=SaveDC(dc);
            IntersectClipRect(dc,cut.left,cut.top,cut.right,cut.bottom);
            fill(dc,cut,self.paint.surface);
            if selected{panel(dc,inset(r,self.px(2)),self.px(6),self.paint.selected,self.paint.selected);}
            let fg=if flags&(MF_GRAYED|MF_DISABLED)!=0{self.paint.muted}else if selected{self.paint.selected_ink}else{self.paint.ink};
            self.stroke(dc,r.right-self.px(14),(r.top+r.bottom)/2,&[(-2,-4),(2,0),(-2,4)],fg);
            RestoreDC(dc,state);
        }
    }
    unsafe fn mnemonic(&self,menu:HMENU,key:char)->Option<isize>{
        let same=|c:char|c.to_lowercase().eq(key.to_lowercase());
        let entries=self.entries.borrow();let mut explicit=Vec::new();let mut initial=Vec::new();let mut selected=None;
        for e in entries.iter().filter(|e|e.menu==menu && e.original_kind&MFT_SEPARATOR==0){
            let state=GetMenuState(menu,e.index,MF_BYPOSITION);
            if state&MF_HILITE!=0{selected=Some(e.index);}
            if state&(MF_DISABLED|MF_GRAYED)!=0{continue}
            if e.key.is_some_and(same){explicit.push(e.index);}
            else if String::from_utf16_lossy(&e.name[..e.name.len()-1]).chars().next().is_some_and(same){initial.push(e.index);}
        }
        let matches=if explicit.is_empty(){initial}else{explicit};
        let first=*matches.first()?;
        let next=selected.and_then(|s|matches.iter().copied().find(|n|*n>s)).unwrap_or(first);
        Some((((if matches.len()==1{MNC_EXECUTE}else{MNC_SELECT})<<16)|next) as isize)
    }
}
impl Drop for Presentation{
    fn drop(&mut self){unsafe{
        for w in self.windows.get_mut().drain(..){if IsWindow(w)!=0{RemoveWindowSubclass(w,Some(popup_proc),76);}}
        for e in self.entries.get_mut().iter(){
            if IsMenu(e.menu)==0{continue}
            let mut check=MENUITEMINFOW{cbSize:size_of::<MENUITEMINFOW>() as _,fMask:MIIM_DATA,..Default::default()};
            if GetMenuItemInfoW(e.menu,e.index,1,&mut check)!=0 && check.dwItemData==e.as_ref() as *const Entry as usize{
                check.fMask=MIIM_FTYPE|MIIM_DATA;check.dwItemData=e.original_data;check.fType=e.original_kind;
                SetMenuItemInfoW(e.menu,e.index,1,&check);
            }
        }
        for item in self.menus.get_mut().iter(){if IsMenu(item.menu)!=0{SetMenuInfo(item.menu,&item.previous);}}
        SelectObject(self.dc,self.previous_font);DeleteDC(self.dc);
        if !self.font.is_null(){DeleteObject(self.font);}if !self.brush.is_null(){DeleteObject(self.brush);}
    }}
}

unsafe extern "system" fn owner_proc(h:HWND,msg:u32,wp:usize,lp:isize,id:usize,data:usize)->isize{
    let style=&*(data as *const Presentation);
    match msg{
        WM_INITMENUPOPUP=>{
            let value=DefSubclassProc(h,msg,wp,lp);
            style.prepare(wp as HMENU);
            return value;
        }
        WM_MEASUREITEM if lp!=0 && style.measure(&mut *(lp as *mut MEASUREITEMSTRUCT))=>return 1,
        WM_DRAWITEM if lp!=0 && style.draw(&*(lp as *const DRAWITEMSTRUCT))=>{
            // Hover can redraw one row directly outside WM_PAINT.
            let d=&*(lp as *const DRAWITEMSTRUCT);
            style.with_entry(d.itemData,|e|if e.submenu{
                for &window in style.windows.borrow().iter(){PostMessageW(window,RETOUCH_SUBMENUS,0,0);}
            });
            return 1
        }
        WM_MENUCHAR=>{
            if let Some(value)=char::from_u32(wp as u16 as u32).and_then(|c|style.mnemonic(lp as HMENU,c)){return value;}
        }
        WM_NCDESTROY=>{RemoveWindowSubclass(h,Some(owner_proc),id);}
        _=>{}
    }
    DefSubclassProc(h,msg,wp,lp)
}

#[link(name="dwmapi")]
extern "system"{fn DwmSetWindowAttribute(hwnd:HWND,attr:u32,value:*const std::ffi::c_void,len:u32)->i32;}
unsafe fn rounded(hwnd:HWND,width:i32,height:i32,style:&Presentation){
    if width<=0 || height<=0{return}
    let radius=style.px(9);
    let mask=CreateRoundRectRgn(0,0,width+1,height+1,2*radius,2*radius);
    if !mask.is_null() && SetWindowRgn(hwnd,mask,0)==0{DeleteObject(mask);}
}
unsafe fn border(hwnd:HWND,dc:HDC,style:&Presentation){
    if dc.is_null(){return}
    let mut win=RECT::default();let mut client=RECT::default();let mut at=POINT::default();
    if GetWindowRect(hwnd,&mut win)==0 || GetClientRect(hwnd,&mut client)==0{return}
    ClientToScreen(hwnd,&mut at);
    let outer=RECT{left:0,top:0,right:win.right-win.left,bottom:win.bottom-win.top};
    let state=SaveDC(dc);
    let left=at.x-win.left;let top=at.y-win.top;
    ExcludeClipRect(dc,left,top,left+client.right,top+client.bottom);
    fill(dc,outer,style.paint.surface);
    panel(dc,outer,style.px(9),style.paint.surface,style.paint.edge);
    RestoreDC(dc,state);
}
// The standard EDIT control tracks its stock menu with TPM_NONOTIFY. On
// Windows 11 the native menu receives this sizing message after it acquires
// its HMENU and before it computes item metrics or paints its first frame.
const MENU_SIZE_WINDOW:u32=0x1e2;
const RETOUCH_SUBMENUS:u32=WM_APP+0x14a;
unsafe extern "system" fn popup_proc(h:HWND,msg:u32,wp:usize,lp:isize,id:usize,data:usize)->isize{
    let style=&*(data as *const Presentation);
    if matches!(msg,MENU_SIZE_WINDOW|WM_WINDOWPOSCHANGING) {
        let menu=SendMessageW(h,MN_GETHMENU,0,0) as HMENU;
        if !menu.is_null() && !style.menus.borrow().iter().any(|r|r.menu==menu){style.prepare(menu);}
    }
    match msg{
        WM_WINDOWPOSCHANGING if lp!=0=>{
            let pos=&*(lp as *const WINDOWPOS);
            if pos.flags&SWP_NOSIZE==0{
                // User32 sends WINDOWPOSCHANGING during SetWindowRgn itself.
                // Compare the outer bounds before replacing an existing region.
                let tag=GetPropW(h,wide("Buddy.Menu.Size").as_ptr()) as usize;
                let size=(pos.cx as usize&0xffff)|((pos.cy as usize&0xffff)<<16);
                if size!=0 && size!=tag{
                    SetPropW(h,wide("Buddy.Menu.Size").as_ptr(),size as _);rounded(h,pos.cx,pos.cy,style);
                }
            }
        }
        WM_NCPAINT=>{
            let dc=GetWindowDC(h);border(h,dc,style);ReleaseDC(h,dc);return 0;
        }
        WM_ERASEBKGND if wp!=0=>{
            let mut r=RECT::default();GetClientRect(h,&mut r);fill(wp as HDC,r,style.paint.surface);return 1;
        }
        WM_PAINT|RETOUCH_SUBMENUS=>{
            let result=if msg==WM_PAINT{DefSubclassProc(h,msg,wp,lp)}else{0};
            let dc=GetDC(h);if !dc.is_null(){style.submenu_tail(h,dc);ReleaseDC(h,dc);}return result;
        }
        WM_PRINT|WM_PRINTCLIENT if wp!=0=>{
            let result=DefSubclassProc(h,msg,wp,lp);
            let dc=wp as HDC;let state=SaveDC(dc);
            if msg==WM_PRINT && lp as i32&PRF_NONCLIENT!=0{
                border(h,dc,style);
                let mut at=POINT::default();let mut win=RECT::default();ClientToScreen(h,&mut at);GetWindowRect(h,&mut win);
                OffsetViewportOrgEx(dc,at.x-win.left,at.y-win.top,null_mut());
            }
            style.submenu_tail(h,dc);RestoreDC(dc,state);return result;
        }
        WM_NCDESTROY=>{
            RemovePropW(h,wide("Buddy.Menu.Size").as_ptr());
            if let Ok(mut windows)=style.windows.try_borrow_mut(){windows.retain(|&window|window!=h);}
            RemoveWindowSubclass(h,Some(popup_proc),id);
        }
        _=>{}
    }
    DefSubclassProc(h,msg,wp,lp)
}
unsafe fn observe_popup(h:HWND,style:&Presentation){
    if style.windows.borrow().contains(&h){return}
    let mut class=[0u16;16];let n=GetClassNameW(h,class.as_mut_ptr(),class.len() as i32);
    if n!=6 || &class[..6]!=[35,51,50,55,54,56]{return} // Standard Win32 popup menu, "#32768".
    if SetWindowSubclass(h,Some(popup_proc),76,style as *const _ as usize)==0{return}
    style.windows.borrow_mut().push(h);
    let corner=3u32; // DWMWCP_ROUNDSMALL
    let dark=u32::from((style.paint.surface&255)+((style.paint.surface>>8)&255)+((style.paint.surface>>16)&255)<384);
    for (attribute,value) in [(33,corner),(20,dark),(34,style.paint.edge)]{
        DwmSetWindowAttribute(h,attribute,(&value as *const u32).cast(),4);
    }
}
unsafe extern "system" fn thread_message(code:i32,wp:usize,lp:isize)->isize{
    if code>=0 && lp!=0{
        let msg=&*(lp as *const CWPSTRUCT);
        if matches!(msg.message,WM_NCCREATE|MENU_SIZE_WINDOW|WM_WINDOWPOSCHANGING){
            ACTIVE.with(|current|{
                if let Some(style)=current.get().as_ref(){observe_popup(msg.hwnd,style);}
            });
        }
    }
    CallNextHookEx(null_mut(),code,wp,lp)
}

unsafe fn run<T>(owner:HWND,point:POINT,colors:Colors,menu:HMENU,action:impl FnOnce()->T)->T{
    // These hooks live only for the synchronous menu loop on this UI thread.
    // The same edit control can open Windows' stock menu through scope().
    let presentation=Presentation::new(owner,point,colors);
    let address=&presentation as *const Presentation;
    let id=address as usize;
    if SetWindowSubclass(owner,Some(owner_proc),id,id)==0{return action()}
    if !menu.is_null(){presentation.prepare(menu);}
    let prior=ACTIVE.with(|current|current.replace(address));
    let hook=SetWindowsHookExW(WH_CALLWNDPROC,Some(thread_message),null_mut(),GetCurrentThreadId());
    struct Cleanup{owner:HWND,id:usize,hook:HHOOK,prior:*const Presentation}
    impl Drop for Cleanup{
        fn drop(&mut self){unsafe{
            if !self.hook.is_null(){UnhookWindowsHookEx(self.hook);}
            ACTIVE.with(|v|v.set(self.prior));
            if IsWindow(self.owner)!=0{RemoveWindowSubclass(self.owner,Some(owner_proc),self.id);}
        }}
    }
    let _guard=Cleanup{owner,id,hook,prior};
    action()
}
pub(crate) unsafe fn scope<T>(owner:HWND,colors:Colors,action:impl FnOnce()->T)->T{
    // Standard EDIT opens by pointer or keyboard on the edit's monitor.
    let mut r=RECT::default();GetWindowRect(owner,&mut r);
    let point=POINT{x:r.left+(r.right-r.left)/2,y:r.top+(r.bottom-r.top)/2};
    run(owner,point,colors,null_mut(),action)
}
pub(crate) unsafe fn track(owner:HWND,menu:HMENU,flags:u32,x:i32,y:i32,colors:Colors)->i32{
    run(owner,POINT{x,y},colors,menu,||TrackPopupMenu(menu,flags,x,y,0,owner,null()))
}
#[cfg(test)]
mod tests;
