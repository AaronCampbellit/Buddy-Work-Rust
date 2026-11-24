//! Choose a chroma key from one in-memory, unprocessed camera frame.
use std::{cell::RefCell, ptr::{null, null_mut}};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND, RECT, ERROR_CLASS_ALREADY_EXISTS},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::*,
};
use crate::ui::wide;

struct Snapshot {
    width: i32,
    height: i32,
    pixels: Vec<u8>, // Top-down BGRA for the native DIB renderer.
    point: (i32, i32),
}

impl Snapshot {
    fn from_image(image: image::DynamicImage) -> Self {
        let rgba = image.into_rgba8();
        let (width, height) = (rgba.width() as i32, rgba.height() as i32);
        let mut pixels = rgba.into_raw();
        for pixel in pixels.chunks_exact_mut(4) { pixel.swap(0, 2); }
        Self { width, height, pixels, point: (width / 2, height / 2) }
    }

    fn color(&self, x: i32, y: i32) -> String {
        let offset = ((y * self.width + x) * 4) as usize;
        let bgr = &self.pixels[offset..offset + 3];
        format!("#{:02x}{:02x}{:02x}", bgr[2], bgr[1], bgr[0])
    }

    fn layout(&self, client: &RECT) -> Option<RECT> {
        let (w, h) = (i64::from(client.right), i64::from(client.bottom));
        if w < 1 || h < 1 { return None; }
        let (iw, ih) = (i64::from(self.width), i64::from(self.height));
        let (vw, vh) = if w * ih <= h * iw {
            (w, (w * ih / iw).max(1))
        } else {
            ((h * iw / ih).max(1), h)
        };
        let (x,y) = (((w - vw) / 2) as i32, ((h - vh) / 2) as i32);
        Some(RECT { left:x, top:y, right:x+vw as i32, bottom:y+vh as i32 })
    }

    fn source_point(&self, r: &RECT, x: i32, y: i32) -> Option<(i32,i32)> {
        if x < r.left || y < r.top || x >= r.right || y >= r.bottom { return None; }
        Some((
            (i64::from(x-r.left)*i64::from(self.width)/i64::from(r.right-r.left)) as i32,
            (i64::from(y-r.top)*i64::from(self.height)/i64::from(r.bottom-r.top)) as i32,
        ))
    }
}

struct Picker { handle: HWND, parent: HWND, snapshot: Snapshot }
thread_local! {
    static OPEN: RefCell<Option<Picker>> = const { RefCell::new(None) };
    static COLOR: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn cancel() {
    COLOR.with(|c| c.borrow_mut().take());
    let window = OPEN.with(|s| s.borrow().as_ref().map(|p| p.handle));
    if let Some(h) = window { unsafe { DestroyWindow(h); } }
}

pub fn take_color() -> Option<String> { COLOR.with(|c| c.borrow_mut().take()) }

pub fn open(parent: HWND, image: image::DynamicImage) -> Result<(),String> {
    create(parent, Snapshot::from_image(image), true).map(|_| ())
}

fn create(parent: HWND, snapshot: Snapshot, visible: bool) -> Result<HWND,String> {
    cancel();
    unsafe {
        let name = wide("BuddyRustKeyPicker");
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc), hInstance: instance, lpszClassName: name.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_CROSS),
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err("Windows could not register the color picker.".into());
        }
        let title = wide("Sample chroma key · Click a pixel or use arrows and Enter · Esc to cancel");
        let h = CreateWindowExW(WS_EX_TOOLWINDOW, name.as_ptr(), title.as_ptr(),
            WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT, 720, 470,
            parent, null_mut(), instance, null());
        if h.is_null() { return Err("Windows could not open the color picker.".into()); }
        OPEN.with(|s| *s.borrow_mut() = Some(Picker { handle:h, parent, snapshot }));
        if visible { ShowWindow(h,SW_SHOWNORMAL); SetForegroundWindow(h); }
        Ok(h)
    }
}

