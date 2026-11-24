//! Chrome for the native image overlay. The original bitmap and reversible
//! annotations remain owned by the editor thread, including OCR selection.
use super::*;
use crate::chrome::{self,fill,inset,mix,panel,text};
use windows_sys::Win32::UI::{Controls::{DRAWITEMSTRUCT,ODS_DISABLED,ODS_FOCUS,ODS_NOFOCUSRECT,ODS_SELECTED},
    Input::KeyboardAndMouse::{EnableWindow,SetFocus}};
pub(super) const COLORS:[[u8;3];6]=[[53,57,229],[245,141,37],[0,214,255],[111,199,48],[255;3],[17;3]];
const LEFT:u32=DT_SINGLELINE|DT_VCENTER|DT_LEFT;
const CENTER:u32=DT_SINGLELINE|DT_VCENTER|DT_CENTER;

#[derive(Clone,Copy)]
pub(super) struct Overlay{pub owner:usize,desktop:usize,pub geometry:[i32;4],pub colors:chrome::Colors,scale:f32}
impl Default for Overlay{
    fn default()->Self{Self{owner:0,desktop:0,geometry:[10,10,840,376],colors:(0x1b1712,0x27221b,0xfaf8f6,0xf5d825),scale:1.}}
}
#[link(name="user32")]
extern "system"{fn GetThreadDesktop(thread:u32)->HANDLE;fn SetThreadDesktop(desktop:HANDLE)->i32;}
impl Overlay{
    pub(super) fn attach_desktop(&self)->Result<(),String>{
        if self.desktop==0||unsafe{SetThreadDesktop(self.desktop as HANDLE)}!=0{return Ok(())}
        Err(format!("Cannot open the image editor on this desktop: {}",std::io::Error::last_os_error()))
    }

    pub(super) fn for_ui(ui:&crate::Ui)->Self{
        let geometry=crate::island::frame().expanded_body;let mut origin=POINT{x:geometry.left,y:geometry.top};
        unsafe{ClientToScreen(ui.hwnd.0,&mut origin);}
        Self{owner:ui.hwnd.0 as usize,desktop:unsafe{GetThreadDesktop(windows_sys::Win32::System::Threading::GetCurrentThreadId())}as usize,geometry:[origin.x,origin.y,(geometry.right-geometry.left).max(320),(geometry.bottom-geometry.top).max(200)],
            colors:ui.appearance,scale:crate::shell::dpi(ui.hwnd.0)as f32/96.*ui.zoom as f32/100.}
    }
}
pub(super) struct EditorChrome{
    pub overlay:Overlay, pub opacity:u32,pub font_size:i32,history:bool,text_point:Option<Point>,pub scan_mode:bool,
    fonts:[HFONT;2],history_revision:u64,
}
impl EditorChrome{
    pub(super) fn new(overlay:Overlay)->Self{
        let fonts=[10,9].map(|size|unsafe{CreateFontW(-(size as f32*overlay.scale).round()as i32,0,0,0,400,0,0,0,
            DEFAULT_CHARSET as u32,OUT_DEFAULT_PRECIS as u32,CLIP_DEFAULT_PRECIS as u32,ANTIALIASED_QUALITY as u32,0,platform::wide("Segoe UI").as_ptr())});
        Self{overlay,fonts,opacity:100,font_size:28,history:false,text_point:None,scan_mode:false,history_revision:u64::MAX}
    }
    pub(super) fn px(&self,n:i32)->i32{(n as f32*self.overlay.scale).round()as i32}
    fn dip(&self,n:i32)->i32{(n as f32/self.overlay.scale).round()as i32}
    pub(super) fn stage(&self,client:RECT)->RECT{
        RECT{left:self.px(14),top:self.px(92),right:client.right-self.px(14),bottom:client.bottom-self.px(41)}
    }
}
impl Drop for EditorChrome{fn drop(&mut self){for f in self.fonts{unsafe{DeleteObject(f);}}}}

