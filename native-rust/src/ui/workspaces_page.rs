//! Native counterparts of the web ClipboardView, FileShelfView and DownloadsView.
//! The clipped grid recycles real keyboard-accessible Windows buttons as it scrolls.
use super::*;
use chrome::{fill, inset, mix, panel, rect, text};
use std::{cell::{Cell, RefCell}, collections::HashMap, path::Path, ptr::{null, null_mut}, rc::Rc};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    UI::{Controls::{SetScrollInfo, DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_NOFOCUSRECT, ODS_SELECTED},
        Input::KeyboardAndMouse::{EnableWindow, GetFocus, GetKeyState, SetFocus, VK_SHIFT, VK_TAB, VK_DOWN, VK_END, VK_HOME, VK_NEXT, VK_PRIOR, VK_RETURN, VK_UP},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass}},
};
#[path = "workspaces_paint.rs"] mod paint;
use paint::{Symbol, Picture};

const VIEW:u16=2200;
const GRID:u16=2201;
const QUERY:u16=2202;
const FILTER:u16=2203;
const CLEAR:u16=2209;
const TOGGLE:u16=2210;
const DOWNLOADS:u16=2211;
const FILES:u16=2212;
const FOLDER:u16=2213;
const CHOOSE:u16=2214;
const NAME:u16=2215;
const TARGET:u16=2216;
const KIND:u16=2217;
const ADD:u16=2218;
const EXTRA_FILTERS:u16=2219;
const RETURN:u16=2220;
const CLEAR_QUERY:u16=2221;
const FIRST_CARD:u16=2240;
const STRIDE:usize=5;
const MAX_SLOTS:usize=100;
const CONTEXT:u16=0x7700;
const CLIP_FILTERS:[&str;4]=["All","Text","Links","Images"];
const SHELF_FILTERS:[&str;6]=["All","Folders","Files","Images","Apps","Links"];
const LEFT:u32=DT_SINGLELINE|DT_LEFT|DT_VCENTER;
const CENTER:u32=DT_SINGLELINE|DT_CENTER|DT_VCENTER;

pub(super) fn handles(page:&str)->bool{matches!(page,"quick"|"files"|"clipboard"|"downloads")}
fn shelf(page:&str)->bool{matches!(page,"quick"|"files")}
fn field(row:&[String],n:usize)->&str{row.get(n).map_or("",String::as_str)}
fn size(bytes:u64)->String{
    if bytes<1024{format!("{bytes} B")}else if bytes<1048576{format!("{} KB",(bytes+512)/1024)}else{format!("{:.1} MB",bytes as f64/1048576.)}
}
fn image_name(path:&Path)->bool{path.extension().is_some_and(|s|matches!(s.to_string_lossy().to_ascii_lowercase().as_str(),"png"|"jpg"|"jpeg"|"bmp"|"gif"|"webp"|"svg"|"tif"|"tiff"|"heic"|"avif"))}
fn local_stamp(value:&str,date:bool)->String{
    use windows_sys::Win32::{Foundation::{FILETIME,SYSTEMTIME},System::Time::{FileTimeToSystemTime,SystemTimeToTzSpecificLocalTime}};
    #[link(name="kernel32")]
    extern "system"{
        fn GetTimeFormatEx(locale:*const u16,flags:u32,time:*const SYSTEMTIME,format:*const u16,out:*mut u16,size:i32)->i32;
        fn GetDateFormatEx(locale:*const u16,flags:u32,time:*const SYSTEMTIME,format:*const u16,out:*mut u16,size:i32,calendar:*const u16)->i32;
    }
    let Some(t)=value.parse::<u64>().ok().and_then(|s|s.checked_mul(10000)).and_then(|s|s.checked_add(116444736000000000))else{return String::new()};
    let ft=FILETIME{dwLowDateTime:t as u32,dwHighDateTime:(t>>32)as u32};let mut utc=SYSTEMTIME::default();let mut local=SYSTEMTIME::default();let mut buf=[0u16;80];
    unsafe{
        if FileTimeToSystemTime(&ft,&mut utc)==0||SystemTimeToTzSpecificLocalTime(null(),&utc,&mut local)==0{return String::new()}
        let n=if date{GetDateFormatEx(null(),0,&local,wide("MMM d").as_ptr(),buf.as_mut_ptr(),80,null())}else{GetTimeFormatEx(null(),2,&local,null(),buf.as_mut_ptr(),80)};
        if n>1{String::from_utf16_lossy(&buf[..n as usize-1])}else{String::new()}
    }
}

