use crate::{shell,Ui};
use std::time::{SystemTime,UNIX_EPOCH};
pub fn now()->u64{SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64}
pub fn setting<'a>(ui:&'a Ui,key:&str,default:&'a str)->&'a str{ui.records.get("settings").into_iter().flatten().find(|r|r.first().is_some_and(|s|s==key)).and_then(|r|r.get(1)).map(String::as_str).unwrap_or(default)}
pub fn set(ui:&mut Ui,key:&str,value:&str){let rows=ui.records.entry("settings".into()).or_default();rows.retain(|r|r.first().is_none_or(|s|s!=key));rows.push(vec![key.to_string(),value.to_string()]);}
fn show(v:&[String],i:usize)->&str{v.get(i).map(String::as_str).unwrap_or_default()}
fn duration(ms:u64)->String{let s=ms/1000;format!("{:02}:{:02}:{:02}",s/3600,(s/60)%60,s%60)}
const OPTIONS:&[(&str,&str,&str,&str)]=&[
    ("open","Open island","hover","click · hover"),("display","Screen","primary","primary, or a display name from the list"),
    ("position","Screen position","top-center","top-left · top-center · top-right"),("scale","Interface scale","100","75–175 percent"),
    ("center","Compact display","time","time · date · time-date · custom"),("custom","Compact text","Buddy","Up to 80 characters"),
    ("theme","Appearance","system","system · light · dark"),("accent","Color theme","cyan","cyan · ocean · violet · rose · forest · amber"),
    ("customAccent","Custom accent","","#RRGGBB or empty"),("customMain","Custom main color","","#RRGGBB or empty"),
    ("customField","Custom field color","","#RRGGBB or empty"),("customTextColor","Custom text color","","#RRGGBB or empty"),
    ("moduleOrder","Workspace order","","Ordered workspace IDs, separated by commas"),
    ("widgets","Compact widgets","on","on · off"),("pomWidget","Compact focus timer","auto","auto · on · off"),("watchWidget","Compact stopwatch","auto","auto · on · off"),("fullscreen","Hide in full screen","on","on · off"),
    ("exclude","Hide for applications","","Executable names separated by commas"),("hide","Temporary hide","5","Seconds from 1 to 600"),("retention","Remember open workspace","15","Seconds from 0 to 600"),
    ("autostart","Start at sign-in","off","on · off"),("motion","Reduced motion","off","on · off"),
    ("notificationPreview","Notification previews","on","on · off"),("notificationPrivacy","Notification privacy","title","full · title · icon"),
    ("notificationGrouping","Group notifications","on","on · off"),("notificationDuration","Preview duration","12","Seconds from 10 to 30")];
pub fn actions(page:&str)->Vec<(&'static str,&'static str)>{match page{
    "home"=>vec![("notes","Open Notes"),("quick","Quick Access"),("media","Media controls"),("widgets","Timers")],
    "notes"=>vec![("new","New note"),("save","Save"),("pin","Pin / unpin"),("remind","Remind in 5 min"),("search","Search title and body"),("clear-search","Clear search"),("schedule","Schedule date and time"),("remind-custom","Set reminder in minutes"),("dismiss","Dismiss reminder"),("snooze","Snooze 5 minutes"),("attach","Attach clipboard image"),("image","View attached image"),("detach","Remove attached image"),("widget","Show as widget"),("copy","Copy note"),("delete","Delete note"),("undo","Undo deletion"),("discard","Discard unsaved edits")],
    "widgets"=>vec![("pom","Start / pause focus"),("watch","Start / pause watch"),("lap","Record lap"),("reset","Reset stopwatch"),("focus","Focus phase"),("short","Short break"),("long","Long break"),("minutes","Set focus minutes"),("clear","Clear note widget")],
    "settings"=>vec![("apply","Apply value"),("cycle","Cycle selection"),("display","Next display"),("compact","Collapse island"),("hide","Hide temporarily"),("defaults","Reset settings")],_=>vec![]}}
