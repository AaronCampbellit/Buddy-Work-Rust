mod rich_text;
pub(crate) use rich_text::{format_key,saved as saved_format};
use crate::{ui::wide, workspace, Ui};
use image::RgbaImage;
use std::{
    cell::{Cell, RefCell}, collections::{HashMap, VecDeque}, mem::{size_of, ManuallyDrop},
    path::PathBuf, ptr::{null, null_mut}, rc::Rc, sync::OnceLock,
};
use windows::{
    core::Interface,
    Win32::{
        Foundation::{HGLOBAL, SIZE}, Graphics::Gdi::HDC as TypedDc,
        System::{Com::{StructuredStorage::*, *}, Ole::*},
        UI::{Controls::RichEdit::*, Shell::SHCreateDataObject},
    },
};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::{DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW},LibraryLoader::{LoadLibraryExW,LOAD_LIBRARY_SEARCH_SYSTEM32},Memory::*},
    UI::{Input::KeyboardAndMouse::{GetKeyState, SetFocus,VK_CONTROL,VK_SHIFT},Shell::*, WindowsAndMessaging::*},
};

pub(crate) const NOTE_EDITOR:u16=1560;
const PREVIEW:u16=1561;
const OPEN:u16=1562;
const REMOVE:u16=1563;
const PASTE:u16=1564;
const MAX_TEXT:usize=256*1024;
const CACHE_BYTES:usize=6*1024*1024;
const THUMB_W:u32=212;
const THUMB_H:u32=112;
const EM_SETSEL_:u32=0xb1;
const EM_GETMODIFY_:u32=0xb8;
const EM_SETMODIFY_:u32=0xb9;
const EM_GETSEL_:u32=0xb0;
const EM_REPLACESEL_:u32=0xc2;
const WM_USER_:u32=0x400;

#[derive(Clone,Debug,PartialEq)]
pub(crate) struct Placement{pub id:String,pub offset:usize}
fn normalized(text:&str)->String{text.replace("\r\n","\n").replace('\r',"\n")}
pub(crate) fn offset_key(note:&str)->String{format!("note-image-positions/{note}")}
pub(crate) fn placements(row:&[String],saved:Option<&Vec<Vec<String>>>)->Vec<Placement>{
    let body:Vec<u16>=row.get(2).map(|s|normalized(s).encode_utf16().collect()).unwrap_or_default();
    let end=body.len();
    let offsets=saved.and_then(|rows|rows.first());
    let mut result=row.iter().skip(7).enumerate().map(|(i,id)|{
        let offset=offsets.and_then(|r|r.get(i)).and_then(|s|s.parse().ok()).unwrap_or(end).min(end);
        let offset=if offset>0&&offset<end&&(0xdc00..=0xdfff).contains(&body[offset]){offset-1}else{offset};
        Placement{id:id.clone(),offset}
    }).collect::<Vec<_>>();
    result.sort_by_key(|item|item.offset);
    result
}

struct Cache{entries:VecDeque<((PathBuf,String,u32,u32),Rc<RgbaImage>)>,bytes:usize}
thread_local!{static THUMBS:RefCell<Cache>=const{RefCell::new(Cache{entries:VecDeque::new(),bytes:0})};}
fn thumbnail(ui:&Ui,id:&str)->Result<Rc<RgbaImage>,String>{
    cached_thumbnail(ui,id,THUMB_W,THUMB_H)
}
pub(crate) fn cached_thumbnail(ui:&Ui,id:&str,width:u32,height:u32)->Result<Rc<RgbaImage>,String>{
    let key=(ui.profile.clone(),id.to_owned(),width,height);
    if let Some(image)=THUMBS.with(|cache|{
        let mut cache=cache.borrow_mut();
        let i=cache.entries.iter().position(|(k,_)|*k==key)?;
        let entry=cache.entries.remove(i)?;
        let result=Rc::clone(&entry.1);
        cache.entries.push_front(entry);
        Some(result)
    }){return Ok(image);}
    let image=Rc::new(workspace::widget_thumbnail(ui,id,width,height)?);
    THUMBS.with(|cache|{
        let mut cache=cache.borrow_mut();
        cache.bytes+=image.len();
        cache.entries.push_front((key,Rc::clone(&image)));
        while cache.bytes>CACHE_BYTES||cache.entries.len()>64{
            if let Some((_,old))=cache.entries.pop_back(){cache.bytes-=old.len();}else{break;}
        }
    });
    Ok(image)
}
fn missing()->RgbaImage{
    RgbaImage::from_fn(56,36,|x,y|image::Rgba(if x<2||x>=54||y<2||y>=34||x==y+10||x+y==45{[130,130,130,255]}else{[232,232,232,255]}))
}
// The on-disk attachment stays at its original resolution. This is the web
// editor's 68 × 48 cover thumbnail, with its two/four pixel inline margins.
const IMAGE_W:u32=68;
const IMAGE_H:u32=48;
const IMAGE_MARGIN_X:u32=4;
const IMAGE_MARGIN_Y:u32=2;
fn note_thumbnail(src:&RgbaImage)->RgbaImage {
    use image::imageops::{crop_imm,resize,FilterType};
    let (w,h)=src.dimensions();
    let cw=(h*IMAGE_W/IMAGE_H).clamp(1,w);
    let ch=(w*IMAGE_H/IMAGE_W).clamp(1,h);
    let crop=crop_imm(src,(w-cw)/2,(h-ch)/2,cw,ch).to_image();
    let picture=resize(&crop,IMAGE_W,IMAGE_H,FilterType::Triangle);
    RgbaImage::from_fn(IMAGE_W+2*IMAGE_MARGIN_X,IMAGE_H+2*IMAGE_MARGIN_Y,|x,y|{
        let x=x as i32-IMAGE_MARGIN_X as i32;let y=y as i32-IMAGE_MARGIN_Y as i32;
        if x<0||x>=IMAGE_W as i32||y<0||y>=IMAGE_H as i32{return image::Rgba([0;4]);}
        let mut pixel=*picture.get_pixel(x as u32,y as u32);
        let (fx,fy)=(x as f32+0.5,y as f32+0.5);
        let dx=fx-fx.clamp(6.0,IMAGE_W as f32-6.0);let dy=fy-fy.clamp(6.0,IMAGE_H as f32-6.0);
        pixel[3]=(pixel[3] as f32*(6.5-dx.hypot(dy)).clamp(0.0,1.0)).round() as u8;
        let (cx,cy)=(fx-(IMAGE_W as f32-10.5),fy-10.5);
        let circle=(9.0-cx.hypot(cy)).clamp(0.0,1.0)*0.84;
        if circle>0.0{
            for (value,tint) in pixel.0.iter_mut().take(3).zip([8.,12.,15.]){*value=(*value as f32*(1.0-circle)+tint*circle).round() as u8;}
            pixel[3]=(pixel[3] as f32+(255.0-pixel[3] as f32)*circle).round() as u8;
            if cx.abs().max(cy.abs())<=2.8 {
                let line=(1.1-(cx.abs()-cy.abs()).abs()).clamp(0.0,1.0);
                for value in pixel.0.iter_mut().take(3) {*value=(*value as f32+(255.0-*value as f32)*line).round() as u8;}
            }
        }
        pixel
    })
}

