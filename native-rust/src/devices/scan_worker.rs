use super::{available, ocr, read_image, show_scan, SCAN};
use crate::Ui;
use std::{cell::RefCell, path::PathBuf, sync::{Arc,mpsc::{self, Receiver, TryRecvError}}, thread};
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const DONE: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 66;

pub(super) const CLIPBOARD:&str="Current clipboard image";
enum Source { Bytes(Arc<Vec<u8>>), File(PathBuf) }

struct Request {
    page: &'static str,
    identity: String,
    source: Source,
}

struct Running {
    page: &'static str,
    identity: String,
    receiver: Receiver<Result<ocr::Scan, String>>,
    visible: bool,
}

#[derive(Default)]
struct Jobs { running: Option<Running>, next: Option<Request>, clipboard:Option<Arc<Vec<u8>>> }
thread_local! { static JOBS: RefCell<Jobs> = RefCell::new(Jobs::default()); }

fn start(hwnd: usize, request: Request) -> Result<Running, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new().name("Buddy OCR".into()).spawn(move || {
        let result = match request.source {
            Source::Bytes(data) => ocr::scan(&data),
            Source::File(path) => read_image(&path).and_then(|bytes|ocr::scan(&bytes)),
        };
        if sender.send(result).is_ok() {
            unsafe { PostMessageW(hwnd as _, DONE, 0, 0); }
        }
    }).map_err(|e| format!("Cannot start local OCR ({e})."))?;
    Ok(Running { page: request.page, identity: request.identity, receiver, visible: true })
}

fn queue(ui: &Ui, identity: String, source: Source) -> Result<String, String> {
    if !available(ui) { return Err("Open Buddy and resume services for image recognition.".into()); }
    if ui.page == "ocr" {
        SCAN.with(|cell| { cell.borrow_mut().take(); });
        ui.set_rows(&[]);
        ui.set_detail("");
    }
    let request = Request { page: ui.page, identity, source };
    JOBS.with(|cell| {
        let mut jobs = cell.borrow_mut();
        if request.identity!=CLIPBOARD{jobs.clipboard=None;}
        if let Some(running) = &mut jobs.running {
            running.visible = false;
            jobs.next = Some(request);
        } else {
            jobs.running = Some(start(ui.hwnd.0 as usize, request)?);
        }
        Ok("Reading image text locally…".into())
    })
}

pub fn file(ui:&Ui, path:PathBuf)->Result<String,String> {
    queue(ui, ui.input(), Source::File(path))
}

pub fn bytes(ui:&Ui, identity:String, data:Vec<u8>)->Result<String,String> {
    queue(ui, identity, Source::Bytes(Arc::new(data)))
}

// Rescan the image the user actually saw, even if another app changes the clipboard.
// Both the worker and the OCR page refer to one buffer. Leaving the page releases it.
pub fn clipboard_image(ui:&Ui,data:Option<Vec<u8>>)->Result<String,String>{
    let image=JOBS.with(|slot|{
        let mut jobs=slot.borrow_mut();
        if let Some(data)=data{jobs.clipboard=Some(Arc::new(data));}
        jobs.clipboard.clone()
    }).ok_or("Choose a clipboard image first.")?;
    queue(ui, CLIPBOARD.into(), Source::Bytes(image))
}

// Discard results belonging to a previous visit even if the same page is reopened.
pub fn cancel() {
    JOBS.with(|cell| {
        let mut jobs = cell.borrow_mut();
        if let Some(running) = &mut jobs.running { running.visible = false; }
        jobs.next = None;
        jobs.clipboard = None;
    });
}