fn note_index(ui:&Ui)->Option<usize>{let id=ui.active_note.as_ref()?;ui.records.get("notes")?.iter().position(|r|r.len()>=7 && r.first()==Some(id))}
fn fill_note(ui:&mut Ui){
    if let Some(n)=note_index(ui).and_then(|i|ui.records.get("notes").and_then(|r|r.get(i))){ui.set_input(show(n,1));if let Err(e)=crate::inline_images::load_note(ui,n){ui.status(&e);}}
    else{ui.set_input("");ui.set_detail("");}
    ui.dirty_note=false;
}
pub(crate) fn note_rows(ui:&mut Ui){
    let q=ui.search.to_lowercase();
    let mut notes=ui.records.get("notes").into_iter().flatten().filter(|r|r.len()>=7).filter(|r|q.is_empty()||show(r,1).to_lowercase().contains(&q)||show(r,2).to_lowercase().contains(&q)).collect::<Vec<_>>();
    notes.sort_by(|a,b|show(b,3).cmp(show(a,3)).then_with(||show(b,6).cmp(show(a,6))));
    let rows=notes.iter().map(|n|format!("{}{}{}",if show(n,3)=="1"{"★  "}else{""},if show(n,1).is_empty(){"Untitled note"}else{show(n,1)},if show(n,5).is_empty(){""}else{"   ⏱"})).collect::<Vec<_>>();
    ui.note_ids=notes.iter().map(|n|n[0].clone()).collect();ui.set_rows(&rows);
    if let Some(i)=ui.active_note.as_ref().and_then(|id|ui.note_ids.iter().position(|s|s==id)){ui.select_row(i);}
}
pub fn save_note(ui:&mut Ui)->Result<(),String>{
    let i=note_index(ui).ok_or("Select a note first.")?;
    let title=ui.input();
    let (body,images)=ui.images.contents()?;
    let format=ui.images.format()?;
    if let Err(reason)=write_note(ui,i,title,body,&images,format){
        ui.dirty_note=true;
        return Err(format!("Note still open with unsaved edits. {reason}"));
    }
    ui.dirty_note=false;
    ui.new_note=false;
    note_rows(ui);
    Ok(())
}
pub(crate) fn write_note(ui:&mut Ui,i:usize,title:String,body:String,images:&[crate::inline_images::Placement],format:Option<String>)->Result<(),String>{
    let previous=ui.records["notes"][i].clone();
    let positions=crate::inline_images::offset_key(&previous[0]);
    let format_key=crate::inline_images::format_key(&previous[0]);
    let old_positions=ui.records.remove(&positions);
    let old_format=ui.records.remove(&format_key);
    let row=&mut ui.records.get_mut("notes").unwrap()[i];
    row[1]=title;row[2]=body;row[6]=now().to_string();row.truncate(7);
    row.extend(images.iter().map(|item|item.id.clone()));
    if !images.is_empty(){ui.records.insert(positions.clone(),vec![images.iter().map(|p|p.offset.to_string()).collect()]);}
    if let Some(rtf)=format{ui.records.insert(format_key.clone(),vec![vec![rtf]]);}
    if let Err(error)=ui.save(){
        ui.records.get_mut("notes").unwrap()[i]=previous;
        for (key,old) in [(positions,old_positions),(format_key,old_format)]{
            if let Some(value)=old{ui.records.insert(key,value);}else{ui.records.remove(&key);}
        }
        return Err(error);
    }Ok(())
}
pub(crate) struct DeletedNote {
    pub(crate) row: Vec<String>,
    metadata: crate::storage::Records,
}
pub(crate) fn delete_note(ui: &mut Ui, index: usize) -> Result<(), String> {
    let row = ui.records.get_mut("notes").unwrap().remove(index);
    let keys = [crate::inline_images::format_key(&row[0]), crate::inline_images::offset_key(&row[0])];
    let metadata = keys.into_iter().filter_map(|key| ui.records.remove(&key).map(|value| (key,value))).collect();
    if let Err(error) = ui.save() {
        ui.records.get_mut("notes").unwrap().insert(index, row);
        ui.records.extend(metadata);
        return Err(error);
    }
    ui.last_removed = Some(DeletedNote { row, metadata });
    Ok(())
}
fn restore_deleted_note(ui: &mut Ui) -> Result<(), String> {
    let deleted = ui.last_removed.take().ok_or("No recently deleted note.")?;
    let id = deleted.row[0].clone();
    ui.records.entry("notes".into()).or_default().push(deleted.row.clone());
    ui.records.extend(deleted.metadata.clone());
    if let Err(error) = ui.save() {
        ui.records.get_mut("notes").unwrap().pop();
        for key in deleted.metadata.keys() { ui.records.remove(key); }
        ui.last_removed = Some(deleted);
        return Err(error);
    }
    ui.active_note = Some(id);
    note_rows(ui);
    fill_note(ui);
    Ok(())
}
fn note_field(ui:&mut Ui,i:usize,value:String)->Result<(),String>{
    let idx=note_index(ui).ok_or("Select a note first.")?;
    let old=std::mem::replace(&mut ui.records.get_mut("notes").unwrap()[idx][i],value);
    if let Err(err)=ui.save(){ui.records.get_mut("notes").unwrap()[idx][i]=old;return Err(err)}
    note_rows(ui);Ok(())
}
fn discard_note(ui:&mut Ui){
    if ui.new_note {
        if let Some(i)=note_index(ui){ui.records.get_mut("notes").unwrap().remove(i);}
        ui.active_note=None;
    }
    ui.new_note=false;
    note_rows(ui);
    fill_note(ui);
}
fn attachment_fields(note: &[String]) -> &[String] {
    note.get(7..).unwrap_or_default()
}
fn attachment_id(note: &[String], selection: usize) -> Result<&str, String> {
    attachment_fields(note).get(selection).map(String::as_str).ok_or_else(|| "The attachment is no longer available.".into())
}
fn attachment_labels(ui: &Ui, note: &[String]) -> Vec<String> {
    attachment_fields(note).iter().enumerate().map(|(index, id)| {
        let dimensions = ui.records.get("clipboard").into_iter().flatten()
            .find(|r| show(r,0)=="image" && show(r,1)==id && r.len()>5)
            .map(|row| format!(" · {} × {}", row[4],row[5])).unwrap_or_default();
        let short: String = id.chars().take(8).collect();
        format!("Image {}{}   {}", index+1, dimensions, short)
    }).collect()
}
#[cfg(test)]
fn remove_attachment(ui:&mut Ui, note:usize, selection:usize)->Result<(),String>{
    attachment_id(&ui.records["notes"][note],selection)?;
    let position=selection+7;
    let id=ui.records.get_mut("notes").unwrap()[note].remove(position);
    if let Err(error)=ui.save(){
        ui.records.get_mut("notes").unwrap()[note].insert(position,id);
        return Err(error);
    }
    Ok(())
}
fn attached_image(ui:&mut Ui,note:usize,remove:bool)->Result<String,String>{
    let choices=attachment_labels(ui,&ui.records["notes"][note]);
    if choices.is_empty(){return Err("This note has no attached images.".into())}
    let choice=if let Some((index,_))=crate::inline_images::selected_note_image(ui){index}else if choices.len()==1{0}else{
        let title=if remove{"Remove an attached image"}else{"View an attached image"};
        let Some(i)=shell::choice(ui,title,"Choose an image in this note",&choices) else{return Ok(String::new())};
        i
    };
    if remove {
        if !shell::confirm(ui,&format!("Remove {} from this note?",choices[choice])) {return Ok(String::new())}
        crate::inline_images::remove_note_image(ui,choice)?;
        Ok("Image removed from the note.".into())
    }else{
        let id=attachment_id(&ui.records["notes"][note],choice)?.to_owned();
        crate::workspace::view_image(ui,&id)?;
        Ok("Image opened.".into())
    }
}
fn edit_note(ui:&mut Ui,key:&str)->Result<String,String>{
    match key{
        "select"=>{let id=ui.selected().and_then(|i|ui.note_ids.get(i)).cloned();if id!=ui.active_note{if ui.dirty_note{if let Err(reason)=save_note(ui){if let Some(old)=ui.active_note.as_ref().and_then(|id|ui.note_ids.iter().position(|s|s==id)){ui.select_row(old);}return Err(reason)}}ui.active_note=id;ui.new_note=false;note_rows(ui);fill_note(ui);}Ok(String::new())},
        "new"=>{if ui.dirty_note {save_note(ui)?;}let list=ui.records.entry("notes".into()).or_default();if list.len()>=1000{return Err("The note limit is 1,000.".into())}
            let t=now().to_string();let id=format!("{}-{}",t,list.len());list.push(vec![id.clone(),String::new(),String::new(),"0".into(),t.clone(),String::new(),t]);ui.search.clear();ui.active_note=Some(id);note_rows(ui);fill_note(ui);ui.new_note=true;ui.dirty_note=true;shell::focus(ui.input_box);Ok("Type a title and note.".into())},
        "save"=>{save_note(ui)?;Ok("Note saved.".into())},
        "search"=>{if let Some(value)=shell::prompt(ui,"Search notes","Search title and body",&ui.search){ui.search=value;note_rows(ui);}Ok(format!("{} matching notes.",ui.note_ids.len()))},
        "clear-search"=>{ui.search.clear();note_rows(ui);Ok(String::new())},
        "discard"=>{
            if ui.dirty_note && shell::confirm(ui,"Discard unsaved changes to this note?") {
                discard_note(ui);
                return Ok("Unsaved changes discarded.".into());
            }
            Ok(String::new())
        },
        "undo"=>{if ui.dirty_note{save_note(ui)?;}restore_deleted_note(ui)?;Ok("Note restored.".into())},
        _=>{let i=note_index(ui).ok_or("Select a note first.")?;if ui.dirty_note{save_note(ui)?;}
            match key{
                "pin"=>{let pinned=ui.records["notes"][i][3]=="1";note_field(ui,3,if pinned{"0"}else{"1"}.into())?;},
                "schedule"=>{if let Some(t)=shell::schedule_reminder(ui){note_field(ui,5,t.to_string())?;ui.due_note=None;return Ok("Reminder scheduled.".into())}},
                "dismiss"=>{note_field(ui,5,String::new())?;ui.due_note=None;},
                "remind"|"snooze"|"remind-custom"=>{let minutes=if key=="remind-custom"{let Some(value)=shell::prompt(ui,"Set reminder","Minutes from now","15")else{return Ok(String::new())};value.trim().parse::<u64>().ok().filter(|v|(1..=525600).contains(v)).ok_or("Enter 1 to 525,600 minutes.")?}else{5};note_field(ui,5,(now()+minutes*60000).to_string())?;ui.due_note=None;return Ok(format!("Reminder set for {} minutes from now.",minutes));},
                "widget"=>{let id=ui.records["notes"][i][0].clone();set(ui,"noteWidget",&id);ui.save()?;},
                "copy"=>{shell::copy_text(ui,&format!("{}\r\n{}",ui.records["notes"][i][1],ui.records["notes"][i][2]))?;},
                "attach"=>{crate::inline_images::paste_note(ui)?;return Ok("Image inserted in the note.".into());},
                "image"|"detach"=>return attached_image(ui,i,key=="detach"),
                "delete"=>{if !shell::confirm(ui,"Delete the selected note?"){return Ok(String::new())}delete_note(ui,i)?;ui.active_note=None;note_rows(ui);fill_note(ui);},
                _=>return Ok(String::new())
            } Ok("Updated.".into())}}
}
fn val(ui:&Ui,k:&str,d:u64)->u64{setting(ui,k,"").parse().unwrap_or(d)}
fn timer_rows(ui:&Ui)->Vec<String>{
    let (remaining,watch)=timers(ui);let mut rows=vec![format!("{}   {}",setting(ui,"phase","Focus"),duration(remaining)),format!("Stopwatch   {}",duration(watch))];
    rows.extend(ui.records.get("laps").into_iter().flatten().filter_map(|r|r.first()).enumerate().map(|(i,s)|format!("Lap {}   {}",i+1,duration(s.parse().unwrap_or(0)))));
    rows
}
pub(crate) fn timers(ui:&Ui)->(u64,u64){let t=now();let end=val(ui,"pomEnd",0);let start=val(ui,"watchStart",0);(if end>0{end.saturating_sub(t)}else{val(ui,"pomLeft",25*60000)},val(ui,"watchElapsed",0)+if start>0{t.saturating_sub(start)}else{0})}
fn widget(ui:&mut Ui,key:&str)->Result<String,String>{
    let message=timer_action(ui,key)?;
    ui.set_rows(&timer_rows(ui));
    Ok(message)
}

