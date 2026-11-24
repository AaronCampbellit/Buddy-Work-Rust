//! Interactive cards which share the island's geometry, without expanding its workspace.
use std::{cell::RefCell, collections::VecDeque, ptr::{null, null_mut}};
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_CLASS_ALREADY_EXISTS, HWND, POINT, RECT},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Controls::{DRAWITEMSTRUCT, ODS_FOCUS, ODS_SELECTED}, Input::KeyboardAndMouse::{EnableWindow, IsWindowEnabled, GetFocus, GetKeyState, SetFocus, VK_CONTROL, VK_SHIFT}, WindowsAndMessaging::*},
};
use crate::{core, devices, inline_images::{self, Controls, Placement}, island::{self, Widget}, shell, ui::{wide,window_text}, workspace, Ui};

pub const MESSAGE:u32 = WM_APP + 83;
const NEXT:u16=591;
const PREVIOUS:u16=590;
const TITLE:u16=411;
const BODY:u16=412;
const CLASS:&str="BuddyRustSideWidgets";
const CARD_CLASS:&str="BuddyRustSideCard";

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Kind { Camera, Focus, Watch, Note, Clip }
impl From<&Widget> for Kind {
    fn from(widget:&Widget)->Self {
        match widget { Widget::Camera=>Self::Camera,Widget::Focus=>Self::Focus,
            Widget::Stopwatch=>Self::Watch,Widget::Note=>Self::Note,Widget::Clipboard=>Self::Clip }
    }
}

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
struct Bounds { x:i32,y:i32,w:i32,h:i32 }
impl Bounds {
    fn rect(r:&RECT)->Self {Self{x:r.left,y:r.top,w:r.right-r.left,h:r.bottom-r.top}}
    fn right(self)->i32 { self.x+self.w }
    fn bottom(self)->i32 { self.y+self.h }
    fn intersect(self,b:Self)->Option<Self>{
        let (x,y)=(self.x.max(b.x),self.y.max(b.y));
        let (w,h)=(self.right().min(b.right())-x,self.bottom().min(b.bottom())-y);
        (w>0&&h>0).then_some(Self{x,y,w,h})
    }
    fn contains(self,x:i32,y:i32)->bool{x>=self.x&&y>=self.y&&x<self.right()&&y<self.bottom()}
    fn win(self)->RECT{RECT{left:self.x,top:self.y,right:self.right(),bottom:self.bottom()}}
}

#[derive(Clone,Default,PartialEq)]
struct Content { id:String,title:String,meta:String,readout:String,body:String,extra:String,
    image:String,placements:Vec<Placement>,format:Option<String>,progress:u8,running:bool,pinned:bool,has_reminder:bool,kind:String,actions_enabled:bool }
fn get(r:&[String],n:usize)->&str{r.get(n).map(String::as_str).unwrap_or("")}
fn number(ui:&Ui,key:&str)->u64{core::setting(ui,key,"0").parse().unwrap_or(0)}
fn time(ms:u64,precise:bool)->String {
    let sec=ms/1000;
    let base=if sec>=3600 {format!("{}:{:02}:{:02}",sec/3600,(sec/60)%60,sec%60)}else{format!("{:02}:{:02}",sec/60,sec%60)};
    if precise {format!("{base}.{:01}",(ms/100)%10)} else {base}
}
fn note_row(ui:&Ui)->Option<&Vec<String>>{
    let id=core::setting(ui,"noteWidget","");
    ui.records.get("notes")?.iter().find(|r|r.len()>=7&&!id.is_empty()&&r[0]==id)
}
fn clip_row(ui:&Ui)->Option<&Vec<String>>{
    let selected=ui.records.get("clipboard_widget")?.first()?;
    ui.records.get("clipboard")?.iter().find(|r|get(r,0)==get(selected,0)&&get(r,1)==get(selected,1))
}
fn identity(ui:&Ui,k:Kind)->String{
    match k {Kind::Note=>note_row(ui).map(|r|r[0].clone()).unwrap_or_default(),
        Kind::Clip=>clip_row(ui).map(|r|format!("{}:{}",get(r,0),get(r,1))).unwrap_or_default(),_=>String::new()}
}
fn model(ui:&Ui,k:Kind)->Content{
    let mut m=Content{actions_enabled:true,..Default::default()};
    match k {
        Kind::Camera=>{
            let state=ui.call_state.as_deref().unwrap_or("").split_whitespace().nth(1).unwrap_or("");
            m.title="Camera".into();m.meta=match state{"live"=>"Live","frozen"=>"Frozen","standby"=>"Standby","disabled"=>"Off","error"=>"Error",_=>"Starting"}.into();
            m.running=state=="live";m.actions_enabled=matches!(state,"live"|"standby"|"frozen");m.kind=state.into();
        }
        Kind::Focus=>{
            let left=core::timers(ui).0;let phase=core::setting(ui,"phase","Focus");
            let preset=match phase{"Short break"=>300000,"Long break"=>900000,_=>1500000};
            let duration=core::setting(ui,"pomDuration","").parse::<u64>().unwrap_or(preset).max(left).max(1);
            m.title="Pomodoro".into();m.meta=phase.into();m.readout=time(left,false);m.extra=format!("{} completed",number(ui,"pomSessions"));
            m.running=number(ui,"pomEnd")>0;m.progress=((u128::from(duration-left)*100)/u128::from(duration))as u8;
        }
        Kind::Watch=>{
            let laps=ui.records.get("laps").map(Vec::as_slice).unwrap_or(&[]);
            m.title="Stopwatch".into();m.running=number(ui,"watchStart")>0;m.meta=if m.running{"Running"}else{"Ready"}.into();m.readout=time(core::timers(ui).1,true);
            m.extra=format!("{} {}",laps.len(),if laps.len()==1{"lap"}else{"laps"});
            m.body=laps.iter().enumerate().rev().take(3).map(|(i,row)|format!("Lap {}     {}",i+1,time(get(row,0).parse().unwrap_or(0),true))).collect::<Vec<_>>().join("\r\n");
            if m.body.is_empty(){m.body="Lap times will appear here.".into();}
        }
        Kind::Note=>if let Some(row)=note_row(ui){
            m.id=identity(ui,k);m.title=row[1].clone();m.body=row[2].clone();m.pinned=row[3]=="1";
            m.has_reminder=!row[5].is_empty();m.meta=if m.has_reminder {Ui::formatted_note_date(&row[5])}else{"mm/dd/yyyy,  --:-- --".into()};
            m.extra=Ui::formatted_note_date(&row[6]);
            m.format=inline_images::saved_format(&ui.records,&m.id);
            m.placements=inline_images::placements(row,ui.records.get(&inline_images::offset_key(&row[0])));
        },
        Kind::Clip=>if let Some(r)=clip_row(ui){
            m.id=identity(ui,k);m.kind=get(r,0).to_owned();m.pinned=get(r,3)=="1";
            m.title=match m.kind.as_str(){"image"=>"Image","link"=>"Link",_=>"Text"}.into();
            let stamp=Ui::formatted_note_date(get(r,2));
            m.extra=stamp.rsplit_once(", ").map_or(stamp.as_str(),|(_,time)|time).to_owned();
            if m.kind=="image"{m.image=get(r,1).into();m.meta=format!("{} × {}",get(r,4),get(r,5));}
            else{m.body=get(r,1).into();}
        },
    }
    m
}
// Each control is a real tab stop with a Windows accessibility name.
#[derive(Clone,Copy)]struct Spec{ id:u16, label:&'static str, x:i32,y:i32,w:i32,h:i32 }
macro_rules! c {($id:literal,$label:literal,$x:literal,$y:literal,$w:literal,$h:literal)=>{
    Spec{id:$id,label:$label,x:$x,y:$y,w:$w,h:$h}
}}
const CAMERA:&[Spec]=&[c!(110,"Pause camera",140,6,28,28),c!(111,"Standby camera",171,6,28,28),c!(112,"Virtual camera settings",219,6,28,28),c!(113,"Camera settings",250,6,28,28)];
const FOCUS:&[Spec]=&[c!(201,"Open Pomodoro in Buddy",13,12,202,30),c!(202,"Start",24,143,66,27),c!(203,"Skip Pomodoro phase",95,143,32,27),c!(204,"Reset Pomodoro",132,143,32,27),
    c!(205,"25 Focus",24,175,50,20),c!(206,"5 Short",79,175,42,20),c!(207,"15 Long",126,175,46,20)];
const WATCH:&[Spec]=&[c!(301,"Open Stopwatch in Buddy",13,12,203,30),c!(302,"Start",24,143,66,27),c!(303,"Lap",95,143,50,27),c!(304,"Reset Stopwatch",150,143,32,27)];
const NOTE:&[Spec]=&[
    c!(401,"Open note in Buddy",13,12,160,33),c!(413,"More note actions",356,12,31,33),
    c!(402,"Pin note",13,55,97,33),c!(403,"Delete note",116,55,100,33),
    c!(411,"Note title",20,99,289,16),c!(412,"Note body",14,124,372,229),
    c!(404,"Reminder date and time",13,358,168,25),c!(405,"Remove reminder",186,358,65,25),
];
const CLIP:&[Spec]=&[
    c!(501,"Open clipboard in Buddy",13,12,194,33),c!(502,"Copy item",16,61,368,105),
    c!(503,"Open",280,169,24,24),c!(504,"Save PNG",305,169,24,24),
    c!(505,"Pin",330,169,24,24),c!(506,"Remove",355,169,24,24),c!(507,"Hide card",356,12,31,33),
];
fn specs(k:Kind)->&'static[Spec]{match k{Kind::Camera=>CAMERA,Kind::Focus=>FOCUS,Kind::Watch=>WATCH,Kind::Note=>NOTE,Kind::Clip=>CLIP}}

struct Thumbnail {width:i32,height:i32,bgra:Vec<u8>}
impl Thumbnail {
    fn load(ui:&Ui,id:&str)->Option<Self>{
        let rgba=workspace::widget_thumbnail(ui,id,720,400).ok()?;
        let (width,height)=(rgba.width()as i32,rgba.height()as i32);
        let mut bgra=rgba.into_raw();let field=ui.appearance.1;
        for pixel in bgra.chunks_exact_mut(4){
            let alpha=u32::from(pixel[3]);
            if alpha!=255{for (channel,value) in pixel[..3].iter_mut().enumerate(){
                let base=(field>>(channel*8))&255;*value=((u32::from(*value)*alpha+base*(255-alpha)+127)/255) as u8;
            }}
            pixel.swap(0,2);
        }
        Some(Self{width,height,bgra})
    }
}
struct Card {kind:Kind, handle:HWND,rich:Option<Controls>, controls:Vec<(Spec,HWND)>, data:Content, image:Option<Thumbnail>, unscrolled:Bounds, area:Bounds, scroll_y:i32,watch_sample:Option<(std::time::Instant,u64)>}
struct Strip {wheel_y:crate::ui::WheelMotion,wheel_x:crate::ui::WheelMotion,handle:HWND,parent:HWND,cards:Vec<Card>, viewport:Bounds, offset:i32, scale:f32, palette:(u32,u32,u32,u32),
    fonts:[HFONT;8], previous:HWND,next:HWND, shown:bool, opacity:u8, nav_bounds:[Bounds;2], notice:Option<(Kind,String,std::time::Instant)>}
