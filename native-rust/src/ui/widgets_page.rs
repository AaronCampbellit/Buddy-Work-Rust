use super::*;
use crate::chrome::{self,fill,icon,inset,mix,panel,rect,text,Icon};
use std::{cell::{Cell,RefCell},ptr::{null,null_mut}};
use windows_sys::Win32::{Foundation::{HWND,POINT,RECT},UI::{
    Controls::{SetScrollInfo,SetScrollPos,DRAWITEMSTRUCT,ODT_COMBOBOX,ODT_BUTTON,ODS_DISABLED,ODS_FOCUS,ODS_NOFOCUSRECT,ODS_SELECTED},
    Input::KeyboardAndMouse::{EnableWindow,VK_HOME,VK_END,VK_NEXT,VK_PRIOR},Shell::{DefSubclassProc,RemoveWindowSubclass,SetWindowSubclass}
}};

const VIEW:u16=1900;
const POM_SIDE:u16=1901;
const WATCH_SIDE:u16=1902;
const NOTE:u16=1903;
const CLIP:u16=1904;
const NOTE_LABEL:u16=1905;
const CLIP_LABEL:u16=1906;
const POM:u16=1910;
const SKIP:u16=1911;
const RESET_POM:u16=1912;
const FOCUS:u16=1913;
const SHORT:u16=1914;
const LONG:u16=1915;
const CUSTOM:u16=1916;
const WATCH:u16=1920;
const LAP:u16=1921;
const RESET_WATCH:u16=1922;
const REFRESH:u16=1930;
const CENTER:u32=DT_SINGLELINE|DT_CENTER|DT_VCENTER;
const LEFT:u32=DT_SINGLELINE|DT_LEFT|DT_VCENTER;
const RIGHT:u32=DT_SINGLELINE|DT_RIGHT|DT_VCENTER;
const CONTENT_HEIGHT:i32=268;

fn value(ui:&Ui,key:&str,default:u64)->u64{core::setting(ui,key,"").parse().unwrap_or(default)}
fn row_field(row:&[String],i:usize)->&str{row.get(i).map(String::as_str).unwrap_or("")}
fn short(s:&str)->String{s.chars().take(80).map(|c|if c.is_control(){' '}else{c}).collect()}
fn clock(ms:u64,tenths:bool)->String{
    let sec=ms/1000;let h=sec/3600;let m=(sec/60)%60;let s=sec%60;
    let mut result=if h>0{format!("{h}:{m:02}:{s:02}")}else{format!("{m:02}:{s:02}")};
    if tenths{result.push('.');result.push(char::from(b'0'+((ms%1000)/100)as u8));}
    result
}

#[derive(Default)]
struct Timers{
    end:u64,left:u64,duration:u64,watch:u64,elapsed:u64,phase:String,sessions:u64,laps:Vec<u64>,lap_count:usize,
}
impl Timers{
    fn read(ui:&Ui)->Self{
        let phase=core::setting(ui,"phase","Focus").to_owned();
        let duration=value(ui,"pomDuration",match phase.as_str(){"Short break"=>300_000,"Long break"=>900_000,_=>1_500_000});
        let laps=ui.records.get("laps");
        Self{end:value(ui,"pomEnd",0),left:value(ui,"pomLeft",duration),duration,
            watch:value(ui,"watchStart",0),elapsed:value(ui,"watchElapsed",0),phase,sessions:value(ui,"pomSessions",0),
            lap_count:laps.map_or(0,Vec::len),laps:laps.into_iter().flatten().rev().take(3).filter_map(|r|r.first()?.parse().ok()).collect()}
    }
}