const EXTRA:&[(usize,&str)]=&[(11,"Undo"),(12,"Redo"),(13,"Delete annotation"),(401,"Tool history"),(420,"Decrease line thickness"),(421,"Increase line thickness"),
    (422,"Decrease opacity"),(423,"Increase opacity"),(424,"Font size"),(425,"Style and view options"),(1,"Copy image"),(2,"Save PNG"),(6,"Close"),
    (400,"Select text"),(7,"Rescan"),(8,"Copy text"),(301,"Zoom out"),(302,"Fit image"),(300,"Zoom in"),(3,"Show original or edits"),(4,"Save original"),
    (14,"Remove crop"),(406,"Remove selected annotation")];
fn create_child(h:HWND,e:&Editor,class:&str,name:&str,id:usize,flags:u32)->HWND{unsafe{
    let child=CreateWindowExW(0,platform::wide(class).as_ptr(),platform::wide(name).as_ptr(),WS_CHILD|WS_TABSTOP|flags,0,0,1,1,h,id as _,GetModuleHandleW(null()),null());
    chrome::scroll::attach(child,flags,false);
    SendMessageW(child,WM_SETFONT,e.chrome.fonts[0]as _,0);
    if class=="BUTTON"{chrome::install_button(child);}
    child
}}
pub(super) unsafe fn create(h:HWND,e:&mut Editor){
    chrome::scroll::set_palette(e.chrome.overlay.colors.0);
    for(i,(_,name))in TOOLS.iter().enumerate(){create_child(h,e,"BUTTON",name,100+i,BS_OWNERDRAW as u32);}
    for(i,name)in["Red","Blue","Yellow","Green","White","Black"].into_iter().enumerate(){create_child(h,e,"BUTTON",name,200+i,BS_OWNERDRAW as u32);}
    for &(id,name)in EXTRA{create_child(h,e,"BUTTON",name,id,BS_OWNERDRAW as u32);}
    create_child(h,e,"LISTBOX","Tool history",405,WS_VSCROLL|(LBS_NOTIFY|LBS_NOINTEGRALHEIGHT)as u32);
    e.edit=create_child(h,e,"EDIT","",450,ES_AUTOHSCROLL as u32);SendMessageW(e.edit,0x1501,1,platform::wide("Type here…").as_ptr()as _);
}
pub(super) unsafe fn layout(h:HWND,e:&mut Editor){
    let mut r=RECT::default();GetClientRect(h,&mut r);let c=&e.chrome;let radius=c.px(22);let w=c.dip(r.right);let height=c.dip(r.bottom);
    let mut keep=Vec::new();
    let mut place=|id:usize,x,y,ww,hh|{let child=GetDlgItem(h,id as _);SetWindowPos(child,null_mut(),c.px(x),c.px(y),c.px(ww),c.px(hh),SWP_NOZORDER|SWP_NOACTIVATE|SWP_SHOWWINDOW);keep.push(id);};
    let mut x=7;
    let compact=w<800;
    let buttons=if w<575{7}else{10};
    let step=if w<650{23}else{26};
    for i in 0..buttons{place(100+i,x,7,step-1,28);x+=step;}
    x+=5;
    for id in [11,12,13,401]{place(id,x,7,23,28);x+=25;}
    x+=4;
    if w>620{for i in 0..6{place(200+i,x,12,15,17);x+=15;}x+=4;}
    if !compact{
        place(420,x,8,19,26);place(421,x+58,8,19,26);x+=82;
        place(422,x,8,19,26);place(423,x+58,8,19,26);x+=82;
        place(424,x,8,36,26);
    }else if x<w-150{place(425,x,8,33,26);}
    place(2,w-141,7,50,28);place(1,w-88,7,54,28);place(6,w-31,7,26,28);
    place(400,9,48,64,24);if e.chrome.scan_mode||e.words.is_some()||e.pending.is_some(){place(7,w-125,48,49,24);place(8,w-68,48,59,24);}
    place(301,w-86,height-25,24,23);place(302,w-63,height-25,35,23);place(300,w-27,height-25,24,23);
    if c.history{
        place(405,w-221,107,207,(height-213).max(35));place(406,w-221,height-99,98,23);place(14,w-118,height-99,103,23);
        place(3,w-221,height-69,98,24);place(4,w-118,height-69,103,24);
    }
    for id in (100..110).chain(200..206).chain(EXTRA.iter().map(|p|p.0)).chain([405]){
        if !keep.contains(&id){ShowWindow(GetDlgItem(h,id as _),SW_HIDE);}
    }
    for(id,enabled)in[(11,!e.undo.is_empty()),(12,!e.redo.is_empty()),(13,e.selected.is_some()),(406,e.selected.is_some()),(14,e.doc.crop.is_some()),(8,e.words.is_some()),
        (7,e.pending.is_none()),(420,e.width>1),(421,e.width<24),(422,e.chrome.opacity>10),(423,e.chrome.opacity<100),(301,e.zoom>0.25),(300,e.zoom<3.)]{EnableWindow(GetDlgItem(h,id),enabled as _);}
    if e.chrome.history && e.chrome.history_revision!=e.revision{
        let list=GetDlgItem(h,405);SendMessageW(list,WM_SETREDRAW,0,0);SendMessageW(list,LB_RESETCONTENT,0,0);
        for(i,mark)in e.doc.marks.iter().enumerate().rev(){let tool=TOOLS.iter().find(|(t,_)|*t==mark.tool).map_or("Annotation",|p|p.1);
            SendMessageW(list,LB_ADDSTRING,0,platform::wide(format!("{tool}    ·    Use {}",i+1)).as_ptr()as _);}
        if e.doc.marks.is_empty(){SendMessageW(list,LB_ADDSTRING,0,platform::wide("No tool use yet").as_ptr()as _);}
        SendMessageW(list,WM_SETREDRAW,1,0);e.chrome.history_revision=e.revision;
    }
    if e.chrome.history{if let Some(i)=e.selected{SendMessageW(GetDlgItem(h,405),LB_SETCURSEL,e.doc.marks.len().saturating_sub(i+1),0);}}
    if e.chrome.text_point.is_none(){ShowWindow(e.edit,SW_HIDE);}
    let region=CreateRoundRectRgn(0,-radius*2,r.right+1,r.bottom+1,radius*2,radius*2);
    if !region.is_null(){
        let visible=CreateRectRgn(0,0,r.right,r.bottom);
        if !visible.is_null(){CombineRgn(region,region,visible,RGN_AND);DeleteObject(visible);}
        if SetWindowRgn(h,region,1)==0{DeleteObject(region);}
    }
    RedrawWindow(h,null(),null_mut(),RDW_INVALIDATE|RDW_ALLCHILDREN);
}
pub(super) unsafe fn paint(h:HWND,e:&Editor,dc:HDC,r:RECT){
    let c=&e.chrome;let (bg,field,ink,_)=c.overlay.colors;let muted=mix(bg,ink,160);let edge=mix(bg,ink,40);
    let w=c.dip(r.right);let height=c.dip(r.bottom);
    let rect=|x,y,ww,hh|chrome::rect(c.px(x),c.px(y),c.px(ww),c.px(hh));
    fill(dc,r,bg);fill(dc,rect(0,77,w,height-104),0x100d09);
    for y in [41,77,height-28]{fill(dc,rect(0,y,w,1),edge);}
    let title=crate::ui::window_text(h,500);let status=title.split_once(" · ").map_or("Ready",|(_,s)|s);
    let scan=if e.pending.is_some(){"Recognizing text…"}else if e.words.is_some(){"Drag to select text · Shift-click selects a line"}
        else if c.scan_mode{status}else{"Markup mode · Choose Select text to copy characters"};
    text(dc,rect(81,44,w-220,30),scan,c.fonts[0],muted,LEFT);
    text(dc,rect(10,height-26,(w-300).max(55),25),status,c.fonts[1],muted,LEFT);
    let b=e.view();let state=if e.original_mode{"Original".into()}else{format!("{} annotation{}",e.doc.marks.len(),if e.doc.marks.len()==1{""}else{"s"})};
    text(dc,rect(w-278,height-26,182,25),&format!("{} × {}px · {state}",b.width,b.height),c.fonts[1],muted,DT_RIGHT|DT_VCENTER|DT_SINGLELINE);
    if w>=800{
        for(id,value)in[(420,format!("{} px",e.width)),(422,format!("{} %",c.opacity))]{
            let mut button=RECT::default();GetWindowRect(GetDlgItem(h,id),&mut button);let mut p=POINT{x:button.right,y:button.top};ScreenToClient(h,&mut p);
            let r=chrome::rect(p.x,p.y,c.px(39),c.px(26));panel(dc,r,0,field,edge);text(dc,r,&value,c.fonts[1],ink,CENTER);
        }
    }
    if c.history{
        let panel_rect=rect(w-232,82,225,(height-116).max(80));panel(dc,panel_rect,c.px(9),bg,edge);
        text(dc,rect(w-220,86,180,18),"Tool history",c.fonts[0],ink,LEFT);
    }
}
pub(super) unsafe fn draw(_h:HWND,e:&Editor,d:&DRAWITEMSTRUCT){
    let id=d.CtlID as usize;let c=&e.chrome;let (bg,field,ink,accent)=c.overlay.colors;let muted=mix(bg,ink,185);
    let hot=chrome::hovered(d.hwndItem)||d.itemState&ODS_SELECTED!=0;let disabled=d.itemState&ODS_DISABLED!=0;
    let focus=d.itemState&ODS_FOCUS!=0&&d.itemState&ODS_NOFOCUSRECT==0;
    let active=(100..110).contains(&id)&&e.tool==TOOLS[id-100].0&&!c.scan_mode||id==400&&c.scan_mode||id==401&&c.history;
    let plain=matches!(id,6|11..=13|100..=109|300..=302|401);
    let face=if id==1{accent}else if active{mix(bg,accent,26)}else if hot{mix(bg,ink,17)}else if plain{bg}else{field};
    let fg=if id==1{if bg&255>150{0xffffff}else{0x171307}}else if disabled{mix(bg,muted,93)}else if active{accent}else{muted};
    let edge=if active{mix(bg,accent,123)}else if plain{face}else{mix(bg,ink,49)};
    let r=d.rcItem;fill(d.hDC,r,bg);
    if(200..206).contains(&id){
        let color=colorref(COLORS[id-200]);let circle=inset(r,c.px(1));panel(d.hDC,circle,c.px(10),color,if focus||e.ink==COLORS[id-200]{accent}else{edge});return
    }
    panel(d.hDC,r,c.px(5),face,if focus{accent}else{edge});let side=c.px(15);
    let mut name=match id{1=>"Copy".into(),2=>"Save".into(),3=>if e.original_mode{"Show edits"}else{"Show original"}.into(),4=>"Save original".into(),
        400=>"Select text".into(),7=>"Rescan".into(),8=>"Copy text".into(),424=>c.font_size.to_string(),425=>"···".into(),302=>format!("{}%",(e.zoom*100.)as i32),406=>"Remove".into(),14=>"Remove crop".into(),_=>String::new()};
    if id==13{name.clear()}
    let icon=matches!(id,1..=4|6|11..=13|100..=109|300|301|401|420..=423);
    if icon{
        let width=if name.is_empty(){0}else{chrome::width(d.hDC,&name,c.fonts[1])+c.px(3)};
        let x=r.left+(r.right-r.left-side-width)/2;crate::Ui::markup_glyph(d.hDC,x,r.top+(r.bottom-r.top-side)/2,side,id,fg,face);
        if width!=0{text(d.hDC,RECT{left:x+side+c.px(3),..r},&name,c.fonts[1],fg,LEFT);}
    }else{text(d.hDC,r,&name,c.fonts[1],fg,CENTER);}
}