fn advance_phase(ui:&mut Ui) {
    let was_focus=setting(ui,"phase","Focus")=="Focus";
    let sessions=val(ui,"pomSessions",0)+u64::from(was_focus);
    let (phase,minutes)=if !was_focus {("Focus",25)} else if sessions%4==0 {("Long break",15)} else {("Short break",5)};
    set(ui,"pomSessions",&sessions.to_string());
    set(ui,"phase",phase);
    set(ui,"pomEnd","0");
    set(ui,"pomDuration",&(minutes*60000).to_string());
    set(ui,"pomLeft",&(minutes*60000).to_string());
}

pub(crate) fn timer_action(ui:&mut Ui,key:&str)->Result<String,String>{
    let (left,elapsed)=timers(ui);
    let timestamp=now();
    let running=val(ui,"pomEnd",0)>0;
    let watching=val(ui,"watchStart",0)>0;
    // A card that first appeared automatically stays available after pausing its timer.
    let previous_settings=ui.records.get("settings").cloned();
    let previous_laps=ui.records.get("laps").cloned();
    let persist_focus=setting(ui,"pomWidget","auto")=="auto" && (running || key=="pom");
    let persist_watch=setting(ui,"watchWidget","auto")=="auto" && (watching || key=="watch");
    match key {
        "pom" if running=>{set(ui,"pomEnd","0");set(ui,"pomLeft",&left.to_string());}
        "pom"=>set(ui,"pomEnd",&(timestamp+left.max(1000)).to_string()),
        "pom-reset"=>{let fallback=match setting(ui,"phase","Focus"){"Short break"=>5,"Long break"=>15,_=>25}*60000;
            let duration=val(ui,"pomDuration",fallback);set(ui,"pomEnd","0");set(ui,"pomLeft",&duration.to_string());}
        "pom-skip"=>advance_phase(ui),
        "focus"|"short"|"long"|"minutes"|"focus50"=>{
            let (phase,minutes)=match key {"short"=>("Short break",5),"long"=>("Long break",15),"focus50"=>("Focus",50),
                "minutes"=>("Focus",ui.input().parse::<u64>().ok().filter(|v|(1..=240).contains(v)).ok_or("Enter a duration from 1 to 240 minutes.")?),_=>("Focus",25)};
            let duration=(minutes*60000).to_string();
            set(ui,"phase",phase);set(ui,"pomEnd","0");set(ui,"pomLeft",&duration);set(ui,"pomDuration",&duration);
        }
        "watch"=>{set(ui,"watchElapsed",&elapsed.to_string());set(ui,"watchStart",&(if watching{0}else{timestamp}).to_string());}
        "reset"=>{set(ui,"watchStart","0");set(ui,"watchElapsed","0");ui.records.remove("laps");}
        "lap"=>{let rows=ui.records.entry("laps".into()).or_default();if rows.len()<200{rows.push(vec![elapsed.to_string()]);}}
        "clear"=>set(ui,"noteWidget",""),
        _=>return Ok(String::new())
    }
    if persist_focus{set(ui,"pomWidget","on");}
    if persist_watch{set(ui,"watchWidget","on");}
    if let Err(error)=ui.save(){
        for (key,previous) in [("settings",previous_settings),("laps",previous_laps)]{
            if let Some(rows)=previous{ui.records.insert(key.to_owned(),rows);}else{ui.records.remove(key);}
        }
        return Err(error)
    }
    Ok("Widget updated.".into())
}
fn settings_rows(ui:&Ui){ui.set_rows(&OPTIONS.iter().map(|&(k,n,d,_)|format!("{}    {}",n,setting(ui,k,d))).collect::<Vec<_>>())}
fn settings(ui:&mut Ui,key:&str)->Result<String,String>{
    if key=="compact"{shell::expand(ui,false);return Ok(String::new())}
    if key=="hide"{shell::hide_temporarily(ui);return Ok(String::new())}
    if key=="defaults"{if !shell::confirm(ui,"Reset appearance and island preferences?"){return Ok(String::new())}for &(k,_,_,_) in OPTIONS{if k=="autostart"{shell::autostart(ui,false)?;}if let Some(s)=ui.records.get_mut("settings"){s.retain(|r|show(r,0)!=k);}}ui.save()?;ui.reorder_navigation();shell::position(ui);settings_rows(ui);return Ok("Settings reset.".into())}
    if key=="display"{let monitors=shell::screens();if monitors.is_empty(){return Err("No attached display was reported.".into())}let old=setting(ui,"display","primary");let i=monitors.iter().position(|m|m.0==old).map(|i|(i+1)%monitors.len()).unwrap_or(0);shell::remember_display(ui,&monitors[i].0);shell::position(ui);settings_rows(ui);ui.save()?;return Ok("Display changed.".into())}
    let n=ui.selected().unwrap_or(0);let &(k,name,default,format)=OPTIONS.get(n).ok_or("Select a setting.")?;
    if key=="select"{ui.set_input(setting(ui,k,default));ui.set_detail(&format!("{}\r\n{}",name,format));return Ok(String::new())}
    let old=setting(ui,k,default);
    let val=if key=="cycle" && format.contains(" · "){let v=format.split(" · ").collect::<Vec<_>>();v[(v.iter().position(|x|*x==old).unwrap_or(0)+1)%v.len()].to_string()}else{ui.input()};
    if format.contains(" · ")&&!format.split(" · ").any(|s|s==val){return Err(format!("Choose one of {}",format))}
    if matches!(k,"scale"|"hide"|"retention"|"notificationDuration"){let (min,max)=match k{"scale"=>(75,175),"hide"=>(1,600),"retention"=>(0,600),_=>(10,30)};val.parse::<i32>().ok().filter(|v|(min..=max).contains(v)).ok_or_else(||format!("Enter a value between {} and {}.",min,max))?;}
    if k=="display" && val!="primary" && !shell::screens().iter().any(|s|s.0==val){return Err("Choose Next display or enter an attached display name.".into())}
    if matches!(k,"customAccent"|"customMain"|"customField"|"customTextColor") && !val.is_empty() && shell::color_value(&val).is_none(){return Err("Enter a six-digit hex color, such as #007F9C.".into());}
    if k=="moduleOrder" && val.split(',').map(str::trim).filter(|s|!s.is_empty()).any(|s|!crate::ui::PAGES.iter().any(|p|p.0==s)){
        return Err(format!("Available workspace IDs: {}",crate::ui::PAGES.iter().map(|p|p.0).collect::<Vec<_>>().join(", ")));
    }
    if val.chars().count()>if k=="exclude"||k=="moduleOrder"{2048}else{80}{return Err("This setting is too long.".into())}
    if k=="autostart"{shell::autostart(ui,val=="on")?;}
    if k=="display"{shell::remember_display(ui,&val);}else{set(ui,k,&val);}if k=="moduleOrder"{ui.reorder_navigation();}shell::position(ui);settings_rows(ui);ui.select_row(n);ui.set_input(&val);ui.save()?;Ok("Setting saved.".into())
}
pub fn render(ui:&mut Ui,page:&str)->String{match page{
    "notes"=>{note_rows(ui);fill_note(ui);if ui.active_note.is_none()&&!ui.note_ids.is_empty(){ui.active_note=Some(ui.note_ids[0].clone());ui.select_row(0);fill_note(ui);}},
    "settings"=>{settings_rows(ui);ui.select_row(0);let _=settings(ui,"select");},
    "widgets"=>{ui.set_rows(&timer_rows(ui));ui.set_detail(&widget_caption(ui));},
    _=>{let rows=ui.records.get("notes").into_iter().flatten().filter(|r|r.len()>=7).take(30).map(|r|show(r,1).to_string()).collect::<Vec<_>>();ui.set_rows(&rows);ui.set_detail(&home_detail(ui));}
};String::new()}
pub fn action(ui:&mut Ui,page:&str,key:&str)->Result<String,String>{match page{"notes"=>edit_note(ui,key),"widgets"=>widget(ui,key),"settings"=>settings(ui,key),_=>{match key{"notes"|"select"=>{if key=="select"{if let Some(r)=ui.selected().and_then(|i|ui.records.get("notes")?.get(i)){ui.active_note=r.first().cloned();}}ui.navigate("notes")},"quick"=>ui.navigate("quick"),"media"=>ui.navigate("media"),"widgets"=>ui.navigate("widgets"),_=>{}}Ok(String::new())}}}
pub(crate) fn widget_enabled(ui:&Ui,key:&str,running:&str)->bool{
    if setting(ui,"widgets","on")!="on"{return false;}
    match setting(ui,key,"auto"){"on"=>true,"off"=>false,_=>val(ui,running,0)>0}
}
fn widget_caption(ui:&Ui)->String{if setting(ui,"widgets","on")!="on"{return String::new()}let id=setting(ui,"noteWidget","");let note=ui.records.get("notes").into_iter().flatten().find(|r|show(r,0)==id).map(|r|show(r,1)).unwrap_or("");let clipboard=crate::workspace::widget_text(ui);[note,clipboard.as_str()].into_iter().filter(|s|!s.is_empty()).collect::<Vec<_>>().join("  ·  ")}
pub(crate) fn due_title(ui:&Ui)->Option<String>{
    let id=ui.due_note.as_ref()?;
    let row=ui.records.get("notes")?.iter().find(|r|r.first()==Some(id))?;
    Some(if show(row,1).is_empty(){"Untitled note".into()}else{show(row,1).into()})
}
pub(crate) fn acknowledge_reminder(ui:&mut Ui,snooze:bool)->Result<(),String>{
    let id=ui.due_note.clone().ok_or("There is no pending reminder.")?;
    if snooze{
        let idx=ui.records.get("notes").and_then(|r|r.iter().position(|r|r.len()>=7&&r.first()==Some(&id))).ok_or("The note was removed.")?;
        let until=(now()+300000).to_string();
        let old=std::mem::replace(&mut ui.records.get_mut("notes").unwrap()[idx][5],until);
        if let Err(e)=ui.save(){ui.records.get_mut("notes").unwrap()[idx][5]=old;return Err(e)}
    }
    ui.due_note=None;Ok(())
}
fn home_detail(ui:&Ui)->String {
    let media=crate::activity::home_media().unwrap_or_default();
    [media,widget_caption(ui)].into_iter().filter(|s|!s.is_empty()).collect::<Vec<_>>().join("\r\n")
}
pub fn tick(ui:&mut Ui){
    if ui.page=="home" && ui.expanded {
        let contents=home_detail(ui);
        if contents!=ui.detail(){ui.set_detail(&contents);}
    }
    if ui.due_note.is_some() && due_title(ui).is_none(){ui.due_note=None;}
    let due=ui.due_note.is_none().then(||ui.records.get("notes")).flatten().and_then(|rows|rows.iter().find(|r|r.len()>=7 && show(r,5).parse::<u64>().is_ok_and(|t|t<=now()) && ui.due_note.as_deref()!=Some(show(r,0)))).map(|r|(r[0].clone(),r[1].clone()));
    if let Some((id,title))=due{ui.due_note=Some(id.clone());let idx=ui.records["notes"].iter().position(|r|r.first()==Some(&id)).unwrap();ui.records.get_mut("notes").unwrap()[idx][5].clear();let _=ui.save();shell::balloon(ui,"Buddy reminder",&title);ui.status(&format!("Reminder  {}",title));}
    if val(ui,"pomEnd",0)>0 && timers(ui).0==0 {advance_phase(ui);let _=ui.save();shell::balloon(ui,"Buddy timer",&format!("{} is ready.",setting(ui,"phase","Focus")));}
    if ui.page=="widgets"&&ui.expanded{let selected=ui.selected();ui.set_rows(&timer_rows(ui));if let Some(i)=selected{ui.select_row(i);}}
    let clock=shell::clock(setting(ui,"center","time"),setting(ui,"custom","Buddy"));let widget=widget_caption(ui);
    let timer=if widget_enabled(ui,"pomWidget","pomEnd"){duration(timers(ui).0)}else if widget_enabled(ui,"watchWidget","watchStart"){duration(timers(ui).1)}else{String::new()};
    if !ui.isolated && ui.camera_check.elapsed()>=std::time::Duration::from_secs(5) {
        ui.camera_check=std::time::Instant::now();
        ui.call_state=crate::devices::activity().ok().and_then(|v|{
            let consumers=v["consumers"].as_u64().unwrap_or(0);
            if consumers>0 {Some(format!("Camera {} · {}",v["state"].as_str().unwrap_or("active"),consumers))}else{None}
        });
    }
    let notification=due_title(ui).map(|title|format!("Reminder · {}",title)).or_else(crate::activity::compact_preview);let compact=[clock,timer,widget,ui.call_state.clone().unwrap_or_default()].into_iter().filter(|s|!s.is_empty()).collect::<Vec<_>>().join("   ·   ");ui.compact_label(if ui.paused{"Buddy paused"}else{notification.as_deref().unwrap_or(&compact)});
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf,ptr::{null,null_mut}};
    use windows::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW,DestroyWindow,WS_POPUP};

    struct TestWindow {
        ui: Option<Ui>,
        folder: PathBuf,
    }
    impl TestWindow {
        fn new(tag: &str) -> Self {
            let folder = std::env::temp_dir().join(format!("BuddyRust-tests-{}-{}-{}", std::process::id(), tag, now()));
            std::fs::create_dir_all(&folder).unwrap();
            let hwnd=unsafe {CreateWindowExW(0, crate::ui::wide("STATIC").as_ptr(), null(), WS_POPUP,0,0,840,580,null_mut(),null_mut(),null_mut(),null())};
            assert!(!hwnd.is_null());
            Self { ui: Some(Ui::new(HWND(hwnd),folder.clone(),true).unwrap()), folder }
        }
    }
    impl Drop for TestWindow {
        fn drop(&mut self) {
            if let Some(ui) = self.ui.take() {
                unsafe { DestroyWindow(ui.hwnd.0); }
            }
            let _ = std::fs::remove_dir_all(&self.folder);
        }
    }

    #[test]
    fn edit_note_widget_and_protected_storage() {
        let mut window=TestWindow::new("note");
        let ui=window.ui.as_mut().unwrap();
        ui.navigate("notes");
        edit_note(ui,"new").unwrap();
        ui.set_input("Synthetic café");
        ui.set_detail("Generated test body");
        let saved=save_note(ui);
        assert_eq!(ui.note_ids.len(),1);
        let path=ui.profile.join("state-v1.dpapi");
        match saved {
            Ok(()) => {
                assert_eq!(ui.records["notes"][0][1],"Synthetic café");
                assert_eq!(ui.records["notes"][0][2],"Generated test body");
                assert_eq!(crate::storage::load(&ui.profile).unwrap(),ui.records);
                let file=std::fs::read(path).unwrap();
                assert!(!file.windows(9).any(|s|s==b"Synthetic"));
                edit_note(ui,"pin").unwrap(); assert_eq!(ui.records["notes"][0][3],"1");
                edit_note(ui,"remind").unwrap(); assert!(ui.records["notes"][0][5].parse::<u64>().unwrap()>now());
                edit_note(ui,"widget").unwrap(); assert!(widget_caption(ui).contains("Synthetic café"));
                let id=ui.records["notes"][0][0].clone();
                let fmt=crate::inline_images::format_key(&id);
                let positions=crate::inline_images::offset_key(&id);
                ui.records.insert(fmt.clone(),vec![vec![r"{\rtf1\ansi {\b Generated} test body}".into()]]);
                ui.records.insert(positions.clone(),vec![vec![]]);
                let before=ui.records.clone();
                delete_note(ui,0).unwrap();
                let stored=crate::storage::load(&ui.profile).unwrap();
                assert!(!stored.contains_key(&fmt) && !stored.contains_key(&positions));
                restore_deleted_note(ui).unwrap();
                assert_eq!(crate::storage::load(&ui.profile).unwrap(),before);

            }
            Err(reason) => {
                assert!(reason.contains("user encryption"),"{reason}");
                assert!(!path.exists());
                assert_eq!(ui.records["notes"][0][1],"");
                assert!(ui.dirty_note);
                ui.navigate("widgets");
                assert_eq!(ui.page,"notes");
                assert_eq!(ui.input(),"Synthetic café");
                assert_eq!(ui.detail(),"Generated test body");
                assert!(edit_note(ui,"new").is_err());
                discard_note(ui);
                assert!(!ui.dirty_note);
                assert!(ui.records["notes"].is_empty());
            }
        }
        ui.navigate("widgets");
        let before=ui.records.clone();
        if let Err(reason)=widget(ui,"watch"){
            assert!(reason.contains("user encryption"),"{reason}");
            assert_eq!(before,ui.records);
            return
        }
        assert!(val(ui,"watchStart",0)>0);
        widget(ui,"lap").unwrap();assert_eq!(ui.records["laps"].len(),1);
        widget(ui,"reset").unwrap();assert_eq!(val(ui,"watchStart",9),0);
        widget(ui,"short").unwrap();assert_eq!(val(ui,"pomLeft",0),300000);
        widget(ui,"pom").unwrap();assert!(val(ui,"pomEnd",0)>now());

    }

    #[test]
    fn every_note_attachment_can_be_selected_and_removed_individually(){
        let mut window=TestWindow::new("attachments");
        let ui=window.ui.as_mut().unwrap();
        let a="a".repeat(64);
        let b="b".repeat(64);
        let mut row=vec![String::new();7];
        row[0]="synthetic-note".into();
        row.extend([a.clone(),b.clone(),a.clone()]);
        ui.records.insert("notes".into(),vec![row.clone()]);
        assert_eq!(attachment_id(&row,0).unwrap(),a);
        assert_eq!(attachment_id(&row,1).unwrap(),b);
        assert_eq!(attachment_id(&row,2).unwrap(),a);
        assert!(attachment_id(&row,3).is_err());
        assert_eq!(attachment_labels(ui,&row).len(),3);
        match remove_attachment(ui,0,1){
            Ok(())=>assert_eq!(attachment_fields(&ui.records["notes"][0]),[a.clone(),a]),
            Err(reason)=>{
                assert!(reason.contains("encryption"),"{reason}");
                assert_eq!(ui.records["notes"][0],row);
            }
        }
    }

    #[test]
    fn unreadable_existing_state_is_preserved() {
        let mut window=TestWindow::new("unreadable");
        let ui=window.ui.as_mut().unwrap();
        let path=ui.profile.join("state-v1.dpapi");
        let invalid=b"intentionally invalid test data";
        std::fs::write(&path,invalid).unwrap();
        let other=Ui::new(ui.hwnd,ui.profile.clone(),true).unwrap();
        assert!(other.read_error.is_some());
        assert!(other.save().is_err());
        assert_eq!(std::fs::read(path).unwrap(),invalid);
    }
}