fn bgra(image:&RgbaImage,bg:u32)->Vec<u8>{
    let (r,g,b)=(bg&255,(bg>>8)&255,(bg>>16)&255);
    let mut data=Vec::with_capacity(image.len());
    for px in image.pixels(){
        let a=u32::from(px[3]);let mix=|n:u8,back:u32|((u32::from(n)*a+back*(255-a)+127)/255) as u8;
        data.extend_from_slice(&[mix(px[2],b),mix(px[1],g),mix(px[0],r),255]);
    }data
}
fn info(image:&RgbaImage)->BITMAPINFO{
    let mut i=BITMAPINFO::default();
    i.bmiHeader.biSize=size_of::<BITMAPINFOHEADER>()as u32;i.bmiHeader.biWidth=image.width()as i32;
    i.bmiHeader.biHeight=-(image.height()as i32);i.bmiHeader.biPlanes=1;i.bmiHeader.biBitCount=32;i
}
unsafe fn draw(dc:HDC,r:RECT,image:&RgbaImage,bg:u32){
    let (w,h)=(r.right-r.left,r.bottom-r.top);if w<=0||h<=0{return;}
    let scale=(f64::from(w)/f64::from(image.width())).min(f64::from(h)/f64::from(image.height()));
    let (width,height)=((f64::from(image.width())*scale).round().max(1.)as i32,(f64::from(image.height())*scale).round().max(1.)as i32);
    SetStretchBltMode(dc,HALFTONE);
    StretchDIBits(dc,r.left+(w-width)/2,r.top+(h-height)/2,width,height,0,0,image.width()as i32,image.height()as i32,bgra(image,bg).as_ptr().cast(),&info(image),DIB_RGB_COLORS,SRCCOPY);
}
pub(crate) fn clipboard_thumb(ui:&Ui,row:usize,dc:TypedDc,r:windows::Win32::Foundation::RECT)->bool{
    let Some(id)=workspace::visible_image_id(ui,row)else{return false};
    let Ok(image)=thumbnail(ui,&id)else{return false};
    unsafe{draw(dc.0,RECT{left:r.left,top:r.top,right:r.right,bottom:r.bottom},&image,ui.appearance.1);}
    true
}
fn map_error(e:windows::core::Error)->String{format!("Windows could not display the inline image ({}).",e.code())}
fn interface(hwnd:HWND)->Result<IRichEditOle,String>{
    let mut raw:*mut std::ffi::c_void=null_mut();unsafe{SendMessageW(hwnd,EM_GETOLEINTERFACE,0,&mut raw as *mut _ as _);}
    if raw.is_null(){return Err("Windows Rich Edit image support is unavailable.".into());}
    Ok(unsafe{IRichEditOle::from_raw(raw)})
}
// The only OLE class that is inserted is the static DIB created from our own
// normalized pixel buffer. No serialized objects or user-provided RTF are loaded.
unsafe fn insert(hwnd:HWND,cp:i32,token:u32,image:&RgbaImage,bg:u32)->Result<(),String>{
    let rich=interface(hwnd)?;
    let data:IDataObject=SHCreateDataObject(None,None,None).map_err(map_error)?;
    // Static OLE images use the traditional, bottom-up 24-bit clipboard DIB.
    // GDI rendering of our custom clipboard control uses a separate top-down DIB.
    let mut header=info(image).bmiHeader;
    let w=image.width() as usize;let h=image.height() as usize;
    let stride=(w*3).next_multiple_of(4);
    header.biBitCount=24;header.biHeight=h as i32;header.biSizeImage=(stride*h)as u32;
    let top=bgra(image,bg);let mut pixels=vec![0;stride*h];
    for y in 0..h{for x in 0..w{
        let src=(y*w+x)*4;let dst=(h-1-y)*stride+x*3;
        pixels[dst..dst+3].copy_from_slice(&top[src..src+3]);
    }}
    let mem=GlobalAlloc(GMEM_MOVEABLE,size_of::<BITMAPINFOHEADER>()+pixels.len());
    if mem.is_null(){return Err("Insufficient memory for image preview.".into());}
    let dest=GlobalLock(mem).cast::<u8>();
    if dest.is_null(){windows_sys::Win32::Foundation::GlobalFree(mem);return Err("Could not load image pixels.".into());}
    std::ptr::copy_nonoverlapping(&header as *const _ as *const u8,dest,size_of::<BITMAPINFOHEADER>());
    std::ptr::copy_nonoverlapping(pixels.as_ptr(),dest.add(size_of::<BITMAPINFOHEADER>()),pixels.len());
    GlobalUnlock(mem);
    let medium=STGMEDIUM{tymed:TYMED_HGLOBAL.0 as u32,u:STGMEDIUM_0{hGlobal:HGLOBAL(mem)},..Default::default()};
    let format=FORMATETC{cfFormat:CF_DIB.0,ptd:null_mut(),dwAspect:DVASPECT_CONTENT.0,lindex:-1,tymed:TYMED_HGLOBAL.0 as u32};
    if let Err(e)=data.SetData(&format,&medium,true){windows_sys::Win32::Foundation::GlobalFree(mem);return Err(map_error(e));}
    let bytes=CreateILockBytesOnHGlobal(None,true).map_err(map_error)?;
    let store=StgCreateDocfileOnILockBytes(&bytes,STGM_CREATE|STGM_READWRITE|STGM_SHARE_EXCLUSIVE,0).map_err(map_error)?;
    let site=rich.GetClientSite().map_err(map_error)?;
    let mut raw:*mut std::ffi::c_void=null_mut();
    OleCreateStaticFromData(&data,&IOleObject::IID,OLERENDER_FORMAT,&format,&site,&store,&mut raw).map_err(map_error)?;
    let object=IOleObject::from_raw(raw);
    let clsid=object.GetUserClassID().map_err(map_error)?;
    let mut item=REOBJECT{cbStruct:size_of::<REOBJECT>() as u32,cp,clsid,
        poleobj:ManuallyDrop::new(Some(object)),pstg:ManuallyDrop::new(Some(store)),polesite:ManuallyDrop::new(Some(site)),
        sizel:SIZE{cx:image.width()as i32*2540/96,cy:image.height()as i32*2540/96},
        dvaspect:DVASPECT_CONTENT.0,dwFlags:REO_BELOWBASELINE|REO_STATIC,dwUser:token};
    let result=rich.InsertObject(&mut item).map_err(map_error);
    ManuallyDrop::drop(&mut item.poleobj);ManuallyDrop::drop(&mut item.pstg);ManuallyDrop::drop(&mut item.polesite);result
}
fn objects(hwnd:HWND)->Result<Vec<(usize,u32)>,String>{
    let rich=interface(hwnd)?;let mut result=Vec::new();
    unsafe{for i in 0..rich.GetObjectCount(){
        let mut item=REOBJECT{cbStruct:size_of::<REOBJECT>()as u32,..Default::default()};
        rich.GetObject(i,&mut item,REO_GETOBJ_NO_INTERFACES).map_err(map_error)?;
        if item.cp<0{return Err("Invalid inline image position.".into());}result.push((item.cp as usize,item.dwUser));
    }}
    result.sort_by_key(|(position,_)|*position);Ok(result)
}
fn raw_text(hwnd:HWND)->Vec<u16>{
    let n=unsafe{SendMessageW(hwnd,WM_GETTEXTLENGTH,0,0)}.clamp(0,MAX_TEXT as isize+128)as usize;
    let mut text=vec![0u16;n+1];
    let request=GETTEXTEX{cb:(text.len()*2)as u32,flags:GT_RAWTEXT,codepage:1200,..Default::default()};
    let n=unsafe{SendMessageW(hwnd,EM_GETTEXTEX,&request as *const _ as _,text.as_mut_ptr() as _)}.max(0)as usize;
    text.truncate(n);text
}
fn selection(hwnd:HWND)->(usize,usize){
    let mut start=0u32;let mut end=0u32;unsafe{SendMessageW(hwnd,EM_GETSEL_,&mut start as *mut _ as _,&mut end as *mut _ as _);}(start as usize,end as usize)
}
fn select(hwnd:HWND,start:usize,end:usize){unsafe{SendMessageW(hwnd,EM_SETSEL_,start,end as _);}}
unsafe fn place(hwnd:HWND,bounds:(i32,i32,i32,i32),visible:bool){
    if visible{crate::ui::pos(hwnd,bounds.0,bounds.1,bounds.2,bounds.3);}
    crate::ui::show(hwnd,visible);
}

#[derive(Default)]
struct NoteHint {
    font:Cell<HFONT>,color:Cell<u32>,ink:Cell<Option<(u32,u32)>>,
    painted:Cell<Option<(i32,i32)>>,
}
impl Drop for NoteHint {
    fn drop(&mut self) { unsafe { if !self.font.get().is_null(){DeleteObject(self.font.get());} } }
}
struct Preview{image:RefCell<Option<(String,Rc<RgbaImage>)>>,field:Cell<u32>}
struct OleGuard;
impl Drop for OleGuard{fn drop(&mut self){unsafe{OleUninitialize();}}}
#[derive(PartialEq)]
struct NoteDocument {
    id:String,
    revision:u64,
    appearance:crate::chrome::Colors,
    zoom:i32,
    dpi:u32,
}