#[derive(Clone)]
struct Card{
    section:&'static str, index:usize, title:String, kind:String, meta:String, stamp:String,
    image:String, icon:Symbol, flags:u8, pinned:bool, available:bool, stored:bool,
}
impl Card{
    fn read(section:&'static str,i:usize,r:&[String],query:&str)->Option<Self>{
        let path=field(r,0);let name=field(r,1);let detail=field(r,2);
        let mut c=Self{section,index:i,title:name.chars().take(720).collect(),kind:String::new(),meta:String::new(),stamp:String::new(),
            image:String::new(),icon:Symbol::File,flags:1,pinned:false,available:true,stored:false};
        match section{
            "clipboard"=>{
                c.kind=match path{"link"=>"Link","image"=>"Image",_=>"Text"}.into();
                let matches=query.is_empty()||if path=="image"{format!("image png {} {}",field(r,4),field(r,5)).contains(query)}else{name.to_lowercase().contains(query)};
                if !matches{return None}
                c.pinned=field(r,3)=="1";c.stamp=local_stamp(detail,false);
                match path{"link"=>{c.icon=Symbol::Link;c.flags|=4;c.meta=c.title.clone();}
                    "image"=>{c.icon=Symbol::Image;c.flags|=8;c.image=name.to_owned();c.title=format!("PNG image · {} × {}",field(r,4),field(r,5));}
                    _=>{c.icon=Symbol::Type;c.flags|=2;}}
            }
            "quick"=>{if detail=="web"{c.kind="Website".into();c.icon=Symbol::Globe;c.flags|=32}else{c.kind="Application".into();c.icon=Symbol::App;c.flags|=16}c.meta=path.into();}
            "files"=>{
                let target=Path::new(path);let st=target.metadata().ok();let folder=st.as_ref().is_some_and(|m|m.is_dir());c.available=st.is_some();c.stored=detail=="copy";
                c.kind=if folder{"Folder".into()}else{target.extension().map_or_else(||"File".into(),|e|e.to_string_lossy().chars().take(12).collect::<String>().to_uppercase())};
                c.flags|=if folder{2}else{4};c.icon=if folder{Symbol::Folder}else if image_name(target){c.flags|=8;c.image=path.into();Symbol::Image}else{Symbol::FileText};
                c.meta=if let Some(st)=st{
                    let date=st.modified().ok().and_then(|t|t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d|local_stamp(&d.as_millis().to_string(),true)).unwrap_or_default();
                    let amount=if folder{String::new()}else{size(st.len())};[amount,date].into_iter().filter(|s|!s.is_empty()).collect::<Vec<_>>().join(" · ")
                }else{"File unavailable".into()};
            }
            "downloads"=>{
                c.available=Path::new(path).exists();c.kind=detail.into();c.icon=if image_name(Path::new(path)){c.image=path.into();Symbol::Image}else{Symbol::File};
                c.meta=format!("{}   {}",size(field(r,3).parse().unwrap_or(0)),local_stamp(field(r,4),true));
                if !c.available{c.meta.push_str("   Missing")}else if detail=="Downloading"{c.meta.push_str("   Downloading")}
            }
            _=>return None,
        }
        Some(c)
    }
    fn actions(&self)->Vec<(&'static str,Symbol,&'static str)>{
        match (self.section,self.kind.as_str()){
            ("clipboard",kind)=>{
                let mut result=match kind{"Image"=>vec![("markup",Symbol::Image,"Open image"),("save",Symbol::Save,"Save image")],"Link"=>vec![("copy",Symbol::Copy,"Copy link")],_=>vec![("rewrite",Symbol::Pencil,"Rewrite text")]};
                result.push(("pin",Symbol::Pin,if self.pinned{"Unpin item"}else{"Pin item"}));result.push(("remove",Symbol::Trash,"Remove item"));result
            }
            ("quick",_)=>vec![("remove",Symbol::Trash,"Remove pin")],
            ("files",_)=>vec![("menu",Symbol::More,"File actions")],
            _=>vec![("reveal",Symbol::FolderOpen,"Show in folder"),("remove",Symbol::Close,"Remove from history")],
        }
    }
}