struct Draft {id:String,previous_title:String,previous_body:String,previous_images:Vec<Placement>,previous_format:Option<String>,title:String,body:String,images:Vec<Placement>,format:Option<String>}
struct Event {id:u16,kind:Kind,identity:String}
thread_local! {
    static STRIP:RefCell<Option<Strip>>=const{RefCell::new(None)};
    static EVENTS:RefCell<VecDeque<Event>>=const{RefCell::new(VecDeque::new())};
    static DRAFT:RefCell<Option<Draft>>=const{RefCell::new(None)};
}

unsafe fn classes()->bool {
    let inst=GetModuleHandleW(null());
    for name in [CLASS,CARD_CLASS]{
        let wide_name=wide(name);
        let wc=WNDCLASSW{lpfnWndProc:Some(procedure),hInstance:inst,lpszClassName:wide_name.as_ptr(),
            hCursor:LoadCursorW(null_mut(),IDC_ARROW),..Default::default()};
        if RegisterClassW(&wc)==0&&GetLastError()!=ERROR_CLASS_ALREADY_EXISTS{return false;}
    }true
}
unsafe fn child(parent:HWND,class:&str,label:&str,id:u16,styles:u32,ex:u32)->HWND{
    let h=CreateWindowExW(ex,wide(class).as_ptr(),wide(label).as_ptr(),WS_CHILD|styles,0,0,1,1,parent,id as usize as _,GetModuleHandleW(null()),null());
    crate::chrome::scroll::attach(h,styles,false);
    if class == "EDIT" { crate::chrome::edit_menu::attach(h); }
    h
}
fn font(size:i32,weight:i32)->HFONT{
    let mut l=LOGFONTW::default();l.lfHeight= -size;l.lfWeight=weight;let name=wide("Segoe UI");l.lfFaceName[..name.len()].copy_from_slice(&name);unsafe{CreateFontIndirectW(&l)}
}
fn set_text(handle:HWND,value:&str){if window_text(handle,256*1024)!=value{unsafe{SetWindowTextW(handle,wide(value).as_ptr());}}}
fn map_dip(value:i32,scale:f32)->i32{(value as f32*scale).round()as i32}
fn rounded_region(b:Bounds,radius:i32)->HRGN{unsafe{
    // The original cards meet the display's top edge and round only the bottom corners.
    let r=CreateRoundRectRgn(b.x,b.y,b.right(),b.bottom(),radius,radius);
    if r.is_null(){return r;}let top=CreateRectRgn(b.x,b.y,b.right(),b.y+(radius/2).min(b.h));
    if !top.is_null(){CombineRgn(r,r,top,RGN_OR);DeleteObject(top);}r
}}
impl Strip{
    unsafe fn new(owner:HWND)->Option<Self>{
        if !classes(){return None}
        let h=CreateWindowExW(WS_EX_TOOLWINDOW|WS_EX_TOPMOST|WS_EX_CONTROLPARENT,wide(CLASS).as_ptr(),wide("Buddy side widgets").as_ptr(),
            WS_POPUP|WS_CLIPCHILDREN,0,0,1,1,owner,null_mut(),GetModuleHandleW(null()),null());
        if h.is_null(){return None;}
        let arrow=|id,label|child(h,"BUTTON",label,id,WS_TABSTOP|BS_OWNERDRAW as u32,0);
        let prev=arrow(PREVIOUS,"Previous widgets");let next=arrow(NEXT,"Next widgets");
        Some(Self{wheel_y:Default::default(),wheel_x:Default::default(),handle:h,parent:owner,previous:prev,next,cards:vec![],viewport:Bounds::default(),offset:0,scale:0.0,
            palette:(0,0,0,0),fonts:[null_mut();8],shown:false,opacity:255,nav_bounds:[Bounds::default();2],notice:None})
    }
    unsafe fn set_opacity(&mut self,alpha:u8){
        if self.opacity==alpha{return}
        let style=GetWindowLongW(self.handle,GWL_EXSTYLE) as u32;
        if alpha<255 {
            if style&WS_EX_LAYERED==0{SetWindowLongW(self.handle,GWL_EXSTYLE,(style|WS_EX_LAYERED) as i32);}
            SetLayeredWindowAttributes(self.handle,0,alpha,LWA_ALPHA);
        }else if style&WS_EX_LAYERED!=0{
            SetWindowLongW(self.handle,GWL_EXSTYLE,(style&!WS_EX_LAYERED) as i32);
            RedrawWindow(self.handle,null(),null_mut(),RDW_INVALIDATE|RDW_ALLCHILDREN);
        }
        self.opacity=alpha;
    }
    unsafe fn add_card(&mut self,kind:Kind)->Option<Card>{
        let h=child(self.handle,CARD_CLASS,match kind{Kind::Camera=>"Camera call controls",Kind::Focus=>"Pomodoro side widget",Kind::Watch=>"Stopwatch side widget",Kind::Note=>"Note side widget",Kind::Clip=>"Clipboard side widget"},0,WS_CLIPCHILDREN,WS_EX_CONTROLPARENT);
        if h.is_null(){return None}
        let rich=if kind==Kind::Note{match Controls::new(h){Ok(editor)=>Some(editor),Err(_)=>{DestroyWindow(h);return None}}}else{None};
        let controls=specs(kind).iter().map(|&s|{
            if s.id==BODY{return (s,rich.as_ref().unwrap().editor);}
            let edit=matches!(s.id,TITLE|BODY|508);
            let styles=if !edit{BS_OWNERDRAW as u32}else if s.id==508{(ES_READONLY|ES_MULTILINE|ES_AUTOVSCROLL)as u32|WS_VSCROLL}else if s.id==BODY{(ES_MULTILINE|ES_WANTRETURN|ES_AUTOVSCROLL)as u32|WS_VSCROLL}else{ES_AUTOHSCROLL as u32};
            let btn=child(h,if edit{"EDIT"}else{"BUTTON"},if edit{""}else{s.label},s.id,WS_TABSTOP|styles,if s.id==508{WS_EX_CLIENTEDGE}else{0});
            SendMessageW(btn,WM_SETFONT,self.fonts[if s.id==TITLE{4}else{0}]as usize,0);
            if !edit{crate::chrome::install_button(btn);}
            if s.id==502{windows_sys::Win32::UI::Shell::SetWindowSubclass(btn,Some(clip_preview_proc),0xB065,0);}
            if edit{SendMessageW(btn,0x00c5,if s.id==TITLE{32768}else{256*1024},0);SendMessageW(btn,0x1501,1,wide(if s.id==TITLE{"Untitled note"}else{s.label}).as_ptr()as isize);}
            (s,btn)
        }).collect();
        Some(Card{kind,handle:h,rich,controls,data:Content::default(),image:None,unscrolled:Bounds::default(),area:Bounds::default(),scroll_y:0,watch_sample:None})
    }
    fn max_offset(&self)->i32{self.cards.last().map_or(0,|c|(c.unscrolled.right()-self.viewport.w).max(0))}
    unsafe fn update(&mut self,ui:&Ui,view:Bounds,items:&[(Widget,RECT)]){
        let (scale,_)=island::scale_and_limit();
        let restyle=self.scale!=scale||self.palette!=ui.appearance;
        if restyle{
            self.scale=scale;self.palette=ui.appearance;
            let fresh=[font(map_dip(13,scale),500),font(map_dip(18,scale),650),font(map_dip(42,scale),660),font(map_dip(9,scale),400),font(map_dip(11,scale),650),font(map_dip(16,scale),400),font(map_dip(11,scale),500),font(map_dip(8,scale),400)];
            for card in &self.cards{for &(spec,h) in &card.controls{if spec.id!=BODY{SendMessageW(h,WM_SETFONT,fresh[if spec.id==TITLE{4}else{0}]as usize,0);}}}
            for h in [self.previous,self.next]{SendMessageW(h,WM_SETFONT,fresh[1]as usize,0);}
            for old in std::mem::replace(&mut self.fonts,fresh){if !old.is_null(){DeleteObject(old);}}
        }
        let mut geometry=restyle||self.viewport!=view||!self.shown||items.len()!=self.cards.len();
        self.viewport=view;
        let mut old=std::mem::take(&mut self.cards);
        for (w,r) in items{
            let k=Kind::from(w);
            let mut c=if let Some(i)=old.iter().position(|c|c.kind==k){old.remove(i)}else{let Some(c)=self.add_card(k)else{continue};geometry=true;c};
            let mut b=Bounds::rect(r);b.x-=view.x;b.y-=view.y;
            if b!=c.unscrolled{c.unscrolled=b;geometry=true;}
            let data=model(ui,k);
            if k==Kind::Clip && data.kind!=c.data.kind{geometry=true;}
            if k==Kind::Watch {
                let active=data.running && b.intersect(Bounds{x:self.offset,y:0,w:view.w,h:view.h}).is_some();
                if active {
                    if c.watch_sample.is_none(){SetTimer(c.handle,3,100,None);}
                    c.watch_sample=Some((std::time::Instant::now(),core::timers(ui).1));
                }else if c.watch_sample.take().is_some(){KillTimer(c.handle,3);}
            }
            if data!=c.data||restyle{
                if data.image!=c.data.image || (restyle && !data.image.is_empty()){c.image=if data.image.is_empty(){None}else{Thumbnail::load(ui,&data.image)};}
                if k==Kind::Note{
                    let pending=DRAFT.with(|d|d.borrow().as_ref().is_some_and(|s|s.id==data.id));
                    if !pending{
                        set_text(GetDlgItem(c.handle,TITLE as i32),&data.title);
                        if let (Some(editor),Some(row))=(&c.rich,note_row(ui)){
                            let matches=editor.contents().is_ok_and(|(s,p)|s==data.body&&p==data.placements);
                            if !matches || (c.data.format!=data.format && editor.format().ok().as_ref()!=Some(&data.format)) {if let Err(e)=editor.load_row(ui,row){self.notice=Some((k,e,std::time::Instant::now()));}}
                        }
                    }
                }
                c.data=data;
                for &(s,h)in &c.controls{
                    let (caption,enabled)=button(&c,s);
                    if s.id==508{set_text(h,&c.data.body);}else if !matches!(s.id,TITLE|BODY){set_text(h,caption);}
                    EnableWindow(h,enabled as i32);
                    ShowWindow(h,if caption.is_empty()&&!matches!(s.id,TITLE|BODY){SW_HIDE}else{SW_SHOWNA});
                    InvalidateRect(h,null(),0);
                }
                InvalidateRect(c.handle,null(),0);
            }
            self.cards.push(c);
        }
        for c in old{DestroyWindow(c.handle);}
        if let Some((k,_,t))=&self.notice{if t.elapsed().as_secs()>=4{if let Some(c)=self.cards.iter().find(|c|c.kind==*k){InvalidateRect(c.handle,null(),0);}self.notice=None;}}
        if geometry{self.layout();}
        let mut p=POINT{x:view.x,y:view.y};ClientToScreen(ui.hwnd.0,&mut p);
        let mut actual=RECT::default();GetWindowRect(self.handle,&mut actual);
        if geometry||actual.left!=p.x||actual.top!=p.y{
            SetWindowPos(self.handle,HWND_TOPMOST,p.x,p.y,view.w,view.h,SWP_NOACTIVATE|SWP_SHOWWINDOW);
        }
        self.shown=true;
    }
    unsafe fn layout(&mut self){
        self.offset=self.offset.clamp(0,self.max_offset());
        let viewport=Bounds{x:0,y:0,w:self.viewport.w,h:self.viewport.h};
        let region=CreateRectRgn(0,0,0,0);
        let clip=CreateRectRgn(0,0,viewport.w,viewport.h);
        let p=|v|map_dip(v,self.scale);
        let mut visible=Vec::new();
        for card in &mut self.cards{
            let mut b=card.unscrolled;b.x-=self.offset;card.area=b;
            let natural_h=if card.kind==Kind::Camera{40}else{400};
            card.scroll_y=card.scroll_y.clamp(0,(p(natural_h)-b.h).max(0));
            MoveWindow(card.handle,b.x,b.y,b.w,b.h,0);
            let intersection=b.intersect(viewport);
            ShowWindow(card.handle,if intersection.is_some(){SW_SHOWNA}else{SW_HIDE});
            if let Some(i)=intersection{
                visible.push((card.kind,i));
                let r=rounded_region(b,p(if card.kind==Kind::Camera{32}else{40}));
                if !r.is_null(){CombineRgn(region,region,r,RGN_OR);DeleteObject(r);}
            }
            for &(s,h) in &card.controls{
                let y=p(s.y)-card.scroll_y;
                if s.id==BODY {if let Some(rich)=&card.rich{
                    let dpi=crate::shell::dpi(card.handle) as f32/96.0;
                    rich.note_layout(true,crate::chrome::rect(p(s.x),y,p(s.w),p(s.h)),(100.0*self.scale/dpi).round() as _,self.palette,13);
                    continue;
                }}
                let x=if s.id==503 && card.data.kind!="image"{305}else{s.x};
                MoveWindow(h,p(x),y,p(s.w),p(s.h),0);
                if s.id==502{
                    let rounded=CreateRoundRectRgn(0,0,p(s.w)+1,p(s.h)+p(15),p(26),p(26));
                    let clip=CreateRectRgn(0,0,p(s.w),p(s.h));
                    if !rounded.is_null() && !clip.is_null(){CombineRgn(rounded,rounded,clip,RGN_AND);}
                    if !clip.is_null(){DeleteObject(clip);}
                    if !rounded.is_null() && SetWindowRgn(h,rounded,0)==0{DeleteObject(rounded);}
                }
            }
            InvalidateRect(card.handle,null(),0);
        }
        CombineRgn(region,region,clip,RGN_AND);DeleteObject(clip);
        let overflow=self.max_offset()>0;
        // Place the two page buttons inside an actual card, so gaps remain click-through.
        let nav=visible.iter().filter(|(k,r)|*k!=Kind::Camera&&r.w>=p(85))
            .max_by_key(|(_,r)|r.w).or_else(||visible.iter().filter(|(_,r)|r.w>=p(54)).max_by_key(|(_,r)|r.w));
        for(i,(h,enabled))in [(self.previous,self.offset>0),(self.next,self.offset<self.max_offset())].into_iter().enumerate(){
            if overflow&&nav.is_some(){
                let (k,r)=nav.unwrap();let size=p(if *k==Kind::Camera{22}else{26});
                let (x,y)=if *k==Kind::Camera{
                    (r.x+p(4)+size*i as i32,r.y)
                }else{
                    (r.right()-p(6)-size*(2-i as i32),(r.bottom()-p(35)).max(r.y))
                };
                self.nav_bounds[i]=Bounds{x,y,w:size,h:size};
                SetWindowPos(h,HWND_TOP,x,y,size,size,SWP_NOACTIVATE|SWP_SHOWWINDOW);
                EnableWindow(h,enabled as i32);
            }else{self.nav_bounds[i]=Bounds::default();ShowWindow(h,SW_HIDE);}
        }
        if SetWindowRgn(self.handle,region,1)==0{DeleteObject(region);}
        InvalidateRect(self.handle,null(),0);
    }
    unsafe fn horizontal(&mut self,direction:i32){
        let step=(self.viewport.w-map_dip(32,self.scale)).max(map_dip(60,self.scale));
        self.offset=(self.offset+step*direction.signum()).clamp(0,self.max_offset());self.layout();
    }
    unsafe fn ensure(&mut self,index:usize,spec:Spec){
        let c=&mut self.cards[index];let p=|v|map_dip(v,self.scale);
        let x=c.unscrolled.x+p(spec.x);let right=x+p(spec.w);
        if x<self.offset{self.offset=x;}else if right>self.offset+self.viewport.w{self.offset=right-self.viewport.w;}
        let y=p(spec.y);let bottom=y+p(spec.h);
        if y<c.scroll_y{c.scroll_y=y;}else if bottom>c.scroll_y+c.area.h{c.scroll_y=bottom-c.area.h;}
        self.layout();
    }
}
fn button<'a>(c:&Card,s:Spec)->(&'a str,bool){
    let d=&c.data;
    match s.id{
        110=>(if d.running{"Pause camera"}else{"Resume live camera"},d.actions_enabled),
        111=>(s.label,d.actions_enabled&&d.kind!="standby"),
        202|302=>(if d.running{"Pause"}else{"Start"},true),303=>("Lap",d.running),
        402=>(if d.pinned{"Unpin note"}else{"Pin note"},true),
        405=>(if d.has_reminder{"Remove reminder"}else{""},d.has_reminder),
        505=>(if d.pinned{"Unpin"}else{"Pin"},true),
        502=>(match d.kind.as_str(){"image"=>"Copy image","link"=>"Open link",_=>"Copy text"},true),
        503=>(match d.kind.as_str(){"image"=>"Open image","link"=>"Copy link",_=>"Rewrite text"},true),
        504=>(if d.kind=="image"{"Save PNG"}else{""},d.kind=="image"),
        506=>(s.label,!d.pinned),508=>(if d.kind=="image"{""}else{s.label},d.kind!="image"),
        _=>(s.label,true),
    }
}

