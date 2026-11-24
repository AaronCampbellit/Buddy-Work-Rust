use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, atomic::AtomicBool};
use std::{fs, thread, time::{Duration, Instant}};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, Management::{UserNotificationListener as Listener, UserNotificationListenerAccessStatus as Access}};
use windows::Media::Control::{GlobalSystemMediaTransportControlsSessionManager as Media, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Playback};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
use windows::core::HSTRING;
use crate::Ui;
mod cli;
mod update;

type Outcome = Result<View, String>;
#[derive(Default, Clone)]
struct View { rows: Vec<String>, detail: String, status: String, url: Option<String>, media_playing: bool, position:i64, duration:i64, privacy: Option<String>, notifications: Option<Vec<(String,String)>> }
impl View {
    fn status(value: &str) -> Self { Self { status: value.to_owned(), ..Self::default() } }
}
#[derive(Default)]
struct Activity {
    cache: BTreeMap<String, View>,
    busy: BTreeSet<&'static str>,
    done: VecDeque<(&'static str, Outcome)>,
    notification_ids: Option<BTreeSet<String>>,
    cleared_notifications:BTreeSet<String>,
    diagnostic_samples: Vec<String>,
    diagnostic_folder: Option<PathBuf>,
    rewrite_cancel: Option<Arc<AtomicBool>>,
    rewrite_source: Option<String>,
    ticks: u64,
    compact: Option<(String,Instant)>,
    privacy: String,
}
fn state() -> MutexGuard<'static, Activity> {
    static DATA: OnceLock<Mutex<Activity>> = OnceLock::new();
    DATA.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}
fn job(key: &'static str, work: impl FnOnce() -> Outcome + Send + 'static) -> Result<String,String> {
    if !state().busy.insert(key) { return Ok("Already working.".into()); }
    thread::spawn(move || {
        let com = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
        let result = work();
        if com { unsafe { RoUninitialize() }; }
        state().done.push_back((key,result));
    });
    Ok("Working…".into())
}
fn werr(e: windows::core::Error) -> String {
    format!("Windows could not complete the request (0x{:08X}).", e.code().0 as u32)
}
fn media(command: &str, seconds: i64) -> windows::core::Result<View> {
    let manager = Media::RequestAsync()?.join()?;
    let sessions = manager.GetSessions()?;
    if sessions.Size()? == 0 { return Ok(View::status("No Windows media session is active.")); }
    let session = manager.GetCurrentSession()?;
    let request = match command {
        "play" => Some(session.TryPlayAsync()?), "pause" => Some(session.TryPauseAsync()?),
        "previous" => Some(session.TrySkipPreviousAsync()?), "next" => Some(session.TrySkipNextAsync()?),
        "seek" => Some(session.TryChangePlaybackPositionAsync(seconds * 10_000_000)?),
        _ => None,
    };
    if let Some(request) = request { if !request.join()? { return Ok(View::status("The media app declined this control.")); } }
    let item = session.TryGetMediaPropertiesAsync()?.join()?;
    let timeline = session.GetTimelineProperties()?;
    let playing = session.GetPlaybackInfo()?.PlaybackStatus()? == Playback::Playing;
    let rows = vec![item.Title()?.to_string(), item.Artist()?.to_string(), item.AlbumTitle()?.to_string(),
        format!("{} · {} / {} seconds", if playing {"Playing"} else {"Paused"}, timeline.Position()?.Duration / 10_000_000, timeline.EndTime()?.Duration / 10_000_000)];
    Ok(View {rows, media_playing: playing, position:timeline.Position()?.Duration/10_000_000, duration:timeline.EndTime()?.Duration/10_000_000, status: session.SourceAppUserModelId()?.to_string(), ..Default::default()})
}
fn notifications(privacy: &str) -> windows::core::Result<View> {
    let listener = Listener::Current()?;
    let access = listener.GetAccessStatus()?;
    if access != Access::Allowed { return Ok(View::status(if access == Access::Denied {"Windows notification access is denied."} else {"Choose Allow access to request Windows notification access."})); }
    let items = listener.GetNotificationsAsync(NotificationKinds::Toast)?.join()?;
    let mut entries = Vec::new();
    for i in (0..items.Size()?).rev().take(100) {
        let item = items.GetAt(i)?;
        let app = item.AppInfo()?.DisplayInfo()?.DisplayName()?.to_string();
        let app_id=item.AppInfo()?.AppUserModelId()?.to_string();
        if app_id.starts_with("Microsoft.ScreenSketch_") || app_id.starts_with("Buddy.Work.Rust.") || app.eq_ignore_ascii_case("SnippingTool") {continue;}
        let id = format!("{}-{}-{}", app_id, item.Id()?, item.CreationTime()?.UniversalTime);
        let mut text = app;
        if privacy != "icon" {
            if let Ok(binding) = item.Notification()?.Visual()?.GetBinding(&KnownNotificationBindings::ToastGeneric()?) {
                let lines = binding.GetTextElements()?;
                for j in 0..lines.Size()?.min(if privacy=="full" {3} else {1}) {
                    let line: String = lines.GetAt(j)?.Text()?.to_string().chars().take(260).collect();
                    if !line.is_empty() { text.push_str(" · "); text.push_str(&line); }
                }
            }
        }
        entries.push((id,text));
    }
    Ok(View {rows: entries.iter().map(|e|e.1.clone()).collect(), privacy:Some(privacy.to_owned()), notifications: Some(entries), status: "Windows notification access is allowed.".into(), ..Default::default()})
}
fn setting(ui: &Ui, key: &str, default: &str) -> String {
    crate::core::setting(ui,key,default).to_owned()
}
fn set(ui: &mut Ui, key: &str, value: &str) -> Result<String,String> {
    let previous = ui.records.get("settings").cloned();
    crate::core::set(ui,key,value);
    if let Err(e) = ui.save() {
        match previous {Some(v) => {ui.records.insert("settings".into(), v);}, None => {ui.records.remove("settings");}}
        return Err(e);
    }
    Ok("Saved.".into())
}