struct State{
    view:Cell<HWND>,grid:Cell<HWND>,page:Cell<&'static str>,scale:Cell<f32>,colors:Cell<chrome::Colors>,fonts:Cell<[HFONT;4]>,
    filter:Cell<usize>,query:RefCell<String>,form:Cell<bool>,typed_path:Cell<bool>,app:Cell<bool>,counts:Cell<[usize;6]>,
    cards:RefCell<Vec<Card>>,slots:Cell<usize>,first:Cell<usize>,scroll:Cell<i32>,suppress:Cell<bool>,dirty:Cell<bool>,
    highlight:Cell<Option<usize>>, search_rect:Cell<RECT>,grid_top:Cell<i32>,hidden_filter:Cell<usize>,download_counts:Cell<(usize,usize)>,can_clear:Cell<bool>,
    pictures:RefCell<HashMap<String,Option<Rc<Picture>>>>,pending:Cell<bool>,
}
fn client(h:HWND)->RECT{unsafe{let mut r=RECT::default();GetClientRect(h,&mut r);r}}
fn changed_text(h:HWND,s:&str){if window_text(h,1024)!=s{label(h,s)}}
fn on(h:HWND)->bool{unsafe{GetWindowLongW(h,GWL_STYLE)as u32&WS_VISIBLE!=0}}
impl State{
    fn hwnd(&self,id:u16)->HWND{unsafe{GetDlgItem(if id>=FIRST_CARD{self.grid.get()}else{self.view.get()},id as _)}}
    fn px(&self,n:i32)->i32{(n as f32*self.scale.get()).round()as i32}
    fn dp(&self,n:i32)->i32{(n as f32/self.scale.get()).round()as i32}
    fn area(&self,x:i32,y:i32,w:i32,h:i32)->RECT{rect(self.px(x),self.px(y),self.px(x+w)-self.px(x),self.px(y+h)-self.px(y))}
    fn place(&self,id:u16,x:i32,y:i32,w:i32,h:i32){
        let hnd=self.hwnd(id);let r=self.area(x,y,w,h);
        unsafe{SetWindowPos(hnd,null_mut(),r.left,r.top,(r.right-r.left).max(1),(r.bottom-r.top).max(1),SWP_NOACTIVATE|SWP_NOZORDER|SWP_SHOWWINDOW);}
    }
    fn create(&self,parent:HWND,id:u16,name:&str,edit:bool)->HWND{
        let style=if edit{ES_AUTOHSCROLL as u32}else{BS_OWNERDRAW as u32};
        let handle=add(parent,if edit{"EDIT"}else{"BUTTON"},name,id,WS_TABSTOP|style|WS_CLIPSIBLINGS,0,self.fonts.get()[0]);
        if !edit{chrome::install_button(handle)}
        unsafe{SetWindowSubclass(handle,Some(child_proc),2,self as *const Self as usize);}
        handle
    }
    fn set_enabled(&self,id:u16,enabled:bool){unsafe{EnableWindow(self.hwnd(id),enabled as _);}}
    fn set_cue(&self,id:u16,cue:&str){unsafe{SendMessageW(self.hwnd(id),0x1501,1,wide(cue).as_ptr()as _);}}
    fn reload(&self,ui:&Ui){
        let page=self.page.get();let sections:&[&'static str]=if shelf(page){&["files","quick"]}else if page=="clipboard"{&["clipboard"]}else{&["downloads"]};
        let query=self.query.borrow().trim().to_lowercase();let mut counts=[0;6];let mut cards=Vec::new();let mut can_clear=false;let mut active=0;
        for &section in sections{
            for (i,row) in ui.records.get(section).into_iter().flatten().enumerate(){
                if section=="clipboard" && field(row,3)!="1"{can_clear=true}
                if section=="downloads" && field(row,2)=="Downloading"{active+=1}
                // Clipboard totals aren't displayed. Filter before allocating long clipboard strings.
                let Some(card)=Card::read(section,i,row,if section=="clipboard"{&query}else{""})else{continue};
                for (j,n) in counts.iter_mut().enumerate(){if card.flags&(1<<j)!=0{*n+=1}}
                if card.flags&(1<<self.filter.get())==0{continue}
                if section!="clipboard"&&!query.is_empty()&&!row.iter().take(2).any(|s|s.to_lowercase().contains(&query)){continue}
                cards.push(card);
            }
        }
        self.can_clear.set(if page=="clipboard"{can_clear}else{counts[0]>0});
        if page=="clipboard"{counts[0]=ui.records.get("clipboard").map_or(0,Vec::len);}
        self.download_counts.set((active,counts[0].saturating_sub(active)));self.counts.set(counts);self.cards.replace(cards);self.dirty.set(false);
    }
    fn layout(&self){
        let r=client(self.view.get());let w=self.dp(r.right);let page=self.page.get();let q=!self.query.borrow().is_empty();
        let mut keep=Vec::new();
        let mut place=|id,x,y,width,height|{self.place(id,x,y,width,height);keep.push(id);};
        let search;
        self.hidden_filter.set(6);
        let labels:&[&str]=if page=="clipboard"{&CLIP_FILTERS}else{&SHELF_FILTERS};
        for (i,name) in labels.iter().enumerate(){changed_text(self.hwnd(FILTER+i as u16),name)}
        changed_text(self.hwnd(CLEAR),if page=="clipboard"{"Clear unpinned"}else{"Clear history"});
        changed_text(self.hwnd(TOGGLE),if self.form.get(){"Done"}else{"Add app / link"});
        let mut top;
        if page=="clipboard"{
            let widths=[47,54,58,66];let group:i32=widths.iter().sum::<i32>()+3*3;
            let clear=if w>=425{118}else{28};let sw=(w-group-clear-16).max(40);
            search=rect(0,3,sw,28);let mut x=sw+8;
            for (i,width) in widths.into_iter().enumerate(){place(FILTER+i as u16,x,4,width,26);x+=width+3;}
            place(CLEAR,w-clear,3,clear,28);self.set_enabled(CLEAR,self.can_clear.get());top=36;
        }else if shelf(page){
            let small=w+60<=700;let tiny=w<550;let files=if tiny{30}else{80};let folder=if tiny{30}else{90};
            let buttons=84+5+37+5+files+5+folder;let start=w-buttons;
            let sw=if small{(start-8).max(44)}else{((w-520).clamp(75,150)).min((start-105).max(44))};
            search=rect(0,2,sw,28);let mut x=if small{0}else{sw+8};let limit=if small{w}else{start-7};let y=if small{34}else{2};
            let widths=[43,62,51,62,53,53];
            for (i,width) in widths.iter().enumerate(){
                if x+width+if i<5{26}else{0}>limit{
                    self.hidden_filter.set(i);place(EXTRA_FILTERS,x,y,(limit-x).clamp(20,31),27);break
                }
                place(FILTER+i as u16,x,y,*width,27);x+=width+4;
            }
            place(TOGGLE,start,2,84,28);place(DOWNLOADS,start+89,2,37,28);place(FILES,start+131,2,files,28);place(FOLDER,w-folder,2,folder,28);
            top=if small{66}else{38};
            if self.form.get(){
                let y=top+16;
                if self.typed_path.get(){place(TARGET,0,y,w-68,25);place(ADD,w-62,y,62,25)}
                else{
                    let choose=if w<490{29}else{83};let name=((w-choose-188)/3).clamp(55,118);let target=(w-choose-name-144).max(38);
                    place(CHOOSE,0,y,choose,25);place(NAME,choose+7,y,name,25);place(TARGET,choose+name+14,y,target,25);
                    place(KIND,w-121,y,60,25);place(ADD,w-54,y,54,25);
                }
                top+=49;
            }
        }else{
            search=RECT::default();place(CLEAR,w-91,1,91,27);self.set_enabled(CLEAR,self.can_clear.get());top=32;
        }
        self.search_rect.set(search);
        if search.right>0{
            place(QUERY,search.left+28,search.top+4,(search.right-search.left-36-if q{16}else{0}).max(12),20);
            if q{place(CLEAR_QUERY,search.right-22,search.top+5,16,18)}
        }
        for id in QUERY..=CLEAR_QUERY{let hwnd=self.hwnd(id);if !hwnd.is_null()&&!keep.contains(&id)&&on(hwnd){show(hwnd,false)}}
        self.grid_top.set(top);
        self.place(GRID,0,top,w,(self.dp(r.bottom)-top).max(1));
        self.place_grid();
        unsafe{InvalidateRect(self.view.get(),null(),0);}
    }
    fn metrics(&self)->(usize,i32,i32,i32){
        let width=self.dp(client(self.grid.get()).right).max(1);let page=self.page.get();
        let columns=if page=="clipboard"{if width+60<=700{3}else{4}}else if shelf(page){((width+5)/154).max(1)as usize}else{if width>390{2}else{1}};
        let height=if page=="clipboard"{139}else if shelf(page){76}else{62};let gap=if page=="downloads"{7}else{9};
        (columns,height,gap,width)
    }
    fn place_grid(&self){
        let grid=self.grid.get();let (columns,ht,gap,_)=self.metrics();let count=self.cards.borrow().len();let rows=count.div_ceil(columns)as i32;
        let height=self.dp(client(grid).bottom).max(1);let content=5+rows*(ht+gap);let scroll=self.scroll.get().clamp(0,(content-height).max(0));
        self.scroll.set(scroll);
        unsafe{let info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as _,fMask:SIF_RANGE|SIF_PAGE|SIF_POS,nMin:0,nMax:content-1,nPage:height as _,nPos:scroll,..Default::default()};SetScrollInfo(grid,SB_VERT,&info,1);}
        let (columns,_,_,width)=self.metrics();let first_row=(scroll-5).max(0)/(ht+gap);let first=first_row as usize*columns;self.first.set(first);
        let rows_visible=(height+ht+gap*2)/(ht+gap)+1;
        let limit=count.saturating_sub(first).min(rows_visible as usize*columns).min(MAX_SLOTS);
        for slot in self.slots.get()..limit{
            let id=FIRST_CARD+(slot*STRIDE)as u16;
            // Overlaid actions precede the full-size button in z-order.
            for j in 1..STRIDE{self.create(grid,id+j as u16,"Item action",false);}
            self.create(grid,id,"Open item",false);
        }
        self.slots.set(self.slots.get().max(limit));
        let cards=self.cards.borrow();let cell_width=width-4-(columns as i32-1)*gap;
        for slot in 0..self.slots.get(){
            let id=FIRST_CARD+(slot*STRIDE)as u16;
            if slot>=limit{for j in 0..STRIDE{let h=self.hwnd(id+j as u16);if on(h){show(h,false)}}continue}
            let index=first+slot;let entry=&cards[index];let col=(index%columns)as i32;
            let x=2+col*gap+cell_width*col/columns as i32;
            let cw=cell_width*(col+1)/columns as i32-cell_width*col/columns as i32;
            let y=5+(index/columns)as i32*(ht+gap)-scroll;
            self.place(id,x,y,cw,ht);self.set_enabled(id,entry.available);
            let action=if entry.section=="clipboard" && entry.kind!="Link"{"Copy"}else{"Open"};
            changed_text(self.hwnd(id),&format!("{action} {}",entry.title.chars().take(80).collect::<String>()));
            let actions=entry.actions();
            for j in 1..STRIDE{
                let aid=id+j as u16;let Some((key,_,desc))=actions.get(j-1)else{show(self.hwnd(aid),false);continue};
                let (ax,ay,aw,ah)=match entry.section{
                    "clipboard"=>(x+cw-5-((actions.len()+1-j)as i32)*25,y+ht-29,24,24),
                    "downloads"=>(x+cw-(3-j as i32)*28,y+2,26,ht-4),
                    _=>(x+cw-25,y+3,22,22)
                };
                self.place(aid,ax,ay,aw,ah);self.set_enabled(aid,!(*key=="remove"&&entry.pinned||*key=="reveal"&&!entry.available));
                changed_text(self.hwnd(aid),&format!("{desc}: {}",entry.title.chars().take(60).collect::<String>()));
            }
        }
        drop(cards);
        unsafe{RedrawWindow(grid,null(),null_mut(),RDW_INVALIDATE|RDW_ALLCHILDREN);}
        if !self.pending.replace(true){unsafe{PostMessageW(GetParent(self.view.get()),WM_COMMAND,RETURN as _,0);}}
    }
    fn scroll_to(&self,pos:i32){self.scroll.set(pos.max(0));self.place_grid();}
    fn initialize_pictures(&self,ui:&Ui){
        self.pending.set(false);let cards=self.cards.borrow();let visible=cards.iter().skip(self.first.get()).take(self.slots.get()).filter(|c|!c.image.is_empty());
        let mut pics=self.pictures.borrow_mut();let keys:Vec<_>=visible.map(|c|(c.section,c.image.clone())).collect();
        pics.retain(|key,_|keys.iter().any(|(_,s)|s==key));
        for (section,key) in keys{
            if pics.contains_key(&key){continue}
            let rgba=if section=="clipboard"{workspace::widget_thumbnail(ui,&key,400,260)}else{workspace::file_card_thumbnail(Path::new(&key))};
            pics.insert(key,rgba.ok().map(|img|Rc::new(Picture::new(img))));
        }
        unsafe{RedrawWindow(self.grid.get(),null(),null_mut(),RDW_INVALIDATE|RDW_ALLCHILDREN);}
    }
    fn card(&self,id:u16)->Option<(Card,usize)>{
        let i=id.checked_sub(FIRST_CARD)? as usize;let card=self.cards.borrow().get(self.first.get()+i/STRIDE)?.clone();
        Some((card,i%STRIDE))
    }
}

