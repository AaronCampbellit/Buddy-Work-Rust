use super::{available, ocr, read_image, show_scan, SCAN};
use crate::Ui;
use std::{cell::RefCell, path::PathBuf, sync::mpsc::{self, Receiver, TryRecvError}, thread};
use windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const DONE: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WM_APP + 66;

enum Source { Bytes(Vec<u8>), File(PathBuf) }

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
struct Jobs { running: Option<Running>, next: Option<Request> }
thread_local! { static JOBS: RefCell<Jobs> = RefCell::new(Jobs::default()); }

fn start(hwnd: usize, request: Request) -> Result<Running, String> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new().name("Buddy OCR".into()).spawn(move || {
        let bytes = match request.source {
            Source::Bytes(data) => Ok(data),
            Source::File(path) => read_image(&path),
        };
        let result = bytes.and_then(|data| ocr::scan(&data));
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
    queue(ui, identity, Source::Bytes(data))
}

// Discard results belonging to a previous visit even if the same page is reopened.
pub fn cancel() {
    JOBS.with(|cell| {
        let mut jobs = cell.borrow_mut();
        if let Some(running) = &mut jobs.running { running.visible = false; }
        jobs.next = None;
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
                Err(e) => ui.status(&e),
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
        Err(e) => ui.status(&e),
        Ok(scan) => {
            let count = scan.lines.iter().map(Vec::len).sum::<usize>();
            if ui.page == "ocr" {
                show_scan(ui, &scan);
                SCAN.with(|cell| *cell.borrow_mut() = Some(scan));
            } else {
                ui.set_detail(&scan.text);
            }
            ui.status(&format!("{count} words recognized locally. Select text in the details field to copy."));
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
}