pub(crate) struct Controls{
    document:RefCell<Option<NoteDocument>>,
    pub editor:HWND,preview:HWND,token:Cell<u32>,keys:RefCell<HashMap<u32,String>>,
    clip:Box<Preview>,hint:Box<NoteHint>,edit_area:Cell<[i32;4]>,corner_size:Cell<(i32,i32,i32)>,_ole:OleGuard,bg:Cell<u32>,
    formatted:Box<Cell<bool>>,retained_format:RefCell<Option<(String,String)>>,style:Cell<(u32,u32,i32,i32)>,load_error:RefCell<Option<String>>,
}
impl Controls{
    pub fn new(parent:HWND)->Result<Self,String>{
        unsafe{
            OleInitialize(None).map_err(map_error)?;
            let ole=OleGuard;
            static RICHEDIT:OnceLock<bool>=OnceLock::new();
            if !RICHEDIT.get_or_init(||!LoadLibraryExW(wide("Msftedit.dll").as_ptr(),null_mut(),LOAD_LIBRARY_SEARCH_SYSTEM32).is_null()){
                return Err("Windows Rich Edit is unavailable.".into());
            }
            let editor=CreateWindowExW(0,wide("RICHEDIT50W").as_ptr(),wide("").as_ptr(),
                WS_CHILD|WS_TABSTOP|WS_VSCROLL|ES_NOOLEDRAGDROP|(ES_MULTILINE|ES_AUTOVSCROLL|ES_WANTRETURN)as u32,0,0,1,1,parent,NOTE_EDITOR as usize as _,null_mut(),null());
            let preview=CreateWindowExW(0,wide("BUTTON").as_ptr(),wide("Open clipboard image in markup").as_ptr(),
                WS_CHILD|WS_TABSTOP|BS_PUSHBUTTON as u32,0,0,1,1,parent,PREVIEW as usize as _,null_mut(),null());
            if editor.is_null()||preview.is_null(){return Err("Windows could not create image controls.".into());}
            crate::chrome::scroll::attach(editor,WS_VSCROLL,false);
            SendMessageW(editor,WM_USER_+53,0,MAX_TEXT as _); // EM_EXLIMITTEXT
            SendMessageW(editor,EM_AUTOURLDETECT,AURL_ENABLEURL as _,wide("http:https:").as_ptr() as _);
            SendMessageW(editor,EM_SETEVENTMASK,0,(ENM_CHANGE|ENM_UPDATE|ENM_LINK)as _);
            let clip=Box::new(Preview{image:Default::default(),field:Cell::new(0xffffff)});
            let formatted=Box::new(Cell::new(false));
            let hint=Box::<NoteHint>::default();
            SetWindowSubclass(editor,Some(hint_proc),2,hint.as_ref() as *const _ as usize);
            SetWindowSubclass(editor,Some(note_proc),1,formatted.as_ref() as *const _ as usize);SetWindowSubclass(preview,Some(preview_proc),1,clip.as_ref() as *const _ as usize);
            Ok(Self{document:Default::default(),editor,preview,token:Cell::new(0),keys:Default::default(),clip,hint,edit_area:Cell::new([0;4]),corner_size:Cell::new((0,0,0)),_ole:ole,
                bg:Cell::new(0xffffff),formatted,retained_format:Default::default(),style:Cell::new((u32::MAX,u32::MAX,0,10)),load_error:Default::default()})
        }
    }
    pub fn layout(&self,page:&str,expanded:bool,bounds:(i32,i32,i32,i32),zoom:i32,appearance:(u32,u32,u32,u32))->bool{
        if page=="notes" {
            let (x,y,w,h)=bounds;
            self.note_layout(expanded,crate::chrome::rect(x,y,w,h),zoom,appearance,10);
            return true;
        }
        let clip=page=="clipboard" && self.clip.image.borrow().is_some();
        self.clip.field.set(appearance.1);
        unsafe { place(self.editor,bounds,false);place(self.preview,bounds,expanded && clip); }
        clip
    }

    pub(crate) fn note_layout(&self, visible:bool, bounds:RECT, zoom:i32, appearance:crate::chrome::Colors, css_px:i32) {
        let (bg,_,text,accent)=appearance;
        let field=crate::chrome::control(appearance);
        self.bg.set(field);
        let dpi=crate::shell::dpi(self.editor);
        let scaled=(dpi as f32/96.0)*(zoom as f32/100.0);
        let font_px=(css_px as f32*scaled).round() as i32;
        let state=(field,text,zoom,font_px);
        unsafe {
            if self.hint.ink.replace(Some((text,accent)))!=Some((text,accent)){self.hint.painted.set(None);InvalidateRect(self.editor,null(),0);}
            let previous=self.style.get();
            if previous!=state {
                if previous.0!=field{SendMessageW(self.editor,EM_SETBKGNDCOLOR,0,field as _);}
                if previous.2!=zoom||previous.3!=font_px {
                    if !self.formatted.get() {
                        let mut fmt=CHARFORMATW::default();fmt.cbSize=size_of::<CHARFORMATW>()as u32;
                        fmt.dwMask=CFM_COLOR|CFM_FACE|CFM_SIZE;fmt.dwEffects=CFE_AUTOCOLOR;fmt.yHeight=css_px*15;
                        let face=wide("Segoe UI");fmt.szFaceName[..face.len()].copy_from_slice(&face);
                        SendMessageW(self.editor,EM_SETCHARFORMAT,SCF_ALL as _,&fmt as *const _ as _);
                        SendMessageW(self.editor,EM_SETCHARFORMAT,0,&fmt as *const _ as _);
                    }
                    if previous.2!=zoom{SendMessageW(self.editor,EM_SETZOOM,zoom.clamp(50,200)as _,100);}
                    let mut lf=LOGFONTW::default();lf.lfHeight= -font_px;lf.lfQuality=CLEARTYPE_QUALITY;
                    let face=wide("Segoe UI");lf.lfFaceName[..face.len()].copy_from_slice(&face);
                    let old=self.hint.font.replace(CreateFontIndirectW(&lf));if !old.is_null(){DeleteObject(old);}
                }
                self.hint.color.set(crate::chrome::mix(bg,text,162));
                self.style.set(state);
            }
            place(self.editor,(bounds.left,bounds.top,bounds.right-bounds.left,bounds.bottom-bounds.top),visible);
            ShowWindow(self.preview,SW_HIDE);
            if visible {
                let mut area=RECT::default();GetClientRect(self.editor,&mut area);
                let (w,h)=(bounds.right-bounds.left,bounds.bottom-bounds.top);
                let diameter=(10.0*scaled).round() as i32;
                if self.corner_size.get()!=(w,h,diameter) {
                    let rgn=CreateRoundRectRgn(0,0,w+1,h+1,diameter,diameter);
                    if !rgn.is_null(){
                        if SetWindowRgn(self.editor,rgn,1)==0 {DeleteObject(rgn);}
                        else {self.corner_size.set((w,h,diameter));}
                    }
                }
                let px=(7.0*scaled).round() as i32;let py=(8.0*scaled).round() as i32;
                InflateRect(&mut area,-px,-py);
                let edges=[area.left,area.top,area.right,area.bottom];
                if self.edit_area.replace(edges)!=edges {
                    SendMessageW(self.editor,0x00b4,0,&area as *const _ as _); // EM_SETRECTNP
                }
            }
        }
    }
    pub(crate) fn modified(&self)->bool{unsafe{SendMessageW(self.editor,EM_GETMODIFY_,0,0)!=0}}
    pub(crate) fn format(&self)->Result<Option<String>,String>{
        if self.formatted.get(){return rich_text::snapshot(self.editor).map(Some);}
        // Preserve a newer or unsupported format when an older editor is only
        // viewing the note or saving a title change.
        if let Some((body,rtf))=self.retained_format.borrow().as_ref(){
            if self.body()==*body{return Ok(Some(rtf.clone()));}
        }
        Ok(None)
    }
    pub fn set_body(&self,text:&str){
        self.document.borrow_mut().take();
        self.formatted.set(false);
        self.retained_format.borrow_mut().take();
        self.keys.borrow_mut().clear();self.load_error.borrow_mut().take();
        unsafe{SetWindowTextW(self.editor,wide("").as_ptr());}
        let (_,color,zoom,font_px)=self.style.get();
        if color!=u32::MAX{unsafe{
            let mut fmt=CHARFORMATW::default();fmt.cbSize=size_of::<CHARFORMATW>() as _;
            fmt.dwMask=CFM_BOLD|CFM_ITALIC|CFM_UNDERLINE|CFM_STRIKEOUT|CFM_COLOR|CFM_SIZE|CFM_FACE;
            fmt.dwEffects=CFE_AUTOCOLOR;
            fmt.yHeight=(font_px as f32*15.0/(crate::shell::dpi(self.editor) as f32/96.0)/(zoom.max(1) as f32/100.0)).round() as i32;
            let face=wide("Segoe UI");fmt.szFaceName[..face.len()].copy_from_slice(&face);
            SendMessageW(self.editor,EM_SETCHARFORMAT,0,&fmt as *const _ as _);
        }}
        select(self.editor,0,usize::MAX);
        let plain=wide(&normalized(text).replace('\n',"\r"));
        unsafe{SendMessageW(self.editor,EM_REPLACESEL_,0,plain.as_ptr()as _);SendMessageW(self.editor,0xcd,0,0);}
        select(self.editor,0,0);
    }
    fn add(&self,cp:i32,id:&str,img:&RgbaImage)->Result<(),String>{
        let token=self.token.get().checked_add(1).ok_or("Too many open images.")?;
        let thumb=note_thumbnail(img);
        unsafe{insert(self.editor,cp,token,&thumb,self.bg.get())?;}
        self.hint.painted.set(None);
        self.token.set(token);self.keys.borrow_mut().insert(token,id.to_owned());Ok(())
    }
    pub fn references(&self,id:&str)->bool{
        self.keys.borrow().values().any(|s|s==id)
    }
    pub fn contents(&self)->Result<(String,Vec<Placement>),String>{
        if let Some(error)=self.load_error.borrow().as_ref(){return Err(error.clone());}
        let raw=raw_text(self.editor);let mut pictures=Vec::new();let mut plain=Vec::with_capacity(raw.len());
        let positions=objects(self.editor)?;let mut next=positions.iter().peekable();let keys=self.keys.borrow();
        for (position,c) in raw.into_iter().enumerate(){
            if let Some(&&(cp,token))=next.peek(){
                if cp==position{
                    let id=keys.get(&token).ok_or("An image in this note could not be identified.")?;
                    pictures.push(Placement{id:id.clone(),offset:plain.len()});next.next();continue;
                }
            }
            plain.push(c);
        }
        if next.peek().is_some(){return Err("The note's image positions could not be read.".into());}
        Ok((normalized(&String::from_utf16_lossy(&plain)),pictures))
    }
    pub fn body(&self)->String{self.contents().map(|(s,_)|s).unwrap_or_else(|_|normalized(&String::from_utf16_lossy(&raw_text(self.editor))))}
    pub(crate) fn selected_image(&self)->Option<(usize,String)>{
        let (a,b)=selection(self.editor);
        let list=objects(self.editor).ok()?;
        let (i,(_,token))=list.iter().enumerate().find(|(_,p)|p.0==a&&b==a+1)?;
        Some((i,self.keys.borrow().get(token)?.to_owned()))
    }
}