pub(super) struct WorkspacesPage{state:Box<State>}
impl WorkspacesPage{
    fn new(ui:&Ui)->Self{
        let state=Box::new(State{view:Cell::new(null_mut()),grid:Cell::new(null_mut()),page:Cell::new(""),scale:Cell::new(ui.scale()),
            colors:Cell::new(ui.appearance),fonts:Cell::new([ui.fonts[0],ui.fonts[1],ui.fonts[3],ui.fonts[4]]),
            filter:Default::default(),query:Default::default(),form:Default::default(),typed_path:Default::default(),app:Cell::new(true),
            counts:Default::default(),cards:Default::default(),slots:Default::default(),first:Default::default(),scroll:Default::default(),dirty:Cell::new(true),
            highlight:Cell::new(None),search_rect:Cell::new(RECT::default()),grid_top:Default::default(),hidden_filter:Default::default(),download_counts:Default::default(),
            can_clear:Default::default(),pictures:Default::default(),pending:Default::default(),suppress:Default::default()});
        let view=add(ui.hwnd.0,"STATIC","Workspace",VIEW,WS_CLIPCHILDREN,WS_EX_CONTROLPARENT,ui.fonts[0]);
        state.view.set(view);show(view,false);
        let ptr=state.as_ref()as *const State as usize;
        unsafe{SetWindowSubclass(view,Some(view_proc),1,ptr);}
        let specs=[(QUERY,"Search"),(FILTER,"All"),(FILTER+1,"Text"),(FILTER+2,"Links"),(FILTER+3,"Images"),(FILTER+4,"Apps"),(FILTER+5,"Links"),
            (EXTRA_FILTERS,"More filters"),(CLEAR,"Clear history"),(TOGGLE,"Add app / link"),(DOWNLOADS,"Downloads"),(FILES,"Add files"),(FOLDER,"Add folder"),
            (CHOOSE,"Choose app"),(NAME,""),(TARGET,""),(KIND,"Choose type"),(ADD,"Add"),(CLEAR_QUERY,"Clear search")];
        for (id,name) in specs{let h=state.create(view,id,name,matches!(id,QUERY|NAME|TARGET));show(h,false);}
        state.set_cue(NAME,"Project wiki");state.set_cue(TARGET,r"C:\Path\App.exe");
        let grid=add(view,"STATIC","Items",GRID,WS_VSCROLL|WS_CLIPCHILDREN,WS_EX_CONTROLPARENT,ui.fonts[0]);
        state.grid.set(grid);unsafe{SetWindowSubclass(grid,Some(view_proc),1,ptr);}
        Self{state}
    }
}

