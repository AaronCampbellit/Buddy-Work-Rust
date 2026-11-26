mod bitmap;
mod files;
mod markup;
mod platform;
pub(crate) use platform::formatted_clipboard;
use crate::Ui;
use bitmap::{Bitmap,MAX_IMAGE};
use sha2::{Digest,Sha256};
use std::{cell::RefCell,collections::HashMap,ffi::OsStr,fs,path::{Path,PathBuf},time::{SystemTime,UNIX_EPOCH}};
use platform::Clip;
pub(crate) use files::valid_url;
pub(crate) fn copy_editor_text(ui:&Ui,value:&str)->Result<(),String>{platform::copy_text(ui.hwnd.0,value)}
pub(crate) fn open_web_link(hwnd:windows_sys::Win32::Foundation::HWND,url:&str)->Result<(),String>{
    if !valid_url(url){return Err("Only HTTP and HTTPS links may be opened.".into());}
    platform::open(hwnd,OsStr::new(url))
}
const MAX_STORE:u64=100*1024*1024;
fn now()->u128 {SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()}
fn get(row:&[String],i:usize)->&str{row.get(i).map(String::as_str).unwrap_or("")}
fn key(page:&str)->&str{if page=="markup"{"clipboard"}else{page}}
fn shortened(s:&str,max:usize)->String{let mut c=s.chars();let mut r:String=c.by_ref().take(max).map(|c|if c.is_control(){' '}else{c}).collect();if c.next().is_some(){r.push('…');}r}
#[derive(Default)]
struct Tracker{sequence:Option<u32>,downloads:files::Downloads,dir:Option<PathBuf>,watch:Option<platform::FolderChanges>,dl_at:u128,settle_at:Option<u128>,query:HashMap<String,String>,type_filter:Option<String>,fresh:Option<(u128,String,String)>}
thread_local! {static TRACK:RefCell<Tracker>=RefCell::new(Tracker::default());}
fn id_valid(id:&str)->bool{id.len()==64&&id.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn image_path(ui:&Ui,id:&str)->Result<PathBuf,String>{if !id_valid(id){return Err("Invalid image identifier.".into());}Ok(ui.profile.join("images").join(format!("{id}.dpapi")))}
fn annotation_path(ui:&Ui,id:&str)->PathBuf{ui.profile.join("markup").join(format!("{id}.dpapi"))}
fn referenced(ui:&Ui,id:&str)->bool {
    if ui.images.references(id){return true;}
    ui.records.values().flatten().flatten().any(|field|field==id)
        || ui.last_removed.as_ref().is_some_and(|note|note.row.iter().any(|field|field==id))
}
fn sweep(ui:&Ui){
    let dir=ui.profile.join("images");
    if let Ok(entries)=fs::read_dir(dir){
        for e in entries.flatten(){
            let p=e.path();let id=p.file_stem().and_then(OsStr::to_str).unwrap_or("");
            if p.extension()==Some(OsStr::new("dpapi"))&&id_valid(id)&&!referenced(ui,id) {
                let _=fs::remove_file(&p);let _=fs::remove_file(annotation_path(ui,id));
            }
        }
    }
}
fn store_image(ui:&Ui,image:&Bitmap)->Result<String,String>{
    let bytes=image.png()?;let id=format!("{:x}",Sha256::digest(&bytes));let path=image_path(ui,&id)?;
    if !path.exists(){
        let dir=path.parent().unwrap();let used=match fs::read_dir(dir){
            Ok(items)=>items.filter_map(|e|e.ok()?.metadata().ok().map(|m|m.len())).sum::<u64>(),
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>0,Err(e)=>return Err(e.to_string())};
        if used.saturating_add(bytes.len() as u64 + 4096)>MAX_STORE{return Err("Clipboard images have reached 100 MB. Remove unused images to continue.".into());}
        crate::storage::write_protected(&path,&bytes)?;
    }
    Ok(id)
}
pub fn image_png(ui:&Ui,id:&str)->Result<Vec<u8>,String>{
    let data=crate::storage::read_protected(&image_path(ui,id)?,MAX_IMAGE)?;
    if format!("{:x}",Sha256::digest(&data))!=id{return Err("Saved image validation failed.".into());}Ok(data)
}
pub fn widget_thumbnail(ui:&Ui,id:&str,max_w:u32,max_h:u32)->Result<image::RgbaImage,String>{
    let mut bitmap=Bitmap::from_png(&image_png(ui,id)?)?;
    for p in bitmap.pixels.chunks_exact_mut(4){p.swap(0,2);}
    let w=bitmap.width as u32;let h=bitmap.height as u32;
    let image=image::RgbaImage::from_raw(w,h,bitmap.pixels).ok_or("Invalid image pixels.")?;
    let scale=(f64::from(max_w.max(1))/f64::from(w)).min(f64::from(max_h.max(1))/f64::from(h)).min(1.);
    if scale>=1.{return Ok(image);}
    Ok(image::imageops::thumbnail(&image,(f64::from(w)*scale).round().max(1.)as u32,(f64::from(h)*scale).round().max(1.)as u32))
}
pub(crate) fn file_card_thumbnail(path:&Path)->Result<image::RgbaImage,String>{
    let mut b=platform::thumbnail(path)?;
    for pixel in b.pixels.chunks_exact_mut(4){pixel.swap(0,2);}
    let image=image::RgbaImage::from_raw(b.width as u32,b.height as u32,b.pixels).ok_or("Invalid shell thumbnail.")?;
    Ok(image::DynamicImage::ImageRgba8(image).thumbnail(96,96).into_rgba8())
}
pub fn visible_image_id(ui:&Ui,index:usize)->Option<String>{
    if ui.page!="clipboard"{return None;}
    let index=*indexes(ui,"clipboard").get(index)?;
    let row=ui.records.get("clipboard")?.get(index)?;
    (get(row,0)=="image").then(||get(row,1).to_owned())
}
pub fn current_clipboard_png(ui:&Ui)->Result<Vec<u8>,String>{
    match platform::read_clip(ui.hwnd.0,true)?{Some(Clip::Image(image))=>image.png(),_=>Err("Copy an image first.".into())}
}
pub fn attach_clipboard_image(ui:&mut Ui)->Result<String,String>{
    match platform::read_clip(ui.hwnd.0,true)?{Some(Clip::Image(b))=>store_image(ui,&b),_=>Err("Copy an image first.".into())}
}
pub fn view_image(ui:&mut Ui,id:&str)->Result<(),String>{let b=Bitmap::from_png(&image_png(ui,id)?)?;markup::show(ui,&annotation_path(ui,id),b,true)}
pub fn view_png(ui:&mut Ui,bytes:&[u8])->Result<(),String>{let id=format!("{:x}",Sha256::digest(bytes));markup::show(ui,&annotation_path(ui,&id),Bitmap::from_png(bytes)?,false)}
fn indexes(ui:&Ui,page:&str)->Vec<usize>{
    let (q,kind)=TRACK.with(|t|{let state=t.borrow();(state.query.get(page).cloned().unwrap_or_default(),state.type_filter.clone())});
    ui.records.get(key(page)).into_iter().flatten().enumerate().filter(|(_,r)|{
        if page=="markup"&&get(r,0)!="image"{return false;}
        if page=="clipboard"&&kind.as_deref().is_some_and(|kind|kind!=get(r,0)){return false;}
        q.is_empty()||r.iter().take(2).any(|v|v.to_lowercase().contains(&q))
    }).map(|(i,_)|i).collect()
}
fn selected(ui:&Ui,page:&str)->Result<(usize,Vec<String>),String>{
    let filtered=indexes(ui,page);let index=*filtered.get(ui.selected().ok_or("Select an item first.")?).ok_or("This selection has changed.")?;
    Ok((index,ui.records[key(page)][index].clone()))
}
pub fn selected_image_id(ui:&Ui)->Option<String> {
    if !matches!(ui.page,"clipboard"|"markup") {return None;}
    let (_,row)=selected(ui,ui.page).ok()?;
    (get(&row,0)=="image").then(||get(&row,1).to_owned())
}
fn widget_row(ui:&Ui)->Result<(usize,Vec<String>),String> {
    let item=ui.records.get("clipboard_widget").and_then(|items|items.first()).ok_or("There is no clipboard widget.")?;
    ui.records.get("clipboard").into_iter().flatten().enumerate()
        .find(|(_,row)|get(row,0)==get(item,0)&&get(row,1)==get(item,1))
        .map(|(index,row)|(index,row.clone())).ok_or("The clipboard item is no longer in history.".into())
}
fn open_clipboard_row(ui:&mut Ui,index:usize,row:&[String])->Result<(),String>{
    TRACK.with(|t|{let mut t=t.borrow_mut();t.query.remove("clipboard");t.type_filter=None;});
    ui.navigate("clipboard");
    if ui.page!="clipboard"{return Err("Save the current note before opening Clipboard.".into());}
    ui.select_row(index);
    detail(ui,"clipboard",row);
    ui.reveal_clipboard_card(index);
    Ok(())
}
pub fn open_widget(ui:&mut Ui)->Result<(),String>{
    let (index,row)=widget_row(ui)?;
    open_clipboard_row(ui,index,&row)
}
pub fn open_fresh(ui:&mut Ui)->Result<bool,String>{
    if ui.isolated||ui.paused{return Ok(false);}
    let Some((stamp,kind,value))=TRACK.with(|t|t.borrow_mut().fresh.take())else{return Ok(false);};
    if now().saturating_sub(stamp)>5000{return Ok(false);}
    let Some((index,row))=ui.records.get("clipboard").into_iter().flatten().enumerate()
        .find(|(_,r)|get(r,0)==kind&&get(r,1)==value).map(|(i,r)|(i,r.clone()))else{return Ok(false)};
    open_clipboard_row(ui,index,&row)?;
    Ok(true)
}
pub fn copy_widget(ui:&Ui)->Result<(),String>{
    let (_,row)=widget_row(ui)?;
    match get(&row,0) {
        "image"=>platform::copy_bitmap(ui.hwnd.0,&Bitmap::from_png(&image_png(ui,get(&row,1))?)?),
        "text"|"link"=>platform::copy_text(ui.hwnd.0,get(&row,1)),
        _=>Err("Unknown clipboard item format.".into())
    }
}
pub fn widget_action(ui:&mut Ui,key:&str)->Result<String,String>{
    let (i,row)=widget_row(ui)?;
    let (kind,value)=(get(&row,0),get(&row,1));
    match key {
        "copy"=>copy_widget(ui)?,
        "open" if kind=="image"=>view_image(ui,value)?,
        "open" if kind=="link"&&files::valid_url(value)=>platform::open(ui.hwnd.0,OsStr::new(value))?,
        "open"=>{open_widget(ui)?;}
        "save" if kind=="image"=>{
            let Some(path)=platform::choose_files(ui.hwnd.0,true,false)?.into_iter().next()else{return Ok(String::new());};
            fs::write(path,image_png(ui,value)?).map_err(|e|e.to_string())?;
        }
        "pin"=>{
            let before=ui.records.clone();
            let pinned=get(&row,3)!="1";
            let target=&mut ui.records.get_mut("clipboard").unwrap()[i];target.resize(6,String::new());target[3]=if pinned{"1"}else{"0"}.into();
            let updated=target.clone();ui.records.insert("clipboard_widget".into(),vec![updated]);
            if let Err(e)=ui.save(){ui.records=before;return Err(e);}
        }
        "remove"=>{
            if get(&row,3)=="1"{return Err("Unpin this item before removing it.".into());}
            persist_rows(ui,"clipboard",|items|{items.remove(i);})?;sweep(ui);
        }
        "rewrite" if kind!="image"=>{ui.navigate("rewrite");crate::activity::set_rewrite_draft(ui,value);}
        _=>return Err("That action is unavailable for this clipboard item.".into())
    }
    if matches!(key,"pin"|"remove")&&ui.page=="clipboard"{render(ui,"clipboard");}
    Ok(String::new())
}
pub fn widget_text(ui:&Ui)->String{
    ui.records.get("clipboard_widget").and_then(|v|v.first()).map(|r|if get(r,0)=="image"{"Clipboard image".into()}else{shortened(get(r,1),48)}).unwrap_or_default()
}
fn remember(ui:&mut Ui,clip:Clip)->Result<bool,String>{
    let (kind,value,w,h)=match clip {
        Clip::Text(s)=>{let kind=if files::valid_url(&s){"link"}else{"text"};(kind,s,String::new(),String::new())},
        Clip::Image(b)=>("image",store_image(ui,&b)?,b.width.to_string(),b.height.to_string())
    };
    let before=ui.records.get("clipboard").cloned().unwrap_or_default();
    let rows=ui.records.entry("clipboard".into()).or_default();
    if rows.first().is_some_and(|r|get(r,0)==kind&&get(r,1)==value){return Ok(false);}
    let pin=rows.iter().position(|r|get(r,0)==kind&&get(r,1)==value).map(|i|get(&rows.remove(i),3).to_owned()).unwrap_or_else(||"0".into());
    rows.insert(0,vec![kind.into(),value,now().to_string(),pin,w,h]);
    let mut unpinned=0;rows.retain(|r|if get(r,3)=="1"{true}else{unpinned+=1;unpinned<=1000});
    if let Err(e)=ui.save(){ui.records.insert("clipboard".into(),before);return Err(e);}
    sweep(ui);Ok(true)
}
fn downloads_cleared_at(ui:&Ui)->u128{
    ui.records.get("downloads_cleared_at").and_then(|v|v.first()?.first()?.parse().ok()).unwrap_or(0)
}
pub fn tick(ui:&mut Ui){
    if ui.isolated||ui.paused||ui.read_error.is_some(){return;}
    let seq=platform::sequence();
    let update=TRACK.with(|t|t.borrow().sequence!=Some(seq));
    let was_running=TRACK.with(|t|t.borrow().sequence.is_some());
    if update {match platform::read_clip(ui.hwnd.0,false){
        Ok(value)=>{TRACK.with(|t|t.borrow_mut().sequence=Some(seq));if let Some(clip)=value{
            match remember(ui,clip){
                Ok(true)=>{
                    if was_running{let row=&ui.records["clipboard"][0];TRACK.with(|t|t.borrow_mut().fresh=Some((now(),get(row,0).into(),get(row,1).into())));}
                    if matches!(ui.page,"clipboard"|"markup"){render(ui,ui.page);}
                }
                Err(e)=>ui.status(&e),_=>{}
            }
        }},Err(_)=>{} // Clipboard contention is retried on the next tick.
    }}
    let stamp=now();
    let due=TRACK.with(|t| {
        let mut t=t.borrow_mut();
        if t.watch.as_ref().is_some_and(platform::FolderChanges::changed) {
            t.settle_at=Some(stamp+2400);
            true
        } else if t.settle_at.is_some_and(|when|stamp>=when) {
            t.settle_at=None;
            true
        } else {
            stamp.saturating_sub(t.dl_at)>=if t.watch.is_some(){30_000}else{2_000}
        }
    });
    if due {
        let cleared_at=downloads_cleared_at(ui);
        let prev=ui.records.get("downloads").cloned().unwrap_or_default();
        let changed=TRACK.with(|t|{let mut t=t.borrow_mut();if t.dir.is_none(){t.dir=platform::downloads();}if t.watch.is_none(){t.watch=t.dir.as_deref().and_then(platform::FolderChanges::watch);}t.dl_at=stamp;
            t.dir.clone().is_some_and(|d|t.downloads.refresh(&d,ui.records.entry("downloads".into()).or_default(),cleared_at))});
        if changed{if let Err(e)=ui.save(){ui.records.insert("downloads".into(),prev);ui.status(&e);}else if ui.page=="downloads"{render(ui,"downloads");}}
    }
}
pub fn actions(page:&str)->Vec<(&'static str,&'static str)>{
    match page{
        "clipboard"=>vec![("copy","Copy"),("markup","Edit image"),("search","Search"),("pin","Pin / unpin"),("ocr","Recognize text"),("open","Open link"),("widget","Show on widget"),("unwidget","Hide widget"),("remove","Remove item"),("clear","Clear unpinned"),("save","Export PNG"),("capture","Read current clipboard"),("refresh","Show all"),("texts","Text only"),("links","Links only"),("images","Images only"),("rewrite","Rewrite selected text")],
        "markup"=>vec![("markup","Open editor"),("paste","Edit clipboard image"),("import","Open image file"),("save","Export original PNG"),("ocr","Recognize text")],
        "files"=>vec![("pick","Add files"),("folder","Add folder"),("open","Open item"),("reveal","Show in Explorer"),("copyfile","Save a file copy"),("copyfolder","Save a folder copy"),("addpath","Link typed path"),("copypath","Copy path"),("copyitem","Copy file to clipboard"),("remove","Remove from shelf"),("trash","Delete saved copy"),("preview","Preview image"),("search","Search"),("refresh","Show all"),("store","Open stored files"),("relocate","Change storage folder")],
        "quick"=>vec![("open","Open pin"),("app","Choose application"),("new","New website"),("addweb","Pin website"),("remove","Remove pin"),("search","Search"),("refresh","Show all")],
        "downloads"=>vec![("open","Open download"),("reveal","Show in Explorer"),("refresh","Refresh"),("clear","Clear history"),("copypath","Copy path"),("preview","Preview image")],_=>vec![]
    }
}
fn caption(page:&str,r:&[String])->String{
    match page{
        "clipboard"|"markup"=>format!("{}{}",if get(r,3)=="1"{"★ "}else{""},if get(r,0)=="image"{format!("Image · {} × {}",get(r,4),get(r,5))}else{shortened(get(r,1),95)}),
        "files"=>format!("{} · {}",get(r,1),if get(r,2)=="copy"{"Saved copy"}else{"Link"}),
        "downloads"=>format!("{} · {} · {} bytes",get(r,1),get(r,2),get(r,3)),
        "quick"=>format!("{} · {}",get(r,1),if get(r,2)=="web"{"Website"}else{"Application"}),_=>String::new()
    }
}
fn detail(ui:&Ui,page:&str,row:&[String]){
    if page=="clipboard"{crate::inline_images::set_clipboard(ui,(get(row,0)=="image").then(||get(row,1)));}
    let text=match page{
        "clipboard"|"markup"=>if get(row,0)=="image"{format!("Image {} × {}\r\n{}",get(row,4),get(row,5),get(row,1))}else{get(row,1).to_owned()},
        "files"|"downloads"=>{let path=Path::new(get(row,0));let meta=fs::metadata(path);let info=meta.map(|s|if s.is_file(){format!("{} bytes",s.len())}else{"Folder".into()}).unwrap_or_else(|_|"Unavailable".into());format!("{}\r\n{}",path.display(),info)},
        "quick"=>{ui.set_input(get(row,0));get(row,1).into()},_=>String::new()
    };ui.set_detail(&text);
}
pub fn render(ui:&mut Ui,page:&str)->String{
    let indexes=indexes(ui,page);if page=="clipboard"&&indexes.is_empty(){crate::inline_images::set_clipboard(ui,None);}let captions=indexes.iter().map(|i|caption(page,&ui.records[key(page)][*i])).collect::<Vec<_>>();
    ui.set_rows(&captions);if let Some(i)=indexes.first(){ui.select_row(0);detail(ui,page,&ui.records[key(page)][*i]);}else{ui.set_detail("");}
    ui.refresh_workspaces();
    match page{"clipboard"=>"Clipboard history","markup"=>"Choose a clipboard image, import a file, or paste an image.","quick"=>"Enter a website URL and label to create a pin.","files"=>"Drop files here to link them or save independent copies.","downloads"=>"Recent downloads and partial-file sizes.",_=>""}.into()
}
fn persist_rows(ui:&mut Ui,page:&str,edit:impl FnOnce(&mut Vec<Vec<String>>))->Result<(),String>{
    let previous=ui.records.get(page).cloned().unwrap_or_default();
    let widget=ui.records.get("clipboard_widget").cloned();
    edit(ui.records.entry(page.into()).or_default());
    if page=="clipboard" {
        if let Some(item)=widget.as_ref().and_then(|items|items.first()) {
            if !ui.records[page].iter().any(|row|get(row,0)==get(item,0)&&get(row,1)==get(item,1)) {ui.records.remove("clipboard_widget");}
        }
    }
    if let Err(e)=ui.save(){
        ui.records.insert(page.into(),previous);
        if let Some(widget)=widget {ui.records.insert("clipboard_widget".into(),widget);}
        return Err(e);
    }
    Ok(())
}
fn import_file(ui:&mut Ui,path:&Path)->Result<(),String>{
    if fs::metadata(path).map_err(|e|e.to_string())?.len()>MAX_IMAGE as u64{return Err("Image exceeds the 25 MB limit.".into());}
    let data=fs::read(path).map_err(|e|e.to_string())?;
    if data.starts_with(b"\x89PNG\r\n\x1a\n"){return view_png(ui,&data);}
    let reader=image::ImageReader::new(std::io::Cursor::new(&data)).with_guessed_format().map_err(|e|e.to_string())?;
    let (w,h)=reader.into_dimensions().map_err(|e|e.to_string())?;Bitmap::size(w as usize,h as usize)?;
    let mut pixels=image::load_from_memory(&data).map_err(|e|e.to_string())?.into_rgba8().into_raw();for px in pixels.chunks_exact_mut(4){px.swap(0,2);}
    let b=Bitmap{width:w as usize,height:h as usize,pixels};
    let id=format!("{:x}",Sha256::digest(&data));markup::show(ui,&annotation_path(ui,&id),b,false)
}
pub fn action(ui:&mut Ui,page:&str,action:&str)->Result<String,String>{
    run_action(ui,page,action,None,page)
}

/// Native cards combine multiple record collections in one shelf. Resolve the
/// card against its original collection without changing the visible workspace.
pub(crate) fn card_action(ui:&mut Ui,page:&str,index:Option<usize>,action:&str)->Result<String,String>{
    let visible_page=ui.page;
    run_action(ui,page,action,index,visible_page)
}

fn run_action(ui:&mut Ui,page:&str,action:&str,card:Option<usize>,visible_page:&str)->Result<String,String>{
    let hwnd=ui.hwnd.0;
    let current=|ui:&Ui|match card{
        Some(i)=>ui.records.get(key(page)).and_then(|v|v.get(i)).cloned()
            .map(|row|(i,row)).ok_or_else(||"This item is no longer available.".to_owned()),
        None=>selected(ui,page),
    };
    let action=if action=="activate"{
        if page=="clipboard"{
            let (_,row)=current(ui)?;
            match get(&row,0){"image"=>"markup","link"=>"open",_=>"copy"}
        }else if page=="markup"{"markup"}else{"open"}
    }else{action};
    let message=match action{
        "texts"|"links"|"images" if page=="clipboard"=>{
            let kind=match action{"texts"=>"text","links"=>"link",_=>"image"};
            TRACK.with(|t|t.borrow_mut().type_filter=Some(kind.into()));
            "Filter applied.".into()
        }
        "rewrite" if page=="clipboard"=>{
            let (_,row)=current(ui)?;
            if get(&row,0)=="image"{return Err("Select clipboard text to rewrite.".into());}
            ui.navigate("rewrite");
            crate::activity::set_rewrite_draft(ui,get(&row,1));
            return Ok("Choose the rewrite you want to send through your Codex CLI.".into());
        }
        "select"=>{let (_,r)=current(ui)?;detail(ui,page,&r);return Ok(String::new());}
        "search"=>{let query=ui.input().trim().to_lowercase();TRACK.with(|t|{t.borrow_mut().query.insert(page.into(),query);});"Search applied.".into()}
        "refresh"=>{TRACK.with(|t|{let mut t=t.borrow_mut();t.query.remove(page);if page=="clipboard"{t.type_filter=None;}});if page=="downloads"&&!ui.isolated{let cleared_at=downloads_cleared_at(ui);let before=ui.records.get("downloads").cloned().unwrap_or_default();let changed=TRACK.with(|t|platform::downloads().is_some_and(|d|t.borrow_mut().downloads.refresh(&d,ui.records.entry("downloads".into()).or_default(),cleared_at)));if changed{if let Err(e)=ui.save(){ui.records.insert("downloads".into(),before);return Err(e);}}}"Updated.".into()}
        "pick"|"folder"|"copyfile"|"copyfolder"|"addpath" if page=="files"=>{
            let paths=match action{"folder"|"copyfolder"=>platform::choose_folder(hwnd).into_iter().collect(),"addpath"=>vec![PathBuf::from(ui.input().trim())],_=>platform::choose_files(hwnd,false,false)?};files::add(ui,paths,action.starts_with("copy"))?}
        "store" if page=="files"=>{let p=files::location(ui);fs::create_dir_all(&p).map_err(|e|e.to_string())?;platform::open(hwnd,p.as_os_str())?;"Opened saved files.".into()}
        "relocate" if page=="files"=>{let Some(dir)=platform::choose_folder(hwnd)else{return Ok("Canceled.".into())};files::relocate(ui,dir)?}
        "new" if page=="quick"=>{ui.set_input("");ui.set_detail("");return Ok("Enter the website URL and its display name.".into());}
        "app"|"addapp"|"addweb" if page=="quick"=>{
            let target=if action=="app"{let Some(p)=platform::choose_files(hwnd,false,true)?.into_iter().next()else{return Ok("Canceled.".into())};if !p.extension().is_some_and(|s|s.eq_ignore_ascii_case("exe")){return Err("Choose a Windows executable.".into());}p.to_string_lossy().into_owned()}else if action=="addapp"{
                let path=PathBuf::from(ui.input().trim());
                if !path.is_absolute()||!path.is_file()||!path.extension().is_some_and(|ext|ext.eq_ignore_ascii_case("exe")){return Err("Choose an existing Windows executable.".into());}
                path.to_string_lossy().into_owned()
            }else{let s=ui.input().trim().to_string();if !files::valid_url(&s){return Err("Enter a full HTTP or HTTPS URL.".into());}s};
            if ui.records.get("quick").is_some_and(|r|r.len()>=16){return Err("Quick Access holds 16 pins.".into());}
            let supplied=ui.detail();let label=if action=="app"{Path::new(&target).file_stem().unwrap_or_default().to_string_lossy().into_owned()}else if supplied.trim().is_empty(){target.clone()}else{supplied.trim().chars().take(120).collect()};
            persist_rows(ui,"quick",|r|r.push(vec![target,label,if action=="addweb"{"web"}else{"app"}.into()]))?;"Pin added.".into()}
        "remove"=>{let(i,row)=current(ui)?;if page=="files"&&get(&row,2)=="copy"{return Err("Use Delete saved copy to remove this managed item.".into());}persist_rows(ui,key(page),|r|{r.remove(i);})?;if page=="clipboard"{sweep(ui);}"Removed.".into()}
        "trash" if page=="files"=>{let(i,r)=current(ui)?;files::trash(ui,i,&r)?}
        "open"|"reveal" if matches!(page,"quick"|"downloads"|"files")=>{let(_,r)=current(ui)?;files::open_selected(ui,page,&r,action=="reveal")?}
        "copypath"|"copyitem"|"preview"=>{let(_,r)=current(ui)?;let p=Path::new(get(&r,0));match action{"copypath"=>platform::copy_text(hwnd,get(&r,0))?,"copyitem"=>platform::copy_path_item(hwnd,p)?,_=>{
            if let Err(original_error)=import_file(ui,p) {
                let image=platform::thumbnail(p).map_err(|_|original_error)?;
                let bytes=image.png()?;view_png(ui,&bytes)?;
            }
        }};"Done.".into()}
        "clear" if page=="clipboard"=>{persist_rows(ui,"clipboard",|r|r.retain(|item|get(item,3)=="1"))?;sweep(ui);"Unpinned history cleared.".into()}
        "clear" if page=="downloads"=>{
            let previous=ui.records.insert("downloads_cleared_at".into(),vec![vec![now().to_string()]]);
            if let Err(err)=persist_rows(ui,"downloads",Vec::clear){
                if let Some(previous)=previous{ui.records.insert("downloads_cleared_at".into(),previous);}else{ui.records.remove("downloads_cleared_at");}
                return Err(err);
            }
            "Download history cleared.".into()
        }
        "capture" if page=="clipboard"=>{if let Some(clip)=platform::read_clip(hwnd,false)?{remember(ui,clip)?;}"Clipboard checked.".into()}
        "paste" if page=="markup"=>{let Some(Clip::Image(img))=platform::read_clip(hwnd,true)? else{return Err("Copy an image first.".into())};remember(ui,Clip::Image(img))?;let id=get(&ui.records["clipboard"][0],1).to_owned();view_image(ui,&id)?;return Ok("Editor opened.".into());}
        "import" if page=="markup"=>{let Some(p)=platform::choose_files(hwnd,false,false)?.into_iter().next() else{return Ok("Canceled.".into());};import_file(ui,&p)?;return Ok("Editor opened.".into());}
        "pin"=>{let(i,_)=current(ui)?;persist_rows(ui,"clipboard",|rows|{let r=&mut rows[i];r.resize(6,String::new());r[3]=if r[3]=="1"{"0"}else{"1"}.into();})?;"Pin updated.".into()}
        "widget"=>{let(_,r)=current(ui)?;persist_rows(ui,"clipboard_widget",|v|*v=vec![r])?;"Clipboard widget updated.".into()}
        "unwidget"=>{persist_rows(ui,"clipboard_widget",Vec::clear)?;"Clipboard widget hidden.".into()}
        "copy"|"markup"|"save"|"ocr"|"open" if matches!(page,"clipboard"|"markup")=>{
            let(_,r)=current(ui)?;let value=get(&r,1);
            if get(&r,0)=="image"{match action{
                "copy"=>platform::copy_bitmap(hwnd,&Bitmap::from_png(&image_png(ui,value)?)?)?,
                "markup"|"open"=>view_image(ui,value)?,
                "save"=>{let Some(path)=platform::choose_files(hwnd,true,false)?.into_iter().next()else{return Ok("Canceled.".into())};fs::write(path,image_png(ui,value)?).map_err(|e|e.to_string())?;}
                "ocr"=>{let bytes=image_png(ui,value)?;return crate::devices::scan_saved(ui,value,bytes);}
                _=>unreachable!()}
            }else if action=="copy"{platform::copy_text(hwnd,value)?;}else if action=="open"&&files::valid_url(value){platform::open(hwnd,OsStr::new(value))?;}else{return Err("Select a compatible clipboard item.".into());}
            return Ok("Done.".into());}
        _=>return Err("Workspace action is unavailable.".into())
    };
    render(ui,visible_page);Ok(message)
}
pub fn drop_files(ui:&mut Ui,paths:Vec<PathBuf>)->Result<String,String>{
    if ui.page=="ocr" {
        if let Some(path)=paths.first(){ui.set_input(&path.to_string_lossy());return crate::devices::action(ui,"ocr","scan");}
    }
    if ui.page=="markup"{if let Some(p)=paths.first(){import_file(ui,p)?;return Ok("Editor opened.".into());}}
    if paths.is_empty(){return Ok("No files dropped.".into());}
    let copy=if ui.isolated{false}else{unsafe{use windows_sys::Win32::UI::WindowsAndMessaging::*;let result=MessageBoxW(ui.hwnd.0,platform::wide("Save independent copies? Choose No to link the original items.").as_ptr(),platform::wide("Add to Buddy file shelf").as_ptr(),MB_YESNOCANCEL|MB_ICONQUESTION);if result==IDCANCEL{return Ok("Canceled.".into());}result==IDYES}};
    let s=files::add(ui,paths,copy)?;ui.navigate("files");Ok(s)
}

#[cfg(test)]
pub(crate) fn with_private_test_desktop(test:impl FnOnce()){
    platform::isolation_test::with_private_desktop(test)
}
