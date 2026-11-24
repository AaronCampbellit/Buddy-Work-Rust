//! Native controls and painting for the split Notes workspace.
use super::*;
use crate::chrome::{control, fill, icon, mix, panel, rect, text, width, Icon};
use std::ptr::null;
use windows_sys::Win32::{
    Foundation::{FILETIME, POINT, RECT, SYSTEMTIME},
    System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
    UI::{
        Controls::{DRAWITEMSTRUCT, ODT_BUTTON, ODT_LISTBOX, ODS_FOCUS, ODS_NOFOCUSRECT, ODS_SELECTED, WM_MOUSELEAVE},
        Input::KeyboardAndMouse::{GetFocus, TrackMouseEvent, TRACKMOUSEEVENT, TME_LEAVE},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
    },
};

const CLOSE: u16 = 2800;
const SCHEDULE: u16 = 2801;
const DISMISS: u16 = 2802;
const ROW_PIN: u16 = 2803;
const ROW_DELETE: u16 = 2804;
const AUTOSAVE: u16 = 2805;
const LIST_MENU: u16 = 2806;
const LEFT: u32 = DT_VCENTER | DT_SINGLELINE | DT_LEFT;
const RIGHT: u32 = DT_VCENTER | DT_SINGLELINE | DT_RIGHT;

// Format the same short month, day and local time as the web editor. Both controls
// use the system locale and time zone, including DST at the note's timestamp.
pub(crate) fn note_date(stamp: &str) -> String {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDateFormatEx(locale: *const u16, flags: u32, time: *const SYSTEMTIME, pattern: *const u16, out: *mut u16, size: i32, calendar: *const u16) -> i32;
        fn GetTimeFormatEx(locale: *const u16, flags: u32, time: *const SYSTEMTIME, pattern: *const u16, out: *mut u16, size: i32) -> i32;
    }
    let Some(ticks) = stamp.parse::<u64>().ok().filter(|&v| v != 0)
        .and_then(|v| v.checked_add(11_644_473_600_000))
        .and_then(|v| v.checked_mul(10_000)) else { return "Unknown time".into() };
    let file = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    let mut utc = SYSTEMTIME::default();
    let mut local = SYSTEMTIME::default();
    let mut day = [0u16; 96];
    let mut hour = [0u16; 96];
    unsafe {
        if FileTimeToSystemTime(&file, &mut utc) == 0 || SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local) == 0 {
            return "Unknown time".into();
        }
        let nd = GetDateFormatEx(null(), 0, &local, wide("MMM d").as_ptr(), day.as_mut_ptr(), 96, null());
        let nt = GetTimeFormatEx(null(), 2, &local, null(), hour.as_mut_ptr(), 96); // TIME_NOSECONDS
        if nd < 2 || nt < 2 { return "Unknown time".into(); }
        format!("{}, {}", String::from_utf16_lossy(&day[..nd as usize-1]), String::from_utf16_lossy(&hour[..nt as usize-1]))
    }
}

pub(super) struct NoteControls {
    close: Handle,
    schedule: Handle,
    dismiss: Handle,
}

struct Layout {
    sidebar: RECT,
    search: RECT,
    add: RECT,
    list: RECT,
    detail: RECT,
    title: RECT,
    updated: RECT,
    close: RECT,
    body: RECT,
    reminder: RECT,
    dismiss: RECT,
    more: RECT,
}

fn move_to(h: Handle, r: RECT, visible: bool) {
    if visible { pos(h, r.left, r.top, r.right-r.left, r.bottom-r.top); }
    show(h, visible);
}

impl Ui {
    pub(crate) fn note_scroll_anchor(&self) -> Option<String> {
        // Navigation first clears this shared LISTBOX. During an ordinary
        // Notes refresh its rows still correspond to the previous note_ids.
        unsafe {
            if self.page != "notes" || self.note_ids.is_empty()
                || SendMessageW(self.list,LB_GETCOUNT,0,0) != self.note_ids.len() as isize {
                return None;
            }
            let top=SendMessageW(self.list,LB_GETTOPINDEX,0,0);
            usize::try_from(top).ok().and_then(|i| self.note_ids.get(i)).cloned()
        }
    }
    pub(crate) fn restore_note_scroll(&self,anchor:Option<String>) {
        if let Some(row)=anchor.and_then(|id|self.note_ids.iter().position(|record|record==&id)){
            unsafe{SendMessageW(self.list,LB_SETTOPINDEX,row,0);}
        }
    }

