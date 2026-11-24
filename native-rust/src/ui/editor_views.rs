//! In-island image and rewrite actions share the native model, file pickers,
//! encrypted markup store and OCR workers used by the application.
use super::*;
use std::cell::Cell;
use chrome::{fill,inset,mix,panel,rect,text};
use windows_sys::Win32::{Foundation::RECT,UI::{Controls::{DRAWITEMSTRUCT,ODS_DISABLED,ODS_FOCUS,ODS_NOFOCUSRECT,ODS_SELECTED},
    Input::KeyboardAndMouse::EnableWindow,Shell::{SetWindowSubclass,DefSubclassProc,RemoveWindowSubclass}}};

const BACK:u16=3300;
const TONE:u16=3301;
const SUBMIT:u16=3302;
const CANCEL:u16=3303;
const COPY:u16=3304;
const SOURCE:u16=3305;
const RESULT:u16=3306;
const PRESET:u16=3307;
const OPTIONS:u16=3308;
const FILE:u16=3309;
const PASTE:u16=3310;
const RESET:u16=3311;
const OPEN:u16=3312;
const SAVE:u16=3313;
const LAST:u16=SAVE;
const LABELS:[&str;4]=["Clearer","Shorter","Professional","Custom"];
const PRESETS:[&str;4]=["clearer","shorter","professional","custom"];
const LEFT:u32=DT_LEFT|DT_VCENTER|DT_SINGLELINE;
pub(super) fn handles(page:&str)->bool{matches!(page,"rewrite"|"ocr"|"markup")}

pub(super) struct EditorViews{
    controls:Vec<Handle>, page:Cell<&'static str>, preset:Cell<usize>, fixed:Cell<bool>, syncing:Cell<bool>,
}
impl EditorViews{
    fn new(ui:&Ui)->Self{
        let mut controls=Vec::new();
        for (id,name) in [(BACK,"Back to clipboard"),(TONE,"Tone"),(SUBMIT,"Rewrite"),(CANCEL,"Cancel"),(COPY,"Copy rewrite"),
            (SOURCE,""),(RESULT,""),(PRESET,""),(OPTIONS,"More actions"),(FILE,"Choose image"),(PASTE,"Clipboard image"),(RESET,"Clear"),(OPEN,"Open editor"),(SAVE,"Save PNG")]{
            let edit=matches!(id,SOURCE|RESULT|PRESET);
            let style=match id{SOURCE|RESULT=>(ES_MULTILINE|ES_AUTOVSCROLL|ES_WANTRETURN)as u32|WS_VSCROLL,PRESET=>ES_AUTOHSCROLL as u32,_=>BS_OWNERDRAW as u32};
            let h=add(ui.hwnd.0,if edit{"EDIT"}else{"BUTTON"},name,id,WS_TABSTOP|style,0,ui.fonts[0]);
            if !edit{chrome::install_button(h)}else{unsafe{SendMessageW(h,EM_SETLIMITTEXT,if id==PRESET{1000}else{256*1024},0);}}
            controls.push(h);show(h,false);
        }
        let pane=Self{controls,page:Cell::new(""),preset:Cell::new(0),fixed:Cell::new(false),syncing:Cell::new(false)};
        unsafe{
            SendMessageW(pane.h(RESULT),0x00cf,1,0);
            SetWindowSubclass(pane.h(RESULT),Some(empty_result_proc),1,0);
            SendMessageW(pane.h(PRESET),0x1501,1,wide("How should it read?").as_ptr()as _);
        }
        pane
    }
    fn h(&self,id:u16)->Handle{self.controls[(id-BACK)as usize]}
}