impl Controls{
    pub(crate) fn load_row(&self,ui:&Ui,row:&[String])->Result<(),String>{
        self.set_body(row.get(2).map(String::as_str).unwrap_or_default());
        if let Some(rtf)=row.first().and_then(|id|saved_format(&ui.records,id)){
            let body=row.get(2).map(String::as_str).unwrap_or_default();
            if rich_text::restore(self.editor,&rtf,body){self.formatted.set(true);}
            else{self.set_body(body);*self.retained_format.borrow_mut()=Some((normalized(body),rtf));}
        }
        let saved=row.first().and_then(|id|ui.records.get(&offset_key(id)));
        for (i,item) in placements(row,saved).iter().enumerate(){
            let image=thumbnail(ui,&item.id).unwrap_or_else(|_|Rc::new(missing()));
            if let Err(error)=self.add((item.offset+i)as i32,&item.id,&image){
                *self.load_error.borrow_mut()=Some(error.clone());return Err(error);
            }
        }
        select(self.editor,0,0);
        unsafe{SendMessageW(self.editor,0xcd,0,0);SendMessageW(self.editor,EM_SETMODIFY_,0,0);}
        Ok(())
    }
    pub(crate) fn paste_image(&self,ui:&Ui,id:&str)->Result<(),String>{
        if self.contents()?.1.len()>=20{return Err("This note already has 20 images.".into());}
        let image=thumbnail(ui,id)?;
        let (start,_)=selection(self.editor);
        self.add(-1,id,&image)?;
        select(self.editor,start+1,start+1);
        unsafe{SetFocus(self.editor);SendMessageW(self.editor,0xb7,0,0);}
        Ok(())
    }
    pub(crate) fn remove_at(&self,index:usize)->Result<(),String>{
        let list=objects(self.editor)?;
        let &(cp,_)=list.get(index).ok_or("The image is no longer in this note.")?;
        select(self.editor,cp,cp+1);
        unsafe{SendMessageW(self.editor,EM_REPLACESEL_,1,wide("").as_ptr()as _);}
        Ok(())
    }
}
pub(crate) fn control_action(id:u16)->Option<&'static str>{
    match id{PASTE=>Some("attach"),OPEN=>Some("image"),REMOVE=>Some("detach"),_=>None}
}
pub(crate) fn load_note(ui:&Ui,row:&[String])->Result<(),String>{
    let editor=&ui.images;
    let key=NoteDocument{id:row.first().cloned().unwrap_or_default(),revision:ui.records.preview_revision(),
        appearance:ui.appearance,zoom:ui.zoom,dpi:crate::shell::dpi(editor.editor)};
    // A page switch hides this particular RichEdit, leaving its text, OLE
    // objects, selection and undo state intact. A changed note/profile/theme
    // or a native edit always reconstructs the document from saved data.
    let reuse=unsafe{GetWindowLongW(editor.editor,GWL_STYLE) as u32&WS_VISIBLE==0
        && SendMessageW(editor.editor,EM_GETMODIFY_,0,0)==0};
    if reuse && editor.document.borrow().as_ref()==Some(&key){return Ok(())}
    editor.load_row(ui,row)?;
    editor.document.replace(Some(key));
    Ok(())
}
pub(crate) fn paste_note(ui:&mut Ui)->Result<(),String>{
    if ui.images.contents()?.1.len()>=20{return Err("This note already has 20 images.".into());}
    let id=workspace::attach_clipboard_image(ui)?;
    ui.images.paste_image(ui,&id)?;
    ui.dirty_note=true;crate::core::save_note(ui)
}
pub(crate) fn selected_note_image(ui:&Ui)->Option<(usize,String)>{ui.images.selected_image()}
pub(crate) fn remove_note_image(ui:&mut Ui,index:usize)->Result<(),String>{
    ui.images.remove_at(index)?;
    ui.dirty_note=true;crate::core::save_note(ui)
}
pub(crate) fn set_clipboard(ui:&Ui,id:Option<&str>){
    if ui.images.clip.image.borrow().as_ref().map(|p|p.0.as_str())==id{return;}
    let image=id.map(|id|(id.to_owned(),thumbnail(ui,id).unwrap_or_else(|_|Rc::new(missing()))));
    *ui.images.clip.image.borrow_mut()=image;
    unsafe{InvalidateRect(ui.images.preview,null(),1);}
    // The Clipboard layout uses its card grid. Its legacy selected-image HWND
    // is hidden, so changing the selection requires no page-wide relayout.
}
pub(crate) fn command(ui:&mut Ui,id:u16,_code:u16)->bool{
    let result=match id {
        PREVIEW if ui.page=="clipboard"=>ui.images.clip.image.borrow().as_ref().map(|p|p.0.clone()),
        OPEN if ui.page=="notes"=>ui.images.selected_image().map(|p|p.1),
        REMOVE if ui.page=="notes"=>{ui.run_action("detach");return true;}
        PASTE if ui.page=="notes"=>{ui.run_action("attach");return true;}
        _=>return false
    };
    if let Some(id)=result{if let Err(message)=workspace::view_image(ui,&id){ui.status(&message);}}
    true
}
unsafe fn send_child_action(h:HWND,id:u16){PostMessageW(GetParent(h),WM_COMMAND,id as usize,0);}
unsafe fn image_hit(h:HWND,lp:LPARAM)->Option<(usize,bool)> {
    let point=windows_sys::Win32::Foundation::POINT{x:lp as i16 as i32,y:(lp>>16) as i16 as i32};
    let nearest=SendMessageW(h,0xd7,0,&point as *const _ as _); // EM_CHARFROMPOS
    let mut numerator=0u32;let mut denominator=0u32;
    SendMessageW(h,EM_GETZOOM,&mut numerator as *mut _ as _,&mut denominator as *mut _ as _);
    let zoom=if denominator==0{1.0}else{numerator as f32/denominator as f32};
    let scale=(crate::shell::dpi(h) as f32/96.0)*zoom;
    for (cp,_) in objects(h).ok()?.into_iter().filter(|(cp,_)|(*cp as isize-nearest).abs()<=1) {
        let mut p=windows_sys::Win32::Foundation::POINT::default();
        SendMessageW(h,0xd6,&mut p as *mut _ as _,cp as _); // EM_POSFROMCHAR
        let x=(point.x-p.x) as f32/scale-IMAGE_MARGIN_X as f32;
        let y=(point.y-p.y) as f32/scale-IMAGE_MARGIN_Y as f32;
        if (0.0..IMAGE_W as f32).contains(&x) && (0.0..IMAGE_H as f32).contains(&y) {
            return Some((cp,x>=IMAGE_W as f32-20.0 && y<20.0));
        }
    }
    None
}
unsafe fn object_at(h:HWND,lp:LPARAM)->Option<usize>{image_hit(h,lp).map(|(cp,_)|cp)}
// Rich Edit reports UTF-16 positions including inline OLE images. Read the
// original range directly so an adjacent image never shifts a link target.
unsafe fn web_link(editor: HWND, range: CHARRANGE) -> Option<String> {
    let len = range.cpMax.checked_sub(range.cpMin)?;
    if range.cpMin < 0 || !(1..=4096).contains(&len) { return None; }
    let mut text = vec![0u16; len as usize + 1];
    let request = TEXTRANGEW { chrg: range, lpstrText: windows::core::PWSTR(text.as_mut_ptr()) };
    let copied = SendMessageW(editor, EM_GETTEXTRANGE, 0, &request as *const _ as _);
    if copied != len as isize { return None; }
    let url = String::from_utf16(&text[..len as usize]).ok()?;
    workspace::valid_url(&url).then_some(url)
}