    pub(crate) fn formatted_note_date(stamp:&str)->String { note_date(stamp) }

    fn edited_row(&self) -> Option<&Vec<String>> {
        let id = self.active_note.as_ref()?;
        self.records.get("notes")?.iter().find(|row| row.len() >= 7 && row.first() == Some(id))
    }

    fn note_geometry(&self, island: RECT) -> Layout {
        let p = |n: i32| (n as f32 * self.scale()).round() as i32;
        let area = RECT { left: island.left+p(30), right: island.right-p(30), top: island.top+p(88), bottom: island.bottom-p(17) };
        let available = (area.right-area.left).max(0);
        let side_width = ((available as f32 * (0.58 / 2.18)).round() as i32).max(p(180)).min(available);
        let sidebar = RECT { right: area.left+side_width, ..area };
        let add = rect(sidebar.right-p(10+89), area.top, p(89), p(29));
        let search = RECT { right: add.left-p(6), bottom: area.top+p(29), ..area };
        let list = RECT { top: area.top+p(35), right: sidebar.right-p(10), ..sidebar };
        let detail = RECT { left: sidebar.right+p(11), top: area.top+p(6), ..area };
        let close = rect(detail.right-p(27), detail.top, p(27), p(25));
        let stamp = self.note_stamp();
        let dc = unsafe { GetDC(self.hwnd.0) };
        let stamp_width = width(dc, &stamp, self.fonts[8]);
        unsafe { ReleaseDC(self.hwnd.0, dc); }
        let updated = rect(close.left-p(7)-stamp_width, detail.top, stamp_width, p(25));
        let title = RECT { right: updated.left-p(7), bottom: detail.top+p(25), ..detail };
        let body = RECT { top: title.bottom+p(4), bottom: area.bottom-p(33), ..detail };
        let remove = self.edited_row().is_some_and(|r| !r[5].is_empty());
        let dismiss = rect(area.right-p(65), area.bottom-p(29), p(65), p(25));
        let reminder_right = if remove { dismiss.left-p(5) } else { area.right };
        let reminder = rect((reminder_right-p(168)).max(detail.left+p(34)), area.bottom-p(29),
            (reminder_right-detail.left-p(34)).clamp(1, p(168)), p(25));
        let more = rect(detail.left, area.bottom-p(29), p(27), p(25));
        Layout { sidebar, search, add, list, detail, title, updated, close, body, reminder, dismiss, more }
    }

    fn note_stamp(&self) -> String {
        if self.new_note { return "New note".into(); }
        self.edited_row().map(|r| note_date(&r[6])).unwrap_or_default()
    }

