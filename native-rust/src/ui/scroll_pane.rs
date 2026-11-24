// A clipped child viewport keeps keyboard navigation and wheel scrolling native.
use super::*;
use std::cell::Cell;
use windows_sys::Win32::{Foundation::RECT,UI::{Controls::SetScrollInfo,Shell::{SetWindowSubclass,DefSubclassProc,RemoveWindowSubclass}}};

pub(super) struct ScrollPane { pub hwnd:Handle, state:Box<State> }
struct State { offset:Cell<i32>,limit:Cell<i32>,scale:Cell<f32>,bg:Cell<u32> }
impl State{
    fn scroll(&self,hwnd:Handle,to:i32){unsafe{
        let next=to.clamp(0,self.limit.get());let old=self.offset.replace(next);
        if old==next{return}
        ScrollWindowEx(hwnd,0,old-next,std::ptr::null(),std::ptr::null(),std::ptr::null_mut(),std::ptr::null_mut(),SW_SCROLLCHILDREN|SW_INVALIDATE|SW_ERASE);
        let info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as u32,fMask:SIF_POS,nPos:next,..Default::default()};SetScrollInfo(hwnd,SB_VERT,&info,1);
    }}
}
unsafe extern "system" fn viewport(hwnd:Handle,msg:u32,wp:usize,lp:isize,id:usize,data:usize)->isize{
    let state=&*(data as *const State);
    match msg{
        WM_ERASEBKGND=>{let mut r=RECT::default();GetClientRect(hwnd,&mut r);chrome::fill(wp as _,r,state.bg.get());1}
        WM_PAINT|WM_PRINTCLIENT=>{let mut ps=PAINTSTRUCT::default();let dc=if msg==WM_PAINT{BeginPaint(hwnd,&mut ps)}else{wp as _};let mut r=RECT::default();GetClientRect(hwnd,&mut r);chrome::fill(dc,r,state.bg.get());if msg==WM_PAINT{EndPaint(hwnd,&ps);}0}
        WM_MOUSEWHEEL=>{let lines=(wp>>16)as u16 as i16 as i32;state.scroll(hwnd,state.offset.get()-(lines*36/120) * (state.scale.get().ceil() as i32).max(1));0}
        WM_VSCROLL=>{let mut r=RECT::default();GetClientRect(hwnd,&mut r);let old=state.offset.get();let step=(18.0*state.scale.get()).round() as i32;
            let next=match wp as u16 as i32{SB_LINEUP=>old-step,SB_LINEDOWN=>old+step,SB_PAGEUP=>old-r.bottom,SB_PAGEDOWN=>old+r.bottom,SB_TOP=>0,SB_BOTTOM=>state.limit.get(),SB_THUMBPOSITION|SB_THUMBTRACK=>{let mut s=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as _,fMask:SIF_TRACKPOS,..Default::default()};GetScrollInfo(hwnd,SB_VERT,&mut s);s.nTrackPos},_=>old};state.scroll(hwnd,next);0}
        WM_COMMAND=>{
            let code=(wp>>16)as u16;let child=lp as Handle;
            if !child.is_null() && (code==BN_SETFOCUS as u16||code==EN_SETFOCUS as u16||code==CBN_SETFOCUS as u16){
                let mut r=RECT::default();let mut client=RECT::default();GetClientRect(hwnd,&mut client);GetWindowRect(child,&mut r);MapWindowPoints(std::ptr::null_mut(),hwnd,(&mut r as *mut RECT).cast(),2);
                let dy=if r.top<0{r.top}else{(r.bottom-client.bottom).max(0)};
                if dy!=0{state.scroll(hwnd,state.offset.get()+dy);}
            }
            SendMessageW(GetParent(hwnd),msg,wp,lp)
        }
        WM_DRAWITEM|WM_CTLCOLORSTATIC|WM_CTLCOLORLISTBOX|WM_CTLCOLOREDIT=>SendMessageW(GetParent(hwnd),msg,wp,lp),
        WM_NCDESTROY=>{RemoveWindowSubclass(hwnd,Some(viewport),id);DefSubclassProc(hwnd,msg,wp,lp)}
        _=>DefSubclassProc(hwnd,msg,wp,lp),
    }
}
impl ScrollPane{
    pub fn new(ui:&Ui,id:u16)->Self{unsafe{
        let hwnd=add(ui.hwnd.0,"STATIC","",id,WS_VSCROLL|WS_CLIPCHILDREN,WS_EX_CONTROLPARENT,ui.fonts[0]);
        let state=Box::new(State{offset:Cell::new(0),limit:Cell::new(0),bg:Cell::new(ui.appearance.0),scale:Cell::new(ui.scale())});
        SetWindowSubclass(hwnd,Some(viewport),id as _,&*state as *const State as usize);Self{hwnd,state}
    }}
    pub fn reset(&self){self.state.scroll(self.hwnd,0)}
    pub fn layout(&self,ui:&Ui,r:RECT,height:i32)->i32{unsafe{
        pos(self.hwnd,r.left,r.top,(r.right-r.left).max(1),(r.bottom-r.top).max(1));
        let mut client=RECT::default();GetClientRect(self.hwnd,&mut client);
        self.state.bg.set(ui.appearance.0);self.state.scale.set(ui.scale());
        let total=ui.px(height);self.state.limit.set((total-client.bottom).max(0));self.state.scroll(self.hwnd,self.state.offset.get());
        let info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as _,fMask:SIF_PAGE|SIF_RANGE|SIF_POS,nMin:0,nMax:(total-1).max(0),nPage:client.bottom.max(1)as _,nPos:self.state.offset.get(),..Default::default()};SetScrollInfo(self.hwnd,SB_VERT,&info,1);
        GetClientRect(self.hwnd,&mut client);(client.right as f32/ui.scale()).round()as i32
    }}
    pub fn place(&self,ui:&Ui,h:Handle,x:i32,y:i32,w:i32,ht:i32){pos(h,ui.px(x),ui.px(y)-self.state.offset.get(),ui.px(w),ui.px(ht));show(h,true)}
}