struct State{
    view:Cell<HWND>,scale:Cell<f32>,colors:Cell<chrome::Colors>,fonts:Cell<[HFONT;4]>,
    timers:RefCell<Timers>,scroll:Cell<i32>,wheel:super::WheelMotion,on:Cell<bool>,tick:Cell<u32>,last_refresh:Cell<u64>,
    notes:RefCell<Vec<String>>,clips:RefCell<Vec<(String,String)>>,
}
impl State{
    fn hwnd(&self,id:u16)->HWND{unsafe{GetDlgItem(self.view.get(),id as i32)}}
    fn px(&self,n:i32)->i32{(n as f32*self.scale.get()).round()as i32}
    fn local(&self,x:i32,y:i32,w:i32,h:i32)->RECT{rect(self.px(x),self.px(y)-self.scroll.get(),self.px(w),self.px(h))}
    fn client(&self)->RECT{let mut r=RECT::default();unsafe{GetClientRect(self.view.get(),&mut r);}r}
    fn width(&self)->i32{((self.client().right as f32/self.scale.get()).round()as i32).max(1)}
    fn move_child(&self,id:u16,x:i32,y:i32,w:i32,h:i32){let r=self.local(x,y,w,h);pos(self.hwnd(id),r.left,r.top,r.right-r.left,r.bottom-r.top);}
    fn place(&self){
        let w=self.width();let half=(w-10)/2;let second=half+10;
        self.move_child(POM_SIDE,second+4,10,half-16,20);self.move_child(WATCH_SIDE,second+4,35,half-16,20);
        self.move_child(NOTE_LABEL,30,63,23,26);self.move_child(NOTE,57,62,half-57,180);
        self.move_child(CLIP_LABEL,second+20,63,63,26);self.move_child(CLIP,second+85,62,half-96,180);
        for (id,x,y,ww,hh) in [(POM,11,181,66,27),(SKIP,82,181,32,27),(RESET_POM,119,181,32,27),
            (FOCUS,11,213,51,20),(SHORT,66,213,42,20),(LONG,112,213,47,20),(CUSTOM,163,213,29,20),
            (WATCH,second+11,181,66,27),(LAP,second+82,181,53,27),(RESET_WATCH,second+140,181,32,27)]{
            self.move_child(id,x,y,ww,hh);
        }
    }
    fn pan(&self,n:i32){
        let bottom=(self.px(CONTENT_HEIGHT)-self.client().bottom).max(0);let n=n.clamp(0,bottom);
        if self.scroll.replace(n)!=n{let hwnd=self.view.get();self.place();unsafe{
            SetScrollPos(hwnd,SB_VERT,n,1);RedrawWindow(hwnd,null(),null_mut(),RDW_INVALIDATE|RDW_ALLCHILDREN);
        }}
    }
    fn ensure_visible(&self,child:HWND){unsafe{
        let viewport=self.view.get();let mut r=RECT::default();GetWindowRect(child,&mut r);
        let mut p=POINT{x:r.left,y:r.top};ScreenToClient(viewport,&mut p);
        let delta=if p.y<0{p.y-self.px(3)}else{(p.y+r.bottom-r.top-self.client().bottom+self.px(3)).max(0)};
        if delta!=0{self.pan(self.scroll.get()+delta);}
    }}
    fn paint(&self,dc:HDC){
        let (bg,field,ink,accent)=self.colors.get();let edge=mix(bg,ink,40);let muted=mix(bg,ink,165);let fonts=self.fonts.get();
        let w=self.width();let half=(w-10)/2;let second=half+10;fill(dc,self.client(),bg);
        let area=|x,y,ww,h|self.local(x,y,ww,h);
        let label=|x,y,ww,h,s:&str,font,color,flags|text(dc,area(x,y,ww,h),s,font,color,flags);
        let image=|x,y,i|{let r=area(x,y,18,18);icon(dc,r.left,r.top,self.px(18),i,accent,field);};
        let n=self.timers.borrow().laps.len() as i32;
        let watch_height=131+((n-1).max(0))*12;
        for (x,y,ww,h) in [(0,0,w,99),(0,112,half,131),(second,112,w-second,watch_height)]{panel(dc,area(x,y,ww,h),self.px(9),field,edge);}
        image(11,23,Icon::Notebook);label(36,17,half-44,16,"Side widgets",fonts[1],ink,LEFT);
        label(36,32,half-44,14,"Shown beside the collapsed island",fonts[2],muted,LEFT);
        for (x,i) in [(11,Icon::Notebook),(second+1,Icon::Clipboard)]{
            let r=area(x,68,15,15);icon(dc,r.left,r.top,self.px(15),i,accent,field);
        }
        let times=self.timers.borrow();let now=core::now();
        let left=if times.end>0{times.end.saturating_sub(now)}else{times.left};
        let watch=times.elapsed.saturating_add(if times.watch>0{now.saturating_sub(times.watch)}else{0});
        image(11,125,Icon::Timer);image(second+11,125,Icon::Clock);
        for (x,title,state,count) in [(0,"Pomodoro",times.phase.as_str(),format!("{} completed",times.sessions)),
            (second,"Stopwatch",if times.watch>0{"Running"}else{"Ready"},format!("{} lap{}",times.lap_count,if times.lap_count==1{""}else{"s"}))]{
            label(x+36,119,half/2,16,title,fonts[1],ink,LEFT);label(x+36,134,half/2,12,state,fonts[2],muted,LEFT);
            label(x+half/2,122,half/2-10,22,&count,fonts[2],muted,RIGHT);
        }
        let readout=clock(left,false);label(11,148,half-22,28,&readout,fonts[3],ink,LEFT);
        label(second+11,148,half-20,28,&clock(watch,true),fonts[3],ink,LEFT);
        let start=21+(chrome::width(dc,&readout,fonts[3]) as f32/self.scale.get()).round() as i32;
        let bar=area(start,162,(half-start-11).max(8),3);fill(dc,bar,mix(field,ink,12));
        if times.duration>0{
            let done=times.duration.saturating_sub(left.min(times.duration));
            let len=((bar.right-bar.left)as u64*done/times.duration)as i32;
            if len>0{fill(dc,rect(bar.left,bar.top,len,bar.bottom-bar.top),accent);}
        }
        if times.laps.is_empty(){
            label(second+11,212,half-22,21,"Lap times will appear here.",fonts[2],muted,CENTER);
        }else{
            for (i,t) in times.laps.iter().enumerate(){let y=213+i as i32*15;
                fill(dc,area(second+11,y,half-22,1),edge);
                label(second+11,y,half/2,13,&format!("Lap {}",times.lap_count-i),fonts[2],muted,LEFT);
                label(second+half/2,y,half/2-11,13,&clock(*t,true),fonts[2],ink,RIGHT);
            }
        }
    }
    fn draw_button(&self,di:&DRAWITEMSTRUCT)->bool{
        let id=di.CtlID as u16;
        if di.CtlType==ODT_COMBOBOX && matches!(id,NOTE|CLIP){
            let (_,_,ink,accent)=self.colors.get();
            let face=chrome::control(self.colors.get());
            let active=di.itemState&ODS_SELECTED!=0;
            fill(di.hDC,di.rcItem,if active{mix(face,accent,38)}else{face});
            if di.itemID!=u32::MAX{unsafe{
                let n=di.itemID as usize;let len=SendMessageW(di.hwndItem,CB_GETLBTEXTLEN,n,0);
                if (0..8192).contains(&len){let mut text_buf=vec![0;len as usize+1];SendMessageW(di.hwndItem,CB_GETLBTEXT,n,text_buf.as_mut_ptr()as _);
                    text(di.hDC,RECT{left:di.rcItem.left+self.px(6),..di.rcItem},&String::from_utf16_lossy(&text_buf[..len as usize]),self.fonts.get()[2],ink,LEFT);}
            }}
            return true;
        }
        if di.CtlType!=ODT_BUTTON||!matches!(id,POM|SKIP|RESET_POM|FOCUS|SHORT|LONG|CUSTOM|WATCH|LAP|RESET_WATCH){return false;}
        let (_,field,ink,accent)=self.colors.get();let t=self.timers.borrow();
        let active=matches!((id,t.phase.as_str(),t.duration),(FOCUS,"Focus",1500000)|(SHORT,"Short break",300000)|(LONG,"Long break",900000));
        let primary=matches!(id,POM|WATCH);let hover=chrome::hovered(di.hwndItem)||di.itemState&ODS_SELECTED!=0;
        let disabled=di.itemState&ODS_DISABLED!=0;
        let base=if primary{accent}else if active{mix(field,accent,45)}else if hover{mix(field,ink,22)}else{field};
        let outline=if primary||active{accent}else{mix(field,ink,43)};let color=if disabled{mix(field,ink,85)}else if primary{
            if (accent&255)*299+((accent>>8)&255)*587+((accent>>16)&255)*114>142000{0x101010}else{0xffffff}
        }else if active{accent}else{ink};
        let (caption,glyph)=match id{
            POM if t.end>0=>("Pause",Some(Icon::Pause)),WATCH if t.watch>0=>("Pause",Some(Icon::Pause)),
            POM|WATCH=>("Start",Some(Icon::Play)), SKIP=>("",Some(Icon::Next)),RESET_POM|RESET_WATCH=>("",Some(Icon::RotateCcw)),
            FOCUS=>("25 Focus",None),SHORT=>("5 Short",None),LONG=>("15 Long",None),CUSTOM=>("",Some(Icon::More)),LAP=>("Lap",Some(Icon::Flag)),_=>return false,
        };
        fill(di.hDC,di.rcItem,field);
        if id==CUSTOM&&!hover&&di.itemState&ODS_FOCUS==0{return true;}
        panel(di.hDC,di.rcItem,self.px(if matches!(id,FOCUS|SHORT|LONG){5}else{6}),base,outline);
        let font=self.fonts.get()[2];
        let mut label=di.rcItem;
        if let Some(glyph)=glyph{
            let width=chrome::width(di.hDC,caption,font);let size=self.px(16);
            let gap=if width==0{0}else{self.px(5)};
            let x=(label.left+label.right-size-gap-width)/2;
            icon(di.hDC,x,(label.top+label.bottom-size)/2,size,glyph,color,base);label.left=x+size+gap;
            text(di.hDC,label,caption,font,color,LEFT);
        }else{text(di.hDC,label,caption,font,color,CENTER);}
        if di.itemState&ODS_FOCUS!=0&&di.itemState&ODS_NOFOCUSRECT==0{unsafe{DrawFocusRect(di.hDC,&inset(di.rcItem,self.px(3)));}}
        true
    }
}