    pub(super) fn layout_notes(&self, visible: bool, island: RECT) {
        if !visible {
            if let Some(c) = self.note_controls.get() {
                for h in [c.close, c.schedule, c.dismiss] { show(h, false); }
                unsafe {
                    if !RemovePropW(self.list, wide("BuddyNoteList").as_ptr()).is_null() {
                        SendMessageW(self.input_box, WM_SETFONT, self.fonts[0] as _, 0);
                        SendMessageW(self.input_box, 0x1501, 0, wide("").as_ptr() as _);
                    }
                }
            }
            return;
        }
        show(self.detail_box,false);
        if window_text(self.search_box,32768)!=self.search { label(self.search_box,&self.search); }
        let c = self.note_controls.get_or_init(|| {
            let button = |name, id| {
                let h = add(self.hwnd.0, "BUTTON", name, id, WS_TABSTOP | BS_OWNERDRAW as u32, 0, self.fonts[0]);
                chrome::install_button(h);
                h
            };
            unsafe {
                SetWindowSubclass(self.list, Some(note_list_proc), 0xB061, 0);
                SetWindowSubclass(self.input_box, Some(note_edit_proc), 0xB062, 0);
                SetWindowSubclass(self.images.editor, Some(note_edit_proc), 0xB062, 0);
                SetWindowSubclass(self.search_box, Some(note_edit_proc), 0xB062, 0);
            }
            NoteControls { close: button("Close note editor", CLOSE), schedule: button("Reminder date and time", SCHEDULE), dismiss: button("Remove reminder", DISMISS) }
        });
        let g = self.note_geometry(island);
        let p = |n: i32| (n as f32 * self.scale()).round() as i32;
        let edit = self.active_note.is_some();
        let remove = self.edited_row().is_some_and(|row| !row[5].is_empty());
        unsafe {
            SetPropW(self.list, wide("BuddyNoteList").as_ptr(), 1usize as _);
            SendMessageW(self.list, LB_SETITEMHEIGHT, 0, p(47) as _);
            SendMessageW(self.input_box, WM_SETFONT, self.fonts[1] as _, 0);
            SendMessageW(self.input_box, 0x1501, 1, wide("Untitled note").as_ptr() as _);
        }
        move_to(self.search_box, RECT { left: g.search.left+p(30), right:g.search.right-p(5), top:g.search.top+p(7), bottom:g.search.bottom-p(5) }, true);
        move_to(self.buttons[0], g.add, true);
        move_to(self.list, g.list, !self.note_ids.is_empty());
        move_to(self.input_box, RECT {left:g.title.left+p(7),right:g.title.right-p(4),top:g.title.top+p(5),bottom:g.title.bottom-p(3)}, edit);
        self.images.note_layout(edit, chrome::inset(g.body,p(1)), self.zoom, self.appearance, 10);
        for h in self.buttons.iter().skip(1) { show(*h, false); }
        move_to(c.close, g.close, edit);
        move_to(c.schedule, g.reminder, edit);
        move_to(c.dismiss, g.dismiss, edit && remove);
        move_to(self.more_go, g.more, edit || self.last_removed.is_some());
    }

    pub(super) fn note_tab_stops(&self) -> Vec<Handle> {
        let mut items = vec![self.search_box, self.buttons[0], self.list, self.input_box];
        if let Some(c) = self.note_controls.get() { items.extend([c.close, self.images.editor, c.schedule, c.dismiss]); }
        items.push(self.more_go);
        items
    }

    pub(super) fn paint_notes(&self, dc: HDC, island: RECT) {
        let p = |n: i32| (n as f32*self.scale()).round() as i32;
        let (bg, _, ink, accent) = self.appearance;
        let edge = mix(bg,ink,42);
        let muted = mix(bg,ink,162);
        let face = control(self.appearance);
        let focused = unsafe { GetFocus() };
        let g = self.note_geometry(island);
        let border = |h| if focused==h { accent } else { edge };
        fill(dc, RECT { left:g.sidebar.right-p(1), ..g.sidebar }, edge);
        panel(dc,g.search,p(6),face,border(self.search_box));
        icon(dc,g.search.left+p(8),g.search.top+p(7),p(15),Icon::Search,muted,face);
        if self.note_ids.is_empty() {
            let (title, sub) = if self.search.is_empty() { ("No notes yet", "Create a note to keep an idea close.") }
                else { ("No matching notes", "Try another search.") };
            let tw = width(dc,sub,self.fonts[8]).max(width(dc,title,self.fonts[1]));
            let x = ((g.list.left+g.list.right-p(31)-tw)/2).max(g.list.left);
            let y = (g.list.top+g.list.bottom)/2;
            icon(dc,x,y-p(11),p(22),Icon::Pencil,muted,bg);
            text(dc,RECT{left:x+p(31),top:y-p(14),right:g.list.right,bottom:y+p(1)},title,self.fonts[1],ink,LEFT);
            text(dc,RECT{left:x+p(31),top:y+p(2),right:g.list.right,bottom:y+p(15)},sub,self.fonts[8],muted,LEFT);
        }
        if self.active_note.is_some() {
            panel(dc,g.title,p(5),face,border(self.input_box));
            text(dc,g.updated,&self.note_stamp(),self.fonts[8],muted,RIGHT);
            panel(dc,g.body,p(6),face,border(self.images.editor));
        } else {
            let title = "Select a note or create a new one.";
            let x = (g.detail.left+g.detail.right-p(31)-width(dc,title,self.fonts[8]))/2;
            let y = (g.detail.top+g.detail.bottom)/2;
            icon(dc,x,y-p(11),p(22),Icon::Clock,muted,bg);
            text(dc,rect(x+p(31),y-p(12),g.detail.right-x-p(31),p(24)),title,self.fonts[8],muted,LEFT);
        }
    }

