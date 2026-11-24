use super::{effects,helper,read_image,choose_image,confirm};
use crate::{storage, Ui};
use serde_json::{Value,json};
use std::{cell::RefCell,io::Cursor,path::{Path,PathBuf},time::{Duration, Instant}};
use windows::Win32::Security::Cryptography::{CryptStringToBinaryW,CRYPT_STRING_BASE64};
type Action=(&'static str,&'static str);
pub fn actions()->Vec<Action>{vec![
    ("inventory","Choose input"),("start","Start output"),("stop","Stop output"),("effects","Edit effects"),
    ("modes","Choose input format"),("switch","Switch live input"),("set","Set selected option"),("apply","Apply effects"),("discard","Discard draft"),
    ("preview","Preview live draft"),("standby-preview","Preview standby"),("output-preview","Preview output"),("sample","Preview camera only"),("pick-key","Sample chroma key"),
    ("sources","Picture-in-picture cameras"),("screens","Picture-in-picture displays"),("asset","Add selected image"),("remove-asset","Remove selected image"),
    ("scenes","Saved scenes"),("save-scene","Save new scene"),("update-scene","Update scene"),("load-scene","Load scene"),("delete-scene","Delete scene"),
    ("freeze","Freeze output"),("resume","Resume output"),("standby","Show standby"),("status","Refresh status"),
    ("available","Keep available for calls"),("unavailable","Remove call availability"),("on","Enable camera for calls"),("off","Disable camera for calls"),
    ("output","Set output format"),("retry","Retry camera"),("install","Install camera component")
]}
#[derive(PartialEq)]enum View{Effects,Devices,Modes,Sources,Screens,Scenes}
struct Studio{dir:PathBuf,state:Value,draft:Value,assets:Value,view:View,rows:Vec<(String,String)>,child:Option<helper::Producer>,snapshot:Value,selected_device:String,selected_mode:i32, retries:u8, retry_at:Option<Instant>}
thread_local!{static STUDIO:RefCell<Option<Studio>>=const{RefCell::new(None)};}
impl Studio {
    fn load(profile:&Path)->Result<Self,String>{
        let dir=profile.join("camera-studio");let store=dir.join("studio.dpapi");
        let state:Value=if store.try_exists().map_err(|e|e.to_string())?{serde_json::from_slice(&storage::read_protected(&store,8*1024*1024)?).map_err(|_|"Saved camera preferences are unreadable.")?}else{effects::blank()};
        if !state["scenes"].is_array()||!state["assets"].is_object(){return Err("Saved camera preferences are invalid.".into());}
        effects::config(&state["effects"],&state["assets"],&dir)?;
        Ok(Self{dir,draft:state["effects"].clone(),assets:state["assets"].clone(),selected_device:state["device"].as_str().unwrap_or("").into(),selected_mode:state["mode"].as_i64().unwrap_or(-1) as i32,state,view:View::Effects,rows:vec![],child:None,retries:0,retry_at:None,snapshot:json!({"ok":true,"state":"stopped","consumers":0})})
    }
    fn persist(&mut self,state:Value)->Result<(),String>{
        storage::write_protected(&self.dir.join("studio.dpapi"),&serde_json::to_vec(&state).map_err(|e|e.to_string())?)?;
        self.state=state;self.cleanup_images();Ok(())
    }
    fn cleanup_images(&self){
        let mut refs=std::collections::HashSet::new();
        for a in std::iter::once(&self.assets).chain(std::iter::once(&self.state["assets"])).chain(self.state["scenes"].as_array().into_iter().flatten().map(|s|&s["assets"])){
            if let Some(m)=a.as_object(){for f in m.values().filter_map(Value::as_str){refs.insert(f);}}
        }
        if let Ok(iter)=std::fs::read_dir(&self.dir){for entry in iter.flatten(){if let Some(name)=entry.file_name().to_str(){if name.len()==68&&name.ends_with(".png")&&name.as_bytes()[..64].iter().all(u8::is_ascii_hexdigit)&&!refs.contains(name){let _=std::fs::remove_file(entry.path());}}}}
    }
    fn alive(&mut self)->bool{let yes=self.child.as_mut().is_some_and(|c|c.alive());if !yes{self.child.take();}yes}
    fn close(&mut self,detach:bool){if let Some(mut h)=self.child.take(){h.close(detach);}self.snapshot=json!({"ok":true,"state":"stopped","consumers":0});}
    fn send(&mut self,line:&str)->Result<Value,String>{
        if !self.alive(){return Err("Start the camera first.".into());}
        let result=self.child.as_mut().unwrap().send(line);
        let v=match result{Ok(r)=>r,Err(e)=>{self.close(self.state["available"]==true);return Err(e)}};
        let v=helper::accepted(v)?;if v["state"].is_string(){self.snapshot=v.clone();}Ok(v)
    }
    fn launch(&mut self,s:&Value)->Result<(),String>{
        if self.alive(){return Err("Stop the active output before starting another.".into());}
        let device=valid_id(&s["device"])?;let mode=s["mode"].as_i64().filter(|n|(-1..=i32::MAX as i64).contains(n)).ok_or("Invalid input mode.")?;
        let available=s["available"]==true;
        let file=effects::temporary(&self.dir,&effects::config(&s["effects"],&s["assets"],&self.dir)?)?;
        let mut args=vec![if available{"--run-available"}else{"--run"}.to_owned(),device,mode.to_string()];
        if available{let values=output(&s["output"])?;for n in values{args.push(n.to_string())}}
        args.push(file.0.to_string_lossy().into());if available{args.push(if s["cameraEnabled"]==true{"1"}else{"0"}.into());}
        let mut c=helper::Producer::start(&helper::installed()?,&args)?;self.snapshot=helper::accepted(c.read(15)?)?;self.child=Some(c);self.retries=0;self.retry_at=None;Ok(())
    }
    fn recover_available(&mut self,force:bool)->Result<(),String>{
        if self.state["available"]!=true||self.alive(){return Ok(());}
        if !force && (self.retries>=3||self.retry_at.is_some_and(|t| t>Instant::now())){
            return Err(self.snapshot["message"].as_str().unwrap_or("Camera unavailable; choose Retry camera.").into());
        }
        if force {self.retries=0;}
        match self.launch(&self.state.clone()){
            Ok(())=>Ok(()),
            Err(e)=>{
                self.retries+=1;self.retry_at=Some(Instant::now()+Duration::from_secs(2*u64::from(self.retries)));
                self.snapshot=json!({"ok":false,"state":"error","consumers":0,"message":e});
                Err(e)
            }
        }
    }
    fn commit_effects(&mut self,next:Value,switch:bool)->Result<(),String>{
        let command=|s:&Value|->Result<(effects::Temp,String),String>{let cfg=effects::config(&s["effects"],&s["assets"],&self.dir)?;
            let f=effects::temporary(&self.dir,&cfg)?;let target=helper::quote(&f.0.to_string_lossy())?;
            let cmd=if switch{format!("switch {} {} {target}",helper::quote(&valid_id(&s["device"])?)?,s["mode"].as_i64().unwrap_or(-1))}else{format!("effects {target}")};Ok((f,cmd))};
        let (_next,cmd)=command(&next)?;
        let old=self.state.clone();let rollback=if self.child.is_some(){Some(command(&old)?)}else{None};
        if self.alive(){self.send(&cmd)?;}
        if let Err(e)=self.persist(next){
            if let Some((_f,line))=rollback{if self.send(&line).is_err(){self.close(false);}} return Err(e);
        }
        self.draft=self.state["effects"].clone();self.assets=self.state["assets"].clone();self.cleanup_images();Ok(())
    }
    fn fields(&mut self,ui:&Ui){self.view=View::Effects;
        self.rows=self.draft.as_object().into_iter().flat_map(|m|m.iter()).map(|(k,v)|(k.clone(),format!("{}    {}",label(k),display(v)))).collect();
        for slot in ["background","standby","logo"]{self.rows.push((format!("asset:{slot}"),format!("{} image    {}",label(slot),if self.assets[slot].is_string(){"added"}else{"none"})));}
        self.show(ui)
    }
    fn show(&self,ui:&Ui){ui.set_rows(&self.rows.iter().map(|x|x.1.clone()).collect::<Vec<_>>());}
    fn selected(&self,ui:&Ui)->Result<String,String>{ui.selected().and_then(|i|self.rows.get(i)).map(|r|r.0.clone()).ok_or("Select an item first.".into())}
    fn current(&self)->Value{let mut next=self.state.clone();next["device"]=self.selected_device.clone().into();next["mode"]=self.selected_mode.into();next}
    fn ensure_consent(&self,ui:&Ui,e:&Value,action:&str)->Result<(),String>{
        if effects::shared(e)&&!confirm(ui,&format!("{action} and share display {} with Buddy Camera?",e["pipSource"].as_str().unwrap_or(""))){return Err("Display sharing canceled.".into());} Ok(())
    }
    fn act(&mut self,ui:&mut Ui,key:&str)->Result<String,String>{
        match key{
        "effects"=>{self.fields(ui);return Ok("Select a setting, enter its value, then use Set selected option.".into())}
        "inventory"|"sources"|"screens"|"modes"=>{
            let exe=helper::installed()?;
            let (v,field,view)=match key{"screens"=>(helper::query(&exe,&["--screens"],15)?,"screens",View::Screens),"modes"=>(helper::query(&exe,&["--modes",&valid_id(&self.current()["device"])?],15)?,"modes",View::Modes),_=>(helper::query(&exe,&["--list"],15)?,"devices",if key=="inventory"{View::Devices}else{View::Sources})};
            self.rows=v[field].as_array().into_iter().flatten().filter_map(|v|if view==View::Modes{Some((v["index"].as_i64()?.to_string(),format!("{} × {} · {:.1} fps · {}",v["width"],v["height"],v["numerator"].as_f64()?/v["denominator"].as_f64()?.max(1.),v["format"].as_str().unwrap_or(""))))}else{Some((valid_id(&v["id"]).ok()?,v["name"].as_str()?.into()))}).collect();
            if view==View::Modes{self.rows.insert(0,("-1".into(),"Automatic".into()));}self.view=view;self.show(ui);return Ok(format!("{} options. Select a row to choose it.",self.rows.len()));
        }
        "scenes"=>{self.view=View::Scenes;self.rows=self.state["scenes"].as_array().unwrap().iter().enumerate().map(|(i,s)|(i.to_string(),format!("{}{}",s["name"].as_str().unwrap_or("Scene"),if effects::shared(&s["effects"]){" · shares display"}else{""}))).collect();self.show(ui);return Ok(format!("{} saved scenes.",self.rows.len()));}
        "select"=>{let row=self.selected(ui)?;match self.view{
            View::Effects=>{ui.set_input(&display(&self.draft[&row]));ui.set_detail(&if row.starts_with("asset:"){"Use Add selected image or Remove selected image.".into()}else{format!("{}\r\n{}",label(&row),hint(&row))});}
            View::Devices=>{self.selected_device=row;self.selected_mode=-1;ui.set_detail("Input selected. Use Choose input format, then Start output or Switch live input.");}
            View::Modes=>{self.selected_mode=row.parse().map_err(|_|"Invalid mode.")?;}
            View::Sources|View::Screens=>{self.draft["pipKind"]=if self.view==View::Sources{"camera"}else{"screen"}.into();self.draft["pipSource"]=row.into();self.draft["pipEnabled"]=true.into();ui.set_detail("Picture-in-picture source staged. Apply effects to activate it.");}
            View::Scenes=>{let scene=&self.state["scenes"][row.parse::<usize>().map_err(|_|"Invalid scene.")?];ui.set_input(scene["name"].as_str().unwrap_or(""));ui.set_detail(&format!("Input {} · mode {}",scene["device"].as_str().unwrap_or(""),scene["mode"]));}
        }return Ok(String::new());}
        "set"=>{let row=self.selected(ui)?;if self.view!=View::Effects||row.starts_with("asset:"){return Err("Choose an effect setting first.".into());}self.draft[&row]=effects::value(&row,ui.input().trim())?;self.fields(ui);return Ok("Draft updated.".into());}
        "asset"|"remove-asset"=>{let row=self.selected(ui)?;let slot=row.strip_prefix("asset:").ok_or("Select a background, standby or logo image row.")?;if key=="asset"{let Some(path)=choose_image(ui)? else{return Ok(String::new())};self.assets[slot]=effects::asset(&read_image(&path)?,&self.dir)?.into();}else{self.assets.as_object_mut().unwrap().remove(slot);if slot=="logo"{self.draft["logoEnabled"]=false.into();}if slot=="background"&&self.draft["backgroundMode"]!="blur"{self.draft["enabled"]=false.into();}}self.cleanup_images();self.fields(ui);return Ok("Draft image updated.".into());}
        "discard"=>{if self.alive(){self.send("preview-clear")?;}self.draft=self.state["effects"].clone();self.assets=self.state["assets"].clone();self.cleanup_images();self.fields(ui);return Ok("Draft discarded.".into());}
        "apply"=>{self.ensure_consent(ui,&self.draft,"Apply effects")?;let mut s=self.state.clone();s["effects"]=self.draft.clone();s["assets"]=self.assets.clone();self.commit_effects(s,false)?;return Ok("Effects applied.".into());}
        "save-scene"|"update-scene"|"delete-scene"|"load-scene"=>{
            let mut s=self.state.clone();let scenes=s["scenes"].as_array_mut().unwrap();
            let index=if key!="save-scene"{if self.view!=View::Scenes{return Err("Select a saved scene.".into());}let i=self.selected(ui)?.parse::<usize>().map_err(|_|"Invalid scene.")?;if i>=scenes.len(){return Err("Scene no longer exists.".into());}Some(i)}else{None};
            if key=="load-scene"{let scene=scenes[index.unwrap()].clone();self.ensure_consent(ui,&scene["effects"],"Load scene")?;for k in ["effects","assets","device","mode"]{s[k]=scene[k].clone();}self.commit_effects(s,true)?;self.selected_device=self.state["device"].as_str().unwrap_or("").into();self.selected_mode=self.state["mode"].as_i64().unwrap_or(-1) as i32;self.fields(ui);return Ok("Scene loaded.".into());}
            if key=="delete-scene"{scenes.remove(index.unwrap());}else{let name=ui.input().trim().to_owned();if name.is_empty()||name.chars().count()>80||name.chars().any(char::is_control){return Err("Enter a scene name (1–80 characters).".into());}
                let dev=valid_id(&self.current()["device"])?;let scene=json!({"name":name,"effects":self.state["effects"],"assets":self.state["assets"],"device":dev,"mode":self.selected_mode});if let Some(i)=index{scenes[i]=scene}else{if scenes.len()>=256{return Err("Remove a scene before saving another.".into());}scenes.push(scene);}}
            self.persist(s)?;return self.act(ui,"scenes");
        }
        "status"=>{self.recover_available(false)?;let v=if self.alive(){self.send("status")?}else{helper::query(&helper::installed()?,&["--status"],15)?};return Ok(status(&v));}
        "pick-key"=>{
            let current=self.send("status")?;
            if !matches!(current["state"].as_str(),Some("live"|"frozen")) {
                return Err("Start live camera output before sampling its chroma key.".into());
            }
            let frame=preview_image(&self.send("preview-camera")?)?;
            super::key_picker::open(ui.hwnd.0,frame)?;
            return Ok("Click the background color in the paused camera snapshot.".into());
        }
        "preview"|"standby-preview"|"sample"|"output-preview"=>{
            let r=match key{"sample"=>self.send("preview-camera")?,"output-preview"=>self.send("preview")?,_=>{
                let safe=effects::preview(&self.draft,&self.state["effects"]);let f=effects::temporary(&self.dir,&effects::config(&safe,&self.assets,&self.dir)?)?;
                let t=if key=="standby-preview"{"1"}else{"0"};let name=f.0.to_string_lossy();if self.alive(){self.send(&format!("preview-draft {} {t}",helper::quote(&name)?))?}else{helper::query(&helper::installed()?,&["--preview",&name,t],15)?}
            }};let pixels=preview_png(&r)?;crate::workspace::view_png(ui,&pixels)?;return Ok("Camera preview opened.".into());
        }
        "freeze"|"resume"|"standby"=>return self.send(key).map(|v|status(&v)),
        "start"|"available"=>{if self.state["available"]==true{return Err("Remove existing call availability before starting another output.".into());}let mut s=self.current();s["available"]=(key=="available").into();s["cameraEnabled"]=true.into();self.ensure_consent(ui,&s["effects"],"Start output")?;self.launch(&s)?;
            if let Err(e)=self.persist(s){self.close(false);if key=="available"{let _=helper::query(&helper::installed()?,&["--remove-available"],15);}return Err(e);} }
        "unavailable"=>{if self.state["available"]!=true{return Ok("Call availability is already off.".into());}self.close(false);helper::query(&helper::installed()?,&["--remove-available"],15)?;let mut s=self.state.clone();s["available"]=false.into();self.persist(s)?;}
        "stop" if self.state["available"]!=true=>{self.close(false);}
        "off"|"on"|"stop"=>{if self.state["available"]!=true{return Err("Enable call availability first.".into());}let on=key=="on";let old=self.state.clone();let mut s=old.clone();s["cameraEnabled"]=on.into();
            if let Err(e)=self.persist(s.clone()){if !on{self.close(false);}return Err(e);}
            let res=if self.alive(){self.send(if on{"camera-on"}else{"camera-off"}).map(|_|())}else if on{self.ensure_consent(ui,&s["effects"],"Enable camera").and_then(|_|self.launch(&s))}else{Ok(())};
            if let Err(e)=res{if on{self.persist(old)?;}else{self.close(false);}return Err(e);}}
        "retry"=>{if self.state["available"]!=true{return Err("Enable call availability first.".into());}if self.alive(){self.send("resume")?;}else{self.recover_available(true)?;}}
        "switch"=>{if !self.alive(){return Err("Start the virtual camera first.".into());}let s=self.current();self.commit_effects(s,true)?;}
        "output"=>{let parts:Vec<_>=ui.input().split(['x','×','@','/',' ']).filter(|x|!x.is_empty()).map(|n|n.parse::<u32>()).collect::<Result<_,_>>().map_err(|_|"Enter width x height @ fps, e.g. 1280x720@30.")?;let mut p=parts;if p.len()==3{p.push(1);}let v=json!(p);output(&v)?;if self.state["available"]==true{return Err("Remove call availability before choosing the output format.".into());}let mut s=self.state.clone();s["output"]=v;self.persist(s)?;return Ok("Output format saved.".into());}
        "install"=>{if self.alive(){return Err("Stop camera output before installing.".into());}if !confirm(ui,"Install or upgrade Buddy Camera for this computer? Windows will request administrator permission."){return Ok("Installation canceled.".into());}
            helper::query(&helper::installer()?,&["--install"],120)?;return Ok("Camera component installed.".into());}
        _=>return Err("Unknown camera action.".into()),
        }
        Ok(status(&self.snapshot))
    }
}
fn status(v:&Value)->String{format!("Camera {} · {} calling applications{}",v["state"].as_str().unwrap_or("ready"),v["consumers"].as_u64().unwrap_or(0),v["message"].as_str().map(|s|format!(" · {s}")).unwrap_or_default())}
fn valid_id(v:&Value)->Result<String,String>{v.as_str().filter(|s|!s.is_empty()&&s.len()<=4096&&!s.chars().any(char::is_control)).map(str::to_owned).ok_or("Choose a connected input camera.".into())}
fn output(v:&Value)->Result<[u32;4],String>{let a=v.as_array().filter(|a|a.len()==4).ok_or("Choose a valid output resolution and frame rate.")?;let mut values=[0u32;4];for(i,v)in a.iter().enumerate(){values[i]=v.as_u64().and_then(|n|u32::try_from(n).ok()).filter(|v|*v>0).ok_or("Invalid output setting.")?;}let[w,h,n,d]=values;if w%2!=0||h%2!=0||u128::from(w)*u128::from(h)*3/2>u32::MAX.into()||10_000_000u64*u64::from(d)<u64::from(n){return Err("Unsupported output resolution or frame rate.".into());}Ok(values)}
fn label(k:&str)->String{let mut out=String::new();for (i,c)in k.chars().enumerate(){if i==0{out.extend(c.to_uppercase());}else if c.is_uppercase(){out.push(' ');out.extend(c.to_lowercase());}else{out.push(c);}}out}
fn display(v:&Value)->String{match v{Value::String(s)=>s.clone(),Value::Null=>String::new(),_=>v.to_string()}}
fn hint(k:&str)->&'static str{match k{"backgroundMode"=>"green-screen, blur or replace","backgroundFit"|"standbyFit"=>"cover or contain","pipKind"=>"camera or screen","pipLayout"=>"inset or presentation","nameCorner"|"logoCorner"|"pipCorner"=>"0 upper left, 1 upper right, 2 lower left, 3 lower right","pipMode"=>"-1 for automatic","pipSource"=>"Use Picture-in-picture cameras or displays to choose from connected inputs.","pipSize"=>"15–50 percent","logoSize"=>"5–40 percent",k if k.to_lowercase().contains("color")||k=="nameBackground"=>"Color in #RRGGBB format",k if k.ends_with("Enabled")||matches!(k,"enabled"|"mirrorCamera"|"mirrorOutput")=>"on or off",_=>"Enter the value in the field above and choose Set selected option."}}
fn preview_image(v:&Value)->Result<image::DynamicImage,String>{
    let data=v["preview"].as_str().and_then(|s|s.strip_prefix("data:image/bmp;base64,")).ok_or("No image available.")?;
    if data.is_empty()||data.len()>4*1024*1024{return Err("Invalid camera preview size.".into());}
    let chars:Vec<_>=data.encode_utf16().collect();let mut n=0;unsafe{
        CryptStringToBinaryW(&chars,CRYPT_STRING_BASE64,None,&mut n,None,None).map_err(|e|e.to_string())?;
        let mut b=vec![0u8;n as usize];CryptStringToBinaryW(&chars,CRYPT_STRING_BASE64,Some(b.as_mut_ptr()),&mut n,None,None).map_err(|e|e.to_string())?;b.truncate(n as usize);
        super::ocr::decode(&b,4_000_000)
    }
}
pub fn preview_png(v:&Value)->Result<Vec<u8>,String>{
    let image=preview_image(v)?;
    let mut buf=Cursor::new(Vec::new());
    image.write_to(&mut buf,image::ImageFormat::Png).map_err(|e|e.to_string())?;
    Ok(buf.into_inner())
}
pub fn sampled_color(ui:&Ui,color:&str)->Result<String,String>{
    with(&ui.profile,|s|{
        s.draft["keyColor"]=effects::value("keyColor",color)?;
        s.fields(ui);
        if let Some(i)=s.rows.iter().position(|r|r.0=="keyColor"){ui.select_row(i);}
        ui.set_input(color);
        ui.set_detail("Sampled from the source camera frame. Choose Apply effects to use this key.");
        Ok(format!("Draft chroma key set to {color}."))
    })
}
fn with<T>(p:&Path,f:impl FnOnce(&mut Studio)->Result<T,String>)->Result<T,String>{STUDIO.with(|cell|{
    let mut b=cell.try_borrow_mut().map_err(|_|"Camera studio is busy.")?;if b.as_ref().is_none_or(|s|s.dir!=p.join("camera-studio")){if let Some(mut old)=b.take(){old.close(old.state["available"]==true);}*b=Some(Studio::load(p)?);} f(b.as_mut().unwrap())
})}
pub fn render(ui:&Ui)->String{with(&ui.profile,|s|{s.fields(ui);Ok(status(&s.snapshot))}).unwrap_or_else(|e|e)}
pub fn action(ui:&mut Ui,key:&str)->Result<String,String>{super::key_picker::cancel();let p=ui.profile.clone();with(&p,|s|s.act(ui,key))}
pub fn activity()->Result<Value,String>{STUDIO.with(|cell|{let mut b=cell.try_borrow_mut().map_err(|_|"Camera studio is busy.")?;match b.as_mut(){Some(s) if s.child.is_some()=>if s.alive(){s.send("status")}else{Ok(json!({"ok":true,"state":"stopped","consumers":0}))},_=>Ok(json!({"ok":true,"state":"stopped","consumers":0}))}})}
pub fn leave(){super::key_picker::cancel();STUDIO.with(|cell|{if let Ok(mut b)=cell.try_borrow_mut(){if let Some(s)=b.as_mut(){if s.alive(){let _=s.send("preview-clear");}}}});}
pub fn stop(){super::key_picker::cancel();STUDIO.with(|cell|{if let Ok(mut b)=cell.try_borrow_mut(){if let Some(s)=b.as_mut(){s.close(s.state["available"]==true);}}});}
pub fn restore(p:&Path){let _=with(p,|s|s.recover_available(true));}
pub fn call_control(key:&str)->Result<String,String>{
    if !matches!(key,"freeze"|"resume"|"standby"){return Err("Unknown call control.".into());}
    STUDIO.with(|cell|{let mut b=cell.try_borrow_mut().map_err(|_|"Camera studio is busy.")?;let s=b.as_mut().ok_or("No calling app is connected.")?;
        let current=s.send("status")?;if current["consumers"].as_u64().unwrap_or(0)==0{return Err("No calling app is connected.".into());}
        s.send(key).map(|v|status(&v))
    })
}