pub fn finish(ui: &mut Ui) {
    ocr::release(false);
    let complete = JOBS.with(|cell| {
        let mut jobs = cell.borrow_mut();
        let current = jobs.running.as_ref()?;
        let result = match current.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("The image recognition worker stopped.".into()),
        };
        let running = jobs.running.take()?;
        if let Some(next) = jobs.next.take() {
            match start(ui.hwnd.0 as usize, next) {
                Ok(new) => jobs.running = Some(new),
                Err(e) => ui.error(&e),
            }
        }
        Some((running,result))
    });
    let Some((running,result)) = complete else { return };
    if !running.visible || !available(ui) || ui.page != running.page { return; }
    if ui.page == "ocr" {
        if ui.input() != running.identity { return; }
    } else if crate::workspace::selected_image_id(ui).as_deref() != Some(&running.identity) { return; }
    match result {
        Err(e) => ui.error(&e),
        Ok(scan) => {
            let count = scan.lines.iter().map(Vec::len).sum::<usize>();
            if ui.page == "ocr" {
                show_scan(ui, &scan);
                SCAN.with(|cell| *cell.borrow_mut() = Some(scan));
            } else {
                ui.set_detail(&scan.text);
            }
            ui.status(&format!("{count} words recognized locally. Select text in the details field to copy."));ui.layout();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_POPUP};
    use std::ptr::{null, null_mut};

    fn completed(text: &str) {
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(ocr::Scan { text: text.into(), lines: vec![] })).unwrap();
        JOBS.with(|slot| {
            slot.borrow_mut().running = Some(Running {
                page: "ocr", identity: "image-a".into(), receiver, visible: true,
            })
        });
    }

    #[test]
    fn only_delivers_to_the_same_ocr_visit_and_input() {
        let directory = std::env::temp_dir().join(format!("BuddyRust-worker-{}", std::process::id()));
        let handle = unsafe {
            CreateWindowExW(0, crate::ui::wide("STATIC").as_ptr(), null(), WS_POPUP, 0, 0,
                800, 600, null_mut(), null_mut(), null_mut(), null())
        };
        assert!(!handle.is_null());
        let mut ui = Ui::new(HWND(handle), directory, true).unwrap();
        ui.navigate("ocr");
        ui.set_input("image-a");
        completed("wrong visit");
        ui.navigate("home");
        ui.navigate("ocr");
        ui.set_input("image-a");
        finish(&mut ui);
        assert!(ui.detail().is_empty());
        completed("wrong image");
        ui.set_input("image-b");
        finish(&mut ui);
        assert!(ui.detail().is_empty());
        ui.set_input("image-a");
        completed("matching result");
        finish(&mut ui);
        assert_eq!(ui.detail(), "matching result");
        drop(ui);
        unsafe { DestroyWindow(handle); }
    }
    #[test]
    #[ignore="uses the private Windows clipboard and pinned OCR models beside the executable"]
    fn clipboard_scan_rescans_the_same_image_and_saved_scans_show_results(){
        use windows_sys::Win32::System::{DataExchange::{OpenClipboard,CloseClipboard,EmptyClipboard,GetClipboardData,RegisterClipboardFormatW,SetClipboardData},Memory::{GlobalAlloc,GlobalLock,GlobalUnlock,GMEM_MOVEABLE}};
        use windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
        let image=include_bytes!("../../tests/fixtures/buddy-ocr.png");
        crate::workspace::with_private_test_desktop(||unsafe{
            let h=CreateWindowExW(0,crate::ui::wide("STATIC").as_ptr(),null(),WS_POPUP,0,0,840,300,null_mut(),null_mut(),null_mut(),null());
            assert!(!h.is_null());
            let mut ui=Ui::new(HWND(h),std::env::temp_dir().join(format!("Buddy-OCR-rescan-{}",std::process::id())),true).unwrap();ui.expanded=true;ui.navigate("ocr");
            let await_text=|ui:&mut Ui|{
                let start=std::time::Instant::now();
                while ui.detail().is_empty()&&start.elapsed()<std::time::Duration::from_secs(120){finish(ui);std::thread::sleep(std::time::Duration::from_millis(30));}
                assert_eq!(ui.detail(),"Buddy OCR 12345\r\nSelect individual characters");assert_ne!(IsWindowEnabled(GetDlgItem(h,3304)),0);
            };
            assert_ne!(OpenClipboard(h),0);assert_ne!(EmptyClipboard(),0);
            let mem=GlobalAlloc(GMEM_MOVEABLE,image.len());let start=GlobalLock(mem);assert!(!start.is_null());
            std::ptr::copy_nonoverlapping(image.as_ptr(),start.cast(),image.len());GlobalUnlock(mem);
            assert!(!SetClipboardData(RegisterClipboardFormatW(crate::ui::wide("PNG").as_ptr()),mem).is_null());CloseClipboard();
            super::super::action(&mut ui,"ocr","clipboard").unwrap();await_text(&mut ui);
            crate::workspace::copy_editor_text(&ui,"Clipboard replaced after scan").unwrap();
            super::super::action(&mut ui,"ocr","scan").unwrap();await_text(&mut ui);
            ui.navigate("markup");super::super::scan_saved(&mut ui,"saved-example",image.to_vec()).unwrap();assert_eq!(ui.page,"ocr");await_text(&mut ui);
            ui.select_row(1);super::super::action(&mut ui,"ocr","word").unwrap();super::super::action(&mut ui,"ocr","copy").unwrap();
            assert_ne!(OpenClipboard(h),0);
            let handle=GetClipboardData(windows_sys::Win32::System::Ole::CF_UNICODETEXT as u32);assert!(!handle.is_null());
            let ptr=GlobalLock(handle).cast::<u16>();assert!(!ptr.is_null());
            assert_eq!(std::slice::from_raw_parts(ptr,4), &[79u16,67,82,0],"Copy text must honor the selected OCR word");GlobalUnlock(handle);CloseClipboard();
            super::super::action(&mut ui,"ocr","clear").unwrap();assert!(ui.detail().is_empty());drop(ui);DestroyWindow(h);
        });
    }

}