    pub(super) fn draw_note_item(&self, di: &DRAWITEMSTRUCT) -> bool {
        if self.page != "notes" { return false; }
        let id = di.CtlID as u16;
        let r = di.rcItem;
        let dc = di.hDC;
        let p = |n: i32| (n as f32*self.scale()).round() as i32;
        let (bg, _, ink, accent) = self.appearance;
        let muted = mix(bg,ink,162);
        let edge = mix(bg,ink,42);
        let focus = di.itemState & ODS_FOCUS != 0 && di.itemState & ODS_NOFOCUSRECT == 0;
        if di.CtlType == ODT_LISTBOX && id == LIST {
            fill(dc,r,bg);
            let Some(id) = self.note_ids.get(di.itemID as usize) else { return true };
            let Some(row) = self.records.get("notes").and_then(|v|v.iter().find(|r|r.first()==Some(id) && r.len()>=7)) else{return true};
            let selected = self.active_note.as_ref()==Some(id) && !self.new_note;
            let hovered = unsafe { GetPropW(self.list,wide("BuddyNoteHover").as_ptr()) as usize };
            let hot = hovered >> 2 == di.itemID as usize + 1;
            let area = RECT { bottom:r.bottom-p(4), ..r };
            let base = if selected { mix(bg,accent,27) } else if hot { mix(bg,ink,16) } else { bg };
            panel(dc,area,p(8),base,if focus {accent} else if selected {mix(bg,accent,100)} else {base});
            if selected { fill(dc,rect(area.left+p(1),area.top+p(8),p(2),area.bottom-area.top-p(16)),accent); }
            let actions = area.right-p(54);
            fill(dc,rect(actions,area.top+p(1),p(1),area.bottom-area.top-p(2)),edge);
            let cy = (area.top+area.bottom)/2;
            let mut x = area.left+p(10);
            let pinned = row[3]=="1";
            if pinned { icon(dc,x,cy-p(13),p(12),Icon::Pin,accent,base); x+=p(17); }
            let stamp = note_date(&row[6]);
            let stamp_width = width(dc,&stamp,self.fonts[8])+p(4);
            let title_right = (actions-p(8+5)-stamp_width).max(x);
            let title = if row[1].is_empty() { "Untitled note" } else { &row[1] };
            text(dc,rect(x,cy-p(16),title_right-x,p(16)),title,self.fonts[1],ink,LEFT);
            text(dc,RECT{left:(actions-p(8)-stamp_width).max(x),right:actions-p(8),top:cy-p(16),bottom:cy},&stamp,self.fonts[8],muted,RIGHT);
            let snippet=row[2].split_whitespace().take(36).collect::<Vec<_>>().join(" ");
            let preview=if snippet.is_empty(){"Empty note"}else{snippet.as_str()};
            text(dc,rect(area.left+p(10),cy+p(1),actions-area.left-p(18),p(14)),preview,self.fonts[3],muted,LEFT);
            for (i, glyph) in [Icon::Pin,Icon::Trash].into_iter().enumerate() {
                let x = actions+p(27)*i as i32;
                let over = hot && hovered & 3 == i+1;
                let fg = if over && i==1 {mix(ink,0x4040ff,175)} else if i==0 && pinned {accent} else if over {ink} else {muted};
                let face = if over {mix(base,fg,22)} else {base};
                if over {panel(dc,rect(x+p(1),area.top+p(2),p(25),area.bottom-area.top-p(4)),p(6),face,face);}
                icon(dc,x+(p(27)-p(14))/2,cy-p(7),p(14),glyph,fg,face);
            }
            return true;
        }
        if di.CtlType != ODT_BUTTON || !matches!(id, CLOSE|SCHEDULE|DISMISS|MORE_GO) {return false;}
        let hot = chrome::hovered(di.hwndItem) || di.itemState & ODS_SELECTED != 0;
        let face = if hot {mix(bg,ink,23)} else if id == SCHEDULE {control(self.appearance)} else {bg};
        fill(dc,r,bg);
        if id==MORE_GO && !hot && !focus && self.last_removed.is_none() { return true; }
        panel(dc,r,p(6),face,if focus {accent} else if id==SCHEDULE||id==DISMISS {edge} else {face});
        let cy = (r.top+r.bottom)/2;
        let glyph = match id { CLOSE=>Icon::Close, SCHEDULE=>Icon::Bell, DISMISS=>Icon::BellOff, _=>Icon::More };
        let x = if matches!(id,SCHEDULE|DISMISS){r.left+p(7)}else{(r.left+r.right-p(16))/2};
        icon(dc,x,cy-p(7),p(14),glyph,if id==SCHEDULE {accent} else if hot {ink} else {muted},face);
        if id == SCHEDULE {
            let stamp = self.edited_row().filter(|r|!r[5].is_empty()).map(|r|note_date(&r[5])).unwrap_or_else(||"mm/dd/yyyy   --:-- --".into());
            text(dc,RECT{left:x+p(20),right:r.right-p(22),..r},&stamp,self.fonts[8],muted,LEFT);
            icon(dc,r.right-p(19),cy-p(6),p(12),Icon::Calendar,muted,face);
        } else if id == DISMISS {
            text(dc,RECT{left:x+p(18),..r},"Remove",self.fonts[8],muted,LEFT);
        }
        true
    }

