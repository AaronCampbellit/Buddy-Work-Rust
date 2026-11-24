//! Native Windows device integration. Device inventories do not start capture.
mod audio;
mod effects;
mod helper;
mod key_picker;
mod ocr;
mod physical;
mod studio;
mod scan_worker;
pub use scan_worker::DONE as OCR_DONE;

// Also called by the shared UI tick when a modal message loop defers completion.
pub fn finish_scan(ui: &mut crate::Ui) {
    physical::release_hidden_preview();
    if let Some(color)=key_picker::take_color() {
        if ui.page=="studio" && available(ui) {
            ui.status(&studio::sampled_color(ui,&color).unwrap_or_else(|e|e));
        }
    }
    scan_worker::finish(ui);
}

use crate::Ui;
use serde_json::Value;
use std::{cell::RefCell,fs::File,io::Read,os::windows::ffi::OsStringExt,path::{Path,PathBuf}};
use windows_sys::Win32::UI::{Controls::Dialogs::*,WindowsAndMessaging::*};

thread_local! {
    static AUDIO:RefCell<Vec<audio::Row>>=const{RefCell::new(Vec::new())};
    static CAMERAS:RefCell<Vec<physical::Camera>>=const{RefCell::new(Vec::new())};
    static SCAN:RefCell<Option<ocr::Scan>>=const{RefCell::new(None)};
}
pub use ocr::Scan;
#[cfg(test)] pub use ocr::Word;
pub fn recognize_words(data:&[u8])->Result<Scan,String>{ocr::scan(data)}
fn choose_image(ui:&Ui)->Result<Option<PathBuf>,String>{
    let mut buf=[0u16;32768];let filter:Vec<_>="Images\0*.png;*.jpg;*.jpeg;*.bmp\0\0".encode_utf16().collect();
    let title:Vec<_>="Open image\0".encode_utf16().collect();
    let mut p=OPENFILENAMEW::default();p.lStructSize=std::mem::size_of::<OPENFILENAMEW>() as u32;
    p.hwndOwner=ui.hwnd.0;p.lpstrFilter=filter.as_ptr();p.lpstrTitle=title.as_ptr();p.lpstrFile=buf.as_mut_ptr();p.nMaxFile=buf.len() as u32;
    p.Flags=OFN_FILEMUSTEXIST|OFN_PATHMUSTEXIST|OFN_NOCHANGEDIR|OFN_EXPLORER;
    unsafe{if GetOpenFileNameW(&mut p)==0{let code=CommDlgExtendedError();if code!=0{return Err(format!("Windows image picker failed ({code})."));}return Ok(None);}}
    let n=buf.iter().position(|c|*c==0).unwrap_or(buf.len());Ok(Some(std::ffi::OsString::from_wide(&buf[..n]).into()))
}
fn read_image(p:&Path)->Result<Vec<u8>,String>{
    let mut b=Vec::new();let f=File::open(p).map_err(|e|e.to_string())?;f.take(25*1024*1024+1).read_to_end(&mut b).map_err(|e|e.to_string())?;
    if b.len()>25*1024*1024{return Err("Choose an image under 25 MB.".into());}Ok(b)
}
fn confirm(ui:&Ui,text:&str)->bool{
    let title=crate::ui::wide("Buddy Camera");let msg=crate::ui::wide(text);
    unsafe{MessageBoxW(ui.hwnd.0,msg.as_ptr(),title.as_ptr(),MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2)==IDYES}
}
pub fn actions(page:&str)->Vec<(&'static str,&'static str)>{
    match page{
        "audio"=>vec![("refresh","Refresh devices"),("mute","Mute / unmute"),("volume","Set volume"),("default","Set default"),("default-comm","Default for calls"),("default-all","Default for all roles")],
        "camera"=>vec![("refresh","Refresh cameras"),("ranges","Read controls"),("preview","Start preview"),("stop","Stop preview"),("pan","Set pan"),("tilt","Set tilt"),("zoom","Set zoom"),("focus","Set focus"),("exposure","Set exposure"),("brightness","Set brightness"),("contrast","Set contrast"),("saturation","Set saturation"),("sharpness","Set sharpness"),("white balance","Set white balance")],
        "studio"=>studio::actions(),
        "ocr"=>vec![("file","Choose image"),("scan","Read image text"),("clipboard","Read clipboard image"),("copy","Copy text"),("word","Select word"),("clear","Clear result")],
        _=>vec![],
    }
}
fn available(ui:&Ui)->bool{ui.expanded&&!ui.paused&&!ui.locked&&!ui.suppressed}
fn refresh(ui:&Ui,page:&str)->Result<String,String>{match page{
    "audio"=>{let rows=audio::list()?;let labels=rows.iter().map(|r|r.label()).collect::<Vec<_>>();ui.set_rows(&labels);AUDIO.with(|a|*a.borrow_mut()=rows);Ok(format!("{} audio endpoints and sessions. Select one to change volume or routing.",labels.len()))}
    "camera"=>{let rows=physical::list()?;let labels=rows.iter().map(|r|r.name.clone()).collect::<Vec<_>>();ui.set_rows(&labels);CAMERAS.with(|a|*a.borrow_mut()=rows);Ok(format!("{} physical cameras. Select a camera to read controls or start its local preview.",labels.len()))}
    _=>Err("Unknown device page.".into())
}}
pub fn render(ui:&mut Ui,page:&str)->String{
    if !available(ui){return "Open Buddy and resume services for device controls.".into();}
    match page{"audio"|"camera"=>refresh(ui,page).unwrap_or_else(|e|e),"studio"=>studio::render(ui),"ocr"=>{SCAN.with(|s|if let Some(v)=s.borrow().as_ref(){show_scan(ui,v)});"Choose or drop an image, or enter a saved clipboard image ID.".into()},_=>String::new()}
}
fn show_scan(ui:&Ui,v:&ocr::Scan){
    ui.set_detail(&v.text);let words=v.lines.iter().flatten().map(|w|format!("{}    ({}, {}) · {} × {}",w.text,w.x,w.y,w.width,w.height)).collect::<Vec<_>>();ui.set_rows(&words);
}
pub fn scan_saved(ui:&mut Ui,id:&str,bytes:Vec<u8>)->Result<String,String>{
    if ui.page!="ocr"{ui.navigate("ocr");}
    ui.set_input(id);scan_worker::bytes(ui,id.to_owned(),bytes)
}
fn ocr_action(ui:&mut Ui,key:&str)->Result<String,String>{
    match key{
        "clear"=>{scan_worker::cancel();SCAN.with(|s|s.borrow_mut().take());ui.set_rows(&[]);ui.set_detail("");ui.set_input("");Ok("Cleared.".into())}
        "file"=>{let Some(p)=choose_image(ui)? else{return Ok(String::new())};ui.set_input(&p.to_string_lossy());scan_worker::file(ui,p)}
        "clipboard"=>{let bytes=crate::workspace::current_clipboard_png(ui)?;
            ui.set_input(scan_worker::CLIPBOARD);scan_worker::clipboard_image(ui,Some(bytes))}
        "scan"=>{let input=ui.input();if input==scan_worker::CLIPBOARD{return scan_worker::clipboard_image(ui,None)}let p=Path::new(input.trim());if p.is_file(){scan_worker::file(ui,p.to_path_buf())}else{scan_saved(ui,&input,crate::workspace::image_png(ui,input.trim())?)}}
        "select"|"word"=>{let i=ui.selected().ok_or("Choose a word.")?;
            let range=SCAN.with(|cell|{let s=cell.borrow();let scan=s.as_ref()?;let mut offset=0usize;let mut n=0;
                for line in &scan.lines{for(j,w)in line.iter().enumerate(){if j>0{offset+=1;}let length=w.text.encode_utf16().count();if n==i{return Some((offset,length));}offset+=length;n+=1;}offset+=2;}None}).ok_or("Scan an image first.")?;
            unsafe{SendMessageW(ui.detail_box,0x00b1,range.0,(range.0+range.1)as isize);}Ok(String::new())}
        "copy"=>{if ui.detail().is_empty(){return Err("Scan an image first.".into());}
            unsafe{let(mut start,mut end)=(0u32,0u32);SendMessageW(ui.detail_box,0x00b0,(&mut start as *mut u32)as usize,(&mut end as *mut u32)as isize);
                if start==end{SendMessageW(ui.detail_box,0x00b1,0,-1);}SendMessageW(ui.detail_box,WM_COPY,0,0);}Ok("Text copied.".into())}
        _=>Err("Unknown OCR action.".into())
    }
}
pub fn action(ui:&mut Ui,page:&str,key:&str)->Result<String,String>{
    if !available(ui){return Err("Open Buddy and resume services for device controls.".into());}
    if page=="studio"{return studio::action(ui,key);}
    if page=="ocr"{return ocr_action(ui,key);}
    if key=="refresh"{return refresh(ui,page);}
    if page=="camera"&&key=="stop"{physical::stop();return Ok("Preview stopped.".into());}
    let index=ui.selected().ok_or("Select a device first.")?;
    match page{
        "audio"=>{let r=AUDIO.with(|a|a.borrow().get(index).cloned()).ok_or("Refresh devices and select one.")?;
            if key=="select"{ui.set_input(&r.volume.map(|v|format!("{:.0}",v*100.)).unwrap_or_default());ui.set_detail(&r.label());return Ok(String::new());}
            audio::apply(&r,key,&ui.input())?;refresh(ui,page)?;if let Some(i)=AUDIO.with(|a|a.borrow().iter().position(|n|n.id==r.id&&n.session==r.session)){ui.select_row(i);}Ok("Audio setting updated.".into())}
        "camera"=>{let c=CAMERAS.with(|a|a.borrow().get(index).cloned()).ok_or("Refresh cameras and select one.")?;
            match key{"select"=>{ui.set_detail(&c.name);Ok(String::new())},"ranges"=>{ui.set_detail(&physical::ranges(&c)?);Ok("Enter a value, auto or default, then choose the property to change.".into())},
                "preview"=>{physical::preview(&c,ui.physical_preview_host())?;Ok("Preview active.".into())},
                "pan"|"tilt"|"zoom"|"focus"|"exposure"|"brightness"|"contrast"|"saturation"|"sharpness"|"white balance"=>{physical::set(&c,key,&ui.input())?;ui.set_detail(&physical::ranges(&c)?);Ok("Camera setting updated.".into())},
                _=>Err("Unknown camera control.".into())}}
        _=>Err("Unknown device page.".into())
    }
}
pub fn stop(){ocr::release(true);scan_worker::cancel();physical::stop();studio::stop();}
pub fn leave(page:&str){scan_worker::cancel();if page=="camera"{physical::stop();}if page=="studio"{studio::leave();}}
pub fn restore(profile:&Path){studio::restore(profile);}
pub fn activity()->Result<Value,String>{studio::activity()}
pub fn call_control(key:&str)->Result<String,String>{studio::call_control(key)}

pub(crate) fn audio_devices(input:bool)->Vec<(usize,audio::Row)>{
    AUDIO.with(|a|a.borrow().iter().enumerate().filter(|(_,r)|
        r.active && (r.flow==windows::Win32::Media::Audio::eCapture)==input
    ).map(|(i,r)|(i,r.clone())).collect())
}

pub(crate) fn studio_fields()->Vec<(usize,String,String,String)>{studio::editor_fields()}
pub(crate) fn studio_state()->(bool,bool,String){studio::draft_state()}
pub(crate) fn camera_previewing()->bool{physical::previewing()}

pub(crate) fn size_preview(w:i32,h:i32){physical::size_preview(w,h)}