impl Ui{
    pub(crate) fn markup_glyph(dc:HDC,x:i32,y:i32,side:i32,command:usize,color:u32,background:u32){
        use Symbol::*;
        let icon=match command{100=>Pointer,101=>Pencil,102=>Arrow,103=>Square,104=>Circle,105=>Type,106=>Highlight,107=>Redact,
            108=>Number,109=>Crop,11=>Undo,12=>Redo,13=>Trash,401=>History,300=>ZoomIn,301=>ZoomOut,420|422=>Minus,421|423=>Plus,
            1=>Copy,2|4=>Save,3=>Eye,6=>Close,_=>return};
        paint::symbol(dc,x,y,side,icon,color,background);
    }
    pub(super) fn layout_workspaces(&self,visible:bool,r:RECT){
        if !visible{
            if let Some(widget)=self.workspaces_page.get(){
                let state=&widget.state;if on(state.view.get()){show(state.view.get(),false);state.pictures.borrow_mut().clear();state.dirty.set(true);}
            }
            return
        }
        let w=self.workspaces_page.get_or_init(||WorkspacesPage::new(self));let s=&w.state;
        let new_page=s.page.replace(self.page)!=self.page;
        let mut changed=s.dirty.get();
        if new_page{
            s.suppress.set(true);label(s.hwnd(QUERY),"");label(s.hwnd(NAME),"");label(s.hwnd(TARGET),"");s.suppress.set(false);
            s.set_cue(QUERY,if self.page=="clipboard"{"Search local history"}else{"Search items"});
            s.query.borrow_mut().clear();s.filter.set(0);s.scroll.set(0);s.form.set(false);s.typed_path.set(false);s.app.set(true);s.highlight.set(None);
            s.pictures.borrow_mut().clear();changed=true;
        }
        let fonts=[self.fonts[0],self.fonts[1],self.fonts[3],self.fonts[4]];
        if s.fonts.replace(fonts)!=fonts{for id in [QUERY,NAME,TARGET]{unsafe{SendMessageW(s.hwnd(id),WM_SETFONT,fonts[2]as _,0);}}changed=true}
        changed|=s.scale.replace(self.scale())!=self.scale()||s.colors.get()!=self.appearance;s.colors.set(self.appearance);
        let h=s.view.get();let mut old=RECT::default();unsafe{GetWindowRect(h,&mut old);let mut points=[POINT{x:old.left,y:old.top},POINT{x:old.right,y:old.bottom}];MapWindowPoints(null_mut(),self.hwnd.0,points.as_mut_ptr(),2);old=RECT{left:points[0].x,top:points[0].y,right:points[1].x,bottom:points[1].y};}
        if (old.left,old.top,old.right,old.bottom)!=(r.left,r.top,r.right,r.bottom)||!on(h){unsafe{SetWindowPos(h,null_mut(),r.left,r.top,r.right-r.left,r.bottom-r.top,SWP_NOACTIVATE|SWP_NOZORDER|SWP_SHOWWINDOW);}changed=true}
        if changed{
            if new_page||s.dirty.get(){s.reload(self)}
            s.layout();s.initialize_pictures(self);
        }
    }
    pub(crate) fn reveal_clipboard_card(&self,index:usize){
        let Some(w)=self.workspaces_page.get()else{return};let s=&w.state;
        if s.page.get()!="clipboard"{return}
        s.suppress.set(true);label(s.hwnd(QUERY),"");s.suppress.set(false);
        s.query.borrow_mut().clear();s.filter.set(0);s.highlight.set(Some(index));s.reload(self);
        let (columns,height,gap,_)=s.metrics();
        s.scroll.set((index/columns)as i32*(height+gap));s.layout();s.initialize_pictures(self);
        unsafe{SetTimer(s.grid.get(),2,5000,None);}
    }
    pub(crate) fn refresh_workspaces(&self){
        if let Some(w)=self.workspaces_page.get(){w.state.dirty.set(true);if w.state.page.get()==self.page&&handles(self.page)&&on(w.state.view.get()){
            w.state.reload(self);w.state.layout();w.state.initialize_pictures(self);
        }}
    }
    fn dispatch_card(&mut self,section:&str,index:Option<usize>,action:&str)->bool{
        match workspace::card_action(self,section,index,action){
            Ok(s)=>{if !s.is_empty(){self.status(&s)}self.layout();s!="Canceled."}
            Err(e)=>{self.error(&e);false}
        }
    }
    pub(super) fn handle_workspaces(&mut self,id:u16,code:u16)->bool{
        if !(VIEW..2800).contains(&id){return false}
        let Some(w)=self.workspaces_page.get()else{return true};let s=&w.state;
        if !handles(self.page)||s.page.get()!=self.page||!on(s.view.get())||s.suppress.get(){return true}
        if id==RETURN{s.initialize_pictures(self);return true}
        if id==QUERY&&code==EN_CHANGE as u16{
            s.query.replace(window_text(s.hwnd(QUERY),32768));s.scroll.set(0);s.reload(self);s.layout();return true
        }
        if code!=BN_CLICKED as u16 && code!=CONTEXT{return true}
        let page=self.page;
        if id>=FIRST_CARD{
            let Some((card,part))=s.card(id)else{return true};
            let actions=card.actions();
            let action=if code==CONTEXT{
                if card.section=="clipboard"&&card.kind=="Image"{"markup"}else{"menu"}
            }else if part==0{
                if card.section=="clipboard"&&card.kind!="Link"{"copy"}else{"open"}
            }else if let Some((action,_,_))=actions.get(part-1){action}else{return true};
            let action=if action=="menu"{
                let options:Vec<(&str,&str)>=match card.section{
                    "files"=>vec![("open","Open"),("copyitem","Copy item"),("copypath","Copy path"),("reveal","Show in folder"),("preview","Preview"),if card.stored{("trash","Delete saved copy")}else{("remove","Remove from shelf")}],
                    "quick"=>vec![("open","Open pin"),("remove","Remove pin")],
                    "downloads"=>vec![("open","Open download"),("reveal","Show in folder"),("copypath","Copy path"),("preview","Preview"),("remove","Remove from history")],
                    _=>{let mut v=vec![("copy","Copy"),("widget","Show on widget"),("unwidget","Hide widget"),("pin","Pin / unpin")];
                        if card.kind=="Image"{v.extend([("markup","View and edit image"),("ocr","Recognize text"),("save","Export PNG")]);}
                        else if card.kind=="Link"{v.push(("open","Open link"))}else{v.push(("rewrite","Rewrite text"))}
                        if !card.pinned{v.push(("remove","Remove item"))}v},
                };
                let labels=options.iter().map(|p|p.1.to_string()).collect::<Vec<_>>();
                let Some(i)=self.choose(s.hwnd(id),&labels)else{return true};options[i].0
            }else{action};
            self.dispatch_card(card.section,Some(card.index),action);return true
        }
        if code==CONTEXT{
            let options:&[(&str,&str)]=match page{
                "clipboard"=>&[("capture","Read current clipboard"),("unwidget","Hide clipboard widget"),("markup","Image markup"),("ocr","Read image text"),("rewrite","AI Rewrite")],
                "downloads"=>&[("refresh","Refresh downloads"),("quick","Back to Quick Access")],
                _=>&[("copyfile","Save a file copy"),("copyfolder","Save a folder copy"),("addpath","Link a typed path"),("store","Open saved files"),("relocate","Change storage folder"),("downloads","Downloads")],
            };
            let labels=options.iter().map(|p|p.1.to_string()).collect::<Vec<_>>();
            let anchor=if matches!(id,VIEW|GRID){s.grid.get()}else{s.hwnd(id)};
            if let Some(i)=self.choose(anchor,&labels){let key=options[i].0;
                match key{"markup"|"ocr"|"rewrite"|"quick"|"downloads"=>self.navigate(key),
                    "addpath"=>{s.typed_path.set(true);s.form.set(true);s.set_cue(TARGET,r"C:\Path\File");label(s.hwnd(TARGET),"");s.layout();crate::shell::focus(s.hwnd(TARGET));},
                    _=>{self.dispatch_card(if shelf(page){"files"}else{page},None,key);}
                }
            }
            return true
        }
        match id{
            x if (FILTER..FILTER+6).contains(&x)=>{s.filter.set((id-FILTER)as usize);s.scroll.set(0);s.reload(self);s.layout();}
            EXTRA_FILTERS=>{let start=s.hidden_filter.get();let counts=s.counts.get();let labels=(start..6).map(|i|format!("{}  {}",SHELF_FILTERS[i],counts[i])).collect::<Vec<_>>();
                let picked=if labels.len()==1{Some(0)}else{self.choose(s.hwnd(id),&labels)};
                if let Some(i)=picked{s.filter.set(start+i);s.scroll.set(0);s.reload(self);s.layout();}}
            CLEAR_QUERY=>{label(s.hwnd(QUERY),"");s.query.borrow_mut().clear();s.scroll.set(0);s.reload(self);s.layout();crate::shell::focus(s.hwnd(QUERY));}
            CLEAR=>{self.dispatch_card(page,None,"clear");}
            TOGGLE=>{s.form.set(!s.form.get());s.typed_path.set(false);s.set_cue(TARGET,if s.app.get(){r"C:\Path\App.exe"}else{"https://example.com"});s.layout();if s.form.get(){crate::shell::focus(s.hwnd(NAME))}}
            KIND=>{let labels=vec!["App".into(),"Website".into()];if let Some(i)=self.choose(s.hwnd(id),&labels){s.app.set(i==0);s.set_cue(TARGET,if i==0{r"C:\Path\App.exe"}else{"https://example.com"});s.layout();}}
            DOWNLOADS=>self.navigate("downloads"),
            FILES|FOLDER|CHOOSE|ADD=>{
                let typed=s.typed_path.get();let app=s.app.get();let title=window_text(s.hwnd(NAME),120);let target=window_text(s.hwnd(TARGET),32768);
                let (section,key)=match id{FILES=>("files","pick"),FOLDER=>("files","folder"),CHOOSE=>("quick","app"),_ if typed=>("files","addpath"),_ if app=>("quick","addapp"),_=>("quick","addweb")};
                if id==ADD{
                    if !typed&&title.trim().is_empty(){self.error("Enter a label for the pin.");return true}
                    self.set_input(&target);self.set_detail(&title);
                }
                if self.dispatch_card(section,None,key){
                    let s=&self.workspaces_page.get().unwrap().state;s.suppress.set(true);label(s.hwnd(QUERY),"");s.suppress.set(false);s.query.borrow_mut().clear();s.filter.set(0);s.scroll.set(0);
                    if matches!(id,CHOOSE|ADD){s.form.set(false);label(s.hwnd(NAME),"");label(s.hwnd(TARGET),"");s.app.set(true);}
                    s.reload(self);s.layout();
                }
            }
            _=>{}
        }
        true
    }
}