unsafe fn follow_link(editor: HWND, range: CHARRANGE) -> bool {
    let Some(url) = web_link(editor, range) else { return false; };
    if let Err(error) = workspace::open_web_link(editor, &url) {
        MessageBoxW(editor, wide(&error).as_ptr(), wide("Buddy Notes").as_ptr(), MB_OK | MB_ICONERROR);
    }
    true
}

pub(crate) unsafe fn link_notification(parent: HWND, lp: LPARAM) -> LRESULT {
    if lp == 0 { return 0; }
    let header = std::ptr::read_unaligned(lp as *const windows::Win32::UI::Controls::NMHDR);
    if header.code != EN_LINK || header.hwndFrom.0 != GetDlgItem(parent, NOTE_EDITOR as i32) { return 0; }
    let link = &*(lp as *const ENLINK);
    if link.msg == WM_LBUTTONUP && link.wParam.0 & 0x0008 != 0 { // MK_CONTROL at the time of the click
        return follow_link(header.hwndFrom.0, link.chrg) as LRESULT;
    }
    0
}

// Handle these before dialog translation so a second WM_CHAR cannot insert
// the clipboard contents again or put a newline in a URL that was just opened.
pub(crate) unsafe fn editor_shortcut(msg: &MSG) -> bool {
    if msg.message != WM_KEYDOWN || GetDlgCtrlID(msg.hwnd) != NOTE_EDITOR as i32 { return false; }
    let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
    if (msg.wParam == 86 && ctrl) || (msg.wParam == 45 && GetKeyState(VK_SHIFT as i32) < 0) {
        SendMessageW(msg.hwnd, WM_PASTE, 0, 0);
        return true;
    }
    if msg.wParam == 13 && ctrl {
        let (a,b) = selection(msg.hwnd);
        return follow_link(msg.hwnd, CHARRANGE { cpMin:a as i32, cpMax:b as i32 });
    }
    false
}

unsafe extern "system" fn note_proc(h:HWND,msg:u32,wp:WPARAM,lp:LPARAM,sub:usize,data:usize)->LRESULT{
    let formatted=data as *const Cell<bool>;
    match msg{
        WM_NCDESTROY=>{RemoveWindowSubclass(h,Some(note_proc),sub);}
        WM_PASTE=>{
            let png=RegisterClipboardFormatW(wide("PNG").as_ptr());
            let image=[png,CF_DIB.0 as u32,CF_DIBV5.0 as u32,CF_BITMAP.0 as u32].into_iter().any(|f|f!=0&&IsClipboardFormatAvailable(f)!=0);
            if image{SendMessageW(GetParent(h),WM_COMMAND,PASTE as usize,0);}
            else if let Some(rtf)=rich_text::clipboard_rtf(h){
                (*formatted).set(true);
                rich_text::insert_rtf(h,&rtf,true);
            }else{SendMessageW(h,EM_PASTESPECIAL,CF_UNICODETEXT.0 as _,0);}
            return 0;
        }
        WM_LBUTTONDOWN=>{
            RemovePropW(h,wide("BuddyPressedImage").as_ptr());
            if let Some((cp,close))=image_hit(h,lp){
                SetFocus(h);select(h,cp,cp+1);
                SetPropW(h,wide("BuddyPressedImage").as_ptr(),(2*cp+usize::from(close)+1) as _);
                return 0;
            }
        }
        WM_LBUTTONUP=>{
            let pressed=RemovePropW(h,wide("BuddyPressedImage").as_ptr()) as usize;
            if pressed!=0{
                if let Some((cp,close))=image_hit(h,lp){
                    if 2*cp+usize::from(close)+1==pressed{select(h,cp,cp+1);send_child_action(h,if close{REMOVE}else{OPEN});}
                }
                return 0;
            }
        }
        WM_LBUTTONDBLCLK if object_at(h,lp).is_some()=>return 0,
        WM_KEYDOWN if matches!(wp,66|73|85)&&GetKeyState(VK_CONTROL as i32)<0=>{(*formatted).set(true);}
        WM_KEYDOWN if wp==0x75=>{ // F6 and Shift+F6 select the next/previous inline object.
            if let Ok(list)=objects(h){
                let (start,end)=selection(h);
                let target=if GetKeyState(VK_SHIFT as i32)<0{list.iter().rev().find(|p|p.0<start).or_else(||list.last())}else{list.iter().find(|p|p.0>=end).or_else(||list.first())};
                if let Some(&(cp,_))=target{select(h,cp,cp+1);SendMessageW(h,0xb7,0,0);return 0;}
            }
        }
        WM_KEYDOWN if wp==13=>{
            let (a,b)=selection(h);
            if b==a+1&&objects(h).is_ok_and(|v|v.iter().any(|p|p.0==a)){send_child_action(h,OPEN);return 0;}
        }
        WM_CONTEXTMENU=>{
            let (a,b)=selection(h);let mut cp=if b==a+1&&objects(h).is_ok_and(|v|v.iter().any(|o|o.0==a)){Some(a)}else{None};
            let mut point=windows_sys::Win32::Foundation::POINT{x:(lp as i16)as i32,y:((lp>>16)as i16)as i32};
            if lp != -1{
                let mut client=point;ScreenToClient(h,&mut client);
                cp=object_at(h,((client.x as isize)&65535)|((client.y as isize)<<16));
            }else{GetCaretPos(&mut point);ClientToScreen(h,&mut point);}
            if let Some(pos)=cp{select(h,pos,pos+1);}
            else if lp != -1{let mut client=point;ScreenToClient(h,&mut client);
                let pos=SendMessageW(h,0xd7,0,&client as *const _ as _).max(0)as usize;
                if pos<a||pos>=b{select(h,pos,pos);}}
            let menu=CreatePopupMenu();if menu.is_null(){return 0;}
            let selected=selection(h);let has_selection=selected.0!=selected.1;
            for (action,label,enabled) in [(WM_CUT,"Cut",has_selection),(WM_COPY,"Copy",has_selection),(WM_PASTE,"&Paste",true),(EM_SETSEL_,"Select all",true)]{
                AppendMenuW(menu,MF_STRING|if enabled{0}else{MF_GRAYED},action as _,wide(label).as_ptr());
            }
            if cp.is_some(){
                AppendMenuW(menu,MF_SEPARATOR,0,null());AppendMenuW(menu,MF_STRING,OPEN as _,wide("Open image in markup").as_ptr());
                AppendMenuW(menu,MF_STRING,REMOVE as _,wide("Remove image").as_ptr());
            }
            let choice=crate::chrome::menu::track(h,menu,TPM_RETURNCMD|TPM_RIGHTBUTTON,point.x,point.y,crate::chrome::menu::colors())as u32;
            DestroyMenu(menu);match choice{
                WM_CUT|WM_COPY|WM_PASTE=>{SendMessageW(h,choice,0,0);}
                EM_SETSEL_=>select(h,0,usize::MAX),
                n if n==OPEN as u32||n==REMOVE as u32=>send_child_action(h,n as u16),_=>{}
            }return 0;
        }
        _=>{}
    }
    DefSubclassProc(h,msg,wp,lp)
}
// RichEdit resolves automatic ink to the Windows system text color, even on
// an explicitly dark background. TOM's temporary formatting changes only the
// display. Streaming, copying, undo and the user's authored colors stay native.
unsafe fn paint_note_ink(h:HWND,state:&NoteHint){
    let Some((text,link))=state.ink.get()else{return};
    let mut client=RECT::default();GetClientRect(h,&mut client);
    let index=|x,y|{let point=windows_sys::Win32::Foundation::POINT{x,y};SendMessageW(h,0xd7,0,&point as *const _ as _)as i32}; // RichEdit EM_CHARFROMPOS
    let start=index(1,1).max(0);let visible_end=index((client.right-1).max(1),(client.bottom-1).max(1)).saturating_add(1);
    if state.painted.get().is_some_and(|(from,to)|from<=start && to>=visible_end){return}
    let Ok(ole)=interface(h)else{return};let Ok(doc)=ole.cast::<ITextDocument>()else{return};
    let Ok(range)=doc.Range(start,start)else{return};
    // On the first paint after inserting an OLE image, hit-testing can still
    // describe the old line layout. Cover a viewport-sized block ahead of it.
    // This keeps updates local even for long, heavily formatted documents.
    let ahead=(client.right.saturating_mul(client.bottom)/24).clamp(256,16_384);
    let limit=visible_end.max(start.saturating_add(ahead));
    let Ok(length)=range.GetStoryLength()else{return};
    let end=limit.min(length);let mut cp=start;
    state.painted.set(Some((start,limit)));
    // Msftedit records an undo item and emits EN_CHANGE even for display-only
    // formatting. Preserve the undo stack and modified state through this call.
    let mut scroll=windows_sys::Win32::Foundation::POINT::default();
    SendMessageW(h,EM_GETSCROLLPOS,0,&mut scroll as *mut _ as _);
    let suspended=doc.Undo(tomSuspend.0).is_ok();
    let modified=SendMessageW(h,EM_GETMODIFY_,0,0);
    let mask=SendMessageW(h,EM_GETEVENTMASK,0,0);
    SendMessageW(h,EM_SETEVENTMASK,0,mask & !(ENM_CHANGE as isize));
    while cp<end{
        if range.SetRange(cp,cp).is_err()||range.EndOf(tomCharFormat.0,tomExtend.0).is_err(){break}
        let next=range.GetEnd().unwrap_or(cp).min(end);if next<=cp{break}
        if range.SetRange(cp,next).is_err(){break}
        if let Ok(font)=range.GetFont(){if font.GetForeColor()==Ok(tomAutoColor.0){
            let is_link=font.cast::<ITextFont2>().and_then(|v|v.GetLinkType()).is_ok_and(|kind|kind!=tomNoLink.0);
            if font.Reset(tomApplyTmp).is_ok(){let _=font.SetForeColor(if is_link{link}else{text}as i32);let _=font.Reset(tomApplyNow);}
        }}
        cp=next;
    }
    if suspended{let _=doc.Undo(tomResume.0);}
    let mut current=windows_sys::Win32::Foundation::POINT::default();
    SendMessageW(h,EM_GETSCROLLPOS,0,&mut current as *mut _ as _);
    if current.x!=scroll.x||current.y!=scroll.y{SendMessageW(h,EM_SETSCROLLPOS,0,&scroll as *const _ as _);}
    if modified==0{SendMessageW(h,EM_SETMODIFY_,0,0);}
    SendMessageW(h,EM_SETEVENTMASK,0,mask);
}