pub fn actions(page: &str) -> Vec<(&'static str,&'static str)> {
    match page {
        "media" => vec![("refresh","Refresh"),("play","Play"),("pause","Pause"),("previous","Previous"),("next","Next"),("seek","Seek to seconds")],
        "notifications" => vec![("refresh","Refresh"),("allow","Allow access"),("preview","Toggle previews"),("privacy","Change privacy"),("settings","Windows settings")],
        "rewrite" => vec![("clearer","Make clearer"),("shorter","Shorten"),("professional","Professional tone"),("custom","Custom rewrite"),("cancel","Cancel"),("login","Sign in"),("status","Check sign-in"),("models","List models"),("model","Set model"),("effort","Set effort"),("default","Use saved preset"),("result","Show last result")],
        "diagnostics" => vec![("capture","Capture 15 seconds"),("open","Open reports folder"),("update","Check for update"),("download","Open release")],
        _ => Vec::new()
    }
}
pub fn render(ui: &mut Ui, page: &str) -> String {
    let data = state().cache.get(page).cloned().unwrap_or_default();
    ui.set_rows(&data.rows);
    if !data.detail.is_empty() { ui.set_detail(&data.detail); }
    if page=="rewrite" {ui.set_input(&setting(ui,"rewrite-instruction",""));}
    match page {
        "media" => "Refresh to see the current track. Enter a time in seconds to seek.".into(),
        "notifications" => format!("Notification previews {}. Privacy: {}.",setting(ui,"notificationPreview","on"),setting(ui,"notificationPrivacy","title")),
        "rewrite" => format!("Enter the text below. For a custom rewrite, put the instruction in the first field. Selecting a rewrite sends that text through your signed-in Codex CLI. Model: {}. Effort: {}.",setting(ui,"rewrite-model","default"),setting(ui,"rewrite-effort","default")),
        "diagnostics" => "The local report records process memory, CPU and handles once a second.".into(),
        _ => String::new()
    }
}
pub(crate) fn open(target: &str) -> Result<String,String> {
    use windows::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    use windows::core::w;
    let result = unsafe { ShellExecuteW(None,w!("open"),&HSTRING::from(target),None,None,SW_SHOWNORMAL) };
    if result.0 as isize <= 32 {Err("Windows could not open the target.".into())} else {Ok("Opened.".into())}
}
pub fn action(ui: &mut Ui, page: &str, name: &str) -> Result<String,String> {
    if name == "select" { return Ok(String::new()); }
    match page {
        "media" => {
            let cmd=name.to_owned();
            let value = if name=="seek" { ui.input().trim().parse::<i64>().ok().filter(|n|(0..=604800).contains(n)).ok_or("Enter a time in seconds between 0 and 604800.")? } else {0};
            job("media",move || media(&cmd,value).map_err(werr))
        }
        "notifications" => match name {
            "settings" => open("ms-settings:notifications"),
            "preview" | "privacy" => {
                if name=="preview" {
                    let new = if setting(ui,"notificationPreview","on")=="on" {"off"} else {"on"};
                    set(ui,"notificationPreview",new)?;
                } else {
                    let mode = match setting(ui,"notificationPrivacy","title").as_str() {"title"=>"icon","icon"=>"full",_=>"title"};
                    set(ui,"notificationPrivacy",mode)?;
                }
                { let mut s=state(); s.notification_ids=None; s.cache.remove("notifications"); }
                ui.set_rows(&[]); Ok(render(ui,page))
            }
            "allow" => {
                // Windows requires the permission request to originate on the UI thread.
                let pending=Listener::Current().and_then(|l|l.RequestAccessAsync()).map_err(werr)?;
                let hide = setting(ui,"notificationPrivacy","title");
                job("notifications",move || { pending.join().map_err(werr)?;notifications(&hide).map_err(werr) })
            }
            _ => { let hide=setting(ui,"notificationPrivacy","title"); job("notifications",move ||notifications(&hide).map_err(werr)) }
        }
        "rewrite" => {
            match name {
                "result" => {
                    let text=state().cache.get("rewrite-result").map(|v|v.detail.clone()).filter(|s|!s.is_empty()).ok_or("No completed rewrite is available.")?;
                    ui.set_detail(&text);
                    return Ok("Showing the last rewrite in this session.".into());
                }
                "cancel" => {stop();return Ok("Cancellation requested.".into());}
                "model" | "effort" => {
                    let value=ui.input().trim().to_owned();
                    if !value.is_empty() && if name=="model" {!cli::model_id(&value)} else {!cli::effort(&value)} {return Err("Enter a valid value, or clear the first field to use the default.".into());}
                    return set(ui,if name=="model" {"rewrite-model"} else {"rewrite-effort"},&value);
                }
                _ => {}
            }
            if state().busy.contains("rewrite") {return Err("Another CLI request is running.".into());}
            let cancel=Arc::new(AtomicBool::new(false));
            state().rewrite_cancel=Some(cancel.clone());
            let (root, custom, text, model, effort)=(ui.profile.clone(),ui.input(),ui.detail(),setting(ui,"rewrite-model",""),setting(ui,"rewrite-effort",""));
            let cmd=if name=="default" {setting(ui,"rewrite-preset","clearer")} else {name.to_string()};
            if matches!(cmd.as_str(),"custom"|"clearer"|"shorter"|"professional") {
                if text.trim().is_empty() {return Err("Enter text to rewrite.".into());}
                if cmd=="custom" && custom.trim().is_empty(){return Err("Enter a custom instruction.".into());}
                let _=set(ui,"rewrite-preset",&cmd);
                if cmd=="custom" {let _=set(ui,"rewrite-instruction",&custom);}
                state().rewrite_source=Some(text.clone());
            }
            job("rewrite",move || cli::run(&root,&cmd,&custom,&text,&model,&effort,&cancel))
        }
        "diagnostics" => match name {
            "capture" => {
                let mut s=state();
                if s.diagnostic_folder.is_some() {return Ok("A capture is already running.".into());}
                s.diagnostic_folder=Some(ui.profile.join("diagnostics"));s.diagnostic_samples.clear();
                Ok("Capturing 15 seconds…".into())
            }
            "open" => {let dir=ui.profile.join("diagnostics");fs::create_dir_all(&dir).map_err(|e|e.to_string())?;open(&dir.to_string_lossy())}
            "update" => job("diagnostics",update::check),
            "download" => { let url=state().cache.get(page).and_then(|s|s.url.clone()).ok_or("Check for an available update first.")?;open(&url) }
            _=>Ok(String::new())
        }
        _=>Ok(String::new())
    }
}
pub(crate) fn rewrite_busy()->bool{state().busy.contains("rewrite")}
pub fn set_rewrite_draft(ui:&mut Ui,text:&str) {
    ui.set_input(&setting(ui,"rewrite-instruction",""));
    ui.set_detail(text);ui.editor_rewrite_draft(text);
    let saved=setting(ui,"rewrite-preset","clearer");
    ui.status(&format!("Text ready. Select a rewrite action to send it. Saved preset: {saved}."));
}