unsafe extern "system" fn proc(h: HWND, msg: u32, wp: usize, lp: isize) -> isize {
    match msg {
        WM_NCDESTROY => {
            OPEN.with(|s| {
                let mut live = s.borrow_mut();
                if live.as_ref().is_some_and(|p| p.handle == h) { live.take(); }
            });
            DefWindowProcW(h,msg,wp,lp)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(h,&mut ps);
            let mut client = RECT::default();
            GetClientRect(h,&mut client);
            FillRect(dc,&client,GetStockObject(BLACK_BRUSH));
            OPEN.with(|s| {
                let live=s.borrow();
                let Some(p)=live.as_ref().filter(|p|p.handle==h) else { return };
                let frame=&p.snapshot;
                let Some(r)=frame.layout(&client) else { return };
                let mut info=BITMAPINFO::default();
                info.bmiHeader.biSize=std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                info.bmiHeader.biWidth=frame.width;
                info.bmiHeader.biHeight= -frame.height;
                info.bmiHeader.biPlanes=1;
                info.bmiHeader.biBitCount=32;
                SetStretchBltMode(dc,COLORONCOLOR);
                StretchDIBits(dc,r.left,r.top,r.right-r.left,r.bottom-r.top,0,0,
                    frame.width,frame.height,frame.pixels.as_ptr().cast(),&info,DIB_RGB_COLORS,SRCCOPY);
                let (px,py)=frame.point;
                let x=r.left+((i64::from(2*px+1)*i64::from(r.right-r.left))/(2*i64::from(frame.width)))as i32;
                let y=r.top+((i64::from(2*py+1)*i64::from(r.bottom-r.top))/(2*i64::from(frame.height)))as i32;
                DrawFocusRect(dc,&RECT{left:x-5,top:y-5,right:x+6,bottom:y+6});
            });
            EndPaint(h,&ps); 0
        }
        WM_SIZE => { InvalidateRect(h,null(),0); 0 }
        WM_KEYDOWN if wp == 27 => { DestroyWindow(h); 0 }
        WM_LBUTTONDOWN | WM_KEYDOWN => {
            let mut client=RECT::default();
            GetClientRect(h,&mut client);
            let picked=OPEN.with(|s| {
                let mut live=s.borrow_mut();
                let p=live.as_mut().filter(|p|p.handle==h)?;
                let frame=&mut p.snapshot;
                if msg==WM_LBUTTONDOWN {
                    let r=frame.layout(&client)?;
                    let x=(lp as u16 as i16)as i32;
                    let y=((lp>>16)as u16 as i16)as i32;
                    frame.point=frame.source_point(&r,x,y)?;
                } else {
                    let (x,y)=frame.point;
                    match wp {
                        37=>frame.point.0=(x-1).max(0), 39=>frame.point.0=(x+1).min(frame.width-1),
                        38=>frame.point.1=(y-1).max(0), 40=>frame.point.1=(y+1).min(frame.height-1),
                        13|32=>{}, _=>return None,
                    }
                    if !matches!(wp,13|32) { InvalidateRect(h,null(),0); return None; }
                }
                Some((p.parent,frame.color(frame.point.0,frame.point.1)))
            });
            if let Some((parent,color))=picked {
                COLOR.with(|c|*c.borrow_mut()=Some(color));
                DestroyWindow(h);
                PostMessageW(parent,super::OCR_DONE,0,0);
            }
            0
        }
        _=>DefWindowProcW(h,msg,wp,lp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Snapshot {
        let rgb = image::RgbImage::from_raw(2,1,vec![0x12,0xab,0x56,0xde,0x01,0x8f]).unwrap();
        Snapshot::from_image(image::DynamicImage::ImageRgb8(rgb))
    }

    #[test]
    fn click_uses_source_pixels_after_letterboxing_and_scaling() {
        let frame=fixture();
        let r=frame.layout(&RECT{left:0,top:0,right:100,bottom:100}).unwrap();
        assert_eq!((r.left,r.top,r.right,r.bottom),(0,25,100,75));
        assert_eq!(frame.source_point(&r,1,24),None);
        assert_eq!(frame.source_point(&r,99,75),None);
        assert_eq!(frame.source_point(&r,100,74),None);
        assert_eq!(frame.source_point(&r,49,25),Some((0,0)));
        assert_eq!(frame.source_point(&r,50,74),Some((1,0)));
        assert_eq!(frame.color(0,0),"#12ab56");
        assert_eq!(frame.color(1,0),"#de018f");
    }

    #[test]
    fn native_window_returns_clicked_color_and_escape_cancels() {
        unsafe {
            let h=create(null_mut(),fixture(),false).unwrap();
            let mut r=RECT::default();GetClientRect(h,&mut r);
            let x=r.right*3/4;
            let y=r.bottom/2;
            SendMessageW(h,WM_LBUTTONDOWN,0,((y as isize)<<16)|x as isize);
            assert_eq!(take_color().as_deref(),Some("#de018f"));
            let h=create(null_mut(),fixture(),false).unwrap();
            SendMessageW(h,WM_KEYDOWN,37,0);
            SendMessageW(h,WM_KEYDOWN,13,0);
            assert_eq!(take_color().as_deref(),Some("#12ab56"));
            let h=create(null_mut(),fixture(),false).unwrap();
            SendMessageW(h,WM_KEYDOWN,27,0);
            assert_eq!(take_color(),None);
        }
    }
}