impl Ui{
    fn rewrite_footer(&self,width:i32)->(String,i32){
        let mut status=window_text(self.notice,4096);
        if status.is_empty()||["Enter the text below","Choose the rewrite","Text ready."].iter().any(|prefix|status.starts_with(prefix)){
            status="Sends this text to OpenAI when you choose Rewrite. Connect in Settings → AI Rewrite.".into();
        }
        let wraps=unsafe{let dc=GetDC(self.hwnd.0);let measured=chrome::width(dc,&status,self.fonts[0]);ReleaseDC(self.hwnd.0,dc);measured>width};
        (status,if wraps{34}else{17})
    }
    pub(crate) fn editor_rewrite_draft(&self,value:&str){
        if self.page!="rewrite"{return}
        let pane=self.editor_views.get_or_init(||EditorViews::new(self));
        pane.page.set("rewrite");pane.syncing.set(true);label(pane.h(SOURCE),value);label(pane.h(RESULT),"");pane.syncing.set(false);
        pane.fixed.set(true);unsafe{SendMessageW(pane.h(SOURCE),0x00cf,1,0);}self.layout();
    }
    pub(crate) fn editor_rewrite_result(&self,value:&str){
        if let Some(pane)=self.editor_views.get(){if self.page=="rewrite"{label(pane.h(RESULT),value);}}
    }
    pub(crate) fn editor_escape(&mut self)->bool{
        if self.page!="rewrite"||!self.expanded{return false}
        if activity::rewrite_busy(){self.run_action("cancel")}else{self.navigate("clipboard")}
        true
    }
    pub(super) fn layout_editors(&self,visible:bool,r:RECT){
        if !visible{
            if let Some(pane)=self.editor_views.get(){for &h in &pane.controls{show(h,false)}if !handles(self.page){pane.page.set("")}}
            return
        }
        let pane=self.editor_views.get_or_init(||EditorViews::new(self));let page=self.page;
        if pane.page.replace(page)!=page&&page=="rewrite"{
            pane.syncing.set(true);pane.fixed.set(false);
            pane.preset.set(PRESETS.iter().position(|s|*s==core::setting(self,"rewrite-preset","clearer")).unwrap_or(0));
            unsafe{SendMessageW(pane.h(SOURCE),0x00cf,0,0);}
            label(pane.h(SOURCE),"");label(pane.h(RESULT),"");label(pane.h(PRESET),&core::setting(self,"rewrite-instruction",""));
            pane.syncing.set(false);
        }
        let w=((r.right-r.left)as f32/self.scale()).round()as i32;
        let h=((r.bottom-r.top)as f32/self.scale()).round()as i32;
        let mut used=Vec::new();
        let place=|handle,x,y,width,height|{pos(handle,r.left+self.px(x),r.top+self.px(y),self.px(width),self.px(height));show(handle,true);};
        let mut put=|id,x,y,width,height|{
            let handle=pane.h(id);unsafe{SendMessageW(handle,WM_SETFONT,self.fonts[0]as _,0);}
            place(handle,x,y,width,height);used.push(id);
        };
        let busy=activity::rewrite_busy();
        let caption=|id,name:&str|{let h=pane.h(id);if window_text(h,100)!=name{label(h,name)}};
        if page=="rewrite"{
            caption(SUBMIT,if window_text(pane.h(RESULT),1).is_empty(){"Rewrite"}else{"Try again"});caption(COPY,"Copy rewrite");caption(TONE,LABELS[pane.preset.get()]);
            put(BACK,14,10,103,28);put(TONE,154,10,96,28);
            let mut x=256;let custom=pane.preset.get()==3;
            if custom{let width=(w-481).clamp(65,180);put(PRESET,x,12,width,24);x+=width+6;}
            let footer_x=w-28;let row2=x+if busy{204}else{139}>footer_x;
            let mut ax=if row2{14}else{x};let ay=if row2{44}else{10};
            put(SUBMIT,ax,ay,62,28);ax+=68;
            if busy{put(CANCEL,ax,ay,53,28);ax+=59;}
            put(COPY,ax,ay,75,28);
            if !pane.fixed.get(){put(OPTIONS,w-38,10,25,28);}
            unsafe{SetPropW(pane.h(RESULT),wide("BuddyRewriteHintColor").as_ptr(),(mix(self.appearance.0,self.appearance.2,158)as usize+1)as _);}
            let two=if row2{86}else{52};let half=(w-38)/2;let (_,footer)=self.rewrite_footer(self.px(w-28));let height=(h-two-38-footer).max(24);
            put(SOURCE,22,two+18,half-16,height);put(RESULT,32+half,two+18,half-16,height);
            for id in [SOURCE,RESULT]{
                let edit=pane.h(id);
                unsafe{
                    let lines=SendMessageW(edit,windows_sys::Win32::UI::Controls::EM_GETLINECOUNT,0,0)as i32;
                    let dc=GetDC(edit);let previous=SelectObject(dc,self.fonts[0]);let mut metrics=TEXTMETRICW::default();GetTextMetricsW(dc,&mut metrics);SelectObject(dc,previous);ReleaseDC(edit,dc);
                    let overflowing=lines*metrics.tmHeight>self.px(height);
                    let visible=GetWindowLongW(edit,GWL_STYLE)as u32&WS_VSCROLL!=0;
                    if visible!=overflowing{windows_sys::Win32::UI::Controls::ShowScrollBar(edit,SB_VERT,overflowing as _);}
                }
            }
            let original=window_text(pane.h(SOURCE),20001);let result=window_text(pane.h(RESULT),256*1024);
            let can_send=!busy&&!original.trim().is_empty()&&original.encode_utf16().count()<=20000&&(!custom||!window_text(pane.h(PRESET),1000).trim().is_empty());
            for(id,enabled)in[(TONE,!busy),(SUBMIT,can_send),(COPY,!busy&&!result.is_empty()),(PRESET,!busy)]{unsafe{EnableWindow(pane.h(id),i32::from(enabled));}}
        }else{
            let ocr=page=="ocr";
            caption(SUBMIT,if ocr{"Rescan"}else{"Read text"});caption(COPY,"Copy text");
            let ready=if ocr{!self.input().trim().is_empty()}else{workspace::selected_image_id(self).is_some()};
            let has_text=ocr&&!self.detail().is_empty();
            for (id,allowed) in [(SUBMIT,ready),(COPY,has_text),(RESET,ready||has_text),(OPEN,ready),(SAVE,ready)]{
                unsafe{EnableWindow(pane.h(id),i32::from(allowed));}
            }
            put(BACK,0,2,101,28);put(FILE,107,2,90,28);put(PASTE,203,2,116,28);
            let actions_y=if w<615{36}else{2};let mut x=if w<615{0}else{325};
            for (id,ww) in if ocr{[(SUBMIT,66),(COPY,72),(RESET,49)]}else{[(OPEN,83),(SAVE,73),(SUBMIT,79)]}{
                put(id,x,actions_y,ww,28);x+=ww+6;
            }
            let top=actions_y+39;
            if ocr{
                place(self.input_box,7,top+3,w-14,19);
                let half=(w-10)*37/100;let start=top+46;let height=(h-start-7).max(20);
                place(self.list,4,start,half-8,height);place(self.detail_box,half+14,start,w-half-22,height);
                unsafe{SendMessageW(self.list,LB_SETITEMHEIGHT,0,self.px(21)as _);}
            }else{
                place(self.list,5,top+18,w-10,(h-top-22).max(20));unsafe{SendMessageW(self.list,LB_SETITEMHEIGHT,0,self.px(30)as _);}
            }
        }
        for id in BACK..=LAST{if !used.contains(&id){show(pane.h(id),false)}}
    }
    pub(super) fn editor_tab_stops(&self)->Vec<Handle>{
        let Some(pane)=self.editor_views.get()else{return Vec::new()};
        let mut result=[BACK,TONE,PRESET,SUBMIT,CANCEL,COPY,OPTIONS,SOURCE,RESULT,FILE,PASTE,OPEN,SAVE,RESET].into_iter().map(|id|pane.h(id)).collect::<Vec<_>>();
        if self.page=="ocr"{result.extend([self.input_box,self.list,self.detail_box])}else if self.page=="markup"{result.push(self.list)}
        result
    }
    pub(super) fn paint_editors(&self,dc:HDC,r:RECT){
        let Some(s)=self.editor_views.get()else{return};let (bg,field,ink,_)=self.appearance;let muted=mix(bg,ink,164);let edge=mix(bg,ink,43);
        let bounds=|handle|unsafe{let mut b=RECT::default();GetWindowRect(handle,&mut b);MapWindowPoints(std::ptr::null_mut(),self.hwnd.0,(&mut b as *mut RECT).cast(),2);b};
        let label_above=|h,name:&str,pad:i32|{
            let b=bounds(h);let box_=rect(b.left-self.px(pad),b.top-self.px(4),b.right-b.left+2*self.px(pad),b.bottom-b.top+self.px(8));
            panel(dc,box_,self.px(6),field,edge);
            text(dc,rect(box_.left,box_.top-self.px(17),box_.right-box_.left,self.px(16)),name,self.fonts[0],muted,LEFT);
        };
        if self.page=="rewrite"{
            text(dc,rect(r.left+self.px(123),r.top+self.px(9),self.px(30),self.px(28)),"Tone",self.fonts[0],ink,LEFT);
            label_above(s.h(SOURCE),"Original",8);label_above(s.h(RESULT),"Rewrite",8);
            let width=r.right-r.left-self.px(28);let (status,height)=self.rewrite_footer(width);
            text(dc,rect(r.left+self.px(14),r.bottom-self.px(10+height),width,self.px(height)),&status,self.fonts[0],muted,DT_LEFT|DT_WORDBREAK|DT_TOP);
        }else if self.page=="ocr"{
            let b=bounds(self.input_box);panel(dc,inset(b,-self.px(3)),self.px(5),field,edge);
            label_above(self.list,"Words",4);label_above(self.detail_box,"Recognized text",5);
        }else{label_above(self.list,"Clipboard images",4)}
    }
    pub(super) fn draw_editor_button(&self,di:&DRAWITEMSTRUCT)->bool{
        let id=di.CtlID as u16;if !(BACK..=LAST).contains(&id)||matches!(id,SOURCE|RESULT|PRESET){return false}
        let (bg,field,ink,accent)=self.appearance;
        let disabled=di.itemState&ODS_DISABLED!=0;let hot=chrome::hovered(di.hwndItem)||di.itemState&ODS_SELECTED!=0;
        let face=if hot&&!disabled{mix(field,ink,15)}else{chrome::control(self.appearance)};let color=if disabled{mix(bg,ink,113)}else{ink};
        let focus=di.itemState&ODS_FOCUS!=0&&di.itemState&ODS_NOFOCUSRECT==0;
        let name=window_text(di.hwndItem,100);let caption=if id==OPTIONS{"···"}else{&name};
        fill(di.hDC,di.rcItem,bg);panel(di.hDC,di.rcItem,self.px(6),face,if focus{accent}else{mix(bg,ink,48)});
        if id==TONE{
            let r=di.rcItem;text(di.hDC,rect(r.left+self.px(9),r.top,r.right-r.left-self.px(27),r.bottom-r.top),caption,self.fonts[0],color,LEFT);
            chrome::icon(di.hDC,r.right-self.px(18),r.top+(r.bottom-r.top-self.px(11))/2,self.px(11),chrome::Icon::Down,color,face);
        }else{text(di.hDC,di.rcItem,caption,self.fonts[0],color,DT_CENTER|DT_SINGLELINE|DT_VCENTER);}
        true
    }
    pub(super) fn editor_command(&mut self,id:u16,code:u16)->bool{
        if matches!(id,INPUT|DETAIL) && self.page=="ocr" && code==EN_CHANGE as u16{self.layout();return true}
        if !(BACK..=LAST).contains(&id){return false}
        let Some(s)=self.editor_views.get()else{return true};if !handles(self.page)||s.syncing.get(){return true}
        if code==EN_CHANGE as u16&&matches!(id,SOURCE|PRESET){
            if id==SOURCE{self.set_detail(&window_text(s.h(SOURCE),256*1024));}else{self.set_input(&window_text(s.h(PRESET),1000));}
            self.layout();return true
        }
        if code!=BN_CLICKED as u16{return true}
        let page=self.page;
        match id{
            BACK=>self.navigate("clipboard"),
            TONE=>{if let Some(index)=self.choose(s.h(id),&LABELS.iter().map(|v|v.to_string()).collect::<Vec<_>>()){s.preset.set(index);self.layout();}}
            OPTIONS=>{let options=[("result","Show last rewrite"),("status","Check sign-in"),("login","Sign in"),("models","List models"),("settings","Rewrite settings")];
                if let Some(i)=self.choose(s.h(id),&options.iter().map(|v|v.1.to_string()).collect::<Vec<_>>()){let key=options[i].0;
                    if key=="settings"{self.navigate("settings")}else{self.run_action(key);if key=="result"{self.editor_rewrite_result(&self.detail());self.layout();}}}}
            SUBMIT if page=="rewrite"=>{let key=PRESETS[s.preset.get()];self.set_detail(&window_text(s.h(SOURCE),256*1024));self.set_input(&window_text(s.h(PRESET),1000));self.run_action(key);}
            COPY if page=="rewrite"=>{let result=window_text(s.h(RESULT),256*1024);if let Err(e)=workspace::copy_editor_text(self,&result){self.error(&e)}else{self.status("Rewrite copied.");self.layout();}}
            FILE=>self.run_action(if page=="ocr"{"file"}else{"import"}),
            PASTE=>self.run_action(if page=="ocr"{"clipboard"}else{"paste"}),
            COPY=>self.run_action("copy"),RESET=>self.run_action("clear"),CANCEL=>self.run_action("cancel"),
            OPEN=>self.run_action("markup"),SAVE=>self.run_action("save"),
            SUBMIT=>self.run_action(if page=="ocr"{"scan"}else{"ocr"}),_=>{}
        }
        true
    }
}