pub fn stop() {
    if let Some(cancel)=&state().rewrite_cancel {cancel.store(true,std::sync::atomic::Ordering::Release);}
}

#[repr(C)]
#[derive(Default)]
struct Memory {cb:u32,faults:u32,peak_ws:usize,ws:usize,peak_paged:usize,paged:usize,peak_nonpaged:usize,nonpaged:usize,pagefile:usize,peak_pagefile:usize,private:usize}
#[link(name="psapi")]
unsafe extern "system" {fn GetProcessMemoryInfo(process:isize,mem:*mut Memory,cb:u32)->i32;}
#[link(name="kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess()->isize;
    fn GetProcessHandleCount(process:isize,n:*mut u32)->i32;
    fn GetProcessTimes(process:isize,created:*mut u64,exited:*mut u64,kernel:*mut u64,user:*mut u64)->i32;
}
fn sample() -> Result<String,String> {
    let mut mem=Memory::default();mem.cb=std::mem::size_of::<Memory>() as u32;
    let (mut c,mut e,mut k,mut u,mut handles)=(0,0,0,0,0);
    let ok=unsafe {let p=GetCurrentProcess();GetProcessMemoryInfo(p,&mut mem,mem.cb)!=0 && GetProcessTimes(p,&mut c,&mut e,&mut k,&mut u)!=0 && GetProcessHandleCount(p,&mut handles)!=0};
    if !ok {return Err("Windows process counters unavailable.".into());}
    Ok(format!("{},{},{},{},{}",stamp(),mem.private,mem.ws,(k+u)/10_000,handles))
}
fn stamp() -> u128 {std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis()}
fn report(folder:&Path,samples:&[String])->Result<String,String> {
    use std::io::Write;
    fs::create_dir_all(folder).map_err(|e|e.to_string())?;
    let path=folder.join(format!("performance-{}.csv",stamp()));
    let mut file=fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|e.to_string())?;
    writeln!(file,"unix_ms,private_bytes,working_set_bytes,cpu_total_ms,handles").and_then(|()|writeln!(file,"{}",samples.join("\r\n"))).map_err(|e|e.to_string())?;
    Ok("Saved 15 samples to the local diagnostics folder.".into())
}
pub fn tick(ui: &mut Ui) {
    let (poll,mut completed)={
        let mut s=state();
        let privacy=setting(ui,"notificationPrivacy","title");
        if s.privacy!=privacy {
            s.privacy=privacy;s.cache.remove("notifications");s.compact=None;
            if ui.page=="notifications"{ui.set_rows(&[]);}
        }
        if setting(ui,"notificationPreview","on")!="on"{s.compact=None;}
        s.ticks=s.ticks.wrapping_add(1);
        if let Some(folder)=s.diagnostic_folder.clone() {
            match sample() {
                Ok(line)=>{s.diagnostic_samples.push(line);
                    if s.diagnostic_samples.len()==15 {
                        let outcome=report(&folder,&s.diagnostic_samples).map(|msg|View::status(&msg));
                        s.done.push_back(("capture",outcome));s.diagnostic_folder=None;
                    }
                }
                Err(e)=>{s.done.push_back(("capture",Err(e)));s.diagnostic_folder=None;}
            }
        }
        (s.ticks%5==2,std::mem::take(&mut s.done))
    };
    if poll && !ui.isolated {
        if matches!(ui.page,"media"|"home") && !state().busy.contains("media") {let _=job("media",|| media("refresh",0).map_err(werr));}
        if (ui.page=="notifications" || setting(ui,"notificationPreview","on")=="on") && !state().busy.contains("notifications") {
            let hide=setting(ui,"notificationPrivacy","title");
            let _=job("notifications",move ||notifications(&hide).map_err(werr));
        }
    }
    while let Some((key,result))=completed.pop_front() {
        let success=result.is_ok();
        let mut view=result.unwrap_or_else(|e|View::status(&e));
        let mut toast=None;
        let mut source_text=None;
        {
            let mut s=state();s.busy.remove(key);
            if key=="rewrite" {
                s.rewrite_cancel=None;
                source_text=s.rewrite_source.take();
                if !view.detail.is_empty() {s.cache.insert("rewrite-result".into(),view.clone());}
            }
            if key=="notifications" {
                if view.privacy.as_ref().is_some_and(|mode|*mode!=s.privacy) {continue;}
                if let Some(entries)=view.notifications.take() {
                    if let Some(prev)=s.notification_ids.as_ref() {
                        let mut fresh=entries.iter().filter(|(id,_)|!prev.contains(id));
                        if let Some((_,first))=fresh.next(){
                            let app=first.split(" · ").next().unwrap_or("");
                            let more=fresh.filter(|(_,t)|t.split(" · ").next()==Some(app)).count();
                            toast=Some(if more>0 && setting(ui,"notificationGrouping","on")=="on" {format!("{app} · {} notifications",more+1)} else {first.clone()});
                        }
                    }
                    let current=entries.iter().map(|(id,_)|id.clone()).collect::<BTreeSet<_>>();
                    s.cleared_notifications.retain(|id|current.contains(id));
                    view.rows=entries.into_iter().filter(|(id,_)|!s.cleared_notifications.contains(id)).map(|(_,row)|row).collect();
                    s.notification_ids=Some(current);
                } else {s.notification_ids=None;}
            }
            if key!="capture" {s.cache.insert(key.into(),view.clone());}
        }
        if let Some(t)=toast {if setting(ui,"notificationPreview","on")=="on" {
            let seconds=setting(ui,"notificationDuration","12").parse::<u64>().unwrap_or(12).clamp(10,30);
            state().compact=Some((t.clone(),Instant::now()+Duration::from_secs(seconds)));
            ui.status(&t);
        }}
        if ui.page=="settings" && matches!(key,"capture"|"diagnostics"|"rewrite"|"notifications"){ui.status(&view.status);ui.layout();}
        if ui.page=="notifications" && key=="notifications"{ui.layout();}
        if ui.page=="home" && key=="media"{ui.layout();}
        if ui.page==key || (ui.page=="diagnostics" && key=="capture") {
            if (success || key=="notifications") && key!="capture" {
                ui.set_rows(&view.rows);
                if !view.detail.is_empty() {
                    if source_text.as_ref().is_some_and(|initial| *initial!=ui.detail()){
                        view.status="Rewrite ready. Choose Show last result to keep or replace the current draft.".into();
                    } else {ui.set_detail(&view.detail);if key=="rewrite"&&source_text.is_some(){ui.editor_rewrite_result(&view.detail);}}
                }
            }
            ui.status(&view.status);if key=="rewrite"{ui.layout();}
        }
    }
}