pub fn set_opacity(alpha:u8){
    STRIP.with(|s|if let Ok(mut state)=s.try_borrow_mut(){
        if let Some(strip)=state.as_mut().filter(|strip|strip.shown){unsafe{strip.set_opacity(alpha);}}
    });
}
pub fn sync(ui:&mut Ui){
    let geometry=island::frame();
    let view=Bounds::rect(&geometry.widget_view);
    if ui.expanded||ui.locked||ui.paused||ui.suppressed||ui.hidden_until.is_some()||geometry.widgets.is_empty()||view.w<1||view.h<1{
        let _=flush(ui);
        STRIP.with(|s|{
            if let Ok(mut state)=s.try_borrow_mut(){if let Some(strip)=state.as_mut(){if strip.shown{unsafe{
                    ShowWindow(strip.handle,SW_HIDE);
                    strip.set_opacity(255);
                    for c in &mut strip.cards{if c.watch_sample.take().is_some(){KillTimer(c.handle,3);}}
                }strip.shown=false;}}}
        });
        return;
    }
    STRIP.with(|s|{
        let Ok(mut b)=s.try_borrow_mut()else{return};
        if b.as_ref().is_some_and(|s|s.parent!=ui.hwnd.0){if let Some(old)=b.take(){destroy(old);}}
        if b.is_none(){*b=unsafe{Strip::new(ui.hwnd.0)};}
        if let Some(strip)=b.as_mut(){unsafe{strip.set_opacity(geometry.widgets_alpha);strip.update(ui,view,&geometry.widgets);}}
    });
}
fn destroy(s:Strip){unsafe{DestroyWindow(s.handle);for f in s.fonts{if !f.is_null(){DeleteObject(f);}}}}
pub fn close(){STRIP.with(|s|{if let Some(strip)=s.borrow_mut().take(){destroy(strip);}});EVENTS.with(|e|e.borrow_mut().clear());}
pub fn contains_screen_point(x:i32,y:i32)->bool{
    STRIP.with(|s|{let Ok(b)=s.try_borrow()else{return false};let Some(strip)=b.as_ref().filter(|s|s.shown)else{return false};
        let mut p=POINT{x,y};unsafe{ScreenToClient(strip.handle,&mut p);}
        if !(Bounds{x:0,y:0,w:strip.viewport.w,h:strip.viewport.h}).contains(p.x,p.y){return false;}
        strip.cards.iter().any(|c|c.area.contains(p.x,p.y)&&unsafe{
            let r=rounded_region(c.area,map_dip(if c.kind==Kind::Camera{32}else{40},strip.scale));
            let hit=!r.is_null()&&PtInRegion(r,p.x,p.y)!=0;if !r.is_null(){DeleteObject(r);}hit
        })
    })
}

