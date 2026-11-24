//! During an island transition, present the silhouette, fading workspace and
//! header in one bitmap. The native controls continue to handle input.
use super::*;

thread_local! {
    static SURFACE: RefCell<Option<Surface>> = const { RefCell::new(None) };
    static LAYERED: Cell<HWND> = const { Cell::new(null_mut()) };
    static COMPOSITED: Cell<u32> = const { Cell::new(0) };
}

pub(super) struct Surface {
    bitmap: WorkspaceBitmap,
    pixels: *mut u32,
}

impl Surface {
    unsafe fn new(width: i32, height: i32) -> Option<Self> {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return None;
        }
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as _,
            biWidth: width,
            biHeight: -height, // Top-down scan lines, in client coordinates.
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..Default::default()
        };
        let mut data = null_mut();
        let image = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut data, null_mut(), 0);
        let original = if image.is_null() || data.is_null() { null_mut() } else { SelectObject(dc, image) };
        if original.is_null() {
            if !image.is_null() { DeleteObject(image); }
            DeleteDC(dc);
            return None;
        }
        let bitmap = WorkspaceBitmap { dc, image, original, width, height, page: "" };
        Some(Self { bitmap, pixels: data.cast() })
    }

    pub(super) unsafe fn snapshot(hwnd:HWND,width:i32,height:i32)->Option<Vec<u32>>{
        let mut surface=Self::new(width,height)?;
        surface.draw(hwnd,&island::frame());
        Some(std::slice::from_raw_parts(surface.pixels,(width*height) as usize).to_vec())
    }
    pub(super) unsafe fn from_pixels(width:i32,height:i32,data:&[u32])->Option<Self>{
        if width.checked_mul(height)? as usize!=data.len(){return None}
        let surface=Self::new(width,height)?;
        std::slice::from_raw_parts_mut(surface.pixels,data.len()).copy_from_slice(data);
        Some(surface)
    }
    #[cfg(test)]
    pub(super) unsafe fn fixture_colors(&self)->usize{
        let len=(self.bitmap.width*self.bitmap.height) as usize;
        std::slice::from_raw_parts(self.pixels,len).iter().filter(|p|matches!(**p&0xffffff,0xfa6751|0x10aba8|0xefbc44)).count()
    }
    pub(super) unsafe fn blend(&self,dc:HDC,frame:&island::Frame,alpha:u8){
        self.bitmap.blend(dc,frame.visible_workspace(island::scale_and_limit().0),alpha,frame.workspace_offset());
    }
    unsafe fn draw(&mut self, hwnd: HWND, frame: &island::Frame) {
        let b = &self.bitmap;
        let pixel_count = b.width as usize * b.height as usize;
        GdiFlush();
        std::slice::from_raw_parts_mut(self.pixels, pixel_count).fill(0);
        if frame.animating || workspace_masked() {
            read_ui(|ui| ui.paint_surface(b.dc));
            composite_workspace(hwnd, b.dc, frame);
            print_children(hwnd, b.dc, Some(false));
        } else {
            // The live controls already have their endpoint bounds and content.
            // A slow first navigation may have presented only the compact frame.
            read_ui(|ui| ui.paint(b.dc));
            print_children(hwnd, b.dc, None);
        }

        // GDI paints RGB. Supply alpha from the same geometry used to hit-test
        // the island, clearing anything outside the new silhouette on reversal.
        GdiFlush();
        let pixels = std::slice::from_raw_parts_mut(self.pixels, pixel_count);
        let body = frame.body;
        let top = body.top.clamp(0, b.height);
        let bottom = body.bottom.clamp(top, b.height);
        let stride = b.width as usize;
        pixels[..top as usize * stride].fill(0);
        pixels[bottom as usize * stride..].fill(0);
        for y in top..bottom {
            let mut left = body.left.clamp(0, b.width);
            let mut right = body.right.clamp(left, b.width);
            while left < right && !island::contains(frame, left, y) { left += 1; }
            while right > left && !island::contains(frame, right - 1, y) { right -= 1; }
            let row_start = y as usize * stride;
            let row = &mut pixels[row_start..row_start + stride];
            row[..left as usize].fill(0);
            for pixel in &mut row[left as usize..right as usize] { *pixel |= 0xff000000; }
            row[right as usize..].fill(0);
        }
    }
}

unsafe fn take_client_surface(hwnd: HWND) -> Option<Surface> {
    let mut client=RECT::default();
    if GetClientRect(hwnd,&mut client)==0 || client.right<1 || client.bottom<1 { return None; }
    SURFACE.with(|s|s.borrow_mut().take())
        .filter(|s|s.bitmap.width==client.right && s.bitmap.height==client.bottom)
        .or_else(||Surface::new(client.right,client.bottom))
}

pub(super) fn active(hwnd: HWND) -> bool {
    LAYERED.get() == hwnd
}

#[cfg(test)]
pub(super) fn bitmap_allocated() -> bool {
    SURFACE.with(|surface| surface.borrow().is_some())
}

// Removing WS_EX_LAYERED allocates a new redirection surface. Cover that
// transition with the destination page at its real, settled size.
#[link(name="dwmapi")]
extern "system" { fn DwmFlush()->i32; }