// The standard EDIT cue-banner message supports single-line inputs only.
// Draw the web placeholder after the multiline result's native client paint.
unsafe extern "system" fn empty_result_proc(h:Handle,msg:u32,wp:usize,lp:isize,id:usize,_:usize)->isize{
    if msg==WM_NCDESTROY{RemoveWindowSubclass(h,Some(empty_result_proc),id);}
    let result=DefSubclassProc(h,msg,wp,lp);
    if matches!(msg,WM_PAINT|WM_PRINT|WM_PRINTCLIENT)&&GetWindowTextLengthW(h)==0{
        let dc=if msg==WM_PAINT{GetDC(h)}else{wp as HDC};
        if !dc.is_null(){
            let color=GetPropW(h,wide("BuddyRewriteHintColor").as_ptr())as usize;
            let mut bounds=RECT::default();GetClientRect(h,&mut bounds);
            text(dc,bounds,"Your rewrite will appear here",SendMessageW(h,WM_GETFONT,0,0)as HFONT,
                color.saturating_sub(1)as u32,DT_LEFT|DT_TOP|DT_SINGLELINE);
            if msg==WM_PAINT{ReleaseDC(h,dc);}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;

    #[test]
    #[ignore="exercises native keyboard and device layouts on a private Windows desktop"]
    fn editor_actions_and_camera_controls_stay_reachable() {
        crate::workspace::with_private_test_desktop(|| unsafe {
            let name=wide("BuddyNativeEditorAndDeviceQA");
            let instance=windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(std::ptr::null());
            let class=WNDCLASSW {lpfnWndProc:Some(crate::window_proc),hInstance:instance,lpszClassName:name.as_ptr(),..Default::default()};
            RegisterClassW(&class);
            let hwnd=CreateWindowExW(0,name.as_ptr(),std::ptr::null(),WS_POPUP|WS_CLIPCHILDREN,0,0,1000,600,std::ptr::null_mut(),std::ptr::null_mut(),instance,std::ptr::null());
            assert!(!hwnd.is_null());
            let profile=std::env::temp_dir().join(format!("Buddy-native-device-editor-{}",std::process::id()));
            let mut ui=Ui::new(windows::Win32::Foundation::HWND(hwnd),profile,true).unwrap();
            ui.expanded=true;
            crate::WINDOW.with(|slot|*slot.borrow_mut()=Some(ui));
            let mut failures=Vec::new();
            for (zoom,width) in [(100,1920),(150,1024),(175,880)] {
                crate::with_ui(|ui|{
                    ui.zoom=zoom;ui.apply_appearance();
                    let work=RECT{left:0,top:0,right:width,bottom:960};
                    let layout=crate::island::Layout::new(work,crate::shell::dpi(hwnd),zoom,crate::island::Placement::Left,300,&[]);
                    let host=layout.host;
                    SetWindowPos(hwnd,std::ptr::null_mut(),host.left,host.top,host.right-host.left,host.bottom-host.top,SWP_NOZORDER|SWP_NOACTIVATE);
                    crate::island::configure(layout,true);crate::island::request(true,true);
                    ui.navigate("rewrite");
                    assert_eq!(IsWindowEnabled(GetDlgItem(hwnd,SUBMIT as i32)),0);
                    ui.navigate("ocr");ui.set_input("fixture.png");ui.set_detail("Recognized sample");ui.layout();
                    for (id,caption) in [(SUBMIT,"Rescan"),(COPY,"Copy text")] {
                        let h=GetDlgItem(hwnd,id as i32);
                        if IsWindowEnabled(h)==0 || window_text(h,100)!=caption {failures.push(format!("{zoom}% OCR {caption} disabled or named {}",window_text(h,100)));}
                    }
                    ui.navigate("studio");
                    let viewport=GetDlgItem(hwnd,3481);
                    let target=if viewport.is_null(){hwnd}else{viewport};
                    let value=GetDlgItem(target,3443);let more=GetDlgItem(target,3428);let apply=GetDlgItem(target,3441);
                    assert!(!value.is_null()&&!apply.is_null()&&!more.is_null());
                    let mut a=RECT::default();let mut b=RECT::default();let mut overlaps=RECT::default();GetWindowRect(value,&mut a);GetWindowRect(apply,&mut b);
                    if IntersectRect(&mut overlaps,&a,&b)!=0{failures.push(format!("{zoom}% studio value and Apply overlap"));}
                    let asset=GetDlgItem(target,3446);
                    if GetWindowLongW(asset,GWL_STYLE) as u32&WS_VISIBLE==0{failures.push(format!("{zoom}% studio Add image unreachable"));}
                    if !viewport.is_null(){
                        SendMessageW(viewport,WM_VSCROLL,SB_BOTTOM as usize,0);
                        let mut v=RECT::default();GetClientRect(viewport,&mut v);GetWindowRect(apply,&mut b);MapWindowPoints(std::ptr::null_mut(),viewport,(&mut b as *mut RECT).cast(),2);
                        if b.left<0||b.right>v.right||b.top<0||b.bottom>v.bottom{failures.push(format!("{zoom}% studio footer clipped after scrolling"));}
                    }
                });
            }
            DestroyWindow(hwnd);crate::WINDOW.with(|slot|slot.borrow_mut().take());
            assert!(failures.is_empty(),"{}",failures.join("\n"));
        });
    }
}
