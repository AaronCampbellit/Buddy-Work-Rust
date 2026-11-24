use super::*;
use crate::chrome::{self,Icon};
use windows_sys::Win32::{Foundation::RECT,UI::{Controls::*,Input::KeyboardAndMouse::EnableWindow}};
const FIRST:u16=3520;
const TBM_GETPOS:u32=WM_USER;
pub(super) struct Controls{buttons:[Handle;3],seek:Handle}
fn create(ui:&Ui)->Controls{unsafe{
    let flags=INITCOMMONCONTROLSEX{dwSize:std::mem::size_of::<INITCOMMONCONTROLSEX>()as _,dwICC:ICC_BAR_CLASSES};InitCommonControlsEx(&flags);
    let buttons=std::array::from_fn(|i|super::preferences::button(ui.hwnd.0,["Previous","Pause","Next"][i],FIRST+i as u16,ui.fonts[0]));
    let seek=add(ui.hwnd.0,"msctls_trackbar32","Media position",FIRST+3,WS_TABSTOP|TBS_NOTICKS,0,ui.fonts[0]);
    Controls{buttons,seek}
}}
impl Ui{
    pub(super) fn layout_home_media(&self,home:bool,r:RECT){
        let media=home.then(activity::media_details).flatten();
        let Some((_,_,now,total,allowed))=media else{if let Some(m)=self.home_media.get(){for h in m.buttons.iter().copied().chain([m.seek]){show(h,false)}}return};
        let m=self.home_media.get_or_init(||create(self));let width=r.right-r.left;let p=|n|self.px(n);
        let x=r.left+self.panel_width(width)+p(20);
        let transport=(r.right-x-p(12)-p(94)).clamp(0,p(62));
        for (i,&h)in m.buttons.iter().enumerate(){pos(h,x+transport+p(i as i32*34),r.top+p(74),p(26),p(26));show(h,true);unsafe{EnableWindow(h,[allowed.previous,allowed.pause,allowed.next][i] as i32);}}
        pos(m.seek,x,r.top+p(123),(r.right-x-p(12)).max(p(20)),p(22));show(m.seek,true);
        unsafe{EnableWindow(m.seek,(total>0&&allowed.seek)as i32);SendMessageW(m.seek,TBM_SETRANGEMAX,0,total.clamp(1,i32::MAX as i64)as _);SendMessageW(m.seek,TBM_SETPOS,1,now.clamp(0,total.max(0))as _);}
    }
    pub(super) fn home_media_tabs(&self)->Vec<Handle>{self.home_media.get().map(|m|m.buttons.iter().copied().chain([m.seek]).collect()).unwrap_or_default()}
    pub(super) fn paint_home_media(&self,dc:HDC,r:RECT){
        let Some((title,artist,pos,total,_))=activity::media_details()else{return};let p=|n|self.px(n);
        let (bg,field,ink,accent)=self.appearance;let muted=chrome::mix(bg,ink,167);
        let pane=RECT{left:r.left+self.panel_width(r.right-r.left)+p(10),top:r.top+p(2),..r};chrome::panel(dc,pane,p(10),field,chrome::mix(bg,ink,41));
        let label=|y,right,s:&str,f,color|chrome::text(dc,RECT{left:pane.left+p(if right{72}else{10}),top:pane.top+p(y),right:pane.right-p(10),bottom:pane.top+p(y+16)},s,self.fonts[f],color,DT_LEFT|DT_SINGLELINE|DT_VCENTER);
        label(9,false,"Now playing",1,muted);let art=chrome::rect(pane.left+p(10),pane.top+p(36),p(52),p(52));chrome::panel(dc,art,p(8),field,chrome::mix(bg,ink,45));
        chrome::icon(dc,art.left+p(10),art.top+p(10),p(32),Icon::Music,accent,field);
        label(32,true,&title,1,ink);label(49,true,&artist,8,muted);
        label(105,false,&format!("{}:{:02} / {}:{:02}",pos/60,pos%60,total/60,total%60),8,muted);
    }
    pub(super) fn draw_home_media(&self,di:&DRAWITEMSTRUCT)->bool{
        let Some(i)=(di.CtlID as u16).checked_sub(FIRST).filter(|n|*n<3)else{return false};
        let (_,field,ink,accent)=self.appearance;let r=di.rcItem;let hot=chrome::hovered(di.hwndItem);let focus=di.itemState&ODS_FOCUS!=0&&di.itemState&ODS_NOFOCUSRECT==0;
        let disabled=di.itemState&ODS_DISABLED!=0;let face=if disabled{field}else if i==1{chrome::mix(field,accent,32)}else if hot{chrome::mix(field,ink,23)}else{field};chrome::fill(di.hDC,r,field);
        chrome::panel(di.hDC,r,r.bottom/2,face,if focus{accent}else{face});let n=self.px(18);chrome::icon(di.hDC,(r.right-n)/2,(r.bottom-n)/2,n,[Icon::Previous,Icon::Pause,Icon::Next][i as usize],if disabled{chrome::mix(field,ink,96)}else if i==1{accent}else{ink},face);true
    }
    pub(super) fn home_media_command(&mut self,id:u16,code:u16)->bool{
        let Some(i)=id.checked_sub(FIRST).filter(|n|*n<3)else{return false};if code==BN_CLICKED as u16{if let Err(e)=activity::action(self,"media",["previous","pause","next"][i as usize]){self.error(&e)}}true
    }
    pub(crate) fn home_seek_scroll(&mut self,sender:Handle,event:u16){
        if event as u32!=TB_ENDTRACK||!self.home_media.get().is_some_and(|m|m.seek==sender){return}
        let value=unsafe{SendMessageW(sender,TBM_GETPOS,0,0)};self.set_input(&value.to_string());if let Err(e)=activity::action(self,"media","seek"){self.error(&e)}
    }
}