fn mix(a:u32,b:u32,percent:u32)->u32{
    [0,8,16].into_iter().map(|shift|((((a>>shift)&255)*(100-percent)+((b>>shift)&255)*percent)/100)<<shift).sum()
}
unsafe fn fill(dc:HDC,b:Bounds,color:u32,r:i32){
    SetDCBrushColor(dc,color);
    if r<=0{FillRect(dc,&b.win(),GetStockObject(DC_BRUSH));}else{
        let old_b=SelectObject(dc,GetStockObject(DC_BRUSH));let old_p=SelectObject(dc,GetStockObject(NULL_PEN));
        RoundRect(dc,b.x,b.y,b.right(),b.bottom(),r,r);SelectObject(dc,old_p);SelectObject(dc,old_b);
    }
}
unsafe fn print(dc:HDC,text:&str,mut r:RECT,font:HFONT,color:u32,flags:u32){
    if text.is_empty(){return;}
    let mut value=wide(text);let previous=SelectObject(dc,font);SetBkMode(dc,TRANSPARENT as i32);SetTextColor(dc,color);
    DrawTextW(dc,value.as_mut_ptr(),-1,&mut r,flags|DT_NOPREFIX);SelectObject(dc,previous);
}
unsafe fn panel(dc:HDC,r:Bounds,face:u32,edge:u32,rad:i32){
    let pen=SelectObject(dc,GetStockObject(DC_PEN));let brush=SelectObject(dc,GetStockObject(DC_BRUSH));
    SetDCBrushColor(dc,face);SetDCPenColor(dc,edge);RoundRect(dc,r.x,r.y,r.right(),r.bottom(),2*rad,2*rad);
    SelectObject(dc,brush);SelectObject(dc,pen);
}
unsafe fn text_width(dc:HDC,value:&str,font:HFONT)->i32{
    let chars=wide(value);let old=SelectObject(dc,font);let mut size=windows_sys::Win32::Foundation::SIZE::default();
    GetTextExtentPoint32W(dc,chars.as_ptr(),chars.len()as i32-1,&mut size);SelectObject(dc,old);size.cx
}
unsafe fn symbol(dc:HDC,r:Bounds,id:u16,color:u32){
    let (x,y)=(|n|r.x+(n*r.w+12)/24,|n|r.y+(n*r.h+12)/24);
    let ink=CreatePen(PS_SOLID,((r.w+2)/10).max(1),color);let old=SelectObject(dc,ink);let brush=SelectObject(dc,GetStockObject(NULL_BRUSH));
    let path=|points:&[(i32,i32)]|{if let Some(&(a,b))=points.first(){MoveToEx(dc,x(a),y(b),null_mut());for &(c,d)in &points[1..]{LineTo(dc,x(c),y(d));}}};
    match id{
        510=>{Rectangle(dc,x(3),y(3),x(22),y(22));Ellipse(dc,x(7),y(6),x(12),y(11));path(&[(4,19),(10,13),(13,16),(17,11),(21,15)]);}
        511=>{path(&[(14,5),(17,2),(22,4),(23,8),(18,13),(14,14)]);path(&[(10,19),(7,22),(2,20),(1,16),(6,11),(10,10)]);path(&[(8,16),(16,8)]);}
        512=>{path(&[(4,7),(4,4),(20,4),(20,7)]);path(&[(12,4),(12,21)]);path(&[(8,21),(16,21)]);}
        109=>path(&[(5,3),(21,12),(5,21),(5,3)]),
        110=>{path(&[(7,4),(7,20)]);path(&[(17,4),(17,20)]);}
        111=>{Rectangle(dc,x(2),y(4),x(22),y(18));path(&[(8,21),(16,21)]);path(&[(9,8),(9,14)]);path(&[(15,8),(15,14)]);}
        112=>{RoundRect(dc,x(1),y(6),x(16),y(19),3,3);path(&[(16,10),(22,6),(22,18),(16,14)]);}
        113=>{path(&[(2,7),(7,7),(9,4),(15,4),(17,7),(22,7),(22,20),(2,20),(2,7)]);Ellipse(dc,x(9),y(9),x(16),y(16));}
        201=>{Ellipse(dc,x(4),y(6),x(20),y(22));path(&[(9,2),(15,2)]);path(&[(12,14),(15,10)]);}
        301=>{path(&[(3,19),(2,16),(2,12),(3,9),(6,6),(9,5),(14,5),(18,7),(21,10),(22,14),(21,18),(20,19)]);path(&[(12,15),(17,10)]);}
        203=>{path(&[(4,4),(17,12),(4,20),(4,4)]);path(&[(20,4),(20,20)]);}
        204|304=>{path(&[(4,9),(7,5),(10,4),(14,4),(18,6),(20,9),(21,13),(19,17),(16,20),(11,21),(7,19),(4,15)]);path(&[(3,3),(3,9),(10,9)]);}
        303=>{path(&[(4,21),(4,3),(9,3),(15,6),(21,4),(21,15),(16,16),(9,13),(4,14)]);}
        _=>{}
    }
    SelectObject(dc,brush);SelectObject(dc,old);DeleteObject(ink);
}
unsafe fn paint_card(strip:&Strip,c:&Card,dc:HDC){
    let (surface,field,text,accent)=strip.palette;
    let p=|v|map_dip(v,strip.scale);
    let rect=|x,y,w,h|Bounds{x:p(x),y:p(y)-c.scroll_y,w:p(w),h:p(h)};
    let bounds=Bounds{x:0,y:0,w:c.area.w,h:c.area.h};
    fill(dc,bounds,surface,0);
    let subtle=mix(text,surface,30);
    let line=|s:&str,x,y,w,h,font:usize,color:u32|print(dc,s,rect(x,y,w,h).win(),strip.fonts[font],color,DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS);
    let content=&c.data;
    let edge=mix(text,surface,83);
    panel(dc,Bounds{x:0,y:-p(22),w:bounds.w,h:bounds.h+p(22)},surface,edge,p(20));
    if c.kind==Kind::Camera{
        symbol(dc,rect(9,12,16,16),112,subtle);
        line("Camera",31,0,53,40,0,subtle);
        fill(dc,rect(85,19,4,4),accent,p(5));line(&content.meta,94,0,46,40,3,accent);
        fill(dc,rect(210,11,1,18),edge,0);
    }else{
        match c.kind{
            Kind::Focus|Kind::Watch=>{
                let lap_count=if c.kind==Kind::Watch&&content.extra!="0 laps"{content.body.lines().count().min(3)as i32}else{0};
                let inner_height=if c.kind==Kind::Focus{153}else{147+(lap_count-1).max(0)*14};
                panel(dc,rect(13,52,374,inner_height),field,edge,p(9));
                symbol(dc,rect(24,65,18,18),if c.kind==Kind::Focus{201}else{301},accent);
                line(&content.title,49,59,194,16,4,text);line(&content.meta,49,74,180,14,3,subtle);
                print(dc,&content.extra,rect(244,59,131,30).win(),strip.fonts[3],subtle,DT_RIGHT|DT_VCENTER|DT_SINGLELINE);
                let ticking=c.watch_sample.map(|(start,value)|time(value.saturating_add(start.elapsed().as_millis()as u64),true));
                let readout=ticking.as_deref().unwrap_or(&content.readout);
                line(readout,24,86,341,54,2,text);
                if c.kind==Kind::Focus{
                    let x=p(24)+text_width(dc,readout,strip.fonts[2])+p(10);
                    let bar=Bounds{x,y:p(113)-c.scroll_y,w:(p(375)-x).max(1),h:p(3).max(1)};
                    fill(dc,bar,mix(text,field,94),p(3));
                    if content.progress>0 {fill(dc,Bounds{w:bar.w*i32::from(content.progress)/100,..bar},accent,p(3));}
                }else if lap_count==0{
                    print(dc,&content.body,rect(24,174,350,17).win(),strip.fonts[3],subtle,DT_CENTER|DT_SINGLELINE|DT_VCENTER);
                }else{
                    for(i,row)in content.body.lines().enumerate(){
                        let y=174+i as i32*14;fill(dc,rect(24,y,350,1),edge,0);
                        let (lap,elapsed)=row.split_once("     ").unwrap_or((row,""));line(lap,24,y+1,150,13,3,subtle);
                        print(dc,elapsed,rect(204,y+1,170,13).win(),strip.fonts[3],text,DT_RIGHT|DT_SINGLELINE|DT_VCENTER);
                    }
                }
            }
            Kind::Note=>{
                let face=crate::chrome::control(strip.palette);
                let focused=GetFocus();
                panel(dc,rect(13,94,301,25),face,if focused==GetDlgItem(c.handle,TITLE as i32){accent}else{edge},p(5));
                panel(dc,rect(13,123,374,231),face,if c.rich.as_ref().is_some_and(|r|r.editor==focused){accent}else{edge},p(6));
                print(dc,&content.extra,rect(318,94,69,25).win(),strip.fonts[3],subtle,DT_RIGHT|DT_VCENTER|DT_SINGLELINE|DT_END_ELLIPSIS);
            }
            Kind::Clip=>{
                panel(dc,rect(15,60,370,139),field,if content.pinned{crate::chrome::mix(surface,accent,105)}else{edge},p(14));
                let indicator=rect(26,176,11,11);let kind=match content.kind.as_str(){"image"=>510,"link"=>511,_=>512};
                symbol(dc,indicator,kind,subtle);
                line(&content.title,40,173,26,16,7,subtle);
                line(&content.extra,68,173,105,16,7,subtle);
            }
            Kind::Camera=>{}
        }
    }
    if let Some((kind,message,t))=&strip.notice{if *kind==c.kind&&t.elapsed().as_secs()<4{
        if c.kind==Kind::Camera{line(message,6,20,94,18,0,accent);}else if c.kind==Kind::Note{line(message,14,383,330,15,3,accent);}else if c.kind==Kind::Clip{line(message,16,203,363,19,3,accent);}else{line(message,18,366,280,18,0,accent);}
    }}
}
// The entire thumbnail (or text excerpt) is the primary accessible button.
unsafe extern "system" fn clip_preview_proc(h:HWND,msg:u32,wp:usize,lp:isize,sub:usize,_:usize)->isize{
    use windows_sys::Win32::UI::Shell::{DefSubclassProc,RemoveWindowSubclass};
    if msg==WM_CONTEXTMENU{SendMessageW(GetParent(h),WM_COMMAND,509,h as _);return 0;}
    if msg==WM_NCDESTROY{RemoveWindowSubclass(h,Some(clip_preview_proc),sub);}
    DefSubclassProc(h,msg,wp,lp)
}

unsafe fn draw_clip_item(strip:&Strip,card:&Card,d:&DRAWITEMSTRUCT){
    use crate::chrome::{self,Icon};
    let id=d.CtlID as u16;let dc=d.hDC;let r=Bounds::rect(&d.rcItem);
    let (bg,field,ink,accent)=strip.palette;let p=|v|map_dip(v,strip.scale);
    let muted=chrome::mix(bg,ink,162);let edge=chrome::mix(bg,ink,42);
    let focus=d.itemState&ODS_FOCUS!=0 && d.itemState&windows_sys::Win32::UI::Controls::ODS_NOFOCUSRECT==0;
    let hot=chrome::hovered(d.hwndItem)||d.itemState&ODS_SELECTED!=0;
    let enabled=IsWindowEnabled(d.hwndItem)!=0;
    if id==501||id==507{
        fill(dc,r,bg,0);
        if id==507 && !hot && !focus{return;}
        let face=if hot{chrome::mix(bg,ink,18)}else{chrome::control(strip.palette)};
        panel(dc,r,face,if focus{accent}else{edge},p(6));
        if id==501{print(dc,"Open clipboard in Buddy",r.win(),strip.fonts[5],ink,DT_SINGLELINE|DT_CENTER|DT_VCENTER);}
        else{chrome::icon(dc,(r.w-p(16))/2,(r.h-p(16))/2,p(16),Icon::Close,muted,face);}
        return;
    }
    if id==502{
        // WM_PRINT can include the square child rectangle outside its shaped
        // HWND. Restore the exact card edge below those transparent corners.
        fill(dc,r,bg,0);
        panel(dc,Bounds{x:-p(1),y:-p(1),w:p(370),h:p(139)},field,if card.data.pinned{chrome::mix(bg,accent,105)}else{edge},p(14));
        // Clip only the top corners. The lower image edge meets the flat action bar.
        let saved=SaveDC(dc);
        // GDI clipping regions use device coordinates. WM_PRINT offsets the
        // viewport into a parent bitmap, whereas live button DCs start at zero.
        let mut clip=[POINT{x:r.x,y:r.y},POINT{x:r.right()+1,y:r.bottom()+p(15)}];
        LPtoDP(dc,clip.as_mut_ptr(),2);
        let box_=CreateRoundRectRgn(clip[0].x,clip[0].y,clip[1].x,clip[1].y,p(26),p(26));
        if !box_.is_null(){ExtSelectClipRgn(dc,box_,RGN_AND);DeleteObject(box_);}
        let face=if hot&&card.data.kind!="image"{chrome::mix(field,ink,12)}else{field};
        fill(dc,r,face,0);
        if let Some(image)=&card.image{
            let scale=(f64::from(r.w)/f64::from(image.width)).max(f64::from(r.h)/f64::from(image.height));
            let (w,h)=((f64::from(image.width)*scale).ceil() as i32,(f64::from(image.height)*scale).ceil() as i32);
            let mut info=BITMAPINFO::default();info.bmiHeader=BITMAPINFOHEADER{biSize:std::mem::size_of::<BITMAPINFOHEADER>()as u32,
                biWidth:image.width,biHeight:-image.height,biPlanes:1,biBitCount:32,..Default::default()};
            SetStretchBltMode(dc,HALFTONE);StretchDIBits(dc,(r.w-w)/2,(r.h-h)/2,w,h,0,0,image.width,image.height,
                image.bgra.as_ptr().cast(),&info,DIB_RGB_COLORS,SRCCOPY);
        }else if card.data.kind=="image"{
            symbol(dc,Bounds{x:(r.w-p(20))/2,y:(r.h-p(20))/2,w:p(20),h:p(20)},510,muted);
        }else{
            let link=card.data.kind=="link";
            let area=Bounds{x:p(10),y:p(9),w:r.w-p(20),h:p(if link{60}else{90})};
            let flags=DT_LEFT|DT_WORDBREAK|DT_EDITCONTROL|DT_END_ELLIPSIS;
            print(dc,&card.data.body,area.win(),strip.fonts[if link{4}else{6}],ink,flags);
            if link{
                let mut measure=area.win();let txt=wide(&card.data.body);let old=SelectObject(dc,strip.fonts[4]);
                DrawTextW(dc,txt.as_ptr(),-1,&mut measure,DT_LEFT|DT_WORDBREAK|DT_CALCRECT|DT_NOPREFIX);SelectObject(dc,old);
                let y=area.y+(measure.bottom-measure.top).min(area.h)+p(7);
                print(dc,&card.data.body,Bounds{x:area.x,y,w:area.w,h:p(15)}.win(),strip.fonts[3],accent,DT_LEFT|DT_SINGLELINE|DT_END_ELLIPSIS);
            }
        }
        if focus{let mut rr=r.win();InflateRect(&mut rr,-p(3),-p(3));DrawFocusRect(dc,&rr);}
        RestoreDC(dc,saved);
        return;
    }
    let pinned=id==505 && card.data.pinned;
    let face=if pinned{chrome::mix(field,accent,30)}else if hot&&enabled{chrome::mix(field,ink,14)}else{field};
    let color=if !enabled{chrome::mix(field,muted,94)}else if pinned{accent}else if hot{ink}else{muted};
    fill(dc,r,field,0);panel(dc,r,face,if focus{accent}else{face},p(6));
    let glyph=match id{503 if card.data.kind=="link"=>Icon::Clipboard,503 if card.data.kind=="text"=>Icon::Pencil,503=>Icon::External,504=>Icon::Save,505=>Icon::Pin,_=>Icon::Close};
    chrome::icon(dc,(r.w-p(14))/2,(r.h-p(14))/2,p(14),glyph,color,face);
}

