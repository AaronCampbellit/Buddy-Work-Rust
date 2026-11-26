// Win32 may disable font smoothing on a remote desktop. Rasterize short labels
// with the installed system font at triple resolution, then blend the coverage.
// RichEdit and Windows input controls continue to handle editable text directly.
use super::*;
use std::{cell::RefCell,collections::VecDeque,mem::size_of};

const SCALE:i32=3;
const CACHE_BYTES:usize=256*1024;

struct Bitmap{handle:HBITMAP,bytes:*mut u32}
impl Bitmap{
    unsafe fn new(dc:HDC,w:i32,h:i32)->Option<Self>{
        let mut info=BITMAPINFO::default();info.bmiHeader=BITMAPINFOHEADER{biSize:size_of::<BITMAPINFOHEADER>() as _,biWidth:w,biHeight:-h,biPlanes:1,biBitCount:32,biCompression:BI_RGB,..Default::default()};
        let mut data=null_mut();let handle=CreateDIBSection(dc,&info,DIB_RGB_COLORS,&mut data,null_mut(),0);
        if handle.is_null(){return None}Some(Self{handle,bytes:data.cast()})
    }
}
impl Drop for Bitmap{fn drop(&mut self){unsafe{DeleteObject(self.handle);}}}
#[derive(PartialEq)]
struct FontKey{height:i32,weight:i32,face:[u16;32]}
struct Mask{font:FontKey,name:String,limit:i32,color:u32,bitmap:Bitmap,w:i32,h:i32}
struct Labels{dc:HDC,items:VecDeque<Mask>,size:usize}
impl Default for Labels{fn default()->Self{Self{dc:unsafe{CreateCompatibleDC(null_mut())},items:VecDeque::new(),size:0}}}
impl Drop for Labels{fn drop(&mut self){unsafe{DeleteDC(self.dc);}}}
thread_local!{static LABELS:RefCell<Labels>=RefCell::new(Labels::default());}

impl Labels{
    unsafe fn raster(&self,font:LOGFONTW,name:&str,limit:i32,color:u32)->Option<Mask>{
        let key=FontKey{height:font.lfHeight,weight:font.lfWeight,face:font.lfFaceName};
        let mut large=font;large.lfHeight*=SCALE;large.lfWidth*=SCALE;large.lfQuality=ANTIALIASED_QUALITY;
        let font=CreateFontIndirectW(&large);if font.is_null(){return None}let old=SelectObject(self.dc,font);
        let result=(||{
            let chars=wide(name);let mut extent=Default::default();
            if GetTextExtentPoint32W(self.dc,chars.as_ptr(),(chars.len()-1)as i32,&mut extent)==0{return None}
            let w=((extent.cx+SCALE-1)/SCALE+1).min(limit);let h=(extent.cy+SCALE-1)/SCALE;
            if w<=0||h<=0||w*h>24_000{return None}
            let sw=w*SCALE;let sh=h*SCALE;let source=Bitmap::new(self.dc,sw,sh)?;let target=Bitmap::new(self.dc,w,h)?;
            let prev=SelectObject(self.dc,source.handle);PatBlt(self.dc,0,0,sw,sh,BLACKNESS);
            SetBkColor(self.dc,0);SetBkMode(self.dc,TRANSPARENT as i32);SetTextColor(self.dc,0xffffff);
            let mut area=rect(0,0,sw,sh);
            DrawTextW(self.dc,chars.as_ptr(),(chars.len()-1) as i32,&mut area,DT_LEFT|DT_TOP|DT_NOPREFIX|DT_END_ELLIPSIS|DT_SINGLELINE);
            GdiFlush();SelectObject(self.dc,prev);
            let high=std::slice::from_raw_parts(source.bytes,(sw*sh) as usize);
            let pixels=std::slice::from_raw_parts_mut(target.bytes,(w*h) as usize);
            for (index,pixel)in pixels.iter_mut().enumerate(){
                let start=index/(w as usize)*SCALE as usize*sw as usize+index%(w as usize)*SCALE as usize;let mut sum=0;
                for y in 0..SCALE as usize{for x in 0..SCALE as usize{sum+=high[start+y*sw as usize+x]&255;}}
                let a=(sum+4)/9;
                let ch=|shift:u32|(((color>>shift)&255)*a+127)/255;
                *pixel=(a<<24)|(ch(0)<<16)|(ch(8)<<8)|ch(16);
            }
            Some(Mask{font:key,name:name.to_owned(),limit,color,bitmap:target,w,h})
        })();
        SelectObject(self.dc,old);DeleteObject(font);result
    }
}
pub(super) unsafe fn draw(dc:HDC,r:RECT,name:&str,font:HFONT,color:u32,flags:u32)->bool{
    if flags&DT_SINGLELINE==0||name.is_empty()||name.len()>400||r.right<=r.left||r.bottom<=r.top{return false}
    let mut lf=LOGFONTW::default();if GetObjectW(font,size_of::<LOGFONTW>()as _,(&mut lf as *mut LOGFONTW).cast())==0{return false}
    let limit=r.right-r.left;
    LABELS.with(|slot|{
        let Ok(mut cache)=slot.try_borrow_mut()else{return false};if cache.dc.is_null(){return false}
        let existing=cache.items.iter().position(|m|m.name==name && (m.limit==limit || (m.w<m.limit && m.w<=limit)) && m.color==color && m.font.height==lf.lfHeight && m.font.weight==lf.lfWeight && m.font.face==lf.lfFaceName);
        if let Some(i)=existing{let m=cache.items.remove(i).unwrap();cache.items.push_front(m);}
        else{
            let Some(m)=cache.raster(lf,name,limit,color)else{return false};
            cache.size+=(m.w*m.h*4)as usize;cache.items.push_front(m);
            while cache.size>CACHE_BYTES||cache.items.len()>64{let m=cache.items.pop_back().unwrap();cache.size-=(m.w*m.h*4)as usize;}
        }
        let Some(m)=cache.items.front()else{return false};
        let x=r.left+if flags&DT_CENTER!=0{(limit-m.w)/2}else if flags&DT_RIGHT!=0{limit-m.w}else{0};
        let y=r.top+if flags&DT_VCENTER!=0{(r.bottom-r.top-m.h)/2}else{0};
        let top=y.max(r.top);let ht=(y+m.h).min(r.bottom)-top;if ht<=0{return true}
        let prev=SelectObject(cache.dc,m.bitmap.handle);
        let drawn=AlphaBlend(dc,x,top,m.w,ht,cache.dc,0,top-y,m.w,ht,BLENDFUNCTION{BlendOp:AC_SRC_OVER as u8,BlendFlags:0,SourceConstantAlpha:255,AlphaFormat:AC_SRC_ALPHA as u8});
        SelectObject(cache.dc,prev);drawn!=0
    })
}