    pub(super) fn note_command(&mut self, id:u16, code:u16)->bool {
        if self.page!="notes" { return false; }
        if matches!(id, INPUT|inline_images::NOTE_EDITOR) && code==EN_CHANGE as u16 && self.active_note.is_some() {
            self.dirty_note=true;
            unsafe { SetTimer(self.input_box, AUTOSAVE as usize, 350, None); }
            return true;
        }
        match id {
            AUTOSAVE => {
                if self.dirty_note && self.active_note.is_some() {
                    if let Err(e)=core::save_note(self){self.error(&e);}else{self.layout();}
                }
            }
            CLOSE if code==BN_CLICKED as u16 => {
                if self.dirty_note {if let Err(e)=core::save_note(self){self.error(&e);return true;}}
                self.active_note=None; self.new_note=false; self.dirty_note=false;
                core::note_rows(self); self.layout();
                crate::shell::focus(if self.note_ids.is_empty(){self.buttons[0]}else{self.list});
            }
            SCHEDULE|DISMISS if code==BN_CLICKED as u16 => self.run_action(if id==SCHEDULE{"schedule"}else{"dismiss"}),
            ROW_PIN|ROW_DELETE|LIST_MENU => {
                let index = code as usize;
                let Some(target)=self.note_ids.get(index).cloned() else{return true;};
                let previous=self.active_note.clone();
                let key = if id==LIST_MENU {
                    let pinned=self.records.get("notes").and_then(|rows|rows.iter().find(|r|r.first()==Some(&target))).is_some_and(|r|r.get(3).map(String::as_str)==Some("1"));
                    let choices=[if pinned{"Unpin note"}else{"Pin note"},"Delete note","Show as widget","Copy note"].map(str::to_owned);
                    let Some(choice)=self.choose(self.list,&choices)else{return true;};
                    ["pin","delete","widget","copy"][choice]
                }else if id==ROW_PIN{"pin"}else{"delete"};
                self.select_row(index);
                if let Err(e)=core::action(self,"notes","select"){self.error(&e);return true;}
                if self.active_note.as_ref()!=Some(&target) { return true; }
                self.run_action(key);
                if previous!=Some(target) {
                    if let Some(prev)=previous {
                        if let Some(i)=self.note_ids.iter().position(|n|n==&prev){self.select_row(i);self.run_action("select");}
                    }else{self.active_note=None;core::note_rows(self);self.layout();}
                }
            }
            MORE_GO if code==BN_CLICKED as u16 => {
                let available=self.actions.iter().copied().filter(|(key,_)| match *key {
                    "undo"=>self.last_removed.is_some(), "new"=>false, _=>self.active_note.is_some()
                }).collect::<Vec<_>>();
                let labels=available.iter().map(|(_,s)|s.to_string()).collect::<Vec<_>>();
                if let Some(index)=self.choose(self.more_go,&labels){self.run_action(available[index].0);}
            }
            _=>return false
        }
        true
    }
}