unsafe fn owner_draw(d:&DRAWITEMSTRUCT){
    let h=d.hwndItem;STRIP.with(|slot|{let Ok(b)=slot.try_borrow()else{return};let Some(s)=b.as_ref()else{return};
        let (surface,field,text,accent)=s.palette;let r=Bounds::rect(&d.rcItem);let p=|v|map_dip(v,s.scale);
        let id=d.CtlID as u16;let primary=matches!(id,202|302);
        let enabled=IsWindowEnabled(h)!=0;let hot=crate::chrome::hovered(h);let active=d.itemState&ODS_SELECTED!=0;
        let focus=d.itemState&ODS_FOCUS!=0;
        if (501..=507).contains(&id){
            if let Some(card)=s.cards.iter().find(|c|c.kind==Kind::Clip){draw_clip_item(s,card,d);}
            return;
        }
        if matches!(id,401..=405|413){
            let muted=crate::chrome::mix(surface,text,162);
            let edge=crate::chrome::mix(surface,text,42);
            let face=if hot||active{crate::chrome::mix(surface,text,19)}else{crate::chrome::control(s.palette)};
            fill(d.hDC,r,surface,0);
            if id==413 && !hot && !focus{return;}
            panel(d.hDC,r,face,if focus{accent}else{edge},p(6));
            if matches!(id,404|405|413){
                use crate::chrome::{icon,Icon};
                let size=p(14);let cy=(r.h-size)/2;
                let glyph=match id{404=>Icon::Bell,405=>Icon::BellOff,_=>Icon::More};
                icon(d.hDC,p(7),cy,size,glyph,if id==404{accent}else{muted},face);
                let info=if id==404{s.cards.iter().find(|c|c.kind==Kind::Note).map(|c|c.data.meta.as_str()).unwrap_or("")}
                    else if id==405{"Remove"}else{""};
                print(d.hDC,info,Bounds{x:p(27),y:0,w:r.w-p(if id==404{47}else{27}),h:r.h}.win(),s.fonts[3],muted,DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS);
                if id==404{icon(d.hDC,r.w-p(19),(r.h-p(12))/2,p(12),Icon::Calendar,muted,face);}
            } else {
                print(d.hDC,&window_text(h,100),r.win(),s.fonts[5],text,DT_CENTER|DT_SINGLELINE|DT_VCENTER);
            }
            return;
        }
        let caption=match id{PREVIOUS=>"‹".into(),NEXT=>"›".into(),_=>window_text(h,100)};
        let fg=if !enabled{mix(text,field,67)}else if primary{if (accent&255)*299+((accent>>8)&255)*587+((accent>>16)&255)*114<149000{0xffffff}else{0x171307}}else{text};
        let outer=matches!(id,201|301|401|501);
        let preset=if matches!(id,205|206|207){s.cards.iter().find(|c|c.kind==Kind::Focus).is_some_and(|c|
            c.data.meta==match id{205=>"Focus",206=>"Short break",_=>"Long break"})}else{false};
        let face=if active{mix(accent,field,63)}else if primary{accent}else if preset{mix(accent,field,93)}else if hot{mix(text,field,93)}else if outer{if surface&255>150{0xffffff}else{field}}else{field};
        let edge=if primary{accent}else if preset{mix(accent,field,52)}else{mix(text,field,82)};
        fill(d.hDC,r,if outer{surface}else{field},0);
        if matches!(id,110..=113){symbol(d.hDC,Bounds{x:r.x+(r.w-p(15))/2,y:r.y+(r.h-p(15))/2,w:p(15),h:p(15)},
            if id==110&&caption.starts_with("Resume"){109}else{id},fg);}
        else{
            panel(d.hDC,r,face,edge,p(if matches!(id,205..=207){5}else{6}));
            if matches!(id,203|204|304){symbol(d.hDC,Bounds{x:(r.w-p(15))/2,y:(r.h-p(15))/2,w:p(15),h:p(15)},id,fg);}
            else if primary||id==303{
                let font=s.fonts[3];let size=p(15);let w=text_width(d.hDC,&caption,font);
                let x=(r.w-size-p(5)-w)/2;symbol(d.hDC,Bounds{x,y:(r.h-size)/2,w:size,h:size},if primary{if caption=="Pause"{110}else{109}}else{303},fg);
                print(d.hDC,&caption,Bounds{x:x+size+p(5),y:0,w:w+p(2),h:r.h}.win(),font,fg,DT_SINGLELINE|DT_VCENTER);
            }else{let font=if outer{s.fonts[5]}else if matches!(id,205..=207){s.fonts[3]}else{s.fonts[0]};
                print(d.hDC,&caption,r.win(),font,if preset{accent}else{fg},DT_CENTER|DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS);}
        }
        if d.itemState&ODS_FOCUS!=0{let mut rect=r.win();InflateRect(&mut rect,-2,-2);DrawFocusRect(d.hDC,&rect);}
    });
}
unsafe fn read_editor(h:HWND,debounce:bool){
    STRIP.with(|slot|{
        let Ok(b)=slot.try_borrow()else{return};let Some(strip)=b.as_ref()else{return};
        let Some(note)=strip.cards.iter().find(|c|c.handle==h&&c.kind==Kind::Note)else{return};
        if note.data.id.is_empty(){return}
        let title=window_text(GetDlgItem(h,TITLE as i32),32768);
        let Some(editor)=&note.rich else{return};
        let Ok((body,images))=editor.contents()else{return};
        // Msftedit normalizes imported RTF when streaming it out. An untouched
        // widget still owns the original authored RTF, including on close.
        let format=if editor.modified(){let Ok(rtf)=editor.format()else{return};rtf}else{note.data.format.clone()};
        DRAFT.with(|d|{
            let mut d=d.borrow_mut();if let Some(old)=d.as_mut(){
                if old.id==note.data.id{old.title=title;old.body=body;old.images=images;old.format=format;}
                return;
            }
            if title!=note.data.title||body!=note.data.body.replace("\r\n","\n").replace('\r',"\n")||images!=note.data.placements||format!=note.data.format{
                *d=Some(Draft{id:note.data.id.clone(),previous_title:note.data.title.clone(),previous_body:note.data.body.clone(),previous_images:note.data.placements.clone(),previous_format:note.data.format.clone(),title,body,images,format});
            }
        });
        if debounce{SetTimer(h,1,300,None);}
    });
}
// WM_PRINT provides the same drawing to Windows thumbnails, accessibility tools
// and an offscreen device context. Preserve native z-order and clipping when
// asking EDIT, Rich Edit and BUTTON children to draw their own client areas.
unsafe fn paint_client(h:HWND,dc:HDC){
    STRIP.with(|slot|{
        let Ok(b)=slot.try_borrow()else{return};
        let Some(s)=b.as_ref()else{return};
        if h==s.handle{
            let mut r=RECT::default();GetClientRect(h,&mut r);
            fill(dc,Bounds::rect(&r),s.palette.0,0);
        }else if let Some(c)=s.cards.iter().find(|c|c.handle==h){paint_card(s,c,dc);}
    });
}
unsafe fn print_children(h:HWND,dc:HDC){
    let flags=PRF_CLIENT|PRF_NONCLIENT|PRF_CHILDREN|PRF_ERASEBKGND;
    let first=GetTopWindow(h);
    let mut child=if first.is_null(){null_mut()}else{GetWindow(first,GW_HWNDLAST)};
    while !child.is_null(){
        if GetWindowLongW(child,GWL_STYLE)as u32&WS_VISIBLE!=0{
            let mut window=RECT::default();GetWindowRect(child,&mut window);
            let mut point=POINT{x:window.left,y:window.top};ScreenToClient(h,&mut point);
            let saved=SaveDC(dc);
            IntersectClipRect(dc,point.x,point.y,point.x+window.right-window.left,point.y+window.bottom-window.top);
            OffsetViewportOrgEx(dc,point.x,point.y,null_mut());
            SendMessageW(child,WM_PRINT,dc as usize,flags as isize);
            RestoreDC(dc,saved);
        }
        child=GetWindow(child,GW_HWNDPREV);
    }
}
unsafe fn print_window(h:HWND,dc:HDC,flags:i32){
    let saved=SaveDC(dc);
    let region=CreateRectRgn(0,0,0,0);
    if !region.is_null(){
        if GetWindowRgn(h,region)>0{ExtSelectClipRgn(dc,region,RGN_AND);}
        DeleteObject(region);
    }
    paint_client(h,dc);
    if flags&PRF_CHILDREN!=0{print_children(h,dc);}
    RestoreDC(dc,saved);
}
unsafe extern "system" fn procedure(h:HWND,msg:u32,wp:usize,lp:isize)->isize{
    match msg{
        WM_ERASEBKGND=>1,
        WM_PRINT|WM_PRINTCLIENT if wp!=0=>{
            let flags=if lp==0{PRF_CLIENT|PRF_CHILDREN}else{lp as i32};
            print_window(h,wp as _,flags);0
        }
        WM_PAINT=>{
            let mut ps=PAINTSTRUCT::default();let dc=BeginPaint(h,&mut ps);
            paint_client(h,dc);
            EndPaint(h,&ps);0
        }
        WM_NOTIFY if wp==inline_images::NOTE_EDITOR as usize=>inline_images::link_notification(h,lp),
        WM_DRAWITEM if lp!=0=>{owner_draw(&*(lp as *const DRAWITEMSTRUCT));1}
        WM_CTLCOLOREDIT|WM_CTLCOLORSTATIC=>{
            let brush=STRIP.with(|slot|{let b=slot.try_borrow().ok()?;let s=b.as_ref()?;let (_,field,text,_)=s.palette;
                let field=if GetDlgCtrlID(lp as HWND)==TITLE as i32{crate::chrome::control(s.palette)}else{field};
                SetBkColor(wp as _,field);SetTextColor(wp as _,text);SetDCBrushColor(wp as _,field);Some(GetStockObject(DC_BRUSH)as isize)});
            brush.unwrap_or_else(||DefWindowProcW(h,msg,wp,lp))
        }
        WM_COMMAND=>{
            let mut id=(wp&65535)as u16;
            if id==inline_images::NOTE_EDITOR{id=BODY;}
            if let Some(action)=inline_images::control_action(id){id=match action{"attach"=>406,"image"=>407,"detach"=>410,_=>id};}
            let notification=(wp>>16)as u16;
            if matches!(id,TITLE|BODY){
                if notification==EN_CHANGE as u16{read_editor(h,true);}
                if notification==EN_SETFOCUS as u16||notification==EN_KILLFOCUS as u16{InvalidateRect(h,null(),0);}
                if notification==EN_KILLFOCUS as u16{PostMessageW(h,WM_TIMER,1,0);}
                return 0;
            }
            if notification!=BN_CLICKED as u16{return 0;}
            if matches!(id,PREVIOUS|NEXT){
                STRIP.with(|slot|{if let Ok(mut b)=slot.try_borrow_mut(){if let Some(s)=b.as_mut(){s.horizontal(if id==PREVIOUS{-1}else{1});}}});
            }else{
                let event=STRIP.with(|slot|{
                    let state=slot.try_borrow().ok()?;let strip=state.as_ref()?;
                    let card=strip.cards.iter().find(|c|c.handle==h)?;
                    Some((strip.parent,Event{id,kind:card.kind,identity:card.data.id.clone()}))
                });
                if let Some((owner,event))=event{
                    EVENTS.with(|events|events.borrow_mut().push_back(event));
                    // Paste must read the clipboard and the caret before the next input.
                    if id==406{SendMessageW(owner,MESSAGE,0,0);}else{PostMessageW(owner,MESSAGE,0,0);}
                }
            }
            0
        }
        WM_TIMER if wp==3=>{
            if IsWindowVisible(h)!=0{InvalidateRect(h,null(),0);}0
        }
        WM_TIMER if wp==1=>{
            KillTimer(h,1);let owner=GetWindow(GetParent(h),GW_OWNER);
            if !owner.is_null(){PostMessageW(owner,MESSAGE,0,0);}0
        }
        WM_MOUSEWHEEL|WM_MOUSEHWHEEL=>{
            wheel(h,msg,(wp>>16)as i16,wp);0
        }
        WM_CLOSE=>0,
        _=>DefWindowProcW(h,msg,wp,lp)
    }
}
unsafe fn wheel(handle:HWND,msg:u32,delta:i16,keys:usize){
    if delta==0 || keys&8!=0{return}
    STRIP.with(|slot|{let Ok(mut b)=slot.try_borrow_mut()else{return};let Some(s)=b.as_mut()else{return};
        let line=map_dip(18,s.scale);
        if msg==WM_MOUSEWHEEL&&keys&4==0{
            if let Some(c)=s.cards.iter_mut().find(|c|c.handle==handle||IsChild(c.handle,handle)!=0){
                let limit=(map_dip(if c.kind==Kind::Camera{40}else{400},s.scale)-c.area.h).max(0);
                if (delta<0&&c.scroll_y<limit)||(delta>0&&c.scroll_y>0){
                    let dy=s.wheel_y.distance(delta,line,c.area.h);
                    if dy!=0{c.scroll_y=(c.scroll_y+dy).clamp(0,limit);s.layout();}return
                }
            }
        }
        let dx=if msg==WM_MOUSEHWHEEL{s.wheel_x.distance_horizontal(delta,map_dip(10,s.scale),s.viewport.w)}
            else{s.wheel_x.distance(delta,line,s.viewport.w)};
        let next=(s.offset+dx).clamp(0,s.max_offset());if next!=s.offset{s.offset=next;s.layout();}
    });
}

