//! Prepare one read-only image of the pinned note using Buddy's own native
//! controls. The worker owns its hidden HWND, COM apartment, and GDI objects.
use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver}, Arc};

struct Job {
    canceled:Arc<AtomicBool>,
    result:Receiver<Option<Vec<u32>>>,
}
impl Drop for Job{
    fn drop(&mut self){self.canceled.store(true,Ordering::Release);}
}

#[derive(Default)]
struct Preview{
    key:Option<u64>,
    job:Option<Job>,
    pixels:Option<Vec<u32>>,
    active:Option<motion_surface::Surface>,
}
thread_local!{static PREVIEW:RefCell<Preview>=RefCell::new(Preview::default());}
pub(super) fn clear(){PREVIEW.with(|s|{
    let mut state=s.borrow_mut();
    if let Some(pixels)=state.pixels.as_mut(){pixels.fill(0);}
    *state=Preview::default();
});}
pub(super) fn active()->bool{PREVIEW.with(|s|s.borrow().active.is_some())}

fn dimensions(ui:&Ui)->Option<(i32,i32)>{
    let mut r=RECT::default();
    if unsafe{GetClientRect(ui.hwnd.0,&mut r)}==0 || r.right<=0 || r.bottom<=0{return None}
    // A single bitmap stays below 12 MiB even on scaled multi-monitor setups.
    (r.right.checked_mul(r.bottom)?<=3*1024*1024).then_some((r.right,r.bottom))
}

fn fingerprint(ui:&Ui,id:&str,width:i32,height:i32)->Option<u64>{
    let notes=ui.records.get("notes")?;
    let frame=island::frame();let body=frame.expanded_body;
    let mut key=DefaultHasher::new();
    (ui.hwnd.0 as usize,&ui.profile,id,width,height,body.left,body.right,body.bottom,ui.zoom,ui.appearance,shell::dpi(ui.hwnd.0)).hash(&mut key);
    ui.records.get("settings").hash(&mut key);
    let mut found=false;
    for row in notes{
        if row.first().is_some_and(|s|s==id){row.hash(&mut key);found=true;}
        else if row.len()>=7 {
            for i in [0,1,3,4,5,6]{row[i].hash(&mut key);}
            for c in row[2].chars().take(160){c.hash(&mut key);}
        }
    }
    ui.records.get(&inline_images::offset_key(id)).hash(&mut key);
    ui.records.get(&inline_images::format_key(id)).hash(&mut key);
    found.then(||key.finish())
}

fn preview_records(ui:&Ui,id:&str)->storage::Records{
    let mut records=storage::Records::new();
    for name in ["settings".to_owned(),inline_images::offset_key(id),inline_images::format_key(id)]{
        if let Some(rows)=ui.records.get(&name){records.insert(name,rows.clone());}
    }
    let rows=ui.records["notes"].iter().filter(|r|r.len()>=7).map(|row|{
        if row[0]==id{return row.clone()}
        // Other notes only contribute their title and sidebar excerpt.
        vec![row[0].clone(),row[1].clone(),row[2].chars().take(160).collect(),
            row[3].clone(),row[4].clone(),row[5].clone(),row[6].clone()]
    }).collect();
    records.insert("notes".into(),rows);records
}