// Painted row actions track the list's actual item rectangles, including after a
// native scrollbar move. Keyboard users have the same actions on the menu key.
unsafe fn row_hit(list:Handle, lp:isize)->Option<(usize,usize)> {
    let x=lp as i16 as i32;
    let y=(lp>>16) as i16 as i32;
    let hit=SendMessageW(list,LB_ITEMFROMPOINT,0,lp) as u32;
    if hit>>16!=0 {return None;}
    let i=(hit & 0xffff) as usize;
    let mut r=RECT::default();
    if SendMessageW(list,LB_GETITEMRECT,i,&mut r as *mut _ as _)<0 || y<r.top || y>=r.bottom {return None;}
    let scale=(r.bottom-r.top) as f32 / 47.0;
    let width=(27.0*scale).round() as i32;
    let zone=if y>=r.bottom-(4.0*scale).round() as i32 || x<r.right-2*width{0}else if x<r.right-width{1}else{2};
    Some((i,zone))
}

unsafe extern "system" fn note_list_proc(h:Handle,msg:u32,wp:usize,lp:isize,id:usize,_:usize)->isize {
    let key=wide("BuddyNoteHover");
    if msg==WM_NCDESTROY {RemoveWindowSubclass(h,Some(note_list_proc),id);}
    if !GetPropW(h,wide("BuddyNoteList").as_ptr()).is_null() {
        match msg {
            WM_LBUTTONDOWN|WM_LBUTTONDBLCLK if row_hit(h,lp).is_some_and(|(_,zone)|zone!=0)=>return 0,
            WM_LBUTTONUP=>{
                if let Some((i,zone))=row_hit(h,lp).filter(|(_,zone)|*zone!=0){
                    SendMessageW(GetParent(h),WM_COMMAND,(if zone==1{ROW_PIN}else{ROW_DELETE}) as usize|(i<<16),h as _);
                    return 0;
                }
            }
            WM_CONTEXTMENU=>{
                let index=if lp == -1 {SendMessageW(h,LB_GETCURSEL,0,0)}else{
                    let mut p=POINT{x:lp as i16 as i32,y:(lp>>16)as i16 as i32};
                    ScreenToClient(h,&mut p);
                    row_hit(h,(p.x as isize&65535)|((p.y as isize)<<16)).map_or(-1,|(i,_)|i as isize)
                };
                if index>=0{SendMessageW(GetParent(h),WM_COMMAND,LIST_MENU as usize|((index as usize)<<16),h as _);}
                return 0;
            }
            WM_MOUSEMOVE=>{
                let value=row_hit(h,lp).map_or(0,|(i,zone)|((i+1)<<2)|zone);
                if GetPropW(h,key.as_ptr()) as usize!=value{
                    if value==0 {RemovePropW(h,key.as_ptr());}else{SetPropW(h,key.as_ptr(),value as _);}
                    InvalidateRect(h,null(),0);
                    let mut event=TRACKMOUSEEVENT{cbSize:std::mem::size_of::<TRACKMOUSEEVENT>()as _,dwFlags:TME_LEAVE,hwndTrack:h,dwHoverTime:0};
                    TrackMouseEvent(&mut event);
                }
            }
            WM_MOUSELEAVE=>{RemovePropW(h,key.as_ptr());InvalidateRect(h,null(),0);}
            _=>{}
        }
    }
    DefSubclassProc(h,msg,wp,lp)
}

unsafe extern "system" fn note_edit_proc(h:Handle,msg:u32,wp:usize,lp:isize,id:usize,_:usize)->isize {
    match msg {
        WM_SETFOCUS|WM_KILLFOCUS=>{InvalidateRect(GetParent(h),null(),0);}
        WM_TIMER if wp==AUTOSAVE as usize=>{
            KillTimer(h,wp);
            SendMessageW(GetParent(h),WM_COMMAND,AUTOSAVE as usize,0);
            return 0;
        }
        WM_NCDESTROY=>{RemoveWindowSubclass(h,Some(note_edit_proc),id);}
        _=>{}
    }
    DefSubclassProc(h,msg,wp,lp)
}
