use super::{*, scroll_pane::ScrollPane};
use crate::chrome::{self,Icon};
use std::cell::Cell;
use windows_sys::Win32::{Foundation::RECT,UI::Controls::{DRAWITEMSTRUCT,ODS_FOCUS,ODS_NOFOCUSRECT}};

const TABS:u16=3200;
const RESET:u16=3206;
const ACCESS:u16=3207;
const ROW:u16=3240;
const CATEGORIES:[(&str,Icon);6]=[("General",Icon::Settings),("Appearance",Icon::Palette),("Island",Icon::PanelTop),("Notifications",Icon::Bell),("AI Rewrite",Icon::Widgets),("Saved files",Icon::Widgets)];
type SettingRow=(&'static str,&'static str,&'static str);
const GENERAL:&[SettingRow]=&[("heading","General",""),("autostart","Launch at login",""),("space","",""),("heading","Performance diagnostics",""),("description","Capture 15 seconds of process CPU, memory and handle counts. The report stays on this device.",""),("capture","On-demand local capture","Capture 15 seconds"),("space","",""),("heading","Application updates",""),("update","Application updates","Check now"),("reports","Local diagnostics","Open reports folder"),("download","Available release","Open download")];
const APPEARANCE:&[SettingRow]=&[("heading","Appearance",""),("heading","Custom colors",""),("customAccent","Accent",""),("customMain","Main background",""),("customField","Secondary background",""),("customTextColor","Text",""),("moduleOrder","Tab order",""),("theme","Theme",""),("accent","Color theme",""),("scale","Interface scale",""),("motion","Reduced motion","")];
const ISLAND:&[SettingRow]=&[("heading","Island",""),("display","Display",""),("position","Placement",""),("open","Opening mode",""),("retention","Remember last page",""),("center","Center display",""),("custom","Custom center text",""),("widgets","Show side widgets",""),("pomWidget","Pomodoro widget",""),("watchWidget","Stopwatch widget",""),("fullscreen","Hide in fullscreen",""),("exclude","Hide for applications",""),("hide","Temporary hide duration","")];
const NOTIFICATIONS:&[SettingRow]=&[("heading","Notifications",""),("notificationPreview","Show previews",""),("notificationPrivacy","Privacy",""),("notificationGrouping","Group by application",""),("notificationDuration","Preview duration","")];
const REWRITE:&[SettingRow]=&[("heading","AI Rewrite",""),("description","Use your locally installed Codex CLI for selected text.",""),("login","Codex connection","Sign in"),("status","Connection status","Check sign-in"),("rewrite","Rewrite preferences","Open AI Rewrite")];
const FILES:&[SettingRow]=&[("heading","Saved files",""),("store","Saved copies","Open folder"),("relocate","Storage location","Change folder"),("files","File shelf","Open Quick Access")];
const GROUPS:[&[SettingRow];6]=[GENERAL,APPEARANCE,ISLAND,NOTIFICATIONS,REWRITE,FILES];

pub(super) struct Preferences{
    category:Cell<usize>, categories:ScrollPane, content:ScrollPane,tabs:Vec<Handle>,access:Handle,
    labels:Vec<Handle>,values:Vec<Handle>,appearance:std::cell::OnceCell<Appearance>,feedback:Handle,feedback_row:Cell<Option<(usize,usize)>>,
}
pub(super) fn button(parent:Handle,name:&str,id:u16,font:HFONT)->Handle{
    let h=add(parent,"BUTTON",name,id,WS_TABSTOP|(BS_NOTIFY|BS_OWNERDRAW) as u32,0,font);chrome::install_button(h);h
}
fn caption(h:Handle,value:&str){if window_text(h,2048)!=value{label(h,value)}}
fn row_height(key:&str)->i32{match key{"heading"=>20,"description"=>32,"space"=>13,_=>40}}
impl Preferences{
    fn new(ui:&Ui)->Self{
        let categories=ScrollPane::new(ui,3210);let content=ScrollPane::new(ui,3211);
        let mut tabs=CATEGORIES.iter().enumerate().map(|(i,(name,_))|button(categories.hwnd,name,TABS+i as u16,ui.fonts[0])).collect::<Vec<_>>();
        tabs.push(button(categories.hwnd,"Reset settings",RESET,ui.fonts[0]));
        let labels=(0..ISLAND.len()).map(|_|add(content.hwnd,"STATIC","",0,0,0,ui.fonts[0])).collect();
        let values=(0..ISLAND.len()).map(|i|button(content.hwnd,"",ROW+i as u16,ui.fonts[0])).collect();
        let feedback=add(content.hwnd,"STATIC","",0,0,0,ui.fonts[0]);show(feedback,false);
        Self{category:Cell::new(0),categories,content,tabs,appearance:Default::default(),feedback,feedback_row:Cell::new(None),access:button(ui.hwnd.0,"Enable previews",ACCESS,ui.fonts[0]),labels,values}
    }
    fn section(&self)->&'static [SettingRow]{GROUPS[self.category.get()]}
}
impl Ui{
    pub(super) fn layout_preferences(&self,on:bool,r:RECT){
        if !on{if let Some(v)=self.preferences.get(){for h in [v.categories.hwnd,v.content.hwnd,v.access]{show(h,false)}}return}
        let v=self.preferences.get_or_init(||Preferences::new(self));let p=|n|self.px(n);
        show(v.categories.hwnd,true);show(v.content.hwnd,true);
        pos(v.access,r.right-p(109),r.top+p(4),p(109),p(26));show(v.access,true);
        let access=activity::notification_permission();caption(v.access,if matches!(access,"allowed"|"denied"){"Windows settings"}else{"Enable previews"});
        let sections=RECT{top:r.top+p(34),right:r.left+p(154),..r};
        let width=v.categories.layout(self,sections,250);
        for (i,&t) in v.tabs.iter().enumerate(){v.categories.place(self,t,0,if i==6{215}else{10+i as i32*33},(width-10).max(66),30);}
        let bounds=RECT{left:sections.right+p(18),top:sections.top,..r};
        if v.category.get()==1 {
            for &h in v.labels.iter().chain(v.values.iter()).chain([&v.feedback]){show(h,false);}
            let w=v.content.layout(self,bounds,AP_HEIGHT);
            v.appearance.get_or_init(||Appearance::new(self,&v.content)).layout(self,&v.content,w);
            return;
        }
        if let Some(a)=v.appearance.get(){a.hide();}
        let message=window_text(self.notice,4096);
        let feedback_row=v.feedback_row.get().filter(|(group,_)|*group==v.category.get()&&!message.trim().is_empty()).map(|row|row.1);
        show(v.feedback,feedback_row.is_some());
        let rows=v.section();let height=rows.iter().map(|r|row_height(r.0)).sum::<i32>()+26+if feedback_row.is_some(){39}else{0};
        let w=v.content.layout(self,bounds,height);
        let mut y=12;
        for i in 0..v.labels.len(){let Some(&(key,title,action))=rows.get(i)else{show(v.labels[i],false);show(v.values[i],false);continue};
            let ht=row_height(key);let field=core::OPTIONS.iter().find(|o|o.0==key);
            let plain=matches!(key,"heading"|"description"|"space");
            let value=field.map(|o|core::setting(self,key,o.2)).unwrap_or(action);
            let boolean=field.is_some_and(|o|o.3=="on · off");
            let right=if boolean{26}else{(w/2).min(182)};
            caption(v.labels[i],title);
            let font=match key{"description"=>self.fonts[0],"heading"=>self.fonts[4],_=>self.fonts[5]};
            unsafe{SendMessageW(v.labels[i],WM_SETFONT,font as _,0);}
            if key!="space"{v.content.place(self,v.labels[i],0,y+if plain{0}else{4},if plain{w-8}else{w-right-18},ht-2);}else{show(v.labels[i],false)}
            if plain{show(v.values[i],false)}else{
                let label=if boolean{title}else if value.is_empty(){"Choose…"}else{value};
                caption(v.values[i],label);
                v.content.place(self,v.values[i],w-right-8,y+1,right,30);
            }y+=ht;
            if feedback_row==Some(i){
                caption(v.feedback,&message);
                unsafe{SendMessageW(v.feedback,WM_SETFONT,self.fonts[0] as _,0);}
                v.content.place(self,v.feedback,0,y,w-8,38);
                y+=39;
            }
        }
    }
    pub(super) fn preference_tab_stops(&self)->Vec<Handle>{self.preferences.get().map(|v|vec![v.access,v.categories.hwnd,v.content.hwnd]).unwrap_or_default()}
    pub(super) fn paint_preferences(&self,dc:HDC,r:RECT){let p=|n|self.px(n);let (bg,_,ink,_)=self.appearance;
        chrome::text(dc,chrome::rect(r.left,r.top,r.right-r.left-p(120),p(32)),&format!("Windows notifications: {}",activity::notification_permission()),self.fonts[3],chrome::mix(bg,ink,182),DT_LEFT|DT_VCENTER|DT_SINGLELINE);
        chrome::fill(dc,chrome::rect(r.left+p(153),r.top+p(34),p(1),r.bottom-r.top-p(34)),chrome::mix(bg,ink,44));
    }
    pub(super) fn draw_preference_item(&self,di:&DRAWITEMSTRUCT)->bool{
        let id=di.CtlID as u16;let Some(v)=self.preferences.get()else{return false};
        if (AP_FRAME..SCALE).contains(&id){return v.appearance.get().is_some_and(|a|a.draw(self,di));}
        if !((TABS..=ACCESS).contains(&id)||(ROW..ROW+v.labels.len()as u16).contains(&id)){return false}
        let row=id.checked_sub(ROW).and_then(|i|v.section().get(i as usize));
        let checkbox=row.and_then(|(k,_,_)|core::OPTIONS.iter().find(|o|o.0==*k && o.3=="on · off"));
        let (bg,_,ink,accent)=self.appearance;let p=|n|self.px(n);let r=di.rcItem;
        let selected=id==TABS+v.category.get()as u16;
        let hot=chrome::hovered(di.hwndItem);let focus=di.itemState&ODS_FOCUS!=0 && di.itemState&ODS_NOFOCUSRECT==0;
        let tab=id>=TABS&&id<RESET;
        let face=if selected{chrome::mix(bg,accent,24)}else if hot{chrome::mix(bg,ink,15)}else{bg};
        let border=if selected || focus{chrome::mix(bg,accent,138)}else if tab{face}else{chrome::mix(bg,ink,46)};
        chrome::fill(di.hDC,r,bg);
        if let Some(&(key,_,default,_))=checkbox{
            let checked=core::setting(self,key,default)=="on";
            let box_r=chrome::rect((r.right-p(14))/2,(r.bottom-p(14))/2,p(14),p(14));
            chrome::panel(di.hDC,box_r,p(3),if checked{accent}else{chrome::control(self.appearance)},if focus{accent}else{chrome::mix(bg,ink,130)});
            if checked{chrome::icon(di.hDC,box_r.left,box_r.top,p(14),Icon::Check,0xffffff,accent);}
        }else{
            chrome::panel(di.hDC,r,p(6),face,border);
            let glyph=if tab{Some(CATEGORIES[(id-TABS)as usize].1)}else if id==ACCESS{Some(Icon::BellRing)}else if id==RESET || row.is_some_and(|s|matches!(s.0,"capture"|"update")){Some(Icon::Refresh)}else{None};
            let mut label_rect=r;
            if let Some(g)=glyph{chrome::icon(di.hDC,p(10),(r.bottom-p(14))/2,p(14),g,if selected{accent}else{ink},face);label_rect.left+=p(30);}
            let flags=if tab{DT_LEFT}else{DT_CENTER};chrome::text(di.hDC,label_rect,&window_text(di.hwndItem,2048),self.fonts[if row.is_some(){4}else{0}],if selected{accent}else{ink},flags|DT_VCENTER|DT_SINGLELINE);
        }true
    }
    pub(super) fn preference_command(&mut self,id:u16,code:u16)->bool{
        if self.page!="settings"||code!=BN_CLICKED as u16{return false}
        let Some(v)=self.preferences.get()else{return false};
        if (CUSTOM_START..=LAST_AP).contains(&id) && v.category.get()==1{
            let result=self.apply_appearance_control(id);
            match result{Ok(())=>{},Err(message)=>self.error(&message)};
            self.layout();return true;
        }
        if (TABS..RESET).contains(&id){v.category.set((id-TABS)as usize);v.content.reset();self.layout();return true}
        if id==RESET{self.run_action("defaults");return true}
        let result=if id==ACCESS{
            activity::action(self,"notifications",if matches!(activity::notification_permission(),"allowed"|"denied"){"settings"}else{"allow"})
        }else if let Some(&(key,title,_))=id.checked_sub(ROW).and_then(|n|v.section().get(n as usize)){
            if let Some(index)=core::OPTIONS.iter().position(|o|o.0==key){
                let (_,_,default,format)=core::OPTIONS[index];let old=core::setting(self,key,default);
                let val=if format=="on · off"{Some(if old=="on"{"off".into()}else{"on".into()})}
                    else if format.contains(" · ")||key=="display"{
                        let opts=if key=="display"{std::iter::once("primary".into()).chain(crate::shell::screens().into_iter().map(|s|s.0)).collect::<Vec<_>>()}else{format.split(" · ").map(str::to_owned).collect()};
                        self.choose(v.values[(id-ROW)as usize],&opts).map(|i|opts[i].clone())
                    }else{crate::shell::prompt(self,title,format,old)};
                if let Some(val)=val{self.select_row(index);self.set_input(&val);self.run_action("apply");}return true
            }
            if matches!(key,"capture"|"update"|"login"|"status"|"download"){
                v.feedback_row.set(Some((v.category.get(),(id-ROW) as usize)));
            }
            match key{"reports"=>activity::action(self,"diagnostics","open"),"capture"|"update"|"download"=>activity::action(self,"diagnostics",key),"login"|"status"=>activity::action(self,"rewrite",key),"store"|"relocate"=>workspace::action(self,"files",key),"files"|"rewrite"=>{self.navigate(key);return true},_=>return false}
        }else{return false};
        match result{Ok(msg)=>self.status(&msg),Err(e)=>self.error(&e)}self.layout();true
    }
}

// The Appearance tab uses a single painted background for fieldsets and labels.
// Every color, reorder action, theme, and slider remains a native focusable control.
const AP_FRAME:u16=3600;
const CUSTOM_START:u16=3601;
const THEME_COLORS:u16=3605;
const MOVE_START:u16=3606;
const MODE_START:u16=3614;
const PALETTE_START:u16=3617;
const REDUCE:u16=3623;
const SCALE:u16=3624;
const LAST_AP:u16=SCALE;
const AP_HEIGHT:i32=689;
const COLORS:[(&str,&str,&str);4]=[
    ("customAccent","Accent","#25d8f5"),("customMain","Main background","#182028"),
    ("customField","Secondary background","#182028"),("customTextColor","Text","#ffffff"),
];
const MODES:[(&str,&str);3]=[("system","System"),("dark","Dark"),("light","Light")];
const PALETTES:[(&str,&str,&str,&str);6]=[
    ("cyan","Classic","#1c252b","#25d8f5"),("ocean","Ocean","#173460","#74b5ff"),
    ("violet","Violet","#371762","#bd9aff"),("rose","Rose","#631732","#ff9dbb"),
    ("forest","Forest","#176245","#73dfb1"),("amber","Amber","#624617","#f5c16b"),
];

fn tab_indices(ui:&Ui)->Vec<usize>{
    let mut order=Vec::with_capacity(TABS_NAMES.len());
    for key in core::setting(ui,"moduleOrder","").split(',').map(str::trim).chain(TABS_NAMES.iter().map(|t|t.0)){
        if let Some(i)=TABS_NAMES.iter().position(|t|t.0==key){if !order.contains(&i){order.push(i);}}
    }
    order
}
const TABS_NAMES:[(&str,&str);4]=[("home","Home"),("quick","Quick Access"),("clipboard","Clipboard"),("notes","Notes")];

struct Appearance{
    frame:Handle,
    actions:Vec<Handle>,
    scale:Handle,
}
impl Appearance {
    fn new(ui:&Ui,pane:&ScrollPane)->Self{
        use windows_sys::Win32::UI::Controls::{InitCommonControlsEx,INITCOMMONCONTROLSEX,ICC_BAR_CLASSES,TBS_NOTICKS};
        let frame=add(pane.hwnd,"STATIC","Appearance settings",AP_FRAME,WS_DISABLED|0x000D,0,ui.fonts[0]); // SS_OWNERDRAW
        let mut actions=Vec::new();
        for id in CUSTOM_START..SCALE{
            let name=match id {
                CUSTOM_START..=3604=>COLORS[(id-CUSTOM_START)as usize].1,
                THEME_COLORS=>"Use theme colors",
                MOVE_START..=3613=>if (id-MOVE_START)%2==0{"Left"}else{"Right"},
                MODE_START..=3616=>MODES[(id-MODE_START)as usize].1,
                PALETTE_START..=3622=>PALETTES[(id-PALETTE_START)as usize].1,
                REDUCE=>"Reduced motion",_=>"",
            };
            actions.push(button(pane.hwnd,name,id,ui.fonts[0]));
        }
        unsafe{InitCommonControlsEx(&INITCOMMONCONTROLSEX{dwSize:std::mem::size_of::<INITCOMMONCONTROLSEX>()as _,dwICC:ICC_BAR_CLASSES});}
        let scale=add(pane.hwnd,"msctls_trackbar32","Island scale",SCALE,WS_TABSTOP|TBS_NOTICKS,0,ui.fonts[0]);
        unsafe {
            SendMessageW(scale,0x406,1,75|(175<<16)); // TBM_SETRANGE
            SendMessageW(scale,0x417,0,5); // TBM_SETLINESIZE (arrows)
            SendMessageW(scale,0x415,0,10); // TBM_SETPAGESIZE
            windows_sys::Win32::UI::Shell::SetWindowSubclass(scale,Some(scale_input),1,0);
            SetWindowPos(frame,HWND_BOTTOM,0,0,0,0,SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE);
        }
        Self{frame,actions,scale}
    }
    fn hide(&self){for &h in self.actions.iter().chain([&self.frame,&self.scale]){show(h,false);}}
    fn action(&self,id:u16)->Handle {self.actions[(id-CUSTOM_START) as usize]}
    fn layout(&self,ui:&Ui,pane:&ScrollPane,width:i32){
        let w=(width-2).max(80);let x=2;
        pane.place(ui,self.frame,x,0,w,AP_HEIGHT);
        let place=|id,xx,y,ww,h|pane.place(ui,self.action(id),x+xx,y,ww,h);
        for (i,&(key,name,default)) in COLORS.iter().enumerate(){
            let raw=core::setting(ui,key,default);let hex=if raw.is_empty(){default}else{raw};
            caption(self.action(CUSTOM_START+i as u16),&format!("{name} color {hex}"));
            place(CUSTOM_START+i as u16,w-60,72+i as i32*42,50,27);
        }
        place(THEME_COLORS,11,241,w-22,28);
        let order=tab_indices(ui);
        for (i,tab) in order.iter().enumerate(){
            for (direction,word) in ["left","right"].iter().enumerate(){
                let id=MOVE_START+(2*i+direction) as u16;
                caption(self.action(id),&format!("Move {} {word}",TABS_NAMES[*tab].1));
                unsafe {windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow(self.action(id),i32::from(if direction==0{i>0}else{i+1<order.len()}));}
                place(id,w-119+direction as i32*55,310+i as i32*35,50,26);
            }
        }
        for i in 0..3{place(MODE_START+i,w-228+i as i32*76,476,72,29);}
        for i in 0..6{
            let min=i as i32*w/6;let max=(i as i32+1)*w/6;
            place(PALETTE_START+i,min,552,max-min-5,36);
        }
        place(REDUCE,w-30,605,26,30);
        let scale=core::setting(ui,"scale","100").parse::<i32>().unwrap_or(100);
        unsafe{SendMessageW(self.scale,0x405,1,scale as _);} // TBM_SETPOS
        pane.place(ui,self.scale,w-209,646,151,27);
    }
    fn draw(&self,ui:&Ui,di:&DRAWITEMSTRUCT)->bool{
        let p=|n|ui.px(n);let id=di.CtlID as u16;let dc=di.hDC;let r=di.rcItem;
        let (bg,field,ink,accent)=ui.appearance;
        let border=chrome::mix(bg,ink,42);let divider=chrome::mix(bg,ink,32);
        let secondary=chrome::mix(bg,ink,185);let face=chrome::control(ui.appearance);
        let focus=di.itemState&ODS_FOCUS!=0 && di.itemState&ODS_NOFOCUSRECT==0;
        let hot=chrome::hovered(di.hwndItem);
        let left=DT_LEFT|DT_SINGLELINE|DT_VCENTER;let center=DT_CENTER|DT_SINGLELINE|DT_VCENTER;
        if id==AP_FRAME {
            chrome::fill(dc,r,bg);
            let w=r.right;let label_at=|xx,y,ww,ht,value:&str,font,color|chrome::text(dc,chrome::rect(p(xx),p(y),ww,p(ht)),value,font,color,left);
            let section=|y,h,name:&str|{
                chrome::panel(dc,chrome::rect(0,p(y+9),w,p(h-9)),p(8),bg,border);
                let title_w=chrome::width(dc,name,ui.fonts[4]);
                chrome::fill(dc,chrome::rect(p(11),p(y),title_w+p(6),p(21)),bg);
                label_at(13,y,title_w+p(3),20,name,ui.fonts[4],ink);
            };
            label_at(0,10,w,22,"Appearance",ui.fonts[4],ink);
            section(37,243,"Custom colors");
            for (i,&(_,label,_)) in COLORS.iter().enumerate(){
                let y=72+i as i32*42;
                label_at(11,y,w-p(90),27,label,ui.fonts[5],ink);
                chrome::fill(dc,chrome::rect(p(11),p(y+33),w-p(22),p(1)),divider);
            }
            section(285,175,"Tab order");
            for (i,tab) in tab_indices(ui).iter().enumerate(){
                label_at(11,309+i as i32*35,w-p(140),27,TABS_NAMES[*tab].1,ui.fonts[5],ink);
            }
            for (y,label) in [(475,"Theme"),(516,"Color theme"),(605,"Reduced motion"),(646,"Island scale")]{
                label_at(0,y,w-p(180),27,label,ui.fonts[4],ink);
            }
            for y in [510,594,637]{chrome::fill(dc,chrome::rect(0,p(y),w,p(1)),divider);}
            let value=core::setting(ui,"scale","100");
            chrome::text(dc,chrome::rect(w-p(54),p(646),p(50),p(27)),&format!("{value}%"),ui.fonts[0],ink,DT_RIGHT|DT_SINGLELINE|DT_VCENTER);
            return true;
        }
        if !(CUSTOM_START..SCALE).contains(&id){return false;}
        chrome::fill(dc,r,bg);let edge=if focus{accent}else{border};
        let f=if hot{chrome::mix(face,ink,15)}else{face};
        if (CUSTOM_START..THEME_COLORS).contains(&id){
            let (key,_,default)=COLORS[(id-CUSTOM_START)as usize];
            let color=crate::shell::color_value(core::setting(ui,key,"")).or_else(||crate::shell::color_value(default)).unwrap();
            chrome::panel(dc,r,p(5),f,edge);
            chrome::panel(dc,chrome::inset(r,p(5)),0,color,chrome::mix(bg,ink,140));
        }else if (MODE_START..PALETTE_START).contains(&id){
            let (key,name)=MODES[(id-MODE_START) as usize];let active=core::setting(ui,"theme","system")==key;
            let radius=p(7);let cy=r.bottom/2;
            chrome::panel(dc,chrome::rect(p(2),cy-radius,2*radius,2*radius),radius,face,if active||focus{accent}else{secondary});
            if active{chrome::panel(dc,chrome::rect(p(5),cy-p(4),p(8),p(8)),p(4),accent,accent);}
            chrome::text(dc,RECT{left:p(21),..r},name,ui.fonts[0],ink,left);
        }else if (PALETTE_START..REDUCE).contains(&id){
            let (key,name,first,second)=PALETTES[(id-PALETTE_START) as usize];let active=core::setting(ui,"accent","cyan")==key;
            let plate=if active{chrome::mix(bg,accent,28)}else if hot{f}else{field};
            chrome::panel(dc,r,p(8),plate,if active||focus{accent}else{border});
            let disk=chrome::rect(p(6),(r.bottom-p(20))/2,p(20),p(20));let a=crate::shell::color_value(first).unwrap();let b=crate::shell::color_value(second).unwrap();
            chrome::panel(dc,disk,p(10),a,border);
            unsafe{
                let clip=CreateEllipticRgn(disk.left+1,disk.top+1,disk.right-1,disk.bottom-1);
                if !clip.is_null(){let saved=SaveDC(dc);ExtSelectClipRgn(dc,clip,RGN_AND);
                    let points=[windows_sys::Win32::Foundation::POINT{x:disk.right,y:disk.top+p(1)},windows_sys::Win32::Foundation::POINT{x:disk.right,y:disk.bottom},windows_sys::Win32::Foundation::POINT{x:disk.left+p(1),y:disk.bottom}];
                    SelectObject(dc,GetStockObject(DC_BRUSH));SelectObject(dc,GetStockObject(DC_PEN));SetDCBrushColor(dc,b);SetDCPenColor(dc,b);Polygon(dc,points.as_ptr(),3);RestoreDC(dc,saved);DeleteObject(clip);}
            }
            chrome::text(dc,RECT{left:p(31),..r},name,ui.fonts[3],ink,left);
        }else if id==REDUCE {
            let checked=core::setting(ui,"motion","off")=="on";let check=chrome::rect((r.right-p(14))/2,(r.bottom-p(14))/2,p(14),p(14));
            chrome::panel(dc,check,p(3),if checked{accent}else{face},if checked||focus{accent}else{secondary});
            if checked{chrome::icon(dc,check.left,check.top,p(14),Icon::Check,0xffffff,accent);}
        }else{
            chrome::panel(dc,r,p(6),f,edge);
            let word=if id==THEME_COLORS{"Use theme colors"}else if (id-MOVE_START)%2==0{"Left"}else{"Right"};
            let disabled=di.itemState&windows_sys::Win32::UI::Controls::ODS_DISABLED!=0;
            chrome::text(dc,r,word,ui.fonts[0],if disabled{chrome::mix(bg,ink,85)}else{ink},center);
        }
        true
    }
}

unsafe extern "system" fn scale_input(h:Handle,msg:u32,wp:usize,lp:isize,sub:usize,_:usize)->isize{
    use windows_sys::Win32::UI::Shell::{DefSubclassProc,RemoveWindowSubclass};
    if msg==WM_NCDESTROY{RemoveWindowSubclass(h,Some(scale_input),sub);}
    let result=DefSubclassProc(h,msg,wp,lp);
    if msg==WM_LBUTTONUP||msg==WM_KEYUP{
        PostMessageW(GetParent(GetParent(h)),WM_COMMAND,SCALE as _,h as _);
    }
    result
}


impl Ui{
    fn apply_appearance_control(&mut self,id:u16)->Result<(),String>{
        let mut changes=Vec::new();
        if (CUSTOM_START..THEME_COLORS).contains(&id){
            use windows_sys::Win32::UI::Controls::Dialogs::{ChooseColorW,CHOOSECOLORW,CC_FULLOPEN,CC_RGBINIT};
            let (key,_,initial)=COLORS[(id-CUSTOM_START)as usize];
            let color=crate::shell::color_value(core::setting(self,key,"")).or_else(||crate::shell::color_value(initial)).unwrap();
            let mut custom=[0xffffff;16];let mut picker=CHOOSECOLORW::default();
            picker.lStructSize=std::mem::size_of::<CHOOSECOLORW>() as _;picker.hwndOwner=self.hwnd.0;
            picker.rgbResult=color;picker.lpCustColors=custom.as_mut_ptr();picker.Flags=CC_FULLOPEN|CC_RGBINIT;
            if unsafe{ChooseColorW(&mut picker)}==0{return Ok(());}
            let rgb=picker.rgbResult;
            changes.push((key,format!("#{:02x}{:02x}{:02x}",rgb&255,(rgb>>8)&255,(rgb>>16)&255)));
        }else if id==THEME_COLORS {
            for &(key,_,_) in &COLORS {changes.push((key,String::new()));}
        }else if (MOVE_START..MODE_START).contains(&id){
            let n=(id-MOVE_START) as usize;
            let (i,direction)=(n/2,n%2);
            let mut order=tab_indices(self);
            let next=if direction==0{i.checked_sub(1)}else{i.checked_add(1).filter(|&k|k<order.len())};
            let Some(j)=next else{return Ok(())};order.swap(i,j);
            // Preserve native utility placements if an earlier release saved a full navigation order.
            let mut full=core::setting(self,"moduleOrder","").split(',').map(str::trim)
                .filter(|k|!k.is_empty()).map(str::to_owned).collect::<Vec<_>>();
            for &(tab,_) in &TABS_NAMES{if !full.iter().any(|key|key==tab){full.push(tab.into());}}
            let mut next=order.iter();
            for key in &mut full{
                if TABS_NAMES.iter().any(|tab|tab.0==key){
                    if let Some(&index)=next.next(){*key=TABS_NAMES[index].0.into();}
                }
            }
            changes.push(("moduleOrder",full.join(",")));
        }else if (MODE_START..PALETTE_START).contains(&id){
            changes.push(("theme",MODES[(id-MODE_START)as usize].0.to_owned()));
        }else if (PALETTE_START..REDUCE).contains(&id){
            changes.push(("accent",PALETTES[(id-PALETTE_START)as usize].0.to_owned()));
        }else if id==REDUCE{
            changes.push(("motion",if core::setting(self,"motion","off")=="on"{"off"}else{"on"}.to_owned()));
        }else if id==SCALE{
            let Some(slider)=self.preferences.get().and_then(|v|v.appearance.get()).map(|a|a.scale)else{return Ok(());};
            let pct=unsafe{SendMessageW(slider,0x400,0,0)}.clamp(75,175);
            changes.push(("scale",pct.to_string()));
        }
        let previous=self.records.get("settings").cloned();
        for (key,value) in &changes {core::set(self,key,value);}
        if let Err(err)=self.save(){
            if let Some(settings)=previous{self.records.insert("settings".into(),settings);}else{self.records.remove("settings");}
            return Err(err);
        }
        if (MOVE_START..MODE_START).contains(&id){self.reorder_navigation();}
        crate::shell::position(self);
        Ok(())
    }
}