unsafe extern "system" fn hint_proc(h:HWND,msg:u32,wp:usize,lp:isize,sub:usize,data:usize)->isize {
    if msg==WM_NCDESTROY {RemoveWindowSubclass(h,Some(hint_proc),sub);return DefSubclassProc(h,msg,wp,lp);}
    let hint=&*(data as *const NoteHint);
    if matches!(msg,WM_SETTEXT|WM_CHAR|WM_KEYDOWN|WM_PASTE|WM_CUT|WM_CLEAR|WM_UNDO|0xc7|WM_IME_COMPOSITION|EM_REDO|EM_STREAMIN|EM_SETTEXTEX|EM_SETCHARFORMAT|EM_REPLACESEL_){hint.painted.set(None);}
    if matches!(msg,WM_PAINT|WM_PRINT|WM_PRINTCLIENT){paint_note_ink(h,hint);}
    let result=DefSubclassProc(h,msg,wp,lp);
    if matches!(msg,WM_PAINT|WM_PRINT|WM_PRINTCLIENT) && GetWindowTextLengthW(h)==0 {
        let dc=if msg==WM_PAINT {GetDC(h)} else {wp as HDC};
        if !dc.is_null() && !hint.font.get().is_null() {
            let mut r=RECT::default();SendMessageW(h,0xb2,0,&mut r as *mut _ as _); // EM_GETRECT
            crate::chrome::text(dc,r,"Write down an idea, task, or reminder…",hint.font.get(),hint.color.get(),DT_LEFT|DT_TOP|DT_SINGLELINE);
        }
        if msg==WM_PAINT && !dc.is_null(){ReleaseDC(h,dc);}
    }
    result
}
unsafe extern "system" fn preview_proc(h:HWND,msg:u32,wp:WPARAM,lp:LPARAM,sub:usize,data:usize)->LRESULT{
    if msg==WM_NCDESTROY{RemoveWindowSubclass(h,Some(preview_proc),sub);}
    if matches!(msg,WM_PAINT|WM_PRINTCLIENT){
        let mut ps=PAINTSTRUCT::default();let dc=if msg==WM_PAINT{BeginPaint(h,&mut ps)}else{wp as HDC};
        let mut rect=RECT::default();GetClientRect(h,&mut rect);
        {let state=&*(data as *const Preview);
            let bg=state.field.get();SetDCBrushColor(dc,bg);FillRect(dc,&rect,GetStockObject(DC_BRUSH));
            if let Some((_,image))=state.image.borrow().as_ref(){
                let pad=(crate::shell::dpi(h)/24).max(2)as i32;
                let area=RECT{left:pad,top:pad,right:rect.right-pad,bottom:rect.bottom-pad};
                draw(dc,area,image,bg);
                if windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus()==h{DrawFocusRect(dc,&area);}
            }
        }if msg==WM_PAINT{EndPaint(h,&ps);}return 0;
    }
    DefSubclassProc(h,msg,wp,lp)
}

impl Drop for Controls{
    fn drop(&mut self){unsafe{
        RemoveWindowSubclass(self.preview,Some(preview_proc),1);RemoveWindowSubclass(self.editor,Some(note_proc),1);RemoveWindowSubclass(self.editor,Some(hint_proc),2);
    }}
}

#[cfg(test)]
mod tests{
    use super::*;
    use windows::Win32::System::Ole::{OleInitialize,OleUninitialize};

    #[test]
    fn earlier_note_ids_default_to_the_end_and_offsets_count_utf16(){
        let mut row=vec![String::new();7];row[2]="A😀\r\nB".into();
        row.extend(["old-a".to_string(),"old-b".to_string()]);
        assert_eq!(placements(&row,None).iter().map(|v|v.offset).collect::<Vec<_>>(),[5,5]);
        let offsets=vec![vec!["4".into(),"1".into()]];
        assert_eq!(placements(&row,Some(&offsets))[0],Placement{id:"old-b".into(),offset:1});
    }

    unsafe fn capture(hwnd:HWND,width:i32,height:i32)->Vec<u8>{
        let dc=CreateCompatibleDC(null_mut());let mut info=BITMAPINFO::default();
        info.bmiHeader.biSize=40;info.bmiHeader.biWidth=width;info.bmiHeader.biHeight=-height;
        info.bmiHeader.biPlanes=1;info.bmiHeader.biBitCount=32;let mut pixels=null_mut();
        let bmp=CreateDIBSection(dc,&info,DIB_RGB_COLORS,&mut pixels,null_mut(),0);
        assert!(!bmp.is_null()&&!pixels.is_null());
        let old=SelectObject(dc,bmp);
        SendMessageW(hwnd,WM_PRINTCLIENT,dc as _,(PRF_CLIENT|PRF_ERASEBKGND)as _);GdiFlush();
        let bytes=std::slice::from_raw_parts(pixels.cast::<u8>(),(width*height*4)as usize).to_vec();
        SelectObject(dc,old);DeleteObject(bmp);DeleteDC(dc);bytes
    }