pub(super) struct WidgetsPage{state:Box<State>}
impl WidgetsPage{
    pub(super) fn new(parent:Handle,font:HFONT)->Self{
        let state=Box::new(State{view:Cell::new(null_mut()),scale:Cell::new(1.),colors:Cell::new((0xffffff,0xffffff,0,0)),fonts:Cell::new([font;4]),timers:Default::default(),
            scroll:Cell::new(0),wheel:Default::default(),on:Cell::new(false),tick:Cell::new(0),last_refresh:Cell::new(0),notes:Default::default(),clips:Default::default()});
        let viewport=add(parent,"STATIC","Timer and side widget settings",VIEW,WS_VSCROLL|WS_CLIPCHILDREN,WS_EX_CONTROLPARENT,font);
        state.view.set(viewport);show(viewport,false);
        let data=state.as_ref()as *const State as usize;unsafe{SetWindowSubclass(viewport,Some(view_proc),1,data);}
        for (id,name) in [(POM_SIDE,"Pomodoro side widget"),(WATCH_SIDE,"Stopwatch side widget"),(NOTE_LABEL,"Note"),(NOTE,"Note widget"),
            (CLIP_LABEL,"Clipboard item"),(CLIP,"Clipboard widget"),(POM,"Start Pomodoro"),(SKIP,"Skip Pomodoro phase"),(RESET_POM,"Reset Pomodoro"),
            (FOCUS,"25 minute focus"),(SHORT,"5 minute short break"),(LONG,"15 minute long break"),(CUSTOM,"Set custom focus minutes"),
            (WATCH,"Start stopwatch"),(LAP,"Record lap"),(RESET_WATCH,"Reset stopwatch")]{
            let label=matches!(id,NOTE_LABEL|CLIP_LABEL);let combo=matches!(id,NOTE|CLIP);let check=matches!(id,POM_SIDE|WATCH_SIDE);
            let class=if label{"STATIC"}else if combo{"COMBOBOX"}else{"BUTTON"};
            let style=if label{0}else if combo{WS_TABSTOP|WS_VSCROLL|(CBS_DROPDOWNLIST|CBS_OWNERDRAWFIXED|CBS_HASSTRINGS) as u32}else if check{WS_TABSTOP|BS_AUTOCHECKBOX as u32}else{WS_TABSTOP|BS_OWNERDRAW as u32};
            let handle=add(viewport,class,name,id,style,0,font);
            unsafe{SetWindowSubclass(handle,Some(focus_proc),3,data);}
            if !label&&!combo&&!check{chrome::install_button(handle);}
        }
        Self{state}
    }
    fn choices(&self,ui:&Ui){
        let state=&self.state;
        let notes=ui.records.get("notes").into_iter().flatten().filter(|r|r.len()>=7).collect::<Vec<_>>();
        let clips=ui.records.get("clipboard").into_iter().flatten().filter(|r|r.len()>=2).collect::<Vec<_>>();
        let note_id=core::setting(ui,"noteWidget","");let pinned=ui.records.get("clipboard_widget").and_then(|v|v.first());
        let fill=|id:u16,labels:Vec<String>,index:usize|unsafe{
            let box_=state.hwnd(id);
            SendMessageW(box_,WM_SETREDRAW,0,0);SendMessageW(box_,CB_RESETCONTENT,0,0);SendMessageW(box_,CB_ADDSTRING,0,wide("Off").as_ptr()as _);
            for label in labels{SendMessageW(box_,CB_ADDSTRING,0,wide(&label).as_ptr()as _);}
            SendMessageW(box_,CB_SETCURSEL,index,0);SendMessageW(box_,WM_SETREDRAW,1,0);InvalidateRect(box_,null(),0);
        };
        let note=notes.iter().position(|r|row_field(r,0)==note_id).map_or(0,|i|i+1);
        let clip=clips.iter().position(|r|pinned.is_some_and(|p|row_field(p,0)==row_field(r,0)&&row_field(p,1)==row_field(r,1))).map_or(0,|i|i+1);
        fill(NOTE,notes.iter().map(|r|if r[1].is_empty(){"Untitled note".into()}else{short(&r[1])}).collect(),note);
        fill(CLIP,clips.iter().map(|r|if r[0]=="image"{format!("Image {} × {}",row_field(r,4),row_field(r,5))}else{short(&r[1])}).collect(),clip);
        *state.notes.borrow_mut()=notes.iter().map(|r|r[0].clone()).collect();
        *state.clips.borrow_mut()=clips.iter().map(|r|(r[0].clone(),r[1].clone())).collect();
    }
    fn sync(&self,ui:&Ui){
        let s=&self.state;let times=Timers::read(ui);
        let interval=if times.watch>0{100}else if times.end>0{500}else{0};
        unsafe{
            for(id,on)in[(POM_SIDE,core::widget_enabled(ui,"pomWidget","pomEnd")),(WATCH_SIDE,core::widget_enabled(ui,"watchWidget","watchStart"))]{
                SendMessageW(s.hwnd(id),BM_SETCHECK,usize::from(on),0);
            }
            EnableWindow(s.hwnd(LAP),i32::from(times.watch>0));
            label(s.hwnd(POM),if times.end>0{"Pause Pomodoro"}else{"Start Pomodoro"});
            label(s.hwnd(WATCH),if times.watch>0{"Pause stopwatch"}else{"Start stopwatch"});
            if s.tick.replace(interval)!=interval{
                KillTimer(s.view.get(),1);
                if interval>0{SetTimer(s.view.get(),1,interval,None);}
            }
        }
        *s.timers.borrow_mut()=times;
    }
    pub(super) fn layout(&self,ui:&Ui,on:bool,bounds:RECT){
        let s=&self.state;let was=s.on.replace(on);let hwnd=s.view.get();
        if !on{if was{s.wheel.reset();unsafe{KillTimer(hwnd,1);}s.tick.set(0);}show(hwnd,false);return;}
        s.scale.set(ui.scale());s.colors.set(ui.appearance);
        let fonts=[ui.fonts[0],ui.fonts[1],ui.fonts[8],ui.fonts[9]];
        if s.fonts.replace(fonts)!=fonts{unsafe{
            let mut child=GetTopWindow(hwnd);while !child.is_null(){SendMessageW(child,WM_SETFONT,fonts[0]as usize,0);child=GetWindow(child,GW_HWNDNEXT);}
        }}
        pos(hwnd,bounds.left,bounds.top,bounds.right-bounds.left,bounds.bottom-bounds.top);
        let height=(bounds.bottom-bounds.top).max(1);let max=s.px(CONTENT_HEIGHT);
        s.scroll.set(if was{s.scroll.get().min((max-height).max(0))}else{0});
        unsafe{let mut info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as u32,fMask:SIF_RANGE|SIF_PAGE|SIF_POS,nMin:0,nMax:max-1,nPage:height as u32,nPos:s.scroll.get(),..Default::default()};SetScrollInfo(hwnd,SB_VERT,&mut info,1);}
        for id in [NOTE,CLIP]{unsafe{SendMessageW(s.hwnd(id),CB_SETITEMHEIGHT,usize::MAX,s.px(21)as _);}}
        s.place();if !was{self.choices(ui);}self.sync(ui);show(hwnd,true);
    }
    pub(super) fn paint(&self,ui:&Ui,dc:HDC,bounds:RECT){
        if self.state.on.get(){fill(dc,bounds,ui.appearance.0);}
    }
    pub(super) fn draw(&self,_ui:&Ui,di:&DRAWITEMSTRUCT)->bool{self.state.draw_button(di)}
    pub(super) fn command(ui:&mut Ui,id:u16,code:u16)->bool{
        if !(1900..=1960).contains(&id)||ui.page!="widgets"{return false;}
        if id==REFRESH{ui.widgets_page().sync(ui);return true;}
        if matches!(id,NOTE|CLIP)&&code==CBN_DROPDOWN as u16{ui.widgets_page().choices(ui);return true;}
        if matches!(id,NOTE|CLIP)&&code!=CBN_SELCHANGE as u16{return true;}
        if !matches!(id,NOTE|CLIP)&&code!=BN_CLICKED as u16{return false;}
        let result=if matches!(id,NOTE|CLIP|POM_SIDE|WATCH_SIDE){Self::change_side(ui,id)}else{
            let key=match id{POM=>"pom",SKIP=>"pom-skip",RESET_POM=>"pom-reset",FOCUS=>"focus",SHORT=>"short",LONG=>"long",
                WATCH=>"watch",LAP=>"lap",RESET_WATCH=>"reset",CUSTOM=>{
                    let Some(val)=crate::shell::prompt(ui,"Custom focus","Focus minutes (1–240)","25")else{return true;};ui.set_input(val.trim());"minutes"
                },_=>return false};
            core::timer_action(ui,key).map(|_|())
        };
        if let Err(e)=result{ui.status(&e);}
        if matches!(id,NOTE|CLIP){ui.widgets_page().choices(ui);}
        crate::shell::position(ui);
        true
    }
    fn change_side(ui:&mut Ui,id:u16)->Result<(),String>{
        let settings=ui.records.get("settings").cloned();let widget=ui.records.get("clipboard_widget").cloned();
        let hwnd=ui.widgets_page().state.hwnd(id);
        let on=if matches!(id,POM_SIDE|WATCH_SIDE){
            let on=unsafe{SendMessageW(hwnd,BM_GETCHECK,0,0)}==1;
            core::set(ui,if id==POM_SIDE{"pomWidget"}else{"watchWidget"},if on{"on"}else{"off"});on
        }else{
            let n=unsafe{SendMessageW(hwnd,CB_GETCURSEL,0,0)};
            if n<0{return Ok(());}
            if id==NOTE{
                let chosen=if n==0{String::new()}else{ui.widgets_page().state.notes.borrow().get(n as usize-1).cloned().ok_or("The note selection changed.")?};
                if n>0&&!ui.records.get("notes").is_some_and(|r|r.iter().any(|v|v.first()==Some(&chosen))){return Err("That note is no longer available.".into());}
                core::set(ui,"noteWidget",&chosen);
            }else if n==0{ui.records.remove("clipboard_widget");}else{
                let selected=ui.widgets_page().state.clips.borrow().get(n as usize-1).cloned().ok_or("The clipboard selection changed.")?;
                let row=ui.records.get("clipboard").into_iter().flatten().find(|r|row_field(r,0)==selected.0&&row_field(r,1)==selected.1).cloned().ok_or("That item is no longer in clipboard history.")?;
                ui.records.insert("clipboard_widget".into(),vec![row]);
            }
            n>0
        };
        if on{core::set(ui,"widgets","on");}
        if let Err(e)=ui.save(){for(k,old)in[("settings",settings),("clipboard_widget",widget)]{if let Some(old)=old{ui.records.insert(k.into(),old);}else{ui.records.remove(k);}}return Err(e);}
        Ok(())
    }
}