pub(crate) fn notification_permission()->&'static str{
    let s=state();let Some(v)=s.cache.get("notifications")else{return "unspecified"};
    if v.status.contains("is allowed"){"allowed"}else if v.status.contains("is denied"){"denied"}else{"unspecified"}
}
pub(crate) fn clear_notice_history(){let mut s=state();s.cleared_notifications=s.notification_ids.clone().unwrap_or_default();if let Some(v)=s.cache.get_mut("notifications"){v.rows.clear();}}
pub(crate) fn notice_titles()->Vec<String>{state().cache.get("notifications").map(|v|v.rows.clone()).unwrap_or_default()}
pub(crate) fn media_details()->Option<(String,String,i64,i64)>{
    let s=state();let v=s.cache.get("media")?;if !v.media_playing{return None}
    let artist=[v.rows.get(1).map(String::as_str).unwrap_or(""),v.status.as_str()].into_iter().filter(|s|!s.is_empty()).collect::<Vec<_>>().join(" · ");
    Some((v.rows.first()?.clone(),artist,v.position,v.duration))
}
pub(crate) fn home_media()->Option<String> {
    let s=state();
    let view=s.cache.get("media")?;
    if !view.media_playing {return None;}
    let label=view.rows.iter().take(2).filter(|v|!v.is_empty()).map(String::as_str).collect::<Vec<_>>().join(" · ");
    (!label.is_empty()).then_some(label)
}

pub fn compact_preview()->Option<String> {
    let mut s=state();
    if s.compact.as_ref().is_some_and(|(_,until)|Instant::now()>=*until) {s.compact=None;}
    s.compact.as_ref().map(|(text,_)|text.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn counters_have_five_numeric_fields() {
        let result=sample().unwrap();
        let fields:Vec<u128>=result.split(',').map(|v| v.parse().expect("numeric performance counter")).collect();
        assert_eq!(fields.len(),5);
        assert!(fields[1]>0 && fields[2]>0 && fields[4]>0);
    }
}
