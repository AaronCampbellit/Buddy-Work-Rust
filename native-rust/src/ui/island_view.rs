use super::*;
use chrome::{fill,icon,inset,mix,panel,rect,text,Icon};
use windows_sys::Win32::{Foundation::RECT,UI::Controls::{DRAWITEMSTRUCT,ODT_BUTTON,ODT_LISTBOX,ODS_FOCUS,ODS_NOFOCUSRECT,ODS_SELECTED}};

pub(super) const TAB:u16=170;
pub(super) const HEAD:u16=180;
pub(super) const COLLAPSE:u16=184;
pub(super) const HOME_PICK:u16=185;
pub(super) const SEARCH:u16=186;
pub(super) const TABS:[(&str,&str,Icon);4]=[("home","Home",Icon::Home),("quick","Quick Access",Icon::Quick),("clipboard","Clipboard",Icon::Clipboard),("notes","Notes",Icon::Note)];
pub(super) const HEADER:[(&str,&str,Icon);4]=[("notifications","Recent notifications",Icon::Bell),("widgets","Widgets",Icon::Widgets),("audio","Devices",Icon::Devices),("settings","Settings",Icon::Settings)];
const CENTER:u32=DT_CENTER|DT_VCENTER|DT_SINGLELINE;
const LEFT:u32=DT_LEFT|DT_VCENTER|DT_SINGLELINE;

pub(super) fn fonts(scale:f32)->Vec<HFONT>{
    [(11,400),(11,650),(18,650),(10,400),(14,650),(15,650),(16,650),(17,650),(9,400),(22,660),(9,650),(11,580)].into_iter().map(|(size,weight)|unsafe{
        let mut lf=LOGFONTW::default();lf.lfHeight=-(size as f32*scale).round() as i32;
        lf.lfWeight=weight;lf.lfQuality=ANTIALIASED_QUALITY;
        let family=wide("Segoe UI Variable Text");lf.lfFaceName[..family.len()].copy_from_slice(&family);CreateFontIndirectW(&lf)
    }).collect()
}

impl Ui {
    pub(super) fn scale(&self)->f32 {crate::shell::dpi(self.hwnd.0) as f32/96.0*self.zoom as f32/100.0}
    pub(super) fn px(&self,n:i32)->i32 {(n as f32*self.scale()).round() as i32}
    fn home_record(&self)->Option<&Vec<String>>{
        let id=self.home_note.as_ref()?;self.records.get("notes")?.iter().find(|r|r.len()>=7&&r.first()==Some(id))
    }
    fn media_visible(&self)->bool {self.page=="home" && activity::home_media().is_some()}
    pub(super) fn panel_width(&self,w:i32)->i32 {if self.media_visible(){(w-self.px(10))*57/100}else{w}}