unsafe extern "system" fn child_proc(hwnd:HWND,msg:u32,wp:usize,lp:isize,sub:usize,data:usize)->isize{
    let state=&*(data as *const State);let id=GetDlgCtrlID(hwnd)as u16;
    match msg{
        WM_SETFOCUS|WM_KILLFOCUS=>{InvalidateRect(hwnd,null(),0);InvalidateRect(state.view.get(),null(),0);}
        WM_MOUSEWHEEL=>return SendMessageW(state.grid.get(),msg,wp,lp),
        WM_CONTEXTMENU if !matches!(id,NAME|TARGET|QUERY)=>{return SendMessageW(GetParent(state.view.get()),WM_COMMAND,((CONTEXT as usize)<<16)|id as usize,hwnd as _);}
        WM_GETDLGCODE if id>=FIRST_CARD||matches!(id,NAME|TARGET)=>{
            if lp!=0{let key=(*(lp as *const MSG)).wParam as u16;
                if matches!(key,VK_TAB|VK_DOWN|VK_UP|VK_PRIOR|VK_NEXT|VK_HOME|VK_END)&&id>=FIRST_CARD||key==VK_RETURN&&matches!(id,NAME|TARGET){return DefSubclassProc(hwnd,msg,wp,lp)|DLGC_WANTMESSAGE as isize}
            }
        }
        WM_KEYDOWN if matches!(id,NAME|TARGET)&&wp==VK_RETURN as usize=>{PostMessageW(GetParent(state.view.get()),WM_COMMAND,ADD as _,0);return 0;}
        WM_KEYDOWN if id>=FIRST_CARD=>{
            let (columns,ht,gap,_)=state.metrics();let index=state.first.get()+(id-FIRST_CARD)as usize/STRIDE;
            let n=state.cards.borrow().len();let rows=(state.dp(client(state.grid.get()).bottom)/(ht+gap)).max(1)as usize;
            if wp==VK_TAB as usize{
                let backwards=GetKeyState(VK_SHIFT as i32)<0;let mut item=index;let mut part=(id-FIRST_CARD)as usize%STRIDE;
                loop{
                    let cards=state.cards.borrow();let Some(card)=cards.get(item)else{break};let actions=card.actions();
                    if backwards{
                        if part>0{part-=1;}else if item>0{item-=1;part=cards[item].actions().len();}else{
                            drop(cards);let candidates=if shelf(state.page.get()){[ADD,FOLDER,TOGGLE]}else{[CLEAR,FILTER+3,QUERY]};
                            if let Some(h)=candidates.into_iter().map(|id|state.hwnd(id)).find(|h|on(*h)&&windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled(*h)!=0){SetFocus(h);}
                            return 0
                        }
                    }else if part<actions.len(){part+=1;}else if item+1<n{item+=1;part=0;}else{SetFocus(GetDlgItem(GetParent(state.view.get()),184));return 0}
                    let card=&cards[item];let actions=card.actions();
                    let enabled=if part==0{card.available}else{actions.get(part-1).is_some_and(|(a,_,_)|!(*a=="remove"&&card.pinned||*a=="reveal"&&!card.available))};
                    drop(cards);if !enabled{continue}
                    let top=5+(item/columns)as i32*(ht+gap);let height=state.dp(client(state.grid.get()).bottom);
                    if top<state.scroll.get()||top+ht>state.scroll.get()+height{state.scroll_to(if top<state.scroll.get(){top}else{top+ht-height});}
                    if let Some(slot)=item.checked_sub(state.first.get()).filter(|n|*n<state.slots.get()){SetFocus(state.hwnd(FIRST_CARD+(slot*STRIDE+part)as u16));}
                    return 0
                }
            }
            let target=match wp as u16{VK_DOWN=>index.saturating_add(columns),VK_UP=>index.saturating_sub(columns),VK_NEXT=>index.saturating_add(rows*columns),VK_PRIOR=>index.saturating_sub(rows*columns),VK_HOME=>0,VK_END=>n.saturating_sub(1),_=>return DefSubclassProc(hwnd,msg,wp,lp)}.min(n.saturating_sub(1));
            let top=5+(target/columns)as i32*(ht+gap);let height=state.dp(client(state.grid.get()).bottom);
            state.scroll_to(if top<state.scroll.get(){top}else{state.scroll.get().max(top+ht-height)});
            if let Some(slot)=target.checked_sub(state.first.get()).filter(|s|*s<state.slots.get()){SetFocus(state.hwnd(FIRST_CARD+(slot*STRIDE)as u16));}
            return 0
        }
        WM_NCDESTROY=>{RemoveWindowSubclass(hwnd,Some(child_proc),sub);}
        _=>{}
    }
    DefSubclassProc(hwnd,msg,wp,lp)
}

