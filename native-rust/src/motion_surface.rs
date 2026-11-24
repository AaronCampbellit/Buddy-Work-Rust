//! During an island transition, present the silhouette, fading workspace and
//! header in one bitmap. The native controls continue to handle input.
use super::*;

thread_local! {
    static SURFACE: RefCell<Option<Surface>> = const { RefCell::new(None) };
    static LAYERED: Cell<HWND> = const { Cell::new(null_mut()) };
    static COMPOSITED: Cell<u32> = const { Cell::new(0) };
}

struct Surface {
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

    unsafe fn draw(&mut self, hwnd: HWND, frame: &island::Frame) {
        let b = &self.bitmap;
        let pixel_count = b.width as usize * b.height as usize;
        GdiFlush();
        std::slice::from_raw_parts_mut(self.pixels, pixel_count).fill(0);
        with_ui(|ui| ui.paint_surface(b.dc));
        composite_workspace(hwnd, b.dc, frame);
        print_children(hwnd, b.dc, Some(false));

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

pub(super) fn active(hwnd: HWND) -> bool {
    LAYERED.get() == hwnd
}

#[cfg(test)]
pub(super) fn bitmap_allocated() -> bool {
    SURFACE.with(|surface| surface.borrow().is_some())
}

pub(super) unsafe fn end(hwnd: HWND) {
    if !active(hwnd) { return; }
    LAYERED.set(null_mut());
    let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    SetWindowLongW(hwnd, GWL_EXSTYLE, ((style & !WS_EX_LAYERED) | COMPOSITED.get()) as _);
    SURFACE.with(|s| s.borrow_mut().take());
}

pub(super) unsafe fn present(hwnd: HWND, frame: &island::Frame) -> bool {
    let mut client = RECT::default();
    GetClientRect(hwnd, &mut client);
    if client.right < 1 || client.bottom < 1 { return false; }

    let mut buffer = SURFACE.with(|s| s.borrow_mut().take())
        .filter(|s| s.bitmap.width == client.right && s.bitmap.height == client.bottom)
        .or_else(|| Surface::new(client.right, client.bottom));
    let Some(surface) = buffer.as_mut() else { return false; };
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
    let size = SIZE { cx: client.right, cy: client.bottom };
    let origin = POINT::default();
    let ok = UpdateLayeredWindow(hwnd, null_mut(), null(), &size, surface.bitmap.dc,
                                 &origin, 0, &alpha, ULW_ALPHA) != 0;
    SURFACE.with(|s| *s.borrow_mut() = buffer);
    if !ok { end(hwnd); }
    ok
}
