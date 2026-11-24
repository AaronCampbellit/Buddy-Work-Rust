//! Native scrolling for settings and other pages whose controls must move together.
use super::*;
use std::{cell::{Cell, RefCell}, ptr::{null, null_mut}};
use windows_sys::Win32::{
    Foundation::{POINT, RECT},
    UI::{
        Controls::SetScrollInfo,
        Input::KeyboardAndMouse::{VK_END, VK_HOME, VK_NEXT, VK_PRIOR},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
    },
};

/// Windows supplies high-resolution wheel deltas in fractions of a 120-unit
/// detent. Keep the fractional pixel at the viewport that actually consumes it.
#[derive(Default)]
pub(crate) struct WheelMotion {
    remainder: Cell<i64>,
}
impl WheelMotion {
    pub(crate) fn reset(&self) { self.remainder.set(0); }
    pub(crate) fn distance(&self, delta: i16, line: i32, page: i32) -> i32 {
        self.steps(-i64::from(delta),line,page,SPI_GETWHEELSCROLLLINES)
    }
    /// A positive WM_MOUSEHWHEEL delta moves the viewport to the right.
    pub(crate) fn distance_horizontal(&self, delta:i16, character:i32, page:i32) -> i32 {
        self.steps(i64::from(delta),character,page,SPI_GETWHEELSCROLLCHARS)
    }
    fn steps(&self, delta:i64, line:i32, page:i32, preference:u32) -> i32 {
        let mut units=3u32;
        unsafe { SystemParametersInfoW(preference,0,(&mut units as *mut u32).cast(),0); }
        let distance=match units {
            u32::MAX => page.max(1) as i64,
            0 => { self.reset(); return 0; }
            n => (i64::from(line.max(1)) * i64::from(n)).min(i64::from(page.max(1))),
        };
        let total = self.remainder.get() + delta * distance;
        self.remainder.set(total % 120);
        (total / 120) as i32
    }
}

pub(super) struct ScrollPane { pub hwnd: Handle, state: Box<State> }

struct State {
    offset: Cell<i32>,
    limit: Cell<i32>,
    scale: Cell<f32>,
    bg: Cell<u32>,
    wheel: WheelMotion,
    // Bounds are in physical pixels relative to the top of the whole document.
    children: RefCell<Vec<(Handle, RECT)>>,
}

impl State {
    fn scroll(&self, hwnd: Handle, target: i32) {
        let next = target.clamp(0, self.limit.get());
        if self.offset.replace(next) == next { return; }
        unsafe {
            let children = self.children.borrow();
            let visible = || children.iter().filter(|(child, _)| GetWindowLongW(*child, GWL_STYLE) as u32 & WS_VISIBLE != 0);
            let flags = SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSIZE | SWP_NOREDRAW | SWP_NOCOPYBITS;
            let mut batch = BeginDeferWindowPos(children.len().max(1) as i32);
            for &(child, r) in visible() {
                if batch.is_null() { break; }
                batch = DeferWindowPos(batch, child, null_mut(), r.left, r.top - next, 0, 0, flags);
            }
            if batch.is_null() || EndDeferWindowPos(batch) == 0 {
                for &(child, r) in visible() {
                    SetWindowPos(child, null_mut(), r.left, r.top - next, 0, 0, flags);
                }
            }
            let info = SCROLLINFO { cbSize: std::mem::size_of::<SCROLLINFO>() as _, fMask: SIF_POS, nPos: next, ..Default::default() };
            SetScrollInfo(hwnd, SB_VERT, &info, 1);
            RedrawWindow(hwnd, null(), null_mut(), RDW_INVALIDATE | RDW_ALLCHILDREN);
        }
    }
    fn reveal(&self, hwnd: Handle, child: Handle) {
        unsafe {
            let mut rc = RECT::default();
            let mut area = RECT::default();
            GetClientRect(hwnd, &mut area);
            GetWindowRect(child, &mut rc);
            let mut p = POINT { x: rc.left, y: rc.top };
            ScreenToClient(hwnd, &mut p);
            let height = rc.bottom - rc.top;
            let adjustment = if p.y < 0 || height > area.bottom { p.y } else { (p.y + height - area.bottom).max(0) };
            if adjustment != 0 { self.scroll(hwnd, self.offset.get() + adjustment); }
        }
    }
    fn page_key(&self, hwnd: Handle, key: u16) {
        let mut area = RECT::default();
        unsafe { GetClientRect(hwnd, &mut area); }
        self.wheel.reset();
        let pos = self.offset.get();
        let target = match key { VK_HOME => 0, VK_END => self.limit.get(), VK_PRIOR => pos - area.bottom, VK_NEXT => pos + area.bottom, _ => return };
        self.scroll(hwnd, target);
    }
}

fn plain_page_control(hwnd: Handle) -> bool {
    let mut name = [0u16;32];
    let len = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
    let name = String::from_utf16_lossy(&name[..len.max(0) as usize]);
    name.eq_ignore_ascii_case("button") || name.eq_ignore_ascii_case("static")
}