/// A layered-window update and its new hit region have both been submitted.
/// Let DWM present them before the timer can replace this displayed shape.
pub(super) unsafe fn synchronize_frame(hwnd: HWND) {
    if IsWindowVisible(hwnd) != 0 {
        // Composition may be unavailable on a remote or private desktop. The
        // ordinary island timer and region remain usable when DwmFlush fails.
        DwmFlush();
    }
}

pub(super) struct Handoff(HWND);
impl Drop for Handoff {
    fn drop(&mut self) { unsafe {
        GdiFlush();
        DwmFlush();
        DestroyWindow(self.0);
    }}
}
unsafe fn settled_surface(hwnd: HWND, frame: &island::Frame) -> Option<Surface> {
    let mut surface=take_client_surface(hwnd)?;
    surface.draw(hwnd,frame);
    Some(surface)
}

pub(super) unsafe fn hold_settled_frame(hwnd: HWND, frame: &island::Frame) -> Option<Handoff> {
    if !active(hwnd) || IsWindowVisible(hwnd)==0 { return None; }
    let surface=settled_surface(hwnd,frame)?;
    {
        let mut window=RECT::default();
        if GetWindowRect(hwnd,&mut window)==0 { return None; }
        let destination=POINT{x:window.left,y:window.top};
        let size=SIZE{cx:surface.bitmap.width,cy:surface.bitmap.height};
        let overlay=CreateWindowExW(WS_EX_LAYERED|WS_EX_TRANSPARENT|WS_EX_NOACTIVATE|WS_EX_TOOLWINDOW,
            windows_sys::w!("STATIC"),null(),WS_POPUP,window.left,window.top,size.cx,size.cy,
            hwnd,null_mut(),null_mut(),null());
        if overlay.is_null() { return None; }
        let alpha=BLENDFUNCTION{BlendOp:AC_SRC_OVER as u8,BlendFlags:0,SourceConstantAlpha:255,AlphaFormat:AC_SRC_ALPHA as u8};
        if UpdateLayeredWindow(overlay,null_mut(),&destination,&size,surface.bitmap.dc,&POINT::default(),0,&alpha,ULW_ALPHA)==0{
            DestroyWindow(overlay);return None
        }
        ShowWindow(overlay,SW_SHOWNOACTIVATE);
        // Commit the overlay before Windows replaces the live window surface.
        DwmFlush();
        Some(Handoff(overlay))
    }
}

pub(super) unsafe fn end(hwnd: HWND) {
    if !active(hwnd) { return; }
    LAYERED.set(null_mut());
    let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    SetWindowLongW(hwnd, GWL_EXSTYLE, ((style & !WS_EX_LAYERED) | COMPOSITED.get()) as _);
    SURFACE.with(|s| s.borrow_mut().take());
}

pub(super) unsafe fn present(hwnd: HWND, frame: &island::Frame) -> bool {
    let Some(mut surface) = take_client_surface(hwnd) else { return false; };
    surface.draw(hwnd, frame);
    if !active(hwnd) {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if style & WS_EX_LAYERED != 0 { return false; }
        let layered = (style & !WS_EX_COMPOSITED) | WS_EX_LAYERED;
        SetWindowLongW(hwnd, GWL_EXSTYLE, layered as _);
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_LAYERED == 0 { return false; }
        COMPOSITED.set(style & WS_EX_COMPOSITED);
        LAYERED.set(hwnd);
    }
    let alpha = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let size = SIZE { cx: surface.bitmap.width, cy: surface.bitmap.height };
    let origin = POINT::default();
    let ok = UpdateLayeredWindow(hwnd, null_mut(), null(), &size, surface.bitmap.dc,
                                 &origin, 0, &alpha, ULW_ALPHA) != 0;
    SURFACE.with(|s| *s.borrow_mut() = Some(surface));
    if !ok { end(hwnd); }
    ok
}


#[cfg(test)]
pub(super) unsafe fn assert_settled_note_covers_compact_buffer(hwnd: HWND) {
    let frame=island::frame();
    assert!(frame.expanded && !frame.animating);
    assert!(!bitmap_allocated());
    // A previous monitor or compact transition can supply a smaller DIB.
    SURFACE.with(|s|*s.borrow_mut()=Surface::new(346,40));
    let surface=settled_surface(hwnd,&frame).unwrap();
    let mut client=RECT::default();GetClientRect(hwnd,&mut client);
    assert_eq!((surface.bitmap.width,surface.bitmap.height),(client.right,client.bottom));
    let pixels=std::slice::from_raw_parts(surface.pixels,(client.right*client.bottom) as usize);
    let at=|x:i32,y:i32|pixels[(y*client.right+x) as usize];
    for x in [frame.body.left+9,frame.body.right-9] {
        assert_eq!(at(x,42)>>24,255,"the settled cover reaches both edges");
    }
    for id in [170,173] {
        let mut tab=RECT::default();GetWindowRect(GetDlgItem(hwnd,id),&mut tab);
        let mut pos=POINT{x:tab.left,y:tab.top};ScreenToClient(hwnd,&mut pos);
        let mut colors=std::collections::HashSet::new();
        for y in pos.y+4..pos.y+tab.bottom-tab.top-4 {
            for x in pos.x+12..pos.x+tab.right-tab.left-12 {
                let pixel=at(x,y);assert_eq!(pixel>>24,255);colors.insert(pixel);
            }
        }
        assert!(colors.len()>8,"the real tab glyph and caption must appear in the settled cover");
    }
}
