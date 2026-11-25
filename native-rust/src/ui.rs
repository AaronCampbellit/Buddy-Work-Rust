use windows_sys::Win32::UI::Controls::EM_SETLIMITTEXT;
use std::{path::PathBuf,time::Instant};
use windows::Win32::Foundation::HWND;
use windows_sys::Win32::{Foundation::HWND as Handle,Graphics::Gdi::*,UI::WindowsAndMessaging::*};
use crate::{activity,chrome,core,devices,inline_images,storage,workspace};
mod island_view;
mod widgets_page;
mod workspaces_page;
mod notes_view;
mod preferences;
mod scroll_pane;
pub(crate) use scroll_pane::WheelMotion;
mod devices_view;
mod editor_views;
mod editor_navigation;
mod home_media;
mod notification_popup;
use widgets_page::WidgetsPage;
use island_view::*;

pub(crate) const RESIZE_HANDLE:u16=187;
pub(crate) fn workspace_child(id:i32)->bool { !matches!(id,105|108|180..=183|187) }
pub(crate) const NAV:u16=100;
pub(crate) const LIST:u16=101;
pub(crate) const INPUT:u16=102;
pub(crate) const DETAIL:u16=103;
pub(crate) const COMPACT:u16=105;
pub(crate) const MORE:u16=106;
pub(crate) const MORE_GO:u16=107;
pub(crate) const COMPACT_TOOLS:u16=108;
pub(crate) const ACTION:u16=300;
const PRIMARY:usize=4;
pub(crate) const PAGES:&[(&str,&str)]=&[
    ("home","Home"),("notes","Notes and reminders"),("quick","Quick Access"),("files","File shelf"),
    ("clipboard","Clipboard"),("markup","Image markup"),("ocr","Read image text"),("downloads","Downloads"),
    ("widgets","Timers and widgets"),("media","Media"),("notifications","Notifications"),("audio","Audio devices"),
    ("camera","Camera controls"),("studio","Virtual camera studio"),("rewrite","AI Rewrite"),
    ("settings","Settings"),("diagnostics","Updates and diagnostics"),
];
pub fn wide(s:&str)->Vec<u16>{s.encode_utf16().filter(|&c|c!=0).chain(Some(0)).collect()}
pub(crate) fn window_text(h:Handle,max:usize)->String{
    unsafe {let len=(GetWindowTextLengthW(h).max(0) as usize).min(max);let mut s=vec![0u16;len+1];
        let n=GetWindowTextW(h,s.as_mut_ptr(),s.len() as i32).max(0) as usize;String::from_utf16_lossy(&s[..n])}
}
fn label(h:Handle,s:&str){unsafe{SetWindowTextW(h,wide(s).as_ptr());}}
pub(crate) fn show(h:Handle,v:bool){unsafe{if (GetWindowLongW(h,GWL_STYLE) as u32&WS_VISIBLE!=0)!=v{ShowWindow(h,if v{SW_SHOWNA}else{SW_HIDE});}}}
fn add(hwnd:Handle,class:&str,name:&str,id:u16,style:u32,edge:u32,font:HFONT)->Handle{
    unsafe {
        // A lazy page can be constructed under a visible island. Let layout
        // position each control before Windows can paint its accessible name.
        let child=CreateWindowExW(edge,wide(class).as_ptr(),wide(name).as_ptr(),WS_CHILD|style,0,0,0,0,hwnd,id as usize as _,std::ptr::null_mut(),std::ptr::null());
        chrome::scroll::attach(child,style,class=="COMBOBOX");
        if class == "EDIT" { chrome::edit_menu::attach(child); }
        SendMessageW(child,WM_SETFONT,font as usize,1);
        child
    }
}
pub(crate) fn pos(h:Handle,x:i32,y:i32,w:i32,ht:i32){unsafe{
    let mut r=windows_sys::Win32::Foundation::RECT::default();
    if GetWindowRect(h,&mut r)==0{return}
    let mut point=windows_sys::Win32::Foundation::POINT{x:r.left,y:r.top};
    ScreenToClient(GetParent(h),&mut point);
    if (point.x,point.y,r.right-r.left,r.bottom-r.top)!=(x,y,w.max(1),ht.max(1)){
        SetWindowPos(h,std::ptr::null_mut(),x,y,w.max(1),ht.max(1),SWP_NOACTIVATE|SWP_NOZORDER);
    }
}}
/// The layered motion bitmap paints these sibling HWNDs. Stage their real hit
/// targets together, without painting each individual move on the display.
#[derive(Default)]
struct ChildLayout {
    changes:std::cell::RefCell<Vec<(Handle,i32,i32,i32,i32,u32)>>,
    moving:bool,
}
impl ChildLayout {
    fn new(moving:bool)->Self{Self{moving,..Self::default()}}
    fn place(&self,h:Handle,on:bool,x:i32,y:i32,w:i32,height:i32){
        if !self.moving {
            if on{pos(h,x,y,w,height);}
            show(h,on);
            return
        }
        unsafe{
            let visible=GetWindowLongW(h,GWL_STYLE) as u32&WS_VISIBLE!=0;
            let mut flags=SWP_NOACTIVATE|SWP_NOZORDER|SWP_NOOWNERZORDER|SWP_NOREDRAW|SWP_NOCOPYBITS;
            let (w,height)=(w.max(1),height.max(1));
            let mut changed=false;
            if on{
                let mut r=windows_sys::Win32::Foundation::RECT::default();
                if GetWindowRect(h,&mut r)!=0{
                    let mut origin=windows_sys::Win32::Foundation::POINT{x:r.left,y:r.top};
                    ScreenToClient(GetParent(h),&mut origin);
                    changed=(origin.x,origin.y,r.right-r.left,r.bottom-r.top)!=(x,y,w,height);
                }
            }
            if !changed{flags|=SWP_NOMOVE|SWP_NOSIZE;}
            if on!=visible{flags|=if on{SWP_SHOWWINDOW}else{SWP_HIDEWINDOW};}
            if changed || on!=visible{self.changes.borrow_mut().push((h,x,y,w,height,flags));}
        }
    }
    fn apply(&self){unsafe{
        let changes=self.changes.take();
        if changes.is_empty(){return}
        let mut batch=BeginDeferWindowPos(changes.len() as i32);
        for &(h,x,y,w,height,flags) in &changes{
            if batch.is_null(){break}
            batch=DeferWindowPos(batch,h,std::ptr::null_mut(),x,y,w,height,flags);
        }
        if batch.is_null() || EndDeferWindowPos(batch)==0{
            for (h,x,y,w,height,flags) in changes{SetWindowPos(h,std::ptr::null_mut(),x,y,w,height,flags);}
        }
    }}
}
// Win32 asks for these brushes synchronously while a workspace switch is
// modifying Ui. The small paint state lives independently of that borrow.
#[derive(Clone, Copy)]
struct ControlPaint {
    owner: Handle,
    title: Handle,
    list: Handle,
    colors: chrome::Colors,
}
thread_local! {
    static NATIVE_COLORS: std::cell::Cell<Option<ControlPaint>> = const { std::cell::Cell::new(None) };
}
pub(crate) unsafe fn control_brush(hwnd: Handle, msg: u32, dc: HDC, child: Handle) -> isize {
    let Some(paint) = NATIVE_COLORS.get().filter(|p| p.owner == hwnd) else { return 0; };
    let (background, field, text, accent) = paint.colors;
    let base = if matches!(GetDlgCtrlID(child), 3305 | 3306) { field }
        else if msg == WM_CTLCOLOREDIT { chrome::control(paint.colors) }
        else if msg == WM_CTLCOLORLISTBOX && child != paint.list { field }
        else { background };
    SetBkColor(dc, base);
    SetTextColor(dc, if child == paint.title { accent } else { text });
    SetDCBrushColor(dc, base);
    GetStockObject(DC_BRUSH) as isize
}
pub struct Ui {
    pub hwnd:HWND,pub profile:PathBuf,pub records:storage::Records,pub page:&'static str,pub isolated:bool,
    pub(crate) pending_widget_note:Option<crate::side_widgets::WidgetOpen>,
    pub(crate) pending_page:Option<&'static str>,
    pub(crate) read_error:Option<String>,pub(crate) paused:bool,pub(crate) expanded:bool,
    pub(crate) collapsed_at:Option<Instant>,pub(crate) opened_by_hover:bool,pub(crate) hidden_until:Option<Instant>,
    pub(crate) camera_check:Instant,pub(crate) call_state:Option<String>,pub(crate) locked:bool,pub(crate) suppressed:bool,pub(crate) new_note:bool,pub(crate) dirty_note:bool,pub(crate) due_note:Option<String>,
    pub(crate) active_note:Option<String>,pub(crate) search:String,pub(crate) note_ids:Vec<String>,
    pub(crate) last_removed:Option<crate::core::DeletedNote>,pub(crate) actions:Vec<(&'static str,&'static str)>,
    navigation:Vec<&'static str>,pub(crate) nav:Handle,pub(crate) list:Handle,pub(crate) input_box:Handle,pub(crate) detail_box:Handle,
    title:Handle,input_label:Handle,detail_label:Handle,notice:Handle,more:Handle,more_go:Handle,compact:Handle,compact_tools:Handle,
    pub(crate) images:inline_images::Controls,
    workspaces_page:std::cell::OnceCell<workspaces_page::WorkspacesPage>,
    note_controls:std::cell::OnceCell<notes_view::NoteControls>,
    preferences:std::cell::OnceCell<preferences::Preferences>,
    home_media:std::cell::OnceCell<home_media::Controls>,
    editor_views:std::cell::OnceCell<editor_views::EditorViews>,
    devices_view:std::cell::OnceCell<devices_view::DevicesView>,
    widgets_page:std::cell::OnceCell<WidgetsPage>,
    tab_order:std::cell::RefCell<Vec<Handle>>,
    tabs:Vec<Handle>,header:Vec<Handle>,collapse:Handle,resize:Handle,home_pick:Handle,home_note:Option<String>,search_box:Handle,
    buttons:Vec<Handle>,fonts:Vec<HFONT>,pub(crate) appearance:(u32,u32,u32,u32),font_size:i32,pub(crate) zoom:i32,pub(crate) quitting:bool,
}
impl Ui{
    pub(crate) fn new(hwnd:HWND,profile:PathBuf,isolated:bool)->Result<Self,String>{
        let (records,read_error)=match storage::load(&profile){Ok(s)=>(s,None),Err(e)=>(Default::default(),Some(e))};
        Self::from_records(hwnd,profile,isolated,records,read_error)
    }
    pub(crate) fn from_records(hwnd:HWND,profile:PathBuf,isolated:bool,records:storage::Records,read_error:Option<String>)->Result<Self,String>{
        let fonts=island_view::fonts(1.0);
        let font=fonts[0];
        let head_font=fonts[2];
        let child=|class:&str,title:&str,id:u16,flags:u32,edge:u32|add(hwnd.0,class,title,id,flags,edge,font);
        let nav=child("LISTBOX","Workspaces",NAV,WS_TABSTOP|WS_VSCROLL|LBS_NOTIFY as u32|LBS_NOINTEGRALHEIGHT as u32,WS_EX_CLIENTEDGE);
        for (_,name) in PAGES{unsafe{SendMessageW(nav,LB_ADDSTRING,0,wide(name).as_ptr() as isize);}}
        let title=add(hwnd.0,"STATIC","Buddy",0,0,0,head_font);
        let input_label=child("STATIC","",0,0,0);
        let detail_label=child("STATIC","",0,0,0);
        let list=child("LISTBOX","Items",LIST,WS_TABSTOP|WS_VSCROLL|(LBS_NOTIFY|LBS_NOINTEGRALHEIGHT|LBS_OWNERDRAWFIXED|LBS_HASSTRINGS) as u32,0);
        let input_box=child("EDIT","",INPUT,WS_TABSTOP|ES_AUTOHSCROLL as u32,0);
        let detail_box=child("EDIT","",DETAIL,WS_TABSTOP|WS_VSCROLL|(ES_MULTILINE|ES_WANTRETURN|ES_AUTOVSCROLL) as u32,0);
        let notice=child("STATIC","",0,0,0);
        let more=child("COMBOBOX","More actions",MORE,WS_TABSTOP|CBS_DROPDOWNLIST as u32|WS_VSCROLL,0);
        let button=|s:&str,id:u16|{
            let hwnd=child("BUTTON",s,id,WS_TABSTOP|BS_OWNERDRAW as u32,0);
            chrome::install_button(hwnd);
            hwnd
        };
        let more_go=button("More actions",MORE_GO);
        let compact=button("Open island",COMPACT);
        let compact_tools=button("Recent notifications",COMPACT_TOOLS);
        let tabs=TABS.iter().enumerate().map(|(i,(_,name,_))|button(name,TAB+i as u16)).collect();
        let header=HEADER.iter().enumerate().map(|(i,(_,name,_))|button(name,HEAD+i as u16)).collect();
        let collapse=button("Collapse island",COLLAPSE);
        let resize=button("Resize Buddy height",RESIZE_HANDLE);
        let home_pick=button("Choose home workspace",HOME_PICK);
        let search_box=child("EDIT","",SEARCH,WS_TABSTOP|ES_AUTOHSCROLL as u32,0);
        unsafe{SendMessageW(search_box,0x1501,0,wide("Search notes").as_ptr() as _);}
        let images=inline_images::Controls::new(hwnd.0)?;
        editor_navigation::attach(detail_box);
        editor_navigation::attach(images.editor);
        let buttons=(0..PRIMARY).map(|n|button("",ACTION+n as u16)).collect::<Vec<_>>();
        if [nav,list,title,input_box,detail_box,compact].iter().any(|h|h.is_null()){return Err("Windows could not create Buddy's controls.".into())}
        unsafe {SendMessageW(input_box,EM_SETLIMITTEXT,32768,0);SendMessageW(detail_box,EM_SETLIMITTEXT,256*1024,0);}
        let home_note=records.get("settings").and_then(|rows|rows.iter().find(|r|r.first().is_some_and(|k|k=="homeNote"))).and_then(|r|r.get(1)).cloned();
        Ok(Self{hwnd,profile,records,page:"home",isolated,pending_widget_note:None,pending_page:None,read_error,paused:false,expanded:isolated,collapsed_at:None,opened_by_hover:false,
            hidden_until:None,camera_check:Instant::now(),call_state:None,locked:false,suppressed:false,new_note:false,dirty_note:false,due_note:None,active_note:None,search:String::new(),note_ids:vec![],
            last_removed:None,actions:vec![],navigation:PAGES.iter().map(|p|p.0).collect(),nav,list,input_box,detail_box,title,input_label,detail_label,notice,more,more_go,compact,compact_tools,buttons,
            images,widgets_page:Default::default(),note_controls:Default::default(),workspaces_page:Default::default(),preferences:Default::default(),editor_views:Default::default(),home_media:Default::default(),devices_view:Default::default(),tab_order:Default::default(),tabs,header,collapse,resize,home_pick,home_note,search_box,fonts,appearance:(0xffffff,0xffffff,0x222222,0x814c18),font_size:100,zoom:100,quitting:false})
    }
    fn widgets_page(&self)->&WidgetsPage{
        self.widgets_page.get_or_init(||WidgetsPage::new(self.hwnd.0,self.fonts[0]))
    }
    pub fn input(&self)->String{window_text(self.input_box,32768)}
    pub fn detail(&self)->String{if self.page=="notes"{self.images.body()}else{window_text(self.detail_box,256*1024)}}
    pub fn set_input(&self,s:&str){label(self.input_box,s)}
    pub fn set_detail(&self,s:&str){if self.page=="notes"{self.images.set_body(s)}else{label(self.detail_box,s)}}
    pub fn selected(&self)->Option<usize>{let n=unsafe{SendMessageW(self.list,LB_GETCURSEL,0,0)};(n>=0).then_some(n as usize)}
    pub fn select_row(&self,n:usize){unsafe{SendMessageW(self.list,LB_SETCURSEL,n,0);}}
    pub fn set_rows(&self, rows: &[String]) {
        unsafe {
            // WM_SETREDRAW(TRUE) adds WS_VISIBLE. Background refreshes also use this
            // list while the current workspace is showing its own cards.
            let visible = GetWindowLongW(self.list, GWL_STYLE) as u32 & WS_VISIBLE != 0;
            if visible { SendMessageW(self.list, WM_SETREDRAW, 0, 0); }
            SendMessageW(self.list, LB_RESETCONTENT, 0, 0);
            for row in rows { SendMessageW(self.list, LB_ADDSTRING, 0, wide(row).as_ptr() as _); }
            if visible {
                SendMessageW(self.list, WM_SETREDRAW, 1, 0);
                RedrawWindow(self.list, std::ptr::null(), std::ptr::null_mut(), RDW_INVALIDATE | RDW_FRAME);
            }
        }
    }
    pub fn status(&self,s:&str){
        label(self.notice,s);
        unsafe { RemovePropW(self.notice,windows_sys::w!("BuddyIssue")); }
    }
    pub fn error(&self,s:&str){label(self.notice,s);unsafe{SetPropW(self.notice,wide("BuddyIssue").as_ptr(),1usize as _);}self.layout();}
    pub fn save(&self)->Result<(),String>{if let Some(e)=&self.read_error{return Err(e.clone())}storage::save(&self.profile,&self.records)}
    pub(crate) fn reorder_navigation(&mut self){
        let desired=core::setting(self,"moduleOrder","");
        let mut order=Vec::new();
        for id in desired.split(',').map(str::trim).chain(PAGES.iter().map(|p|p.0)){
            if !order.contains(&id){if let Some(&(id,_))=PAGES.iter().find(|p|p.0==id){order.push(id);}}
        }
        unsafe{
            SendMessageW(self.nav,LB_RESETCONTENT,0,0);
            for id in &order{let name=PAGES.iter().find(|p|p.0==*id).unwrap().1;SendMessageW(self.nav,LB_ADDSTRING,0,wide(name).as_ptr() as isize);}
            SendMessageW(self.nav,LB_SETCURSEL,order.iter().position(|p|*p==self.page).unwrap_or(0),0);
        }
        self.navigation=order;
        self.layout();
    }
    // User commands that only select a page can wait for the outer dispatch
    // to present the switch. Commands that edit the destination immediately
    // use navigate directly.
    pub(crate) fn switch_page(&mut self,page:&'static str){
        if self.expanded && !crate::island::frame().animating && !crate::workspace_masked()
            && crate::stage_selection(self.hwnd.0,page){return}
        self.navigate(page);
    }
    pub fn navigate(&mut self,page:&'static str){
        if self.load_page(page){
            self.layout();
            crate::repaint_after_update(self.hwnd.0);
        }
    }
    pub(crate) fn load_page(&mut self,page:&'static str)->bool{
        self.pending_page=None;
        if self.pending_widget_note.take().is_some(){crate::note_preview::clear();}
        if !PAGES.iter().any(|p|p.0==page){return false;}
        if self.page=="notes" && self.dirty_note {
            if let Err(message)=core::save_note(self) {
                let index=self.navigation.iter().position(|&p|p==self.page).unwrap();
                unsafe{SendMessageW(self.nav,LB_SETCURSEL,index,0);}
                self.error(&message);return false;
            }
        }
        if self.page!=page {devices::leave(self.page);}
        unsafe{RemovePropW(self.notice,wide("BuddyIssue").as_ptr());}
        self.page=page;self.set_rows(&[]);
        // Notes renders its selected title and rich body (or its empty state)
        // together. Clearing Rich Edit here would load an extra blank document.
        if page!="notes"{self.set_input("");self.set_detail("");}
        self.dirty_note=false;
        let index=PAGES.iter().position(|p|p.0==page).unwrap();
        unsafe{SendMessageW(self.nav,LB_SETCURSEL,self.navigation.iter().position(|&p|p==page).unwrap(),0);}
        label(self.title,PAGES[index].1);
        let (i,d)=match page{
            "notes"=>("Title","Note"),"files"=>("Path or folder","File details"),"quick"=>("Target","Name or details"),
            "clipboard"=>("Search or text","Selected item"),"markup"=>("Image or tool","Annotation"),
            "rewrite"=>("Instruction or preset","Text"),"ocr"=>("Image file or clipboard ID","Recognized text"),
            "settings"=>("Value","Settings"),"diagnostics"=>("Update channel","Report"),"studio"=>("Value or source","Scene settings"),
            "audio"|"camera"=>("Device or value","Device details"),"widgets"=>("Minutes or label","Widget details"),
            "downloads"=>("Downloads folder","Selected download"),_=>("Selection","Details")};
        label(self.input_label,i);label(self.detail_label,d);
        let actions=match page{"audio"|"camera"|"studio"|"ocr"=>devices::actions(page),"files"|"quick"|"clipboard"|"markup"|"downloads"=>workspace::actions(page),"media"|"notifications"|"rewrite"|"diagnostics"=>activity::actions(page),_=>core::actions(page)};
        self.actions=actions;self.configure_actions();
        let message=match page{"audio"|"camera"|"studio"|"ocr"=>devices::render(self,page),"files"|"quick"|"clipboard"|"markup"|"downloads"=>workspace::render(self,page),"media"|"notifications"|"rewrite"|"diagnostics"=>activity::render(self,page),_=>core::render(self,page)};
        if page=="notes" && window_text(self.search_box,32768)!=self.search {label(self.search_box,&self.search);}
        self.status(&self.read_error.clone().unwrap_or(message));
        true
    }
    pub(crate) fn run_action(&mut self,key:&str){
        let page=self.page;
        let result=match page{"audio"|"camera"|"studio"|"ocr"=>devices::action(self,page,key),"files"|"quick"|"clipboard"|"markup"|"downloads"=>workspace::action(self,page,key),"media"|"notifications"|"rewrite"|"diagnostics"=>activity::action(self,page,key),_=>core::action(self,page,key)};
        match result{Ok(s) if !s.is_empty()=>self.status(&s),Err(s)=>self.error(&s),_=>{}}
        self.layout();
        if key=="new" && self.page=="notes" {crate::shell::focus(self.input_box);}
    }
    pub(crate) fn command(&mut self,id:u16,code:u16){
        if self.home_media_command(id,code) || self.note_command(id,code) || self.handle_workspaces(id,code) || self.preference_command(id,code) || self.editor_command(id,code) || self.device_command(id,code){return}
        if inline_images::command(self,id,code) || WidgetsPage::command(self,id,code){return}
        if self.island_command(id,code){return}
        if id==NAV && code==LBN_SELCHANGE as u16{let index=unsafe{SendMessageW(self.nav,LB_GETCURSEL,0,0)};if let Some(&p)=self.navigation.get(index as usize){self.switch_page(p);}}
        else if id==LIST && code==LBN_DBLCLK as u16 && matches!(self.page,"quick"|"files"|"downloads"|"clipboard"|"markup"){self.run_action("activate");}
        else if id==LIST && (code==LBN_SELCHANGE as u16 || code==LBN_DBLCLK as u16){self.run_action("select");}
        else if (id==INPUT||id==DETAIL||id==inline_images::NOTE_EDITOR) && self.page=="notes"{if code==EN_CHANGE as u16 && self.active_note.is_some(){self.dirty_note=true;}if code==EN_KILLFOCUS as u16 && self.dirty_note{if let Err(e)=core::save_note(self){self.error(&e)}}}
        else if id==COMPACT_TOOLS {notification_popup::toggle(self);}
        else if id==COMPACT {crate::shell::expand(self,true);}
        else if id==MORE_GO{let n=unsafe{SendMessageW(self.more,CB_GETCURSEL,0,0)};if n>=0{if let Some(&(key,_))=self.actions.get(n as usize+PRIMARY){self.run_action(key);}}}
        else if id>=ACTION && id<ACTION+PRIMARY as u16 {if let Some(&(key,_))=self.actions.get((id-ACTION) as usize){self.run_action(key);}}
    }
    fn configure_actions(&self){
        // The custom pages dispatch their own menus from self.actions. Home
        // and Notes also reuse the first native button and its accessible name.
        if matches!(self.page,"home"|"notes"){
            if let Some(&(_,name))=self.actions.first(){label(self.buttons[0],name);}
            return;
        }
        if workspaces_page::handles(self.page)||editor_views::handles(self.page)
            ||matches!(self.page,"widgets"|"settings"|"audio"|"camera"|"studio"|"notifications"){return;}
        for (n,h) in self.buttons.iter().enumerate(){if let Some((_,s))=self.actions.get(n){label(*h,s);}}
        unsafe{SendMessageW(self.more,CB_RESETCONTENT,0,0);for (_,s) in self.actions.iter().skip(PRIMARY){SendMessageW(self.more,CB_ADDSTRING,0,wide(s).as_ptr() as isize);}SendMessageW(self.more,CB_SETCURSEL,0,0);}
    }
    pub(crate) fn apply_appearance(&mut self){
        self.appearance=crate::shell::palette(self);
        NATIVE_COLORS.set(Some(ControlPaint { owner: self.hwnd.0, title: self.title, list: self.list, colors: self.appearance }));
        chrome::scroll::set_palette(self.appearance.0);
        chrome::menu::set_colors(self.appearance);
        let size=(self.scale()*100.0).round() as i32;
        if size!=self.font_size {
            let updated=island_view::fonts(self.scale());
            if updated.iter().all(|h|!h.is_null()){
                for h in [self.nav,self.list,self.input_box,self.detail_box,self.input_label,self.detail_label,self.notice,self.more,self.search_box]{
                    unsafe{SendMessageW(h,WM_SETFONT,updated[0] as _,0);}
                }
                unsafe{SendMessageW(self.title,WM_SETFONT,updated[2] as _,0);}
                for font in self.fonts.drain(..){unsafe{DeleteObject(font);}}
                self.fonts=updated;self.font_size=size;
            }else{for font in updated{unsafe{DeleteObject(font);}}}
        }
        unsafe{RedrawWindow(self.hwnd.0,std::ptr::null(),std::ptr::null_mut(),RDW_ALLCHILDREN|RDW_ERASE|RDW_INVALIDATE);}
    }
    pub(crate) fn compact_label(&self,s:&str){
        if window_text(self.compact,1024)!=s {label(self.compact,s);}
        unsafe{InvalidateRect(self.hwnd.0,std::ptr::null(),0);}
    }
}
impl Drop for Ui {
    fn drop(&mut self) {
        if NATIVE_COLORS.get().is_some_and(|paint| paint.owner==self.hwnd.0) { NATIVE_COLORS.set(None); }
        for font in self.fonts.drain(..) { unsafe { DeleteObject(font); } }
    }
}