unsafe extern "system" fn view_proc(hwnd:HWND,msg:u32,wp:usize,lp:isize,sub:usize,data:usize)->isize{
    let s=&*(data as *const State);let grid=hwnd==s.grid.get();
    match msg{
        WM_COMMAND=>return SendMessageW(GetParent(s.view.get()),WM_COMMAND,wp,lp),
        WM_CONTEXTMENU=>return SendMessageW(GetParent(s.view.get()),WM_COMMAND,((CONTEXT as usize)<<16)|if grid{GRID}else{VIEW}as usize,hwnd as _),
        WM_DRAWITEM if lp!=0=>{s.draw(&*(lp as *const DRAWITEMSTRUCT));return 1;}
        WM_CTLCOLOREDIT=>{let (_,field,ink,_)=s.colors.get();let dc=wp as HDC;SetTextColor(dc,ink);SetBkColor(dc,field);SetDCBrushColor(dc,field);return GetStockObject(DC_BRUSH)as _;}
        WM_PAINT=>{let mut ps=PAINTSTRUCT::default();let dc=BeginPaint(hwnd,&mut ps);s.paint(dc,grid);EndPaint(hwnd,&ps);return 0;}
        WM_PRINT|WM_PRINTCLIENT=>{if wp!=0{s.paint(wp as HDC,grid);if lp as i32&PRF_CHILDREN!=0{paint::print_children(hwnd,wp as HDC);}}return 0;}
        WM_ERASEBKGND=>return 1,
        WM_MOUSEWHEEL=>{s.scroll_to(s.scroll.get()-(((wp>>16)as i16)as i32)*45/120);return 0;}
        WM_VSCROLL if grid=>{let height=s.dp(client(hwnd).bottom);let delta=match (wp&0xffff)as i32{
            SB_LINEDOWN=>25,SB_LINEUP=>-25,SB_PAGEDOWN=>height,SB_PAGEUP=>-height,SB_TOP=>-s.scroll.get(),SB_BOTTOM=>i32::MAX/2,
            SB_THUMBTRACK|SB_THUMBPOSITION=>{let mut info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as _,fMask:SIF_TRACKPOS,..Default::default()};GetScrollInfo(hwnd,SB_VERT,&mut info);info.nTrackPos-s.scroll.get()},_=>0};
            s.scroll_to(s.scroll.get()+delta);return 0;}
        WM_TIMER if wp==2=>{KillTimer(hwnd,2);s.highlight.set(None);RedrawWindow(hwnd,null(),null_mut(),RDW_INVALIDATE|RDW_ALLCHILDREN);return 0;}
        WM_NCDESTROY=>{RemoveWindowSubclass(hwnd,Some(view_proc),sub);}
        _=>{}
    }
    DefSubclassProc(hwnd,msg,wp,lp)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn shelf_uses_the_source_collection_and_clipboard_search_uses_original_text(){
        let pin=vec!["https://example.com".into(),"Team handbook".into(),"web".into()];
        let card=Card::read("quick",2,&pin,"").unwrap();
        assert_eq!(card.section,"quick");
        assert_eq!(card.index,2);
        assert_ne!(card.flags&(1<<5),0);
        assert_eq!(card.flags&(1<<2),0);
        let clip=vec!["text".into(),format!("{} find this", "a".repeat(1000)),"0".into(),"1".into()];
        let card=Card::read("clipboard",10,&clip,"find this").unwrap();
        assert!(card.pinned);
        assert!(card.title.len()<clip[1].len());
        let image=vec!["image".into(),"abc".into(),"0".into(),"0".into(),"320".into(),"200".into()];
        assert!(Card::read("clipboard",0,&image,"320").is_some());
        assert!(Card::read("clipboard",0,&image,"unrelated").is_none());
    }
}