unsafe extern "system" fn focus_proc(h:HWND,msg:u32,wp:usize,lp:isize,sub:usize,data:usize)->isize{
    let state=&*(data as *const State);
    if msg==WM_SETFOCUS{state.ensure_visible(h);}
    let id=GetDlgCtrlID(h) as u16;
    let combo=matches!(id,NOTE|CLIP);
    if msg==WM_MOUSEWHEEL && (!combo || SendMessageW(h,CB_GETDROPPEDSTATE,0,0)==0){
        return SendMessageW(state.view.get(),msg,wp,lp);
    }
    if !combo && msg==WM_GETDLGCODE && lp!=0 && matches!((*(lp as *const MSG)).wParam as u16,VK_HOME|VK_END|VK_NEXT|VK_PRIOR){
        return DefSubclassProc(h,msg,wp,lp)|DLGC_WANTMESSAGE as isize;
    }
    if !combo && msg==WM_KEYDOWN{
        let movement=match wp as u16{VK_HOME=>Some(SB_TOP),VK_END=>Some(SB_BOTTOM),VK_NEXT=>Some(SB_PAGEDOWN),VK_PRIOR=>Some(SB_PAGEUP),_=>None};
        if let Some(action)=movement{return SendMessageW(state.view.get(),WM_VSCROLL,action as _,0);}
    }
    if matches!(msg,WM_PAINT|WM_PRINT|WM_PRINTCLIENT) && matches!(id,NOTE|CLIP|NOTE_LABEL|CLIP_LABEL|POM_SIDE|WATCH_SIDE) {
        let mut ps=PAINTSTRUCT::default();
        let dc=if msg==WM_PAINT{BeginPaint(h,&mut ps)}else{wp as HDC};
        if dc.is_null(){return 0;}
        let mut r=RECT::default();GetClientRect(h,&mut r);
        let (_,field,ink,accent)=state.colors.get();let fonts=state.fonts.get();
        let muted=mix(field,ink,174);let focused=windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus()==h;
        fill(dc,r,field);
        if matches!(id,NOTE|CLIP){
            let face=chrome::control(state.colors.get());
            panel(dc,r,state.px(6),face,if focused{accent}else{mix(field,ink,42)});
            let n=SendMessageW(h,CB_GETCURSEL,0,0);
            let mut title=String::new();
            if n>=0{let len=SendMessageW(h,CB_GETLBTEXTLEN,n as usize,0);
                if (0..8192).contains(&len){let mut buffer=vec![0;len as usize+1];SendMessageW(h,CB_GETLBTEXT,n as _,buffer.as_mut_ptr() as _);title=String::from_utf16_lossy(&buffer[..len as usize]);}
            }
            text(dc,RECT{left:r.left+state.px(10),right:r.right-state.px(19),..r},&title,fonts[2],ink,LEFT);
            icon(dc,r.right-state.px(17),(r.bottom-state.px(12))/2,state.px(12),Icon::Down,ink,face);
        }else if matches!(id,POM_SIDE|WATCH_SIDE){
            let on=SendMessageW(h,BM_GETCHECK,0,0)==1;
            let size=state.px(13);let check=rect(0,(r.bottom-size)/2,size,size);
            panel(dc,check,state.px(3),if on{accent}else{chrome::control(state.colors.get())},if focused||on{accent}else{muted});
            if on{text(dc,check,"✓",fonts[1],if (accent&255)*299+((accent>>8)&255)*587+((accent>>16)&255)*114<142000{0xffffff}else{0},CENTER);}
            text(dc,RECT{left:size+state.px(3),..r},&window_text(h,100),fonts[0],ink,LEFT);
            if focused{DrawFocusRect(dc,&r);}
        } else {text(dc,r,&window_text(h,128),fonts[2],muted,LEFT);}
        if msg==WM_PAINT{EndPaint(h,&ps);}
        return 0;
    }
    if msg==WM_NCDESTROY{RemoveWindowSubclass(h,Some(focus_proc),sub);}
    DefSubclassProc(h,msg,wp,lp)
}
unsafe fn print_children(h:HWND,dc:HDC){
    let mut child=GetTopWindow(h);
    while !child.is_null(){
        if GetWindowLongW(child,GWL_STYLE)as u32&WS_VISIBLE!=0{
            let mut r=RECT::default();GetWindowRect(child,&mut r);let mut origin=POINT{x:r.left,y:r.top};ScreenToClient(h,&mut origin);
            let save=SaveDC(dc);IntersectClipRect(dc,origin.x,origin.y,origin.x+r.right-r.left,origin.y+r.bottom-r.top);
            OffsetViewportOrgEx(dc,origin.x,origin.y,null_mut());
            SendMessageW(child,WM_PRINT,dc as _,(PRF_CLIENT|PRF_ERASEBKGND|PRF_NONCLIENT)as _);RestoreDC(dc,save);
        }
        child=GetWindow(child,GW_HWNDNEXT);
    }
}
unsafe extern "system" fn view_proc(h:HWND,msg:u32,wp:usize,lp:isize,sub:usize,data:usize)->isize{
    let s=&*(data as *const State);
    match msg{
        WM_NCDESTROY=>{KillTimer(h,1);RemoveWindowSubclass(h,Some(view_proc),sub);}
        WM_COMMAND=>return SendMessageW(GetParent(h),WM_COMMAND,wp,lp),
        WM_DRAWITEM if lp!=0=>{if s.draw_button(&*(lp as *const DRAWITEMSTRUCT)){return 1;}}
        WM_CTLCOLORSTATIC|WM_CTLCOLORBTN|WM_CTLCOLORLISTBOX|WM_CTLCOLOREDIT=>{
            let dc=wp as HDC;let (_,field,ink,_)=s.colors.get();SetTextColor(dc,ink);SetBkColor(dc,field);
            SetDCBrushColor(dc,field);return GetStockObject(DC_BRUSH)as _;
        }
        WM_ERASEBKGND=>return 1,
        WM_PAINT=>{let mut ps=PAINTSTRUCT::default();let dc=BeginPaint(h,&mut ps);s.paint(dc);EndPaint(h,&ps);return 0;}
        WM_PRINT|WM_PRINTCLIENT=>{if wp!=0{
            let dc=wp as HDC;s.paint(dc);if lp as i32&PRF_CHILDREN!=0{print_children(h,dc);}
        }return 0;}
        WM_MOUSEWHEEL=>{let distance=s.wheel.distance((wp>>16)as i16,s.px(12),s.client().bottom);s.pan(s.scroll.get().saturating_add(distance));return 0;}
        WM_VSCROLL=>{s.wheel.reset();let jump=match (wp&0xffff)as i32{
            SB_LINEUP=>-s.px(16),SB_LINEDOWN=>s.px(16),SB_PAGEUP=>-s.client().bottom,SB_PAGEDOWN=>s.client().bottom,
            SB_TOP=>-s.px(CONTENT_HEIGHT),SB_BOTTOM=>s.px(CONTENT_HEIGHT),
            SB_THUMBTRACK|SB_THUMBPOSITION=>{let mut info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as _,fMask:SIF_TRACKPOS,..Default::default()};GetScrollInfo(h,SB_VERT,&mut info);info.nTrackPos-s.scroll.get()}
            _=>0
        };s.pan(s.scroll.get()+jump);return 0;}
        WM_TIMER if wp==1=>{
            if s.on.get(){let now=core::now();
                if now.saturating_sub(s.last_refresh.get())>=1000{s.last_refresh.set(now);PostMessageW(GetParent(h),WM_COMMAND,REFRESH as _,0);}
                let r=s.local(0,148,s.width(),31);InvalidateRect(h,&r,0);
            }
            return 0;
        }
        _=>{}
    }
    DefSubclassProc(h,msg,wp,lp)
}
impl Drop for WidgetsPage{
    fn drop(&mut self){unsafe{if IsWindow(self.state.view.get())!=0{DestroyWindow(self.state.view.get());}}}
}