    unsafe fn fixture_clipboard(h:HWND,format:u32,bytes:&[u8]){
        use windows_sys::Win32::System::DataExchange::{OpenClipboard,CloseClipboard,EmptyClipboard,SetClipboardData};
        let mem=GlobalAlloc(GMEM_MOVEABLE,bytes.len());assert!(!mem.is_null());
        let ptr=GlobalLock(mem).cast::<u8>();assert!(!ptr.is_null());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(),ptr,bytes.len());GlobalUnlock(mem);
        assert_ne!(OpenClipboard(h),0);assert_ne!(EmptyClipboard(),0);
        assert!(!SetClipboardData(format,mem).is_null());CloseClipboard();
    }
    unsafe extern "system" fn test_parent(h:HWND,msg:u32,wp:usize,lp:isize,sub:usize,data:usize)->isize{
        if msg==WM_COMMAND&&matches!(wp as u16,PREVIEW|PASTE){(*(data as *const Cell<u16>)).set(wp as u16);}
        if msg==WM_NCDESTROY{RemoveWindowSubclass(h,Some(test_parent),sub);}
        DefSubclassProc(h,msg,wp,lp)
    }
    fn optional_picture(name:&str,bytes:&[u8],w:u32,h:u32){
        if let Some(folder)=std::env::var_os("BUDDY_IMAGE_TEST_OUTPUT"){
            let mut rgba=bytes.to_vec();
            for p in rgba.chunks_exact_mut(4){p.swap(0,2);p[3]=255;}
            let image=RgbaImage::from_raw(w,h,rgba).unwrap();
            image.save(PathBuf::from(folder).join(name)).unwrap();
        }
    }

    #[test]
    #[ignore="checks native note text pixels and RTF on a private Windows window station"]
    fn automatic_note_color_tracks_theme_on_both_editors(){
        workspace::with_private_test_desktop(||unsafe{
            let parent=CreateWindowExW(0,wide("STATIC").as_ptr(),wide("Note color test").as_ptr(),WS_POPUP,0,0,450,300,null_mut(),null_mut(),null_mut(),null());
            let mut ui=Ui::new(windows::Win32::Foundation::HWND(parent),std::env::temp_dir().join(format!("Buddy-note-colors-{}",std::process::id())),true).unwrap();
            let note=["map","Coastal trail guide","Map palette\n\nSea glass marks the trail. Navy is for place names.\nBase map: https://www.openstreetmap.org","1","1","","1"].map(str::to_owned);
            let rtf=r"{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}{\colortbl ;\red0\green105\blue109;}\f0\fs20 {\b\fs30\cf1 Map palette}\par\par Sea glass marks the trail. Navy is for place names.\par Base map: https://www.openstreetmap.org\par}";
            let dark=(0x181212,0x232020,0xf6f1f4,0xce9500);let light=(0xffffff,0xffffff,0x291809,0xaa5500);
            let area=crate::chrome::rect(0,0,420,230);
            let verify=|edit:&Controls,palette:crate::chrome::Colors,css:i32|{
                edit.note_layout(true,area,100,palette,css);select(edit.editor,0,0);
                let pixels=capture(edit.editor,420,230);
                let ink_in_run=|cp:usize,ink:u32|{
                    let mut p=windows_sys::Win32::Foundation::POINT::default();SendMessageW(edit.editor,0xd6,&mut p as *mut _ as _,cp as _);
                    let c=ink.to_le_bytes();let mut count=0;
                    for y in p.y.max(0)..(p.y+17).min(230){for x in p.x.max(0)..(p.x+150).min(420){
                        let at=(y*420+x)as usize*4;
                        if pixels[at].abs_diff(c[2])<36&&pixels[at+1].abs_diff(c[1])<36&&pixels[at+2].abs_diff(c[0])<36{count+=1;}
                    }}count
                };
                assert!(ink_in_run(0,0x6d6900)>25,"custom heading ink");
                let count=ink_in_run(13,palette.2);assert!(count>20,"{css}px body must paint with theme ink {:#x}, got {count} pixels",palette.2);
                let doc:ITextDocument=interface(edit.editor).unwrap().cast().unwrap();let font=doc.Range(13,14).unwrap().GetFont().unwrap();
                assert_eq!(font.GetForeColor().unwrap(),tomAutoColor.0,"stored body color is automatic");
                assert_eq!(doc.Range(0,1).unwrap().GetFont().unwrap().GetForeColor().unwrap(),0x6d6900,"preserve explicit heading color");
                let saved=edit.format().unwrap().unwrap();assert!(!saved.contains(r"\red244\green241\blue246")&&!saved.contains(r"\red9\green24\blue41"));saved
            };
            for css in [10,13]{
                let edit=Controls::new(parent).unwrap();ui.records.insert(format_key("map"),vec![vec![rtf.into()]]);edit.note_layout(true,area,100,dark,css);edit.load_row(&ui,&note).unwrap();
                verify(&edit,dark,css);verify(&edit,light,css);let saved=verify(&edit,dark,css);
                ui.records.insert(format_key("map"),vec![vec![saved]]);edit.set_body("");edit.load_row(&ui,&note).unwrap();verify(&edit,light,css);
                // Use an actual nonzero scroll offset and a nonempty selection.
                let long=(0..140).map(|i|format!("Line {i} text\n")).collect::<String>();
                edit.set_body(&long);edit.note_layout(true,area,100,light,css);
                select(edit.editor,600,604);let mut pos=windows_sys::Win32::Foundation::POINT{x:0,y:180};
                SendMessageW(edit.editor,EM_SETSCROLLPOS,0,&mut pos as *mut _ as _);
                let scroll=||{let mut p=windows_sys::Win32::Foundation::POINT::default();SendMessageW(edit.editor,EM_GETSCROLLPOS,0,&mut p as *mut _ as _);(p.x,p.y)};
                let original=scroll();assert!(original.1>0);
                edit.note_layout(true,area,100,dark,css);capture(edit.editor,420,230);
                assert_eq!(scroll(),original,"theme refresh keeps the scrolled note viewport");
                assert_eq!(selection(edit.editor),(600,604),"theme refresh keeps the selected text");
                drop(edit);
            }
            DestroyWindow(parent);drop(ui);
        });
    }

    #[test]
    #[ignore="uses a private Windows window station and clipboard"]
    fn styled_clipboard_and_inline_image_survive_reopening() {
        workspace::with_private_test_desktop(|| unsafe {
            let parent=CreateWindowExW(0,wide("STATIC").as_ptr(),wide("Rich note").as_ptr(),WS_POPUP,0,0,440,320,null_mut(),null_mut(),null_mut(),null());
            assert!(!parent.is_null());
            let edit=Controls::new(parent).unwrap();
            let palette=(0xffffff,0xffffff,0,0xff0000);
            edit.layout("notes",true,(0,0,420,280),100,palette);
            let sample="<p><strong>Bold</strong> <em>italic</em> <span style='color:#cb2030;font-size:16pt'><u>color</u></span> 😀</p><div><a href='https://example.test/qa'>link</a></div>";
            let expected="Bold italic color 😀\nlink (https://example.test/qa)";
            fixture_clipboard(parent,RegisterClipboardFormatW(wide("HTML Format").as_ptr()),sample.as_bytes());
            SendMessageW(edit.editor,WM_PASTE,0,0);
            assert_eq!(edit.body(),expected);
            let verify=|h:HWND|{
                let style=|i:usize|{select(h,i,i+1);let mut f=CHARFORMATW::default();f.cbSize=size_of::<CHARFORMATW>()as _;
                    SendMessageW(h,EM_GETCHARFORMAT,SCF_SELECTION as _,&mut f as *mut _ as _);f};
                assert_ne!((style(1).dwEffects&CFE_BOLD).0,0,"bold run");
                assert_eq!((style(5).dwEffects&CFE_BOLD).0,0,"plain run ends bold");
                assert_ne!((style(6).dwEffects&CFE_ITALIC).0,0,"italic run");
                let colored=style(13);assert_ne!((colored.dwEffects&CFE_UNDERLINE).0,0);
                assert_eq!(colored.crTextColor.0,0x3020cb);assert_eq!(colored.yHeight,320);
            };
            verify(edit.editor);
            edit.add(20,"qa-image",&missing()).unwrap();
            let (body,images)=edit.contents().unwrap();
            assert_eq!(body,expected);assert_eq!(images[0].offset,20);
            let rtf=edit.format().unwrap().expect("rich clipboard import marked editor formatted");
            edit.set_body("");
            assert!(rich_text::restore(edit.editor,&rtf,&body),"unexpected round trip: {:?}; RTF: {rtf}",edit.body());
            edit.formatted.set(true);
            edit.add(images[0].offset as i32,"qa-image",&missing()).unwrap();
            edit.layout("notes",true,(0,0,410,270),150,palette);
            verify(edit.editor);
            assert_eq!(edit.contents().unwrap(),(body,images));
            edit.set_body("plain note");
            select(edit.editor,1,2);
            let mut plain=CHARFORMATW::default();plain.cbSize=size_of::<CHARFORMATW>() as _;
            SendMessageW(edit.editor,EM_GETCHARFORMAT,SCF_SELECTION as _,&mut plain as *mut _ as _);
            assert_eq!((plain.dwEffects&(CFE_BOLD|CFE_ITALIC|CFE_UNDERLINE)).0,0);
            assert!(edit.format().unwrap().is_none());
            select(edit.editor,0,usize::MAX);
            fixture_clipboard(parent,RegisterClipboardFormatW(wide("Rich Text Format").as_ptr()),br"{\rtf1\ansi {\b Bold} {\i italic} {\*\generator omitted;}{\field{\*\fldinst ignored}{\fldrslt label}}}");
            SendMessageW(edit.editor,WM_PASTE,0,0);
            assert_eq!(edit.body(),"Bold italic label");
            DestroyWindow(parent);drop(edit);
        });
    }

    #[test]
    #[ignore="run alone with --ignored --test-threads=1; uses a private Windows window station"]
    fn private_desktop_native_note_inline_pixels_and_keyboard(){
        workspace::with_private_test_desktop(||unsafe{
            OleInitialize(None).ok().unwrap();
            let hwnd=CreateWindowExW(0,wide("STATIC").as_ptr(),wide("Synthetic note images").as_ptr(),WS_POPUP,0,0,430,220,null_mut(),null_mut(),null_mut(),null());
            assert!(!hwnd.is_null());
            ShowWindow(hwnd,SW_SHOWNA);
            let controls=Controls::new(hwnd).unwrap();let palette=(0xffffff,0xffffff,0,0xff0000);
            controls.layout("notes",true,(0,0,420,200),100,palette);
            controls.set_body("A😀B\nSecond");
            let image=RgbaImage::from_fn(64,32,|x,_|image::Rgba(if x<32{[255,0,0,255]}else{[0,255,0,255]}));
            controls.add(3,"first",&image).unwrap();
            controls.add(9,"second",&image).unwrap();
            assert_eq!(controls.contents().unwrap(),("A😀B\nSecond".into(),vec![Placement{id:"first".into(),offset:3},Placement{id:"second".into(),offset:8}]));
            let bytes=capture(controls.editor,420,200);optional_picture("inline-note.png",&bytes,420,200);
            let colors=|bytes:&[u8],color:[u8;3]|bytes.chunks_exact(4).filter(|p|p[..3]==color).count();
            let red=colors(&bytes,[0,0,255]);let green=colors(&bytes,[0,255,0]);assert!(red>200&&green>200,"red {red}, green {green}");
            // Locate a pixel in the first image on the real Rich Edit surface.
            // The remove affordance and the picture route to distinct commands.
            let hit=bytes.chunks_exact(4).position(|pixel|pixel[..3]==[0,0,255]).unwrap();
            let click=|x:i32,y:i32| (x as isize&65535)|((y as isize)<<16);
            let near=click((hit%420)as i32+4,(hit/420)as i32+4);
            assert_eq!(image_hit(controls.editor,near),Some((3,false)));
            let mut origin=windows_sys::Win32::Foundation::POINT::default();
            SendMessageW(controls.editor,0xd6,&mut origin as *mut _ as _,3);
            for (point,action) in [(near,OPEN),(click(origin.x+62,origin.y+12),REMOVE)] {
                SendMessageW(controls.editor,WM_LBUTTONDOWN,1,point);SendMessageW(controls.editor,WM_LBUTTONUP,0,point);
                let mut message=MSG::default();
                assert_ne!(PeekMessageW(&mut message,hwnd,WM_COMMAND,WM_COMMAND,PM_REMOVE),0);
                assert_eq!(message.wParam,action as usize);
            }
            select(controls.editor,0,0);SendMessageW(controls.editor,WM_KEYDOWN,0x75,0);
            assert_eq!(controls.selected_image(),Some((0,"first".to_owned())));
            SendMessageW(controls.editor,WM_KEYDOWN,0x75,0);
            assert_eq!(controls.selected_image(),Some((1,"second".to_owned())));
            SendMessageW(controls.editor,WM_KEYDOWN,13,0);let mut command=MSG::default();
            assert_ne!(PeekMessageW(&mut command,hwnd,WM_COMMAND,WM_COMMAND,PM_REMOVE),0);assert_eq!(command.wParam,OPEN as usize);
            SendMessageW(controls.editor,EM_REPLACESEL_,1,wide("").as_ptr()as _);
            assert_eq!(controls.contents().unwrap().1.len(),1);
            controls.layout("notes",true,(0,0,420,200),150,palette);
            let zoomed=capture(controls.editor,420,200);let zoom_red=colors(&zoomed,[0,0,255]);
            assert!(zoom_red>red/2,"zoomed {zoom_red}, original pair {red}");
            let pressed=Cell::new(0u16);
            SetWindowSubclass(hwnd,Some(test_parent),4,&pressed as *const _ as usize);
            let text=wide(" plain ✓");let bytes=std::slice::from_raw_parts(text.as_ptr().cast::<u8>(),text.len()*2);
            fixture_clipboard(hwnd,CF_UNICODETEXT.0 as _,bytes);
            select(controls.editor,usize::MAX,usize::MAX);SendMessageW(controls.editor,WM_PASTE,0,0);
            assert!(controls.body().ends_with(" plain ✓"));
            let mut png=std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image.clone()).write_to(&mut png,image::ImageFormat::Png).unwrap();
            fixture_clipboard(hwnd,RegisterClipboardFormatW(wide("PNG").as_ptr()),png.get_ref());
            SendMessageW(controls.editor,WM_PASTE,0,0);
            assert_eq!(pressed.get(),PASTE);
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardState,SetKeyboardState};
            let mut prior=[0u8;256];assert_ne!(GetKeyboardState(prior.as_mut_ptr()),0);
            for (modifier,key) in [(VK_CONTROL,86),(VK_SHIFT,45)]{
                let mut keys=prior;keys[modifier as usize]=0x80;assert_ne!(SetKeyboardState(keys.as_ptr()),0);
                fixture_clipboard(hwnd,CF_UNICODETEXT.0 as _,bytes);controls.set_body("");
                assert!(editor_shortcut(&MSG{hwnd:controls.editor,message:WM_KEYDOWN,wParam:key,..Default::default()}));
                assert_eq!(controls.body()," plain ✓");
                if key==86{
                    pressed.set(0);
                    fixture_clipboard(hwnd,RegisterClipboardFormatW(wide("PNG").as_ptr()),png.get_ref());
                    assert!(editor_shortcut(&MSG{hwnd:controls.editor,message:WM_KEYDOWN,wParam:key,..Default::default()}));
                    assert_eq!(pressed.get(),PASTE,"Ctrl+V delivers the clipboard PNG to the note editor");
                }
            }
            SetKeyboardState(prior.as_ptr());
            let url="https://example.test/docs?p=1#fig";
            controls.set_body(&format!("😀 {url} end"));
            controls.add(2,"prefix",&missing()).unwrap();
            let from=4;let to=from+url.encode_utf16().count() as i32;
            assert_eq!(web_link(controls.editor,CHARRANGE{cpMin:from,cpMax:to}).as_deref(),Some(url));
            assert!(web_link(controls.editor,CHARRANGE{cpMin:0,cpMax:to}).is_none());
            assert!(web_link(controls.editor,CHARRANGE{cpMin:0,cpMax:i32::MAX}).is_none());
            select(controls.editor,from as usize+2,from as usize+3);
            let mut format=CHARFORMATW::default();format.cbSize=size_of::<CHARFORMATW>()as u32;
            SendMessageW(controls.editor,EM_GETCHARFORMAT,SCF_SELECTION as _,&mut format as *mut _ as _);
            assert_ne!((format.dwEffects & CFE_LINK).0,0,"Rich Edit should mark the URL as a link");
            *controls.clip.image.borrow_mut()=Some(("fixture".into(),Rc::new(image)));
            controls.layout("clipboard",true,(0,0,240,96),100,palette);
            let clip=capture(controls.preview,240,96);optional_picture("inline-clipboard.png",&clip,240,96);
            let clip_red=colors(&clip,[0,0,255]);let clip_green=colors(&clip,[0,255,0]);
            assert!(clip_red>1000&&clip_green>1000,"clipboard red {clip_red}, green {clip_green}");
            SetFocus(controls.preview);SendMessageW(controls.preview,BM_CLICK,0,0);
            assert_eq!(pressed.get(),PREVIEW);
            controls.layout("notes",true,(0,0,420,90),100,palette);
            let body=(0..40).map(|i|format!("Line {i}")).collect::<Vec<_>>().join("\n");
            controls.set_body(&body);SetFocus(controls.editor);
            controls.add(body.encode_utf16().count()as i32,"below-fold",&missing()).unwrap();
            select(controls.editor,0,0);
            SendMessageW(controls.editor,WM_KEYDOWN,0x75,0);
            assert_eq!(controls.selected_image(),Some((0,"below-fold".into())));
            let mut scroll=windows_sys::Win32::Foundation::POINT::default();
            SendMessageW(controls.editor,EM_GETSCROLLPOS,0,&mut scroll as *mut _ as _);
            let first=SendMessageW(controls.editor,0xce,0,0);
            let lines=SendMessageW(controls.editor,0xba,0,0);
            assert!(scroll.y>0||first>0,"The selected offscreen object must scroll into view (scroll {}, first {}, lines {}).",scroll.y,first,lines);
            println!("Keyboard selection scrolled {} pixels to the image in a 40-line note.",scroll.y);
            println!("Native Clipboard preview painted {clip_red} red and {clip_green} green pixels and dispatched markup from its focusable button. Private clipboard text and image paste commands passed.");
            println!("Native Rich Edit painted {red} red and {green} green pixels, selected two separate images with F6, dispatched Enter, removed the selection, and painted {zoom_red} red pixels at 150%.");
            DestroyWindow(hwnd);drop(controls);OleUninitialize();
        });
    }
}