impl Preview{
    fn poll(&mut self){
        if let Some(job)=&self.job{
            match job.result.try_recv(){
                Ok(pixels)=>{self.pixels=pixels;self.job=None;}
                Err(mpsc::TryRecvError::Disconnected)=>self.job=None,
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
    }
}

// The private host handles only owner painting. It never opens a tray icon,
// registers hotkeys, listens to the clipboard, persists records or starts devices.
unsafe extern "system" fn preview_proc(hwnd:HWND,msg:u32,wp:usize,lp:isize)->isize{
    match msg{
        WM_DRAWITEM if read_ui(|u|u.draw_item(lp))==Some(true)=>1,
        WM_CTLCOLORSTATIC|WM_CTLCOLORBTN|WM_CTLCOLORLISTBOX|WM_CTLCOLOREDIT=>{
            let brush=ui::control_brush(hwnd,msg,wp as _,lp as _);
            if brush!=0{brush}else{DefWindowProcW(hwnd,msg,wp,lp)}
        }
        WM_ERASEBKGND=>1,
        _=>DefWindowProcW(hwnd,msg,wp,lp),
    }
}

struct Render{
    layout:island::Layout,
    profile:PathBuf,
    records:storage::Records,
    id:String,
    width:i32,
    height:i32,
    zoom:i32,
}
impl Render {
    unsafe fn run(self,canceled:&AtomicBool)->Option<Vec<u32>>{
        if canceled.load(Ordering::Acquire){return None}
        let com=CoInitializeEx(null(),COINIT_APARTMENTTHREADED as u32);
        if com<0{return None}
        let name=wide("BuddyPinnedNotePreview");
        let instance=GetModuleHandleW(null());
        let class=WNDCLASSW{lpfnWndProc:Some(preview_proc),hInstance:instance,lpszClassName:name.as_ptr(),..Default::default()};
        RegisterClassW(&class);
        // A hidden window on the same monitor uses the island's real DPI.
        let host=self.layout.host;
        let hwnd=CreateWindowExW(WS_EX_TOOLWINDOW|WS_EX_NOACTIVATE,name.as_ptr(),null(),WS_POPUP|WS_CLIPCHILDREN,
            host.left,host.top,self.width,self.height,null_mut(),null_mut(),instance,null());
        let image=if hwnd.is_null(){None}else{
            let image=(||{
                island::configure(self.layout,true);island::request(true,true);
                let mut preview=Ui::from_records(TypedWindow(hwnd),self.profile,true,self.records,None).ok()?;
                // Skip the normal workspace-leave hooks for this passive host.
                preview.page="notes";preview.expanded=true;preview.zoom=self.zoom;
                preview.active_note=Some(self.id);
                preview.apply_appearance();
                preview.navigate("notes");
                WINDOW.with(|slot|*slot.borrow_mut()=Some(preview));
                if canceled.load(Ordering::Acquire) || read_ui(|u|u.images.contents().is_ok())!=Some(true){return None}
                motion_surface::Surface::snapshot(hwnd,self.width,self.height)
            })();
            DestroyWindow(hwnd);
            WINDOW.with(|slot|{slot.borrow_mut().take();});
            image
        };
        CoUninitialize();image
    }
}

pub(super) fn warm(ui:&Ui,id:&str){
    let Some((width,height))=dimensions(ui)else{clear();return};
    let Some(key)=fingerprint(ui,id,width,height)else{clear();return};
    PREVIEW.with(|slot|{
        let mut state=slot.borrow_mut();
        state.poll();
        if state.key==Some(key){return}
        if let Some(job)=&state.job{
            // Coalesce changes while an obsolete worker releases its HWNDs.
            job.canceled.store(true,Ordering::Release);state.key=None;return
        }
        *state=Preview::default();
        let Some(layout)=island::current_layout()else{return};
        let render=Render{layout,profile:ui.profile.clone(),records:preview_records(ui,id),id:id.to_owned(),width,height,zoom:ui.zoom};
        let (send,result)=mpsc::sync_channel(1);let canceled=Arc::new(AtomicBool::new(false));
        let stop=canceled.clone();
        if std::thread::Builder::new().name("Buddy pinned note preview".into()).spawn(move||{
            let pixels=unsafe{render.run(&stop)};
            if !stop.load(Ordering::Acquire){let _=send.send(pixels);}
        }).is_ok(){state.key=Some(key);state.job=Some(Job{canceled,result});}
    });
}

pub(super) fn validate(ui:&Ui){
    if !active(){return}
    let id=core::setting(ui,"noteWidget","");
    let key=dimensions(ui).and_then(|(w,h)|fingerprint(ui,id,w,h));
    if key.is_none() || !PREVIEW.with(|s|s.borrow().key==key){clear();}
}

pub(super) fn activate(ui:&Ui,id:&str)->bool{
    let Some((width,height))=dimensions(ui)else{clear();return false};
    let expected=fingerprint(ui,id,width,height);
    PREVIEW.with(|slot|{
        let mut state=slot.borrow_mut();state.poll();
        if expected.is_none() || expected!=state.key{*state=Preview::default();return false}
        if let Some(pixels)=state.pixels.take(){
            state.active=unsafe{motion_surface::Surface::from_pixels(width,height,&pixels)};
        }
        state.active.is_some()
    })
}
pub(super) unsafe fn paint(dc:HDC,frame:&island::Frame,alpha:u8)->bool{
    PREVIEW.with(|s|{
        let state=s.borrow();
        if let Some(image)=&state.active{image.blend(dc,frame,alpha);true}else{false}
    })
}


#[cfg(test)]
pub(crate) fn fixture_colors()->usize{
    PREVIEW.with(|s|s.borrow().active.as_ref().map_or(0,|image|unsafe{image.fixture_colors()}))
}