unsafe fn menu(h:HWND,anchor:i32,items:&[(usize,String)])->Option<usize>{
    let m=CreatePopupMenu();if m.is_null(){return None}
    for (id,label)in items{AppendMenuW(m,MF_STRING,*id,platform::wide(label).as_ptr());}
    let mut r=RECT::default();GetWindowRect(GetDlgItem(h,anchor),&mut r);
    let choice=TrackPopupMenu(m,TPM_RETURNCMD|TPM_NONOTIFY,r.left,r.bottom,0,h,null());DestroyMenu(m);(choice>0).then_some(choice as usize)
}
pub(super) unsafe fn clip_history(e:&Editor,dc:HDC,client:RECT){
    if e.chrome.history{ExcludeClipRect(dc,client.right-e.chrome.px(232),e.chrome.px(82),client.right-e.chrome.px(7),client.bottom-e.chrome.px(34));}
}
pub(super) unsafe fn print_controls(h:HWND,e:&Editor,dc:HDC){
    let mut child=GetTopWindow(h);
    while !child.is_null(){
        if GetWindowLongW(child,GWL_STYLE)as u32 & WS_VISIBLE!=0{
            let mut r=RECT::default();GetWindowRect(child,&mut r);let mut p=POINT{x:r.left,y:r.top};ScreenToClient(h,&mut p);
            let save=SaveDC(dc);IntersectClipRect(dc,p.x,p.y,p.x+r.right-r.left,p.y+r.bottom-r.top);OffsetViewportOrgEx(dc,p.x,p.y,null_mut());
            let id=GetDlgCtrlID(child);
            if id!=405 && id!=450{
                let mut di=DRAWITEMSTRUCT::default();di.CtlID=id as _;di.hwndItem=child;di.hDC=dc;GetClientRect(child,&mut di.rcItem);
                if windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(child)==0{di.itemState=ODS_DISABLED;}
                draw(h,e,&di);
            }else{SendMessageW(child,WM_PRINT,dc as _,(PRF_CLIENT|PRF_NONCLIENT)as _);}
            RestoreDC(dc,save);
        }
        child=GetWindow(child,GW_HWNDNEXT);
    }
}
pub(super) unsafe fn select_history(h:HWND,e:&mut Editor)->Result<(),String>{
    let selected=SendMessageW(GetDlgItem(h,405),LB_GETCURSEL,0,0);
    if selected>=0{if let Some(i)=e.doc.marks.len().checked_sub(selected as usize+1){e.selected=Some(i);e.tool=Tool::Select;e.words=None;e.chrome.scan_mode=false;}}
    layout(h,e);Ok(())
}
pub(super) unsafe fn start_text(h:HWND,e:&mut Editor,lp:isize)->Result<(),String>{
    let Some(p)=e.point(h,lp,false)else{return Ok(())};
    let x=lp as i16 as i32;let y=(lp>>16)as i16 as i32;
    let mut r=RECT::default();GetClientRect(h,&mut r);
    e.chrome.text_point=Some(p);SetWindowTextW(e.edit,platform::wide("").as_ptr());
    SetWindowPos(e.edit,HWND_TOP,x.min(r.right-e.chrome.px(130)),y,e.chrome.px(130),e.chrome.px(28),SWP_SHOWWINDOW);SetFocus(e.edit);Ok(())
}
pub(super) unsafe fn command(h:HWND,e:&mut Editor,id:usize)->Result<(),String>{
    match id{
        400=>{
            if e.chrome.scan_mode{e.chrome.scan_mode=false;e.edited();}
            else{e.chrome.scan_mode=true;if e.words.is_none()&&e.pending.is_none(){super::command(h,e,7)?;}}
        }
        401=>{e.chrome.history=!e.chrome.history;}
        406=>super::command(h,e,13)?,
        420=>e.width=(e.width-1).max(1),421=>e.width=(e.width+1).min(24),
        422=>e.chrome.opacity=(e.chrome.opacity-5).max(10),423=>e.chrome.opacity=(e.chrome.opacity+5).min(100),
        424=>{let items=[10,12,14,16,18,20,24,28,32,36,48,60,72,96].map(|v|(500+v,format!("{v} px")));if let Some(n)=menu(h,424,&items){e.chrome.font_size=(n-500)as i32;}}
        425=>{
            let mut items=TOOLS.iter().enumerate().map(|(i,(_,s))|(100+i,s.to_string())).collect::<Vec<_>>();
            items.extend([(420,"Thinner line".into()),(421,"Thicker line".into()),(422,"Decrease opacity".into()),(423,"Increase opacity".into()),(424,"Font size".into()),(3,"Show original / edits".into()),(4,"Save original PNG".into()),
                (303,"Pan left".into()),(304,"Pan right".into()),(305,"Pan up".into()),(306,"Pan down".into())]);
            if let Some(action)=menu(h,425,&items){super::command(h,e,action)?;}
        }
        440|441=>{
            let Some(p)=e.chrome.text_point.take()else{return Ok(())};
            let value=crate::ui::window_text(e.edit,513);
            let result=if id==440&&!value.trim().is_empty(){e.drag=vec![p];let r=e.finish(p,false);e.drag.clear();r}else{Ok(())};
            ShowWindow(e.edit,SW_HIDE);SetFocus(h);result?;
        }
        _=>{}
    }
    Ok(())
}