// Standard controls can receive the wheel because they have focus. In the strip the
// pointer picks the card, and a RichEdit scrolls first until it reaches its end.
unsafe fn wheel_message(msg:&MSG)->bool{
    if !matches!(msg.message,WM_MOUSEWHEEL|WM_MOUSEHWHEEL)||msg.wParam&8!=0{return false}
    let delta=(msg.wParam>>16)as i16;
    let target=STRIP.with(|slot|{
        let b=slot.try_borrow().ok()?;let s=b.as_ref().filter(|s|s.shown)?;
        if msg.hwnd!=s.handle&&IsChild(s.handle,msg.hwnd)==0{return None}
        let mut point=POINT{x:msg.lParam as i16 as i32,y:(msg.lParam>>16)as i16 as i32};ScreenToClient(s.handle,&mut point);
        let card=s.cards.iter().find(|c|c.area.contains(point.x,point.y))?;
        if msg.message==WM_MOUSEWHEEL&&msg.wParam&4==0{
            if let Some(editor)=card.rich.as_ref().map(|r|r.editor){
                let mut here=point;MapWindowPoints(s.handle,editor,&mut here,1);let mut client=RECT::default();GetClientRect(editor,&mut client);
                if here.x>=0&&here.y>=0&&here.x<client.right&&here.y<client.bottom{
                    let mut info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as _,fMask:SIF_RANGE|SIF_PAGE|SIF_POS,..Default::default()};
                    if GetScrollInfo(editor,SB_VERT,&mut info)!=0&&((delta>0&&info.nPos>info.nMin)||(delta<0&&info.nPos<(info.nMax-(info.nPage as i32-1).max(0)))){return Some((editor,true))}
                }
            }
        }
        Some((card.handle,false))
    });
    match target{
        Some((edit,true)) if msg.hwnd==edit=>false,
        Some((edit,true))=>{SendMessageW(edit,msg.message,msg.wParam,msg.lParam);true}
        Some((card,false))=>{wheel(card,msg.message,delta,msg.wParam);true}
        None=>false,
    }
}

