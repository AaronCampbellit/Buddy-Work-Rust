use super::*;
use crate::chrome::{self,Icon};
use std::cell::Cell;
use super::scroll_pane::ScrollPane;
use windows_sys::Win32::UI::Shell::{DefSubclassProc,RemoveWindowSubclass,SetWindowSubclass};
use windows_sys::Win32::{Foundation::RECT,UI::Controls::{DRAWITEMSTRUCT,ODS_DISABLED,ODS_FOCUS,ODS_NOFOCUSRECT}};
const BASE:u16=3410;
const NAMES:&[(&str,Icon)]=&[
    ("Input",Icon::Mic),("Output",Icon::Devices),("Camera",Icon::Camera),("Virtual camera",Icon::Video),
    ("Refresh",Icon::Refresh),("Open Windows device settings",Icon::Sliders),
    ("Choose device",Icon::Down),("Mute",Icon::Volume),("Volume",Icon::Volume),
    ("Set default",Icon::Check),("Default for calls",Icon::Devices),("App mixer",Icon::Sliders),
    ("Choose camera",Icon::Camera),("Start preview",Icon::Play),("Stop preview",Icon::Close),("Camera controls",Icon::Sliders),
    ("Choose input",Icon::Camera),("Start output",Icon::Play),("More camera actions",Icon::More),("Close recent notifications",Icon::Close),
    ("Framing",Icon::Camera),("Picture",Icon::Sliders),("Refresh cameras",Icon::Refresh),
    ("Background",Icon::Palette),("Name & logo",Icon::Note),("Picture-in-picture",Icon::Video),("Standby",Icon::Pause),("Saved scenes",Icon::Folder),("Camera & output",Icon::Camera),
    ("Show preview",Icon::Camera),("Discard edits",Icon::Close),("Apply changes",Icon::Check),("Choose option",Icon::Down),("Set option",Icon::Pencil),
    ("Scene menu",Icon::More),("Choose input format",Icon::Down),("Add image",Icon::Plus)
];
const VIEWPORT:u16=3481;
const HEADER:i32=35;
const SECTION_WIDTHS:[i32;6]=[72,78,97,56,76,97];

