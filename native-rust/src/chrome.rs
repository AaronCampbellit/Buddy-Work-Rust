//! Small GDI drawing primitives. The same routines paint screen, print and capture DCs.
use std::{cell::Cell, ptr::{null, null_mut}, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::{HWND, RECT}, Graphics::Gdi::*,
    UI::{Controls::WM_MOUSELEAVE, Input::KeyboardAndMouse::*,
        Shell::{SetWindowSubclass, DefSubclassProc, RemoveWindowSubclass}, WindowsAndMessaging::*}
};
use crate::ui::wide;

pub type Colors=(u32,u32,u32,u32);
pub fn rect(x:i32,y:i32,w:i32,h:i32)->RECT { RECT{left:x,top:y,right:x+w,bottom:y+h} }
pub fn inset(r:RECT,n:i32)->RECT { RECT{left:r.left+n,top:r.top+n,right:r.right-n,bottom:r.bottom-n} }
pub fn mix(a:u32,b:u32,amount:u32)->u32 {
    [0,8,16].into_iter().map(|s| ((((a>>s)&255)*(255-amount)+((b>>s)&255)*amount+127)/255)<<s).sum()
}
pub fn control((bg,field,_,_):Colors)->u32 {if bg&255>150 {0xffffff} else {mix(field,0xffffff,6)}}
pub fn fill(dc:HDC,r:RECT,color:u32) {unsafe{
    SetDCBrushColor(dc,color);FillRect(dc,&r,GetStockObject(DC_BRUSH));
}}
pub fn panel(dc:HDC,r:RECT,radius:i32,inside:u32,edge:u32) {unsafe{
    if r.right<=r.left || r.bottom<=r.top{return}
    let brush=SelectObject(dc,GetStockObject(DC_BRUSH));let pen=SelectObject(dc,GetStockObject(DC_PEN));
    SetDCBrushColor(dc,inside);SetDCPenColor(dc,edge);
    RoundRect(dc,r.left,r.top,r.right,r.bottom,2*radius,2*radius);
    SelectObject(dc,brush);SelectObject(dc,pen);
}}
pub fn text(dc:HDC,r:RECT,s:&str,font:HFONT,color:u32,flags:u32) {unsafe{
    let previous=SelectObject(dc,font);SetBkMode(dc,TRANSPARENT as i32);SetTextColor(dc,color);
    let mut r=r;let data=wide(s);DrawTextW(dc,data.as_ptr(),(data.len()-1) as i32,&mut r,flags|DT_NOPREFIX|DT_END_ELLIPSIS);
    SelectObject(dc,previous);
}}
pub fn width(dc:HDC,s:&str,font:HFONT)->i32 {unsafe{
    let previous=SelectObject(dc,font);let s=wide(s);let mut size=Default::default();
    GetTextExtentPoint32W(dc,s.as_ptr(),(s.len()-1) as i32,&mut size);SelectObject(dc,previous);size.cx
}}

thread_local!{static HOVERED:Cell<HWND>=const{Cell::new(null_mut())};}
pub fn hovered(hwnd:HWND)->bool { HOVERED.with(|v|v.get()==hwnd) }
unsafe extern "system" fn button_proc(hwnd:HWND,msg:u32,wp:usize,lp:isize,id:usize,_:usize)->isize{
    if GetDlgCtrlID(hwnd)==crate::ui::RESIZE_HANDLE as i32{
        match msg {
            WM_NCHITTEST=>return HTTRANSPARENT as isize,
            WM_GETDLGCODE=>return DLGC_WANTARROWS as isize,
            WM_KEYDOWN if wp==VK_UP as usize || wp==VK_DOWN as usize=>{
                PostMessageW(GetParent(hwnd),WM_COMMAND,if wp==VK_UP as usize{188}else{189},hwnd as _);return 0;
            }
            _=>{}
        }
    }
    match msg {
        WM_MOUSEMOVE if !hovered(hwnd)=>{
            HOVERED.with(|v|{let old=v.replace(hwnd);if !old.is_null(){InvalidateRect(old,null(),0);}});
            let mut event=TRACKMOUSEEVENT{cbSize:std::mem::size_of::<TRACKMOUSEEVENT>() as _,dwFlags:TME_LEAVE,hwndTrack:hwnd,dwHoverTime:0};
            TrackMouseEvent(&mut event);InvalidateRect(hwnd,null(),0);
        }
        WM_MOUSELEAVE|WM_NCDESTROY=>{
            if hovered(hwnd){HOVERED.with(|v|v.set(null_mut()));InvalidateRect(hwnd,null(),0);}
            if msg==WM_NCDESTROY {RemoveWindowSubclass(hwnd,Some(button_proc),id);}
        }
        WM_SETCURSOR=>{SetCursor(LoadCursorW(null_mut(),IDC_HAND));return 1;}
        _=>{}
    }
    DefSubclassProc(hwnd,msg,wp,lp)
}
pub fn install_button(hwnd:HWND){unsafe{SetWindowSubclass(hwnd,Some(button_proc),0xB017,0);}}

// The artwork is an alpha atlas of the same Lucide strokes used by Buddy's React renderer.
// Downsample the coverage in the current theme colors at the effective monitor DPI.
#[repr(usize)]
#[derive(Copy,Clone)]
pub enum Icon{Home,Quick,Clipboard,Note,Bell,Widgets,Devices,Settings,Up,Down,Plus,Search,Pencil,Clock,More,Timer,Music,External,Pin,Save}
fn atlas()->&'static image::RgbaImage {
    static IMAGE:OnceLock<image::RgbaImage>=OnceLock::new();
    IMAGE.get_or_init(||image::load_from_memory(include_bytes!("island-icons.png")).expect("Buddy icon atlas").to_rgba8())
}
pub fn icon(dc:HDC,x:i32,y:i32,size:i32,id:Icon,ink:u32,base:u32){
    if size<1 || size>160{return}
    let len=size as usize;let offset=id as u32*64;let atlas=atlas();
    let mut pixels=vec![0u32;len*len];
    for iy in 0..len {for ix in 0..len {
        let sx=ix*64/len;let ex=((ix+1)*64/len).max(sx+1);
        let sy=iy*64/len;let ey=((iy+1)*64/len).max(sy+1);
        let mut alpha=0;
        for py in sy..ey {for px in sx..ex {alpha+=u32::from(atlas.get_pixel(offset+px as u32,py as u32)[3]);}}
        let c=mix(base,ink,alpha/((ex-sx)*(ey-sy)) as u32);
        pixels[iy*len+ix]=((c&255)<<16)|(c&0xff00)|((c>>16)&255);
    }}
    let mut info=BITMAPINFO::default();
    info.bmiHeader=BITMAPINFOHEADER{biSize:std::mem::size_of::<BITMAPINFOHEADER>() as _,biWidth:size,biHeight:-size,biPlanes:1,biBitCount:32,biCompression:BI_RGB,..Default::default()};
    unsafe{SetDIBitsToDevice(dc,x,y,size as _,size as _,0,0,0,size as _,pixels.as_ptr().cast(),&info,DIB_RGB_COLORS);}
}