    // Use the actual button as the menu anchor for keyboard and pointer activation alike.
    pub(super) fn choose(&self,anchor:Handle,labels:&[String])->Option<usize>{unsafe{
        let menu=CreatePopupMenu();if menu.is_null(){return None}
        for (index,label) in labels.iter().enumerate(){AppendMenuW(menu,MF_STRING,index+1,wide(&label.replace('&',"&&")).as_ptr());}
        let mut r=RECT::default();GetWindowRect(anchor,&mut r);
        let choice=TrackPopupMenu(menu,TPM_RETURNCMD|TPM_NONOTIFY|TPM_LEFTALIGN,r.left,r.bottom,0,self.hwnd.0,std::ptr::null());
        DestroyMenu(menu);(choice>0).then(||(choice-1) as usize)
    }}
    fn related(&self)->&'static [&'static str] {match self.page{
        "quick"|"files"|"downloads"=>&["quick","files","downloads"],
        "clipboard"|"markup"|"ocr"|"rewrite"=>&["clipboard","markup","ocr","rewrite"],
        "audio"|"camera"|"studio"|"media"=>&["audio","camera","studio","media"],
        "settings"|"diagnostics"=>&["settings","diagnostics"],_=>&[]
    }}
    fn more_menu(&mut self){
        let actions=self.actions.iter().skip(PRIMARY).copied().collect::<Vec<_>>();
        let pages=self.related().iter().copied().filter(|p|*p!=self.page).collect::<Vec<_>>();
        let mut choices=actions.iter().map(|(_,s)|s.to_string()).collect::<Vec<_>>();
        choices.extend(pages.iter().filter_map(|page|PAGES.iter().find(|p|p.0==*page).map(|p|p.1.to_string())));
        if let Some(i)=self.choose(self.more_go,&choices){
            if let Some((key,_))=actions.get(i){self.run_action(key)}else if let Some(page)=pages.get(i-actions.len()){self.navigate(page)}
        }
    }
    pub(super) fn island_command(&mut self,id:u16,code:u16)->bool{
        if id==SEARCH && code==EN_CHANGE as u16 && self.page=="notes"{
            self.search=window_text(self.search_box,32768);core::note_rows(self);self.redraw();return true
        }
        if code!=BN_CLICKED as u16{return false}
        if let Some(tab)=id.checked_sub(TAB).and_then(|i|TABS.get(i as usize)){self.navigate(tab.0);return true}
        if let Some(action)=id.checked_sub(HEAD).and_then(|i|HEADER.get(i as usize)){if action.0=="notifications"{notification_popup::toggle(self)}else{self.navigate(action.0)}return true}
        match id{
            COLLAPSE=>crate::shell::expand(self,false),
            MORE_GO=>self.more_menu(),
            HOME_PICK=>{
                let mut choices=vec!["Notes".to_owned()];
                let notes=self.records.get("notes").into_iter().flatten().filter(|n|n.len()>=7).collect::<Vec<_>>();
                choices.extend(notes.iter().map(|r|if r[1].is_empty(){"Untitled note".to_owned()}else{r[1].clone()}));
                if let Some(i)=self.choose(self.home_pick,&choices){
                    let next=i.checked_sub(1).and_then(|i|notes.get(i)).map(|r|r[0].clone());
                    let old=self.home_note.clone();core::set(self,"homeNote",next.as_deref().unwrap_or(""));
                    match self.save(){Ok(())=>self.home_note=next,Err(e)=>{core::set(self,"homeNote",old.as_deref().unwrap_or(""));self.error(&e);}}self.layout();
                }
            }
            ACTION if self.page=="home"=>{
                if let Some(id)=self.home_record().map(|r|r[0].clone()){self.active_note=Some(id);}
                self.navigate("notes");
            }
            _=>return false
        }
        true
    }
    fn redraw(&self){unsafe{RedrawWindow(self.hwnd.0,std::ptr::null(),std::ptr::null_mut(),RDW_INVALIDATE|RDW_ALLCHILDREN);}}
    fn body(&self)->RECT{
        let body=crate::island::frame().body;
        if body.right>body.left && body.bottom>body.top{return body}
        let mut r=RECT::default();unsafe{GetClientRect(self.hwnd.0,&mut r);}r
    }
    pub(crate) fn layout_motion_header(&self){
        let frame=crate::island::frame();
        let body=self.body();let p=|n|self.px(n);
        let surface_w=body.right-body.left;let surface_h=body.bottom-body.top;
        let open=self.expanded||frame.animating;
        let place=|handle,on,x,y,w,h|{
            if on{pos(handle,body.left+x,body.top+y,w,h);}
            show(handle,on);
        };
        place(self.compact,!open,0,0,surface_w,surface_h);
        place(self.compact_tools,!open,surface_w-p(45),p(3),p(29),(surface_h-p(7)).min(p(34)));
        let progress=frame.header_progress.clamp(0.,1.);
        let header_pad=p(18+(12.0*progress).round() as i32);
        for (i,tool)in self.header.iter().enumerate(){
            let on=self.expanded || (frame.animating && i==0);
            let offset=if self.expanded{p(37)*(3-i as i32)}else{0};
            place(*tool,on,surface_w-header_pad-p(30)-offset,p(5+(3.0*progress).round() as i32),p(30),p(30));
        }
    }
    pub(crate) fn layout(&self){
        let frame=crate::island::frame();let body=self.body();
        let p=|n|self.px(n);

        let content=if frame.animating{frame.expanded_body}else{body};
        let w=content.right-content.left;let h=content.bottom-content.top;
        let open=self.expanded || frame.animating;
        let home=open && self.page=="home";let notes=open && self.page=="notes";
        let custom=workspaces_page::handles(self.page) || editor_views::handles(self.page) || matches!(self.page,"widgets"|"settings"|"audio"|"camera"|"studio"|"notifications");
        let utility=open && !home && !notes && !custom;
        let place=|handle:Handle,on:bool,x:i32,y:i32,w:i32,h:i32|{
            let id=unsafe{GetDlgCtrlID(handle)};
            let origin=if workspace_child(id){content}else{body};
            if on {pos(handle,origin.left+x,origin.top+y,w,h);}
            show(handle,on);
        };
        self.layout_motion_header();
        let gap=p(4);let inner=w-p(60);let tw=((inner-3*gap)/4).min(p(154));let start=(w-(tw*4+gap*3))/2;
        let configured=core::setting(self,"moduleOrder","");
        let mut order=Vec::with_capacity(4);
        for key in configured.split(',').map(str::trim).chain(TABS.iter().map(|t|t.0)){
            if let Some(i)=TABS.iter().position(|t|t.0==key){if !order.contains(&i){order.push(i);}}
        }
        for(i,tab)in order.iter().enumerate(){place(self.tabs[*tab],open,start+i as i32*(tw+gap),p(51),tw,p(31));}
        place(self.resize,open && !frame.animating,(w-p(70))/2,h-p(15),p(70),p(15));
        place(self.collapse,open,w-p(28),p(46),p(22),p(26));
        let left=p(30);let top=p(88);let bottom=h-p(17);let main_h=bottom-top;
        let widgets_visible=open && self.page=="widgets";
        let widget_bounds=rect(content.left+left,content.top+top,inner,main_h);
        if widgets_visible{self.widgets_page().layout(self,true,widget_bounds);}
        else if let Some(widgets)=self.widgets_page.get(){widgets.layout(self,false,widget_bounds);}
        self.layout_home_media(home,widget_bounds);
        let panel_width=self.panel_width(inner);let note_w=panel_width-p(20);
        place(self.home_pick,home,left+p(10),top+p(12),note_w,p(27));
        let selected_home=self.home_record().is_some();
        let (x,y,bw,bh)=if selected_home {(left+p(10),top+p(48),note_w,main_h-p(58))}
            else{(left+(panel_width-p(101))/2,(top+p(39)+bottom-p(28))/2-p(2),p(101),p(28))};
        let textbox=|ctrl,on,x,y,w,h|{let n=p(3);place(ctrl,on,x+n,y+n,w-2*n,h-2*n);};
        if !notes{
            place(self.buttons[0],home||utility,if home{x}else{left},if home{y}else{bottom-p(28)},if home{bw}else{(inner-p(56))/4-p(4)},if home{bh}else{p(29)});
            for i in 1..PRIMARY {
                {place(self.buttons[i],utility && self.actions.len()>i,left+i as i32*((inner-p(56))/4),bottom-p(28),(inner-p(56))/4-p(4),p(28));}
            }
            place(self.more_go,utility && (self.actions.len()>PRIMARY || !self.related().is_empty()),w-p(65),bottom-p(28),p(34),p(27));
            show(self.search_box,false);
            let list_w=inner*41/100;
            if utility{unsafe{SendMessageW(self.list,LB_SETITEMHEIGHT,0,p(28) as _);}}
            place(self.list,utility,left,top+p(29),list_w,main_h-p(65));
            let ux=left+list_w+p(12);let uw=inner-list_w-p(12);
            textbox(self.input_box,utility,ux,top+p(22),uw,p(26));
            let dy=top+p(71);let dh=main_h-p(108);
            let has_images=self.images.layout(self.page,utility,(content.left+ux,content.top+dy,uw,dh),self.zoom,self.appearance);
            textbox(self.detail_box,utility && !has_images,ux,dy,uw,dh);
        }
        let list_w=inner*41/100;let ux=left+list_w+p(12);let uw=inner-list_w-p(12);
        place(self.title,utility,left,top,list_w,p(24));place(self.input_label,utility,ux,top,uw,p(19));
        place(self.detail_label,utility,ux,top+p(50),uw,p(18));
        show(self.more,false);show(self.nav,false);
        self.layout_notes(notes,content);
        self.layout_workspaces(open && workspaces_page::handles(self.page),widget_bounds);
        self.layout_editors(open && editor_views::handles(self.page),widget_bounds);
        self.layout_preferences(open && self.page=="settings",widget_bounds);
        self.layout_device_view(open && matches!(self.page,"audio"|"camera"|"studio"|"notifications"),widget_bounds);
        let issue=self.read_error.is_some() || unsafe{!GetPropW(self.notice,wide("BuddyIssue").as_ptr()).is_null()};
        place(self.notice,open&&issue,left,bottom-p(18),inner-p(45),p(16));
        if !frame.animating {
            let mut focus_order=Vec::new();
            if open {
                focus_order.push(self.resize);
                focus_order.extend(self.header.iter().copied());
                focus_order.extend(order.iter().map(|i|self.tabs[*i]));
                match self.page {
                    "home"=>{focus_order.extend([self.home_pick,self.buttons[0]]);focus_order.extend(self.home_media_tabs());}
                    "notes"=>{focus_order.extend(self.note_tab_stops());}
                    page if editor_views::handles(page)=>focus_order.extend(self.editor_tab_stops()),
                    "settings"=>focus_order.extend(self.preference_tab_stops()),
                    "audio"|"camera"|"studio"|"notifications"=>focus_order.extend(self.device_tab_stops()),
                    page if workspaces_page::handles(page)=>{focus_order.push(unsafe{GetDlgItem(self.hwnd.0,2200)});}
                    "widgets"=>{focus_order.push(unsafe{GetDlgItem(self.hwnd.0,1900)});}
                    _=>{focus_order.extend([self.list,self.input_box,self.detail_box,unsafe{GetDlgItem(self.hwnd.0,1561)}]);
                        focus_order.extend(self.buttons.iter().copied());focus_order.push(self.more_go);}
                }
                focus_order.push(self.collapse);
            }else{focus_order.extend([self.compact_tools,self.compact]);}
            focus_order.retain(|hwnd|!hwnd.is_null() && unsafe{GetWindowLongW(*hwnd,GWL_STYLE) as u32&WS_VISIBLE!=0});
            if *self.tab_order.borrow()!=focus_order{
                // IsDialogMessage follows sibling Z order, including Shift+Tab in reverse.
                unsafe{
                    let mut previous=HWND_TOP;
                    for &hwnd in &focus_order{
                        SetWindowPos(hwnd,previous,0,0,0,0,SWP_NOACTIVATE|SWP_NOMOVE|SWP_NOSIZE|SWP_NOREDRAW);
                        previous=hwnd;
                    }
                }
                self.tab_order.replace(focus_order);
            }
        }
        crate::workspace_layout_changed(self.hwnd.0);
        self.redraw();
    }

    pub(crate) fn paint(&self,dc:HDC){
        self.paint_surface(dc);
        if self.expanded{self.paint_workspace(dc);}
    }
    pub(crate) fn paint_surface(&self,dc:HDC){
        let r=self.body();let (bg,_,ink,_)=self.appearance;let p=|n|self.px(n);
        let w=r.right-r.left;let h=r.bottom-r.top;
        let box_at=|x,y,w,h| rect(r.left+x,r.top+y,w,h);
        let frame=crate::island::frame();let progress=frame.progress.clamp(0.0,1.0);
        let edge=chrome::divider(self.appearance);let muted=chrome::muted(self.appearance);
        fill(dc,r,bg);
        let rad=p((20.0+6.0*progress) as i32);
        panel(dc,rect(r.left,r.top-rad,w,h+rad),rad,bg,edge);
        if !self.expanded && !frame.animating{return}
        panel(dc,box_at((w-p(44))/2,h-p(8),p(44),p(3)),p(2),mix(bg,muted,150),mix(bg,muted,150));
        let clock=crate::shell::clock(core::setting(self,"center","time"),core::setting(self,"custom","Buddy"));
        let mut center=clock;let mut left_status=None;
        for (run,label,timer) in [("pomEnd","Pomodoro",core::timers(self).0),("watchStart","Stopwatch",core::timers(self).1)]{
            if core::setting(self,run,"0")!="0" {let sec=timer/1000;center=format!("{:02}:{:02}",sec/60,sec%60);left_status=Some((Icon::Timer,label.to_owned()));break}
        }
        if left_status.is_none(){left_status=activity::home_media().map(|m|(Icon::Music,m));}
        let progress=frame.header_progress.clamp(0.,1.);
        let header_height=p(39+(7.0*progress).round() as i32);
        let header_font=[4,5,6,7,2][(4.0*progress).round() as usize];
        text(dc,box_at(w/3,0,w/3,header_height),&center,self.fonts[header_font],ink,CENTER);
        if let Some((glyph,s))=left_status{
            let pad=p(18+(12.0*progress).round() as i32);
            icon(dc,r.left+pad,r.top+(header_height-p(16))/2,p(16),glyph,muted,bg);
            text(dc,box_at(pad+p(24),0,w/3-pad-p(25),header_height),&s,self.fonts[0],muted,LEFT);
        }
    }
    pub(crate) fn paint_workspace(&self,dc:HDC){
        let f=crate::island::frame();let r=if f.animating{f.expanded_body}else{self.body()};
        let p=|n|self.px(n);
        let (w,h)=(r.right-r.left,r.bottom-r.top);
        let box_at=|x,y,w,h|rect(r.left+x,r.top+y,w,h);
        let (_,field,_,_)=self.appearance;let edge=chrome::divider(self.appearance);
        let inner=w-p(60);let top=p(88);let bottom=h-p(17);
        fill(dc,box_at(p(30),p(46),inner,p(1).max(1)),edge);
        if self.page=="widgets" {if let Some(widgets)=self.widgets_page.get(){widgets.paint(self,dc,box_at(p(30),top,inner,bottom-top));}}
        else if self.page=="home" {
            let panel_w=self.panel_width(inner);
            panel(dc,box_at(p(30),top+p(2),panel_w,bottom-top-p(2)),p(10),field,edge);
            self.paint_home_media(dc,box_at(p(30),top,inner,bottom-top));
        } else if self.page=="notes" { self.paint_notes(dc,r); }
        else if editor_views::handles(self.page){self.paint_editors(dc,box_at(p(30),top,inner,bottom-top));}
        else if self.page=="settings" { self.paint_preferences(dc,box_at(p(30),top,inner,bottom-top)); }
        else if matches!(self.page,"audio"|"camera"|"studio"|"notifications"){self.paint_device_view(dc,box_at(p(30),top,inner,bottom-top));}

    }

    pub(crate) fn draw_item(&self,lp:isize)->bool{
        if lp==0{return false}
        let di=unsafe{&*(lp as *const DRAWITEMSTRUCT)};
        if self.draw_home_media(di) || self.draw_note_item(di) || self.draw_editor_button(di) || self.draw_preference_item(di) || self.draw_device_item(di){return true}
        if self.widgets_page.get().is_some_and(|widgets|widgets.draw(self,di)){return true}
        let (bg,field,ink,accent)=self.appearance;
        let id=di.CtlID as u16;let r=di.rcItem;let dc=di.hDC;let p=|n|self.px(n);
        let muted=chrome::muted(self.appearance);let edge=mix(bg,ink,43);let control=chrome::control(self.appearance);
        let focus=chrome::visible_focus() && di.itemState&ODS_FOCUS!=0 && di.itemState&ODS_NOFOCUSRECT==0;
        if di.CtlType==ODT_LISTBOX && id==LIST {
            let selected=di.itemState&ODS_SELECTED!=0;
            let base=if selected{mix(bg,accent,27)}else{bg};
            fill(dc,r,bg);
            if di.itemID==u32::MAX{return true}
            let i=di.itemID as usize;let is_note=self.page=="notes";
            panel(dc,inset(r,p(1)),p(7),base,if focus{accent}else if selected{mix(bg,accent,108)}else{bg});
            let mut at=r.left+p(10);
            if self.page=="clipboard"{
                let thumb=windows::Win32::Foundation::RECT{left:at,top:r.top+p(3),right:at+p(46),bottom:r.bottom-p(3)};
                if inline_images::clipboard_thumb(self,i,windows::Win32::Graphics::Gdi::HDC(dc),thumb){at+=p(52);}
            }
            let mut name=String::new();
            unsafe{
                let n=SendMessageW(self.list,LB_GETTEXTLEN,i,0);
                if n>=0 && n<1024*1024{let mut s=vec![0u16;n as usize+1];if SendMessageW(self.list,LB_GETTEXT,i,s.as_mut_ptr() as _)>=0{name=String::from_utf16_lossy(&s[..n as usize]);}}
            }
            let split=is_note && self.note_ids.get(i).is_some();
            let first=RECT{left:at,right:r.right-p(6),top:r.top+p(2),bottom:if split{(r.top+r.bottom)/2+p(2)}else{r.bottom}};
            text(dc,first,&name,self.fonts[if is_note{1}else{0}],ink,LEFT);
            if split{
                if let Some(note)=self.note_ids.get(i).and_then(|id|self.records.get("notes")?.iter().find(|r|r.len()>=7&&r.first()==Some(id))){
                    text(dc,RECT{top:first.bottom,bottom:r.bottom-p(2),..first},note.get(2).and_then(|s|s.lines().next()).unwrap_or("Empty note"),self.fonts[3],muted,LEFT);
                }
            }
            return true
        }
        if di.CtlType!=ODT_BUTTON{return false}
        let hot=chrome::hovered(di.hwndItem)||di.itemState&ODS_SELECTED!=0;
        let tw=r.right-r.left;let th=r.bottom-r.top;
        let mut name=window_text(di.hwndItem,8192);
        let mut canvas=bg;let mut face;let mut line;let mut color=ink;
        let mut font=self.fonts[0];let mut glyph=None;let mut size=p(16);let mut left=false;
        let mut icon_only=false;
        if id==RESIZE_HANDLE {
            fill(dc,r,bg);let color=if focus||hot{accent}else{mix(bg,muted,150)};
            panel(dc,rect(r.left+(tw-p(44))/2,r.top+th-p(8),p(44),p(3)),p(2),color,color);return true
        }
        if let Some(tab)=id.checked_sub(TAB).and_then(|i|TABS.get(i as usize)){
            glyph=Some(tab.2);font=self.fonts[11];size=p(17);
            let active=tab.0==self.page || (tab.0=="quick" && matches!(self.page,"files"|"downloads")) || (tab.0=="clipboard" && matches!(self.page,"markup"|"ocr"|"rewrite"));
            face=if active{mix(bg,accent,25)}else if hot{mix(bg,ink,16)}else{bg};
            line=face;color=if active{accent}else{muted};
        }else if let Some(tool)=id.checked_sub(HEAD).and_then(|i|HEADER.get(i as usize)){
            icon_only=true;glyph=Some(tool.2);size=p(18);color=if tool.0=="notifications"{accent}else{muted};
            face=if hot||self.page==tool.0{mix(bg,ink,16)}else{bg};line=face;
        }else if id==COMPACT_TOOLS || id==COLLAPSE{
            icon_only=true;glyph=Some(if id==COLLAPSE{Icon::Up}else{Icon::Bell});size=if id==COLLAPSE{p(17)}else{p(18)};
            color=if id==COLLAPSE{muted}else{accent};face=if hot{mix(bg,ink,16)}else{bg};line=face;
        }else if id==COMPACT{
            // The compact body lies under the bell. Preserve its shaped outline when printing children.
            let rad=p(20);fill(dc,r,bg);panel(dc,rect(r.left,r.top-rad,tw,th+rad),rad,bg,edge);
            let display=if self.paused{"Buddy paused".to_owned()} else if let Some(msg)=activity::compact_preview(){msg}
                else if let Some(msg)=core::due_title(self){format!("Reminder · {msg}")}else{
                    let (pom,watch)=core::timers(self);
                    if core::setting(self,"pomEnd","0")!="0"{format!("{:02}:{:02}",pom/60000,pom/1000%60)}
                    else if core::setting(self,"watchStart","0")!="0"{format!("{:02}:{:02}",watch/60000,watch/1000%60)}
                    else{crate::shell::clock(core::setting(self,"center","time"),core::setting(self,"custom","Buddy"))}
                };
            let transient=if core::setting(self,"pomEnd","0")!="0"{Some((Icon::Timer,"Pomodoro".to_string()))}
                else if core::setting(self,"watchStart","0")!="0"{Some((Icon::Timer,"Stopwatch".to_string()))}
                else{activity::home_media().map(|m|(Icon::Music,m))};
            if let Some((glyph,s))=transient{
                icon(dc,r.left+p(18),r.top+(th-p(16))/2,p(16),glyph,muted,bg);
                text(dc,rect(r.left+p(40),r.top,tw/3-p(40),th),&s,self.fonts[3],muted,LEFT);
            }
            text(dc,rect(r.left+tw/4,r.top,tw/2,th),&display,self.fonts[4],ink,CENTER);
            if focus{let mut ring=inset(r,p(5));unsafe{DrawFocusRect(dc,&mut ring);}}
            return true
        }else if id==HOME_PICK{
            canvas=field;face=control;line=edge;left=true;font=self.fonts[1];
            name=self.home_record().and_then(|r|r.get(1)).filter(|s|!s.is_empty()).cloned().unwrap_or_else(||"Notes".into());
        }else if id==MORE_GO{
            icon_only=true;glyph=Some(Icon::More);face=control;line=edge;
        }else if id>=ACTION && id<ACTION+PRIMARY as u16{
            if self.page=="home"{canvas=field}
            if self.page=="home" && id==ACTION {
                if let Some(note)=self.home_record(){
                    let face=mix(field,accent,if hot{36}else{22});panel(dc,r,p(8),face,if focus{accent}else{mix(field,accent,100)});
                    icon(dc,r.left+p(10),r.top+p(9),p(17),Icon::Note,accent,face);
                    text(dc,rect(r.left+p(32),r.top+p(6),tw-p(65),p(24)),if note[1].is_empty(){"Untitled note"}else{&note[1]},self.fonts[1],ink,LEFT);
                    icon(dc,r.right-p(25),r.top+p(10),p(13),Icon::External,muted,face);
                    text(dc,rect(r.left+p(10),r.top+p(35),tw-p(20),th-p(62)),if note[2].is_empty(){"Empty note"}else{&note[2]},self.fonts[3],muted,DT_WORDBREAK);
                    let stamp=notes_view::note_date(&note[6]);
                    text(dc,rect(r.left+p(10),r.bottom-p(21),tw-p(80),p(15)),&format!("Updated {stamp}"),self.fonts[8],muted,LEFT);
                    let images=note.len().saturating_sub(7);if images>0{text(dc,rect(r.right-p(80),r.bottom-p(21),p(70),p(15)),&format!("{images} image{}",if images==1{""}else{"s"}),self.fonts[8],muted,DT_RIGHT|DT_VCENTER|DT_SINGLELINE);}
                    return true
                }
                glyph=Some(Icon::Note);
            }else if self.page=="home" && id==ACTION+2 {glyph=Some(Icon::Music);name=activity::home_media().unwrap_or_else(||"Media controls".into());}
            else if self.page=="notes" {
                glyph=Some(match id-ACTION{0=>Icon::Plus,1=>Icon::Save,2=>Icon::Pin,_=>Icon::Clock});
            }
            if self.page=="notes" && id<=ACTION+1 {face=accent;line=accent;color=if (accent&255)*299+((accent>>8)&255)*587+((accent>>16)&255)*114>146000 {0x171307}else{0xffffff};font=self.fonts[1];}
            else {face=control;line=edge;}
        }else{return false}
        if hot && face!=bg {face=mix(face,ink,12);}
        if focus{line=accent;}
        fill(dc,r,canvas);panel(dc,r,p(if id>=TAB && id<TAB+4{8}else{6}),face,line);
        if id==HOME_PICK {icon(dc,r.right-p(18),r.top+(th-p(14))/2,p(14),Icon::Down,muted,face);}
        if icon_only {if let Some(g)=glyph{icon(dc,r.left+(tw-size)/2,r.top+(th-size)/2,size,g,color,face);}return true}
        let text_width=chrome::width(dc,&name,font);
        let block=(text_width+if glyph.is_some(){size+p(6)}else{0}).min(tw-p(8));
        let mut x=r.left+if left{p(10)}else{(tw-block)/2};
        if let Some(g)=glyph{icon(dc,x,r.top+(th-size)/2,size,g,color,face);x+=size+p(6);}
        text(dc,RECT{left:x,right:r.right-if left{p(23)}else{p(3)},..r},&name,font,color,LEFT);
        true
    }
}