// The device chooser stays visible while the device controls scroll beneath it.
fn fixed(index:usize)->bool { index<6 || index==19 }
fn section_rows(width:i32)->([(i32,i32);6],i32){
    let (mut x,mut y)=(11,83);
    let points=SECTION_WIDTHS.map(|w|{
        if x+w>width-11 && x>11{x=11;y+=30;}
        let at=(x,y);x+=w+4;at
    });
    (points,y-83)
}
unsafe extern "system" fn device_canvas(hwnd:Handle,msg:u32,wp:usize,lp:isize,id:usize,_:usize)->isize{
    if msg==WM_NCDESTROY{RemoveWindowSubclass(hwnd,Some(device_canvas),id);}
    if !matches!(msg,WM_PAINT|WM_PRINTCLIENT){return DefSubclassProc(hwnd,msg,wp,lp)}
    let mut ps=PAINTSTRUCT::default();let dc=if msg==WM_PAINT{BeginPaint(hwnd,&mut ps)}else{wp as HDC};
    if !dc.is_null(){crate::with_ui(|ui|{
        let Some(v)=ui.devices_view.get()else{return};let mut r=RECT::default();GetClientRect(hwnd,&mut r);
        chrome::fill(dc,r,ui.appearance.0);
        r.top=-v.pane.offset()-ui.px(HEADER);r.bottom=r.top+ui.px(v.height.get());ui.paint_device_content(dc,r);
    });}
    if msg==WM_PAINT{EndPaint(hwnd,&ps);}0
}
pub(super) struct DevicesView{pane:ScrollPane,height:Cell<i32>,page:Cell<&'static str>,buttons:Vec<Handle>,preview_host:Handle,output:Cell<bool>,selected:Cell<usize>,picture:Cell<bool>,section:Cell<usize>}
impl DevicesView{
    fn index(&self,page:&str)->usize{match page{"camera"=>2,"studio"=>3,_=>usize::from(self.output.get())}}
}
fn studio_section(key:&str)->usize{
    if key.starts_with("name")||key.starts_with("logo")||key=="asset:logo"{1}
    else if key.starts_with("pip"){2}else if key.starts_with("standby")||key=="asset:standby"{3}else{0}
}
fn rows(h:Handle)->Vec<String>{unsafe{
    let cnt=SendMessageW(h,LB_GETCOUNT,0,0).clamp(0,2048);
    (0..cnt as usize).filter_map(|i|{let len=SendMessageW(h,LB_GETTEXTLEN,i,0);if !(0..=32768).contains(&len){return None}
        let mut s=vec![0u16;len as usize+1];SendMessageW(h,LB_GETTEXT,i,s.as_mut_ptr()as _);Some(String::from_utf16_lossy(&s[..len as usize]))
    }).collect()
}}
impl Ui{
    pub(crate) fn physical_preview_host(&self)->Handle{self.devices_view.get().map(|v|v.preview_host).unwrap_or(std::ptr::null_mut())}
    pub(super) fn layout_device_view(&self,on:bool,r:RECT){
        if !on{if let Some(v)=self.devices_view.get(){for &h in &v.buttons{show(h,false)}show(v.preview_host,false);show(v.pane.hwnd,false);v.page.set("");}return}
        let v=self.devices_view.get_or_init(||{
            let pane=ScrollPane::new(self,VIEWPORT);
            unsafe{SetWindowSubclass(pane.hwnd,Some(device_canvas),VIEWPORT as usize,0);}
            let preview_host=add(pane.hwnd,"STATIC","Camera preview",3480,WS_CLIPCHILDREN,0,self.fonts[0]);
            let buttons=NAMES.iter().enumerate().map(|(i,(name,_))|super::preferences::button(if fixed(i){self.hwnd.0}else{pane.hwnd},name,BASE+i as u16,self.fonts[0])).collect();
            DevicesView{pane,preview_host,buttons,height:Cell::new(0),page:Cell::new(""),output:Cell::new(false),selected:Cell::new(usize::MAX),picture:Cell::new(false),section:Cell::new(0)}
        });
        if v.page.replace(self.page)!=self.page{v.pane.reset();}
        let p=|x|self.px(x);let logical=|px:i32|(px as f32/self.scale()).round()as i32;
        let viewport=RECT{top:r.top+p(HEADER),..r};let full_width=logical(r.right-r.left);
        let content_height=|w|match self.page{
            "audio"=>if devices::audio_devices(!v.output.get()).is_empty(){130}else if w<560{403}else{241},
            "camera"=>if w<480{356}else{195},
            "studio"=>246+section_rows(w).1,
            _=>65+34*activity::notice_titles().len().max(1)as i32,
        };
        let height=logical(r.bottom-r.top);let mut total=height.max(content_height(full_width));
        let w=v.pane.layout(self,viewport,total-HEADER);let actual=height.max(content_height(w));
        if total!=actual{total=actual;v.pane.layout(self,viewport,total-HEADER);}
        v.height.set(total);show(v.pane.hwnd,true);
        let mut used=vec![false;NAMES.len()];
        let mut place=|i:usize,x:i32,y:i32,ww:i32,hh:i32,s:Option<&str>|{
            if let Some(s)=s{if window_text(v.buttons[i],2048)!=s{label(v.buttons[i],s)}}
            if fixed(i){pos(v.buttons[i],r.left+p(x),r.top+p(y),p(ww),p(hh));show(v.buttons[i],true);}
            else{v.pane.place(self,v.buttons[i],x,y-HEADER,ww,hh);}used[i]=true;
        };
        if self.page!="camera"{show(v.preview_host,false);}
        if self.page=="notifications"{place(19,w-29,0,26,25,None);}
        else{
            let room=(full_width-if self.page=="audio"{74}else{38}).clamp(160,302);let mut x=3;
            for(i,width)in [58,65,76,103].into_iter().enumerate(){let ww=width*room/302;place(i,x,5,ww,24,None);x+=ww;}
            if self.page=="audio"{place(4,full_width-70,5,32,25,None);}place(5,full_width-32,5,32,25,None);
            let half=if self.page=="audio"&&w<560{w}else{(w-10)/2};
            match self.page{
                "audio"=>{
                    let all=devices::audio_devices(!v.output.get());let endpoints=all.iter().filter(|(_,e)|e.session.is_none());
                    if let Some((id,dev))=endpoints.clone().find(|(id,_)|*id==v.selected.get()).or_else(||endpoints.clone().find(|(_,d)|d.roles.contains(&"default"))).or_else(||endpoints.clone().next()){
                        v.selected.set(*id);
                        place(6,11,78,half-22,27,Some(&dev.name));
                        place(7,half-80,44,69,25,Some(if dev.mute==Some(true){"Muted"}else{"On"}));
                        let level=dev.volume.map(|v|format!("{}   {:.0}%",if self.devices_view.get().unwrap().output.get(){"Output volume"}else{"Input level"},v*100.)).unwrap_or_else(||"Volume unavailable".into());
                        place(8,11,113,half-22,27,Some(&level));place(9,11,167,100,24,None);place(10,117,167,127,24,None);
                        if all.iter().any(|(_,r)|r.id==dev.id&&r.session.is_some()){place(11,if w<560{11}else{half+21},if w<560{253}else{79},half-22,26,Some("Choose app session"));}
                    }
                }
                "camera"=>{
                    let names=rows(self.list);let col=if w<480{w}else{(w*44/100).min(345)};let control_x=if w<480{0}else{col+15};let control_y=if w<480{160}else{0};
                    place(12,91,38,col-129,28,Some(self.selected().and_then(|i|names.get(i)).or_else(||names.first()).map(String::as_str).unwrap_or("System camera")));
                    place(22,col-28,38,28,28,None);
                    let live=devices::camera_previewing();
                    let pw=p(col);let preview_height=if w<480{83}else{total-108};let ph=p(preview_height);
                    if live{v.pane.place(self,v.preview_host,0,108-HEADER,col,preview_height);}else{show(v.preview_host,false);}
                    if live{devices::size_preview(pw,ph);}
                    place(if live{14}else{13},0,74,95,28,None);
                    unsafe{windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(v.buttons[13],(!names.is_empty())as _);}
                    place(20,control_x,38+control_y,54,26,None);place(21,control_x+58,38+control_y,48,26,None);
                    if live{place(15,control_x,153+control_y,151,28,None);}
                }
                "studio"=>{
                    let footer=total-32;let (sections,dy)=section_rows(w);
                    let col=(w-34)/2;let sec=v.section.get();let (running,dirty,device)=devices::studio_state();
                    place(17,w-136,47,111,27,Some(if running{"Stop virtual camera"}else{"Start virtual camera"}));
                    unsafe{windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(v.buttons[17],(running||!device.is_empty())as _);}
                    for (i,(x,y)) in sections.into_iter().enumerate(){place(23+i,x,y,SECTION_WIDTHS[i],26,None);}
                    place(29,11,114+dy,99,24,None);place(30,w-220,footer+2,96,25,None);place(31,w-115,footer+2,99,25,None);
                    unsafe{for b in [30,31]{windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(v.buttons[b],dirty as _);}}
                    place(18,(w-255).min(326),footer+2,28,25,None);
                    let x=col+28;
                    if sec<4{let fields=devices::studio_fields();
                        let chosen=fields.iter().find(|(i,key,_,_)|Some(*i)==self.selected()&&studio_section(key)==sec).or_else(||fields.iter().find(|(_,key,_,_)|studio_section(key)==sec));
                        if let Some((_,key,name,value))=chosen{
                            place(32,x,119+dy,w-x-19,25,Some(name));
                            let shown=if key.starts_with("asset:"){"Choose image…"}else if value.is_empty(){"Set value…"}else{value};
                            place(33,x,150+dy,w-x-19,26,Some(shown));
                        }
                        if matches!(sec,0|1|3){place(36,x,178+dy,93,25,None);}
                    }else if sec==4{place(34,x,120+dy,w-x-19,26,Some("Saved scenes"));}
                    else{place(16,x,120+dy,w-x-19,26,Some(if device.is_empty(){"Choose camera…"}else{"Choose input"}));place(35,x,153+dy,w-x-19,26,None);}
                }
                _=>{}
            }
        }
        for (h,used)in v.buttons.iter().zip(used){if !used{show(*h,false)}}
    }
    pub(super) fn device_tab_stops(&self)->Vec<Handle>{self.devices_view.get().map(|v|{
        // The island orders its direct children. Windows enters the scroll pane
        // at its own place in that order and traverses its controls there.
        let mut order:Vec<_>=v.buttons.iter().enumerate()
            .filter_map(|(i,&button)|fixed(i).then_some(button)).collect();
        order.push(v.pane.hwnd);
        order
    }).unwrap_or_default()}
    pub(super) fn paint_device_view(&self,dc:HDC,r:RECT){
        unsafe{let state=SaveDC(dc);IntersectClipRect(dc,r.left,r.top,r.right,r.top+self.px(HEADER));self.paint_device_content(dc,r);RestoreDC(dc,state);}
    }
    fn paint_device_content(&self,dc:HDC,r:RECT){
        let Some(v)=self.devices_view.get()else{return};let w=((r.right-r.left)as f32/self.scale()).round()as i32;let p=|n|self.px(n);
        let a=|x,y,ww,hh|chrome::rect(r.left+p(x),r.top+p(y),p(ww),p(hh));
        let (bg,field,ink,accent)=self.appearance;let edge=chrome::mix(bg,ink,43);let muted=chrome::mix(bg,ink,168);
        let t=|x,y,ww,hh,s:&str,f,c,flags|chrome::text(dc,a(x,y,ww,hh),s,self.fonts[f],c,flags|DT_VCENTER);
        let left=DT_LEFT|DT_SINGLELINE;
        if self.page=="notifications"{
            let items=activity::notice_titles();t(10,0,w-60,27,"Recent notifications",1,ink,left);chrome::fill(dc,a(0,30,w,1),edge);
            if items.is_empty(){t(10,35,w-20,52,"No recent notifications",3,muted,DT_CENTER|DT_SINGLELINE);}
            for (i,s)in items.iter().take(((r.bottom-r.top-p(35))/p(34)).max(0)as usize).enumerate(){let y=35+34*i as i32;chrome::icon(dc,r.left+p(10),r.top+p(y+8),p(15),Icon::Bell,accent,bg);t(34,y,w-45,34,s,0,ink,left);}return
        }
        chrome::panel(dc,a(0,2,(w-if self.page=="audio"{68}else{32}).min(312),30),p(8),field,edge);
        let half=if self.page=="audio"&&w<560{w}else{(w-10)/2};
        if self.page=="audio"{
            let all=devices::audio_devices(!v.output.get());
            let Some((_,dev))=all.iter().find(|(id,row)|*id==v.selected.get()&&row.session.is_none())else{
                t(8,47,w-16,70,if v.output.get(){"No connected output devices."}else{"No connected microphones."},0,muted,DT_CENTER|DT_SINGLELINE);return
            };
            let(mx,my)=if w<560{(0,174)}else{(half+10,0)};for (x,y) in [(0,37),(mx,37+my)]{chrome::panel(dc,a(x,y,half,163),p(8),field,edge);}
            t(11,42,half-94,25,if v.output.get(){"Speakers / headphones"}else{"Microphone"},1,ink,left);
            t(11,140,54,23,"Signal",3,muted,left);let bar=a(68,150,half-85,4);chrome::fill(dc,bar,edge);
            if let Some(peak)=dev.peak{chrome::fill(dc,RECT{right:bar.left+((bar.right-bar.left)as f32*peak.clamp(0.,1.)).round()as i32,..bar},accent);}
            t(mx+11,42+my,half-30,25,"App mixer",1,ink,left);
            if !all.iter().any(|(_,r)|r.id==dev.id&&r.session.is_some()){t(mx+11,73+my,half-30,26,"No shared audio sessions on this device.",0,muted,left);}
            t(mx+11,110+my,half-35,43,"Signal is measured before mute. Device mute is separate from Teams mute.",3,muted,DT_WORDBREAK);
            t(8,202+my,half-12,34,"Changes Windows audio. Apps with a fixed device may keep using it.",3,muted,DT_WORDBREAK);
        }else{
            let height=((r.bottom-r.top)as f32/self.scale()).round()as i32;
            if self.page=="camera"{
                let col=if w<480{w}else{(w*44/100).min(345)};let control_x=if w<480{0}else{col+15};let control_y=if w<480{160}else{0};
                t(0,38,88,28,"Preview camera",3,muted,left);
                let live=devices::camera_previewing();let ph=if w<480{83}else{height-108};let panel=a(0,108,col,ph);let gray=chrome::mix(bg,ink,90);
                chrome::panel(dc,panel,p(8),gray,gray);let mid=108+ph/2;
                chrome::icon(dc,r.left+p(col/2-11),r.top+p(mid-23),p(22),Icon::Camera,muted,gray);
                t(0,mid+1,col,24,if live{"Camera preview active"}else{"Camera preview is off"},3,ink,DT_CENTER|DT_SINGLELINE);
                t(control_x,74+control_y,w-control_x-7,21,if v.picture.get(){"Picture adjustments"}else{"Pan, tilt & zoom"},1,ink,left);
                t(control_x,95+control_y,w-control_x-7,27,if live{"Select a camera control to adjust its value."}else{"Preview is off. Start it to check the image and available controls."},3,muted,DT_WORDBREAK);
                t(control_x,119+control_y,w-control_x-7,24,"Camera adjustments also affect other apps using this camera.",3,muted,DT_WORDBREAK);
            }else{
                let (running,dirty,_)=devices::studio_state();let col=(w-34)/2;let footer=height-32;let dy=section_rows(w).1;
                chrome::panel(dc,a(0,38,w,height-36),p(8),field,edge);
                t(11,45,170,30,"Virtual camera",1,ink,left);
                t(w-203,46,64,28,if running{"Live"}else{"Off"},3,muted,DT_RIGHT|DT_SINGLELINE);
                t(117,112+dy,col-111,25,"Preview off",3,muted,left);
                let preview=a(11,143+dy,col,(footer-148-dy).max(1));chrome::panel(dc,preview,p(8),bg,bg);
                chrome::fill(dc,a(11,footer-6,w-22,1),edge);t(11,footer,(w-272).min(219),29,if dirty{"Unsaved changes"}else{"Saved on this device"},3,muted,left);
            }

        }
    }
    pub(super) fn draw_device_item(&self,di:&DRAWITEMSTRUCT)->bool{
        let Some(i)=(di.CtlID as u16).checked_sub(BASE).map(usize::from).filter(|i|*i<NAMES.len())else{return false};
        let Some(v)=self.devices_view.get()else{return false};let (bg,field,ink,accent)=self.appearance;let r=di.rcItem;let p=|n|self.px(n);
        let active=(i<4&&v.index(self.page)==i)||(self.page=="camera"&&i==20+usize::from(v.picture.get()))||(self.page=="studio"&&i==23+v.section.get());let focus=di.itemState&ODS_FOCUS!=0&&di.itemState&ODS_NOFOCUSRECT==0;
        let parent=if i<4||i>=6{field}else{bg};let face=if active{chrome::mix(field,accent,32)}else if chrome::hovered(di.hwndItem){chrome::mix(parent,ink,17)}else if i<4{parent}else{chrome::control(self.appearance)};
        chrome::fill(di.hDC,r,parent);chrome::panel(di.hDC,r,p(6),face,if focus||active{chrome::mix(parent,accent,130)}else if i<4{face}else{chrome::mix(bg,ink,45)});
        let color=if di.itemState&ODS_DISABLED!=0{chrome::mix(parent,ink,92)}else if active{accent}else{ink};let only=matches!(i,4|5|18|19|22);let size=p(14);let name=window_text(di.hwndItem,2048);
        let with_icon=!(i==20||i==21||(23..29).contains(&i));let x=if only{(r.right-size)/2}else{((r.right-size-p(5)-chrome::width(di.hDC,&name,self.fonts[3]))/2).max(p(5))};
        if with_icon{chrome::icon(di.hDC,x,(r.bottom-size)/2,size,NAMES[i].1,color,face); }
        if !only{chrome::text(di.hDC,RECT{left:if with_icon{x+size+p(5)}else{0},..r},&name,self.fonts[3],color,(if with_icon{DT_LEFT}else{DT_CENTER})|DT_VCENTER|DT_SINGLELINE);}true
    }
    pub(super) fn device_command(&mut self,id:u16,code:u16)->bool{
        if code!=BN_CLICKED as u16{return false}
        let Some(i)=id.checked_sub(BASE).map(usize::from).filter(|i|*i<NAMES.len())else{return false};
        let Some(v)=self.devices_view.get()else{return false};let anchor=v.buttons[i];
        match i{
            0..=3=>{if i<2{v.output.set(i==1);v.selected.set(usize::MAX);}self.navigate(["audio","audio","camera","studio"][i]);}
            4=>self.run_action("refresh"),
            5=>{if let Err(e)=activity::open(if self.page=="audio"{"ms-settings:sound"}else{"ms-settings:camera"}){self.error(&e);}}
            6..=11=>{
                let current=v.selected.get();let all=devices::audio_devices(!v.output.get());
                if i==6{let endpoints=all.iter().filter(|(_,row)|row.session.is_none()).collect::<Vec<_>>();
                    if let Some(n)=self.choose(anchor,&endpoints.iter().map(|(_,r)|r.name.clone()).collect::<Vec<_>>()){v.selected.set(endpoints[n].0);self.select_row(endpoints[n].0);self.run_action("default-all");}return true}
                if i==11{let device=all.iter().find(|(n,_)|*n==current).map(|(_,d)|d.id.as_str());let sessions=all.iter().filter(|(_,d)|d.session.is_some()&&Some(d.id.as_str())==device).collect::<Vec<_>>();
                    if let Some(n)=self.choose(anchor,&sessions.iter().map(|(_,r)|r.name.clone()).collect::<Vec<_>>()){
                        self.select_row(sessions[n].0);let cmds=["Mute / unmute".into(),"Set volume".into()];match self.choose(anchor,&cmds){Some(0)=>self.run_action("mute"),Some(1)=>self.device_volume(),_=>{}}
                    }return true}
                self.select_row(current);match i{7=>self.run_action("mute"),8=>self.device_volume(),9=>self.run_action("default"),_=>self.run_action("default-comm")}
            }
            12=>{let names=rows(self.list);if let Some(n)=self.choose(anchor,&names){self.select_row(n);self.run_action("select")}}
            13|14=>{if self.selected().is_none(){self.select_row(0);}self.run_action(if i==13{"preview"}else{"stop"})}
            20|21=>{v.picture.set(i==21);self.layout();}
            22=>self.run_action("refresh"),
            23..=28=>{v.section.set(i-23);self.run_action("effects");}
            29=>self.run_action(if v.section.get()==3{"standby-preview"}else{"preview"}),
            30|31=>self.run_action(if i==30{"discard"}else{"apply"}),
            32|33=>{let section=v.section.get();let opts=devices::studio_fields().into_iter().filter(|(_,k,_,_)|studio_section(k)==section).collect::<Vec<_>>();
                let selection=if i==32{self.choose(anchor,&opts.iter().map(|x|x.2.clone()).collect::<Vec<_>>())}
                    else{opts.iter().position(|x|Some(x.0)==self.selected()).or_else(||(!opts.is_empty()).then_some(0))};
                if let Some(n)=selection{let (row,key,title,val)=&opts[n];self.select_row(*row);self.run_action("select");
                    if i==33&&key.starts_with("asset:"){self.run_action("asset");return true}
                    if i==33{let value=match val.as_str(){"true"=>Some("false".into()),"false"=>Some("true".into()),_=>crate::shell::prompt(self,title,&self.detail(),val)};
                        if let Some(value)=value{self.set_input(&value);self.run_action("set");}}
                }
            }
            34=>{self.run_action("scenes");let items=rows(self.list);if let Some(n)=self.choose(anchor,&items){self.select_row(n);self.run_action("load-scene");}self.run_action("effects");}
            35|16=>{self.run_action(if i==16{"inventory"}else{"modes"});let items=rows(self.list);if let Some(n)=self.choose(anchor,&items){self.select_row(n);self.run_action("select");}self.run_action("effects");}
            36=>{let asset=match v.section.get(){0=>"asset:background",1=>"asset:logo",_=>"asset:standby"};
                if let Some((row,_,_,_))=devices::studio_fields().iter().find(|x|x.1==asset){self.select_row(*row);self.run_action("asset");}}
            15|18=>{
                let actions=self.actions.clone();if let Some(n)=self.choose(anchor,&actions.iter().map(|a|a.1.to_owned()).collect::<Vec<_>>()){
                    let (key,name)=actions[n];
                    if self.page=="studio"{self.advanced_studio(anchor,key,name);}
                    else{if self.selected().is_none(){self.select_row(0);}
                        if !matches!(key,"refresh"|"ranges"|"preview"|"stop"){let Some(val)=crate::shell::prompt(self,name,"Value, auto or default",&self.input())else{return true};self.set_input(&val);}
                        self.run_action(key);
                    }
                }}
            17=>self.run_action(if devices::studio_state().0{"stop"}else{"start"}),19=>self.navigate("home"),_=>{}
        }true
    }
    fn advanced_studio(&mut self,anchor:Handle,key:&str,name:&str){
        let collection=match key{
            "modes"|"inventory"|"sources"|"screens"|"scenes"=>Some(key),
            "set"|"asset"|"remove-asset"=>Some("effects"),
            "load-scene"|"delete-scene"|"update-scene"=>Some("scenes"),_=>None
        };
        if let Some(group)=collection{self.run_action(group);let options=rows(self.list);let choice=self.choose(anchor,&options);
            if let Some(n)=choice{ self.select_row(n);self.run_action("select"); }else{self.run_action("effects");return}
        }
        if matches!(key,"set"|"save-scene"|"output"){
            let Some(val)=crate::shell::prompt(self,name,if key=="save-scene"{"Scene name"}else if key=="output"{"Output format"}else{"Value"},&self.input())else{self.run_action("effects");return};self.set_input(&val);
        }
        if collection!=Some(key){self.run_action(key);}self.run_action("effects");
    }
    fn device_volume(&mut self){
        self.run_action("select");if let Some(val)=crate::shell::prompt(self,"Volume","Volume from 0 to 100",&self.input()){self.set_input(&val);self.run_action("volume");}
    }
}
