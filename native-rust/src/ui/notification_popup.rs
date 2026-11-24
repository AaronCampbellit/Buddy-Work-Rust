// An owned popup can extend below the collapsed island without changing its size.
use super::*;
use std::{cell::Cell,ptr::{null,null_mut}};
use windows_sys::Win32::{Foundation::RECT,System::LibraryLoader::GetModuleHandleW,UI::Input::KeyboardAndMouse::*};

struct Popup{rows:Vec<String>,offset:usize,colors:chrome::Colors,fonts:Vec<HFONT>,scale:f32}
impl Drop for Popup{fn drop(&mut self){for font in self.fonts.drain(..){unsafe{DeleteObject(font);}}}}
impl Popup{
    fn p(&self,x:i32)->i32{(x as f32*self.scale).round() as i32}
    fn paint(&self,dc:HDC,r:RECT){
        let (bg,_,ink,accent)=self.colors;let muted=chrome::mix(bg,ink,170);let p=|n|self.p(n);
        chrome::panel(dc,r,p(10),bg,chrome::mix(bg,ink,51));
        let t=|x,y,w,h,s:&str,font,color,flags|chrome::text(dc,chrome::rect(p(x),p(y),p(w),p(h)),s,self.fonts[font],color,flags|DT_VCENTER|DT_SINGLELINE);
        let w=(r.right as f32/self.scale).round()as i32;
        t(12,0,w-65,34,"Recent notifications",1,ink,DT_LEFT);
        if !self.rows.is_empty(){t(w-55,0,46,34,"Clear",8,muted,DT_CENTER);}
        chrome::fill(dc,chrome::rect(1,p(34),r.right-2,p(1)),chrome::mix(bg,ink,38));
        if self.rows.is_empty(){t(8,35,w-16,57,"No recent notifications",3,muted,DT_CENTER);return}
        for(i,row)in self.rows.iter().skip(self.offset).take(4).enumerate(){
            let y=35+i as i32*48;let (name,detail)=row.split_once(" · ").unwrap_or((row,""));
            chrome::icon(dc,p(10),p(y+16),p(15),chrome::Icon::Bell,accent,bg);
            t(35,y+5,w-44,16,name,8,muted,DT_LEFT);t(35,y+19,w-45,20,detail,1,ink,DT_LEFT);
            if i>0{chrome::fill(dc,chrome::rect(p(8),p(y),r.right-p(16),p(1)),chrome::mix(bg,ink,26));}
        }
    }
}
thread_local!{static ACTIVE:Cell<Handle>=const{Cell::new(null_mut())};}
unsafe extern "system" fn proc(hwnd:Handle,msg:u32,wp:usize,lp:isize)->isize{
    let ptr=GetWindowLongPtrW(hwnd,GWLP_USERDATA)as *mut Popup;if ptr.is_null(){return DefWindowProcW(hwnd,msg,wp,lp)}
    match msg{
        WM_MOUSEACTIVATE=>MA_ACTIVATE as _,
        WM_ACTIVATE if wp as u16 as u32==WA_INACTIVE=>{PostMessageW(hwnd,WM_CLOSE,0,0);0}
        WM_DISPLAYCHANGE|WM_DPICHANGED|WM_CLOSE=>{DestroyWindow(hwnd);0}
        WM_KEYDOWN if wp==VK_ESCAPE as usize=>{DestroyWindow(hwnd);0}
        WM_MOUSEWHEEL|WM_KEYDOWN if msg==WM_MOUSEWHEEL||matches!(wp as u16,VK_UP|VK_DOWN|VK_PRIOR|VK_NEXT|VK_HOME|VK_END)=>{
            let s=&mut*ptr;let max=s.rows.len().saturating_sub(4);
            let delta=if msg==WM_MOUSEWHEEL{-((wp>>16)as u16 as i16 as isize)/120}else{match wp as u16{VK_UP=>-1,VK_DOWN=>1,VK_PRIOR=>-4,VK_NEXT=>4,VK_HOME=>-(max as isize),_=>max as isize}};
            s.offset=s.offset.saturating_add_signed(delta).min(max);InvalidateRect(hwnd,null(),0);0
        }
        WM_LBUTTONUP|WM_KEYDOWN if msg==WM_LBUTTONUP||wp==VK_RETURN as usize||wp==VK_SPACE as usize=>{
            let s=&mut*ptr;let mut r=RECT::default();GetClientRect(hwnd,&mut r);let x=lp as i16 as i32;let y=(lp>>16)as i16 as i32;
            if !s.rows.is_empty()&&(msg==WM_KEYDOWN||(y<s.p(34)&&x>r.right-s.p(58))){
                activity::clear_notice_history();s.rows.clear();s.offset=0;SetWindowPos(hwnd,null_mut(),0,0,r.right,s.p(94),SWP_NOMOVE|SWP_NOZORDER|SWP_NOACTIVATE);InvalidateRect(hwnd,null(),0);
            }0
        }
        WM_PAINT|WM_PRINTCLIENT=>{let mut ps=PAINTSTRUCT::default();let dc=if msg==WM_PAINT{BeginPaint(hwnd,&mut ps)}else{wp as _};let mut r=RECT::default();GetClientRect(hwnd,&mut r);(&*ptr).paint(dc,r);if msg==WM_PAINT{EndPaint(hwnd,&ps);}0}
        WM_ERASEBKGND=>1,
        WM_NCDESTROY=>{SetWindowLongPtrW(hwnd,GWLP_USERDATA,0);drop(Box::from_raw(ptr));ACTIVE.with(|a|if a.get()==hwnd{a.set(null_mut())});DefWindowProcW(hwnd,msg,wp,lp)}
        _=>DefWindowProcW(hwnd,msg,wp,lp),
    }
}
pub(super) fn toggle(ui:&Ui){unsafe{
    let existing=ACTIVE.with(|a|a.get());if !existing.is_null(){DestroyWindow(existing);return}
    let class=wide("BuddyRustNotificationCenter");let inst=GetModuleHandleW(null());let wc=WNDCLASSW{lpfnWndProc:Some(proc),hInstance:inst,hCursor:LoadCursorW(null_mut(),IDC_ARROW),lpszClassName:class.as_ptr(),..Default::default()};RegisterClassW(&wc);
    let rows=activity::notice_titles();let frame=crate::island::frame().body;let mut at=windows_sys::Win32::Foundation::POINT{x:frame.right,y:frame.top};ClientToScreen(ui.hwnd.0,&mut at);
    let w=(frame.right-frame.left-ui.px(60)).min(ui.px(340)).max(ui.px(120));let h=ui.px(if rows.is_empty(){94}else{36+48*rows.len().min(4) as i32});
    let hwnd=CreateWindowExW(WS_EX_TOOLWINDOW|WS_EX_TOPMOST,class.as_ptr(),wide("Recent notifications").as_ptr(),WS_POPUP,at.x-ui.px(30)-w,at.y+ui.px(42),w,h,ui.hwnd.0,null_mut(),inst,null());
    if hwnd.is_null(){return}
    let popup=Box::new(Popup{rows,offset:0,colors:ui.appearance,fonts:super::island_view::fonts(ui.scale()),scale:ui.scale()});SetWindowLongPtrW(hwnd,GWLP_USERDATA,Box::into_raw(popup)as _);
    let region=CreateRoundRectRgn(0,0,w+1,h+1,ui.px(20),ui.px(20));if SetWindowRgn(hwnd,region,0)==0{DeleteObject(region);}
    ACTIVE.with(|a|a.set(hwnd));ShowWindow(hwnd,SW_SHOW);SetForegroundWindow(hwnd);SetFocus(hwnd);
}}