pub fn dialog(msg:&MSG)->bool{unsafe{
    if wheel_message(msg){return true}
    STRIP.with(|slot|{
        let Ok(mut b)=slot.try_borrow_mut()else{return false};let Some(s)=b.as_mut().filter(|s|s.shown)else{return false};
        let inside=msg.hwnd==s.handle||IsChild(s.handle,msg.hwnd)!=0;
        if msg.message==WM_KEYDOWN&&msg.wParam==0x75{
            // The note editor uses F6 to select each inline image. Escape
            // already returns to the island from any control in the strip.
            if GetDlgCtrlID(msg.hwnd)as u16==inline_images::NOTE_EDITOR{return false;}
            if inside{SetForegroundWindow(s.parent);SetFocus(s.parent);return true;}
            if msg.hwnd!=s.parent&&IsChild(s.parent,msg.hwnd)==0{return false;}
            let first=s.cards.iter().enumerate().find_map(|(i,c)|c.controls.iter().find_map(|&(spec,h)|{
                let (name,on)=button(c,spec);(on&&!name.is_empty()).then_some((i,spec,h))
            }));
            if let Some((i,spec,h))=first{s.ensure(i,spec);SetForegroundWindow(s.handle);SetFocus(h);return true;}
        }
        if !inside{return false;}
        if msg.message==WM_KEYDOWN{
            if msg.wParam==0x1b{SetForegroundWindow(s.parent);SetFocus(s.parent);return true;}
            let control=GetDlgCtrlID(msg.hwnd)as u16;
            if GetKeyState(VK_CONTROL as i32)<0 && matches!(msg.wParam,0x25|0x27) {
                if matches!(control,TITLE|inline_images::NOTE_EDITOR|508){return false;}
                s.horizontal(if msg.wParam==0x25{-1}else{1});return true;
            }
            if msg.wParam==13&&!matches!(control,TITLE|BODY|508|inline_images::NOTE_EDITOR)&&control>0{
                PostMessageW(msg.hwnd,BM_CLICK,0,0);return true;
            }
            if msg.wParam==9{
                let stops:Vec<_>=s.cards.iter().enumerate().flat_map(|(i,c)|c.controls.iter().filter_map(move|&(spec,h)|{
                    let (name,en)=button(c,spec);(en&&(!name.is_empty()||matches!(spec.id,TITLE|BODY))).then_some((i,spec,h))
                })).collect();
                if stops.is_empty(){return true;}
                let backward=GetKeyState(VK_SHIFT as i32)<0;
                let index=stops.iter().position(|r|r.2==GetFocus());
                let n=match(index,backward){(Some(0),true)=>None,(Some(i),true)=>Some(i-1),(Some(i),false)if i+1<stops.len()=>Some(i+1),
                    (None,true)=>Some(stops.len()-1),(None,false)=>Some(0),_=>None};
                if let Some(n)=n{let (i,spec,h)=stops[n];s.ensure(i,spec);SetFocus(h);}else{SetForegroundWindow(s.parent);SetFocus(s.parent);}
                return true;
            }
        }
        // Process Enter/Space normally through the standard Windows button procedure.
        false
    })
}}
fn notify(ui:&Ui,kind:Kind,msg:String){
    if msg.is_empty(){return;}
    STRIP.with(|slot|{if let Ok(mut b)=slot.try_borrow_mut(){if let Some(s)=b.as_mut(){
        s.notice=Some((kind,msg.clone(),std::time::Instant::now()));if let Some(c)=s.cards.iter().find(|c|c.kind==kind){unsafe{InvalidateRect(c.handle,null(),0);}}
    }}});
    ui.status(&msg);
}
pub fn flush(ui:&mut Ui)->Result<(),String>{flush_note(ui,true)}
fn flush_note(ui:&mut Ui,refresh_main:bool)->Result<(),String>{
    // Read the native editors before changing mode, even when Windows deferred
    // EN_CHANGE inside a dialog or accessibility text replacement.
    let note=STRIP.with(|cell|{
        let state=cell.try_borrow().ok()?;
        let strip=state.as_ref().filter(|s|s.shown)?;
        strip.cards.iter().find(|c|c.kind==Kind::Note&&!c.data.id.is_empty()).map(|c|c.handle)
    });
    if let Some(h)=note{unsafe{read_editor(h,false);}}
    let Some(draft)=DRAFT.with(|d|d.borrow_mut().take())else{return Ok(())};
    let apply=|ui:&mut Ui|->Result<(),String>{
        let idx=ui.records.get("notes").and_then(|rows|rows.iter().position(|r|r.len()>=7&&get(r,0)==draft.id)).ok_or("The edited widget note no longer exists.")?;
        let old=ui.records["notes"][idx].clone();
        let positions=inline_images::offset_key(&draft.id);
        let old_positions=ui.records.get(&positions).cloned();
        let old_images=inline_images::placements(&old,old_positions.as_ref());
        let old_format=inline_images::saved_format(&ui.records,&draft.id);
        if get(&old,1)==draft.title&&get(&old,2)==draft.body&&old_images==draft.images&&old_format==draft.format{return Ok(())}
        if get(&old,1)!=draft.previous_title||get(&old,2)!=draft.previous_body||old_images!=draft.previous_images||old_format!=draft.previous_format{
            return Err("The note changed elsewhere. Your widget edits are still open.".into());
        }
        core::write_note(ui,idx,draft.title.clone(),draft.body.clone(),&draft.images,draft.format.clone())?;
        if refresh_main&&ui.page=="notes"&&ui.active_note.as_deref()==Some(&draft.id)&&!ui.dirty_note{core::render(ui,"notes");}
        Ok(())
    };
    if let Err(e)=apply(ui){DRAFT.with(|d|*d.borrow_mut()=Some(draft));return Err(e);}Ok(())
}
fn open(ui:&mut Ui,page:&'static str){shell::expand_from_side(ui);if ui.expanded{ui.navigate(page);}}
fn note_action(ui:&mut Ui,mut id:u16)->Result<String,String>{
    let r=note_row(ui).cloned().ok_or("This note is no longer shown.")?;
    let i=ui.records["notes"].iter().position(|n|n.first()==r.first()).ok_or("The note is gone.")?;
    if id==413{
        id=unsafe{
            let source=STRIP.with(|cell|cell.borrow().as_ref()?.cards.iter().find(|c|c.kind==Kind::Note).map(|c|GetDlgItem(c.handle,413))).unwrap_or(null_mut());
            if source.is_null(){return Ok(String::new());}
            let mut rect=RECT::default();GetWindowRect(source,&mut rect);
            let menu=CreatePopupMenu();if menu.is_null(){return Ok(String::new());}
            for (key,name,on) in [(404,"Set reminder…",true),(405,"Clear reminder",!r[5].is_empty()),(408,"Copy note text",true),(409,"Hide widget",true)]{
                AppendMenuW(menu,MF_STRING|if on{0}else{MF_GRAYED},key,wide(name).as_ptr());
            }
            let selected=crate::chrome::menu::track(source,menu,TPM_RETURNCMD|TPM_RIGHTBUTTON|TPM_RIGHTALIGN,rect.right,rect.bottom,ui.appearance);DestroyMenu(menu);selected as u16
        };
        if id==0{return Ok(String::new());}
    }
    match id {
        401=>{
            shell::expand_from_side(ui);
            if ui.expanded{
                ui.search.clear();ui.new_note=false;ui.active_note=Some(r[0].clone());
                ui.navigate("notes");
            }
            return Ok(String::new());
        }
        408=>{shell::copy_text(ui,&format!("{}\r\n\r\n{}",r[1],r[2]))?;return Ok("Note copied.".into());}
        406|410=>{
            let image=if id==406{Some(workspace::attach_clipboard_image(ui)?)}else{None};
            let card=STRIP.with(|s|{
                let strip=s.try_borrow_mut().map_err(|_|"Note editor is busy.")?;
                let card=strip.as_ref().and_then(|s|s.cards.iter().find(|c|c.kind==Kind::Note)).ok_or("Open the note card.")?;
                let edit=card.rich.as_ref().ok_or("Note editor unavailable.")?;
                if let Some(image)=image{edit.paste_image(ui,&image)?;}else{
                    let (index,_)=edit.selected_image().ok_or("Select an image in the note.")?;edit.remove_at(index)?;
                }
                Ok::<_,String>(card.handle)
            })?;
            unsafe{read_editor(card,false);}
            flush(ui)?;return Ok("Note updated.".into());
        }
        407=>{let selected=STRIP.with(|s|s.borrow().as_ref().and_then(|s|s.cards.iter().find(|c|c.kind==Kind::Note)?.rich.as_ref()?.selected_image()));
            if let Some((_,image))=selected{workspace::view_image(ui,&image)?;return Ok(String::new());}
            let list=&r[7..];let index=if list.len()>1{let names=list.iter().enumerate().map(|(i,_)|format!("Image {}",i+1)).collect::<Vec<_>>();
                let Some(i)=shell::choice(ui,"Note images","Select an attached image",&names)else{return Ok(String::new())};i}else{0};
            workspace::view_image(ui,list.get(index).ok_or("This note has no attachments.")?)?;return Ok(String::new());}
        403=>{if !shell::confirm(ui,"Delete this note?"){return Ok(String::new());}
            core::set(ui,"noteWidget","");
            if let Err(e)=core::delete_note(ui,i){core::set(ui,"noteWidget",&r[0]);return Err(e);}
            if ui.active_note.as_deref()==Some(&r[0]){ui.active_note=None;}
            return Ok("Note deleted.".into());}
        409=>{return hide_note(ui,&r[0]);}
        _=>{}
    }
    let mut updated=r.clone();
    match id{
        402=>updated[3]=if r[3]=="1"{"0"}else{"1"}.into(),
        404=>{let Some(t)=shell::schedule_reminder(ui)else{return Ok(String::new())};updated[5]=t.to_string();}
        405=>updated[5].clear(),
        _=>return Err("Unknown note control.".into())
    }
    updated[6]=core::now().to_string();ui.records.get_mut("notes").unwrap()[i]=updated;
    if let Err(e)=ui.save(){ui.records.get_mut("notes").unwrap()[i]=r;return Err(e);}
    if matches!(id,404|405)&&ui.due_note.as_deref()==Some(&r[0]){ui.due_note=None;}
    if ui.page=="notes"&&!ui.dirty_note{core::render(ui,"notes");}
    Ok("Note updated.".into())
}
fn hide_note(ui:&mut Ui,id:&str)->Result<String,String>{
    if core::setting(ui,"noteWidget","")!=id{return Err("This note is no longer shown.".into());}
    core::set(ui,"noteWidget","");
    if let Err(e)=ui.save(){core::set(ui,"noteWidget",id);return Err(e);}
    Ok("Note card hidden.".into())
}
fn accepts(ui:&Ui,event:&Event)->bool{
    !ui.expanded && !ui.locked && !ui.suppressed && !ui.paused && ui.hidden_until.is_none()
        && core::setting(ui,"widgets","on")=="on" && event.identity==identity(ui,event.kind)
}
fn action(ui:&mut Ui,event:Event)->Result<String,String>{
    if !accepts(ui,&event){return Ok(String::new());}
    match event.id{
        110|111=>{
            if ui.call_state.is_none(){return Err("No calling app is connected.".into());}
            let shown=model(ui,Kind::Camera);
            let status=devices::activity()?;
            let state=status["state"].as_str().unwrap_or("");
            if status["consumers"].as_u64().unwrap_or(0)==0||status["cameraEnabled"]==false||
                !matches!(state,"live"|"frozen"|"standby")||state!=shown.kind{
                return Err("Camera status changed. Wait for the card to refresh.".into());
            }
            let value=if event.id==111{"standby"}else if shown.running{"freeze"}else{"resume"};
            let msg=devices::call_control(value)?;ui.camera_check=std::time::Instant::now()-std::time::Duration::from_secs(10);Ok(msg)
        }
        112|113=>{open(ui,if event.id==112{"studio"}else{"camera"});Ok(String::new())}
        201|301=>{open(ui,"widgets");Ok(String::new())}
        202..=207|302..=304=>{let key=match event.id{202=>"pom",203=>"pom-skip",204=>"pom-reset",205=>"focus",206=>"short",207=>"long",302=>"watch",303=>"lap",_=>"reset"};core::timer_action(ui,key)}
        401..=410|413=>note_action(ui,event.id),
        501=>{shell::expand_from_side(ui);workspace::open_widget(ui)?;Ok(String::new())}
        502..=507|509=>{
            let item=clip_row(ui).ok_or("The clipboard widget is gone.")?;
            let kind=get(item,0);
            let key=match event.id{
                502 if kind=="link"=>"open",502=>"copy",503 if kind=="link"=>"copy",503 if kind=="text"=>"rewrite",503=>"open",
                504=>"save",505=>"pin",506=>"remove",507=>"hide",509 if kind=="image"=>"open",_=>return Ok(String::new()),
            };
            if key=="hide"{
                let old=ui.records.remove("clipboard_widget").ok_or("The clipboard widget is gone.")?;
                if let Err(e)=ui.save(){ui.records.insert("clipboard_widget".into(),old);return Err(e);}
                return Ok("Clipboard card hidden.".into());
            }
            if key=="rewrite"{shell::expand_from_side(ui);}
            workspace::widget_action(ui,key)
        }
        _=>Err("Unknown widget control.".into())
    }
}
pub fn command(ui:&mut Ui,_wp:usize){
    let events=EVENTS.with(|e|std::mem::take(&mut *e.borrow_mut()));
    let opening_note=events.iter().any(|e|e.kind==Kind::Note && e.id==401 && accepts(ui,e));
    // Save the main editor under its original ID before committing the side
    // draft. Its conflict check can then protect simultaneous edits to one note.
    if opening_note && ui.page=="notes" && ui.dirty_note{
        if let Err(e)=core::save_note(ui){notify(ui,Kind::Note,e);return;}
    }
    // The open command loads the freshly committed widget note exactly once.
    if let Err(e)=flush_note(ui,!opening_note){notify(ui,Kind::Note,e);return;}
    for event in events{let k=event.kind;match action(ui,event){Ok(s)|Err(s)=>notify(ui,k,s)}}
    sync(ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe fn printed_pixel(hwnd:HWND,x:i32,y:i32)->u32{
        let mut r=RECT::default();GetClientRect(hwnd,&mut r);
        let mut info=BITMAPINFO::default();info.bmiHeader.biSize=std::mem::size_of::<BITMAPINFOHEADER>()as u32;
        info.bmiHeader.biWidth=r.right;info.bmiHeader.biHeight= -r.bottom;info.bmiHeader.biPlanes=1;info.bmiHeader.biBitCount=32;
        let dc=CreateCompatibleDC(null_mut());let mut bits=null_mut();
        let bmp=CreateDIBSection(dc,&info,DIB_RGB_COLORS,&mut bits,null_mut(),0);
        assert!(!bmp.is_null());let old=SelectObject(dc,bmp);
        // A private station has no desktop compositor. Dispatch the production
        // WM_PRINT request directly so the thumbnail check observes app pixels.
        SendMessageW(hwnd,WM_PRINT,dc as _,(PRF_CLIENT|PRF_NONCLIENT|PRF_CHILDREN|PRF_ERASEBKGND)as _);
        GdiFlush();let color=GetPixel(dc,x,y);SelectObject(dc,old);DeleteObject(bmp);DeleteDC(dc);color
    }
    // Uses native, offscreen Windows controls and an isolated profile. No camera is opened.
    #[test]
    #[ignore="uses a private Windows window station; when signed out, supply synthetic BUDDY_ISLAND_FIXTURES"]
    fn side_cards_edit_without_expanding_and_leave_gaps_click_through(){
        use windows::Win32::Foundation::HWND as TypedHandle;
        use crate::island::{Layout,Placement};
        workspace::with_private_test_desktop(||unsafe{
            let h=CreateWindowExW(0,wide("STATIC").as_ptr(),wide("Synthetic side cards").as_ptr(),WS_POPUP,0,0,1000,400,
                null_mut(),null_mut(),GetModuleHandleW(null()),null());
            assert!(!h.is_null());
            let folder=std::env::temp_dir().join(format!("Buddy-side-test-{}-{}",std::process::id(),core::now()));
            let mut ui=Ui::new(TypedHandle(h),folder.clone(),true).unwrap();ui.expanded=false;
            core::set(&mut ui,"widgets","on");core::set(&mut ui,"pomWidget","on");core::set(&mut ui,"watchWidget","on");core::set(&mut ui,"noteWidget","example");
            ui.records.insert("notes".into(),vec![["example","Shopping","Pick up tea","0","1","","1"].map(str::to_owned).to_vec()]);
            use sha2::{Digest,Sha256};
            let pixels=image::RgbaImage::from_pixel(64,32,image::Rgba([4,190,37,255]));
            let mut bytes=std::io::Cursor::new(Vec::new());pixels.write_to(&mut bytes,image::ImageFormat::Png).unwrap();
            let mut image_id=format!("{:x}",Sha256::digest(bytes.get_ref()));
            let images=folder.join("images");
            let can_save=crate::storage::write_protected(&images.join(format!("{image_id}.dpapi")),bytes.get_ref()).is_ok();
            let color=if can_save{0x25be04}else{
                // A test-only, machine-protected synthetic image supplied by the fixture
                // generator is readable on signed-out Windows. User profile writes
                // keep the regular per-user protection and must fail transactionally.
                let input=std::path::PathBuf::from(std::env::var_os("BUDDY_ISLAND_FIXTURES").expect("BUDDY_ISLAND_FIXTURES on a signed-out VM")).join("fixture-clip");
                image_id=crate::storage::load(&input).unwrap()["clipboard"][0][1].clone();
                std::fs::create_dir_all(&images).unwrap();let name=format!("{image_id}.dpapi");
                std::fs::copy(input.join("images").join(&name),images.join(&name)).unwrap();
                0xa8ab10
            };
            ui.records.get_mut("notes").unwrap()[0].push(image_id.clone());
            ui.records.insert(inline_images::offset_key("example"),vec![vec!["4".to_owned()]]);
            let clip=["image",&image_id,"","1","64","32"].map(str::to_owned).to_vec();
            ui.records.insert("clipboard".into(),vec![clip.clone()]);ui.records.insert("clipboard_widget".into(),vec![clip]);
            ui.call_state=Some("Camera frozen · 1".into());
            let fixture=|w:i32,dpi:u32|Layout::new(RECT{left:0,top:0,right:w,bottom:870},dpi,100,Placement::Left,300,
                &[Widget::Camera,Widget::Focus,Widget::Stopwatch,Widget::Note,Widget::Clipboard]);
            island::configure(fixture(1240,96),false);island::request(false,true);sync(&mut ui);
            let (strip,camera,gap,below)=STRIP.with(|s|{let state=s.borrow();let s=state.as_ref().unwrap();assert_eq!(s.cards.len(),5);
                let r=s.cards[0].area;let to_screen=|x,y|{let mut p=POINT{x,y};ClientToScreen(s.handle,&mut p);p};
                (s.handle,to_screen(r.x+10,12),to_screen(r.right()+3,12),to_screen(r.x+15,70))});
            assert!(contains_screen_point(camera.x,camera.y));assert!(!contains_screen_point(gap.x,gap.y));assert!(!contains_screen_point(below.x,below.y));
            let region=CreateRectRgn(0,0,0,0);assert!(GetWindowRgn(strip,region)>0);
            assert_eq!(PtInRegion(region,295,12),0);assert_eq!(PtInRegion(region,30,70),0);DeleteObject(region);
            let controls=STRIP.with(|slot|{
                let s=slot.borrow();let s=s.as_ref().unwrap();
                s.cards.iter().enumerate().flat_map(|(i,c)|c.controls.iter().filter_map(move|&(spec,h)|{
                    let (caption,on)=button(c,spec);
                    (on&&!caption.is_empty()&&!matches!(spec.id,TITLE|BODY|508)).then_some((i,spec,h))
                })).collect::<Vec<_>>()
            });
            let tab_order=STRIP.with(|slot|{
                let s=slot.borrow();let s=s.as_ref().unwrap();
                s.cards.iter().flat_map(|c|c.controls.iter().filter_map(move|&(spec,h)|{
                    let (name,enabled)=button(c,spec);(enabled&&!name.is_empty()).then_some((spec,h))
                })).collect::<Vec<_>>()
            });
            ShowWindow(h,SW_SHOWNOACTIVATE);
            assert!(dialog(&MSG{hwnd:h,message:WM_KEYDOWN,wParam:0x75,..Default::default()}));
            for (index,(spec,hwnd)) in tab_order.iter().enumerate(){
                if index>0{assert!(dialog(&MSG{hwnd:GetFocus(),message:WM_KEYDOWN,wParam:9,..Default::default()}));}
                assert_eq!(GetFocus(),*hwnd,"Tab should focus {}",spec.label);
                if matches!(spec.id,TITLE|BODY) {
                    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState,SetKeyboardState};
                    let mut previous=[0u8;256];assert_ne!(GetKeyboardState(previous.as_mut_ptr()),0);
                    let mut pressed=previous;pressed[VK_CONTROL as usize]=0x80;
                    assert_ne!(SetKeyboardState(pressed.as_ptr()),0);
                    for arrow in [0x25,0x27] {
                        assert!(!dialog(&MSG{hwnd:*hwnd,message:WM_KEYDOWN,wParam:arrow,..Default::default()}),
                            "The note editor owns Ctrl+Left and Ctrl+Right");
                    }
                    SetKeyboardState(previous.as_ptr());
                }
                if spec.id==BODY{
                    let mut key=MSG{hwnd:*hwnd,message:WM_KEYDOWN,wParam:0x75,..Default::default()};
                    assert!(!dialog(&key));DispatchMessageW(&mut key);
                    let selected=STRIP.with(|slot|slot.borrow().as_ref().unwrap().cards.iter().find(|c|c.kind==Kind::Note).unwrap().rich.as_ref().unwrap().selected_image());
                    assert_eq!(selected,Some((0,image_id.clone())));
                }
            }
            for (i,spec,hwnd) in controls{
                STRIP.with(|slot|slot.borrow_mut().as_mut().unwrap().ensure(i,spec));
                assert_ne!(GetWindowLongW(hwnd,GWL_STYLE)as u32&WS_TABSTOP,0);
                assert!(!window_text(hwnd,100).is_empty());
                SendMessageW(hwnd,BM_CLICK,0,0);
                let routed=EVENTS.with(|e|e.borrow_mut().pop_front().map(|e|e.id));
                assert_eq!(routed,Some(spec.id),"Click on {}",spec.label);
            }
            let (px,py)=STRIP.with(|slot|{
                let mut b=slot.borrow_mut();let s=b.as_mut().unwrap();let i=s.cards.iter().position(|c|c.kind==Kind::Clip).unwrap();
                let preview=s.cards[i].controls.iter().find(|(s,_)|s.id==502).unwrap().0;s.ensure(i,preview);let c=&s.cards[i];(c.area.x+200,c.area.y+110)
            });
            assert_eq!(printed_pixel(strip,px,py),color,"WM_PRINT should render the protected thumbnail");
            let title=STRIP.with(|slot|{let mut b=slot.borrow_mut();let s=b.as_mut().unwrap();
                let i=s.cards.iter().position(|c|c.kind==Kind::Note).unwrap();
                let (spec,control)=s.cards[i].controls.iter().find(|(a,_)|a.id==TITLE).copied().unwrap();s.ensure(i,spec);
                assert!(s.offset>0);control});
            SetWindowTextW(title,wide("Shopping list").as_ptr());
            command(&mut ui,0);
            if can_save{
                assert_eq!(ui.records["notes"][0][1],"Shopping list");
                assert_eq!(crate::storage::load(&folder).unwrap()["notes"][0][1],"Shopping list");
            }else{
                assert_eq!(ui.records["notes"][0][1],"Shopping");
                assert_eq!(window_text(title,32768),"Shopping list");
                DRAFT.with(|d|assert_eq!(d.borrow().as_ref().expect("unsaved draft").title,"Shopping list"));
                assert!(flush(&mut ui).unwrap_err().contains("encryption"));
                SetWindowTextW(title,wide("Shopping").as_ptr());flush(&mut ui).unwrap();
                assert!(DRAFT.with(|d|d.borrow().is_none()));
            }
            let watch=STRIP.with(|slot|slot.borrow().as_ref().unwrap().cards.iter().find(|c|c.kind==Kind::Watch).unwrap().handle);
            let placements=inline_images::placements(&ui.records["notes"][0],ui.records.get(&inline_images::offset_key("example")));
            assert_eq!(placements[0].offset,4);assert_eq!(placements[0].id,image_id);
            let watch_button=GetDlgItem(watch,302);assert_ne!(GetWindowLongW(watch_button,GWL_STYLE)as u32&WS_TABSTOP,0);
            SendMessageW(watch_button,BM_CLICK,0,0);
            command(&mut ui,0);assert_eq!(number(&ui,"watchStart")>0,can_save);assert!(!ui.expanded);
            let clip_card=STRIP.with(|state|state.borrow().as_ref().unwrap().cards.iter().find(|c|c.kind==Kind::Clip).unwrap().handle);
            let preview=GetDlgItem(clip_card,502);
            assert_eq!(window_text(preview,120),"Copy image");
            assert_eq!(IsWindowEnabled(GetDlgItem(clip_card,506)),0,"pinned remove button must be disabled");
            SendMessageW(preview,WM_CONTEXTMENU,preview as usize,-1);
            let opened=EVENTS.with(|q|q.borrow_mut().pop_back()).unwrap();
            assert_eq!(opened.id,509);assert_eq!(opened.identity,format!("image:{image_id}"));
            let remove=Event{id:506,kind:Kind::Clip,identity:opened.identity};
            assert!(action(&mut ui,remove).unwrap_err().contains("Unpin"));
            let stale=Event{id:505,kind:Kind::Clip,identity:"text:previous".into()};action(&mut ui,stale).unwrap();assert_eq!(get(clip_row(&ui).unwrap(),3),"1");
            island::configure(fixture(1100,144),false);sync(&mut ui);
            STRIP.with(|slot|{let s=slot.borrow();let s=s.as_ref().unwrap();assert_eq!(s.scale,1.5);assert!(s.offset<=s.max_offset());});
            // On a short display the card scrolls vertically first. At its lower edge
            // the same gesture continues into the rest of the horizontal strip.
            let layout=Layout::new(RECT{left:0,top:0,right:1100,bottom:330},144,100,Placement::Left,300,
                &[Widget::Camera,Widget::Focus,Widget::Stopwatch,Widget::Note,Widget::Clipboard]);
            island::configure(layout,false);sync(&mut ui);
            let target=STRIP.with(|slot|{let mut b=slot.borrow_mut();let s=b.as_mut().unwrap();let target=s.cards.iter_mut().find(|c|c.kind==Kind::Focus).unwrap();target.scroll_y=0;s.offset=120;s.layout();s.cards.iter().find(|c|c.kind==Kind::Focus).unwrap().handle});
            let offsets=||STRIP.with(|slot|{let b=slot.borrow();let s=b.as_ref().unwrap();(s.cards.iter().find(|c|c.handle==target).unwrap().scroll_y,s.offset)});
            let initial=offsets();wheel(target,WM_MOUSEWHEEL,-120,0);let first=offsets();assert!(first.0>initial.0 && first.1==initial.1);
            for _ in 0..12{wheel(target,WM_MOUSEWHEEL,-120,0);}let end=offsets();assert!(end.1>first.1,"wheel at the bottom of a clipped card reaches the next widget");
            let before=end.1;wheel(target,WM_MOUSEHWHEEL,30,0);let after=offsets().1;assert!(after>before&&after-before<100,"fractional wheel must not page a whole card");
            ui.locked=true;sync(&mut ui);assert_eq!(IsWindowVisible(strip),0);
            close();drop(ui);DestroyWindow(h);std::fs::remove_dir_all(folder).unwrap();
        });
    }
}