unsafe extern "system" fn child_proc(h: Handle, msg: u32, wp: usize, lp: isize, id: usize, data: usize) -> isize {
    let state = &*(data as *const State);
    let parent = GetParent(h);
    match msg {
        WM_SETFOCUS => state.reveal(parent, h),
        // Single-line fields, buttons and the scale slider belong to the
        // surrounding scroll pane. A wheel gesture must not change a setting.
        WM_MOUSEWHEEL if GetWindowLongW(h,GWL_STYLE) as u32 & WS_VSCROLL == 0 => return SendMessageW(parent,msg,wp,lp),
        WM_GETDLGCODE if lp != 0 && plain_page_control(h) => {
            if matches!((*(lp as *const MSG)).wParam as u16, VK_HOME | VK_END | VK_NEXT | VK_PRIOR) {
                return DefSubclassProc(h,msg,wp,lp) | DLGC_WANTMESSAGE as isize;
            }
        }
        WM_KEYDOWN if matches!(wp as u16,VK_HOME | VK_END | VK_NEXT | VK_PRIOR) && plain_page_control(h) => {
            state.page_key(parent,wp as u16);
            return 0;
        }
        WM_NCDESTROY => { RemoveWindowSubclass(h,Some(child_proc),id); }
        _ => {}
    }
    DefSubclassProc(h,msg,wp,lp)
}

unsafe extern "system" fn viewport(hwnd: Handle, msg: u32, wp: usize, lp: isize, id: usize, data: usize) -> isize {
    let state = &*(data as *const State);
    match msg {
        WM_ERASEBKGND => {
            let mut r=RECT::default();GetClientRect(hwnd,&mut r);chrome::fill(wp as _,r,state.bg.get());1
        }
        WM_PAINT | WM_PRINTCLIENT => {
            let mut ps=PAINTSTRUCT::default();let dc=if msg==WM_PAINT {BeginPaint(hwnd,&mut ps)} else {wp as _};
            let mut r=RECT::default();GetClientRect(hwnd,&mut r);chrome::fill(dc,r,state.bg.get());if msg==WM_PAINT {EndPaint(hwnd,&ps);}0
        }
        WM_MOUSEWHEEL => {
            let mut r=RECT::default();GetClientRect(hwnd,&mut r);
            let move_by=state.wheel.distance((wp>>16)as i16,(12.*state.scale.get()).round()as i32,r.bottom);
            state.scroll(hwnd,state.offset.get().saturating_add(move_by));0
        }
        WM_VSCROLL => {
            state.wheel.reset();
            let mut r=RECT::default();GetClientRect(hwnd,&mut r);
            let old=state.offset.get();let step=(18.*state.scale.get()).round()as i32;
            let next=match wp as u16 as i32 {
                SB_LINEUP=>old-step,SB_LINEDOWN=>old+step,SB_PAGEUP=>old-r.bottom,SB_PAGEDOWN=>old+r.bottom,
                SB_TOP=>0,SB_BOTTOM=>state.limit.get(),
                SB_THUMBPOSITION|SB_THUMBTRACK=>{
                    let mut s=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as _,fMask:SIF_TRACKPOS,..Default::default()};
                    GetScrollInfo(hwnd,SB_VERT,&mut s);s.nTrackPos
                }
                _=>old,
            };
            state.scroll(hwnd,next);0
        }
        WM_COMMAND|WM_DRAWITEM|WM_CTLCOLORSTATIC|WM_CTLCOLORLISTBOX|WM_CTLCOLOREDIT => SendMessageW(GetParent(hwnd),msg,wp,lp),
        WM_NCDESTROY => {RemoveWindowSubclass(hwnd,Some(viewport),id);DefSubclassProc(hwnd,msg,wp,lp)}
        _ => DefSubclassProc(hwnd,msg,wp,lp),
    }
}

impl ScrollPane {
    pub fn new(ui:&Ui,id:u16)->Self {unsafe{
        let hwnd=add(ui.hwnd.0,"STATIC","",id,WS_VSCROLL|WS_CLIPCHILDREN,WS_EX_CONTROLPARENT,ui.fonts[0]);
        let state=Box::new(State{offset:Cell::new(0),limit:Cell::new(0),bg:Cell::new(ui.appearance.0),scale:Cell::new(ui.scale()),wheel:Default::default(),children:Default::default()});
        SetWindowSubclass(hwnd,Some(viewport),id as _,&*state as *const State as usize);Self{hwnd,state}
    }}
    pub(super) fn offset(&self)->i32 {self.state.offset.get()}
    pub fn reset(&self) {self.state.wheel.reset();self.state.scroll(self.hwnd,0)}
    pub fn layout(&self,ui:&Ui,r:RECT,height:i32)->i32 {unsafe{
        pos(self.hwnd,r.left,r.top,(r.right-r.left).max(1),(r.bottom-r.top).max(1));
        let mut client=RECT::default();GetClientRect(self.hwnd,&mut client);
        self.state.bg.set(ui.appearance.0);self.state.scale.set(ui.scale());
        let total=ui.px(height);self.state.limit.set((total-client.bottom).max(0));self.state.scroll(self.hwnd,self.offset());
        let info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as _,fMask:SIF_PAGE|SIF_RANGE|SIF_POS,nMin:0,nMax:(total-1).max(0),nPage:client.bottom.max(1)as _,nPos:self.offset(),..Default::default()};
        SetScrollInfo(self.hwnd,SB_VERT,&info,1);
        GetClientRect(self.hwnd,&mut client);(client.right as f32/ui.scale()).round()as i32
    }}
    pub fn place(&self,ui:&Ui,h:Handle,x:i32,y:i32,w:i32,ht:i32) {
        let r=chrome::rect(ui.px(x),ui.px(y),ui.px(w),ui.px(ht));
        {
            let mut children=self.state.children.borrow_mut();
            if let Some((_,bounds))=children.iter_mut().find(|(child,_)|*child==h) {*bounds=r;}
            else {
                children.push((h,r));
                unsafe {SetWindowSubclass(h,Some(child_proc),0x5C46,&*self.state as *const State as usize);}
            }
        }
        pos(h,r.left,r.top-self.offset(),r.right-r.left,r.bottom-r.top);
        show(h,true);
    }
}
