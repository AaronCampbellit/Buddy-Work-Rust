use super::helper::quote;
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{fs::{self,OpenOptions},io::{Cursor,Write},path::{Path,PathBuf},sync::atomic::{AtomicU32,Ordering}};
pub fn defaults()->Value{serde_json::from_str(include_str!("effects.json")).expect("valid embedded camera defaults")}
pub fn shared(e:&Value)->bool{e["pipEnabled"]==true && e["pipKind"]=="screen"}
pub fn preview(draft:&Value,applied:&Value)->Value{
    let mut v=draft.clone();if shared(draft)&&(!shared(applied)||draft["pipSource"]!=applied["pipSource"]){v["pipEnabled"]=false.into();}v
}
pub fn value(key:&str,s:&str)->Result<Value,String>{
    let bounds=match key{"pipSize"=>Some((15,50)),"logoSize"=>Some((5,40)),"tolerance"|"spill"=>Some((0,100)),"softness"=>Some((1,100)),"pipCorner"|"logoCorner"|"nameCorner"=>Some((0,3)),"pipMode"=>Some((-1,i32::MAX)),_=>None};
    if let Some((lo,hi))=bounds{let n=s.parse::<i32>().ok().filter(|&n|(lo..=hi).contains(&n)).ok_or_else(||format!("Choose a whole number from {lo} to {hi}."))?;return Ok(n.into());}
    let options:&[&str]=match key{"backgroundMode"=>&["green-screen","blur","replace"],"pipKind"=>&["screen","camera"],"pipLayout"=>&["inset","presentation"],"backgroundFit"|"standbyFit"=>&["contain","cover"],_=>&[]};
    if !options.is_empty(){if !options.contains(&s){return Err(format!("Choose {}.",options.join(" or ")));}return Ok(s.into());}
    match key{
        "mirrorCamera"|"mirrorOutput"|"enabled"|"nameEnabled"|"logoEnabled"|"pipEnabled"=>match s{"true"|"on"|"1"=>Ok(true.into()),"false"|"off"|"0"=>Ok(false.into()),_=>Err("Enter on or off.".into())},
        "keyColor"|"standbyColor"|"standbyTextColor"|"nameColor"|"nameBackground" if s.len()==7&&s.starts_with('#')&&s.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)=>Ok(s.into()),
        "standbyTitle"|"nameText"|"pipSource" if !s.chars().any(char::is_control)&&s.chars().count()<=if key=="pipSource"{4096}else{120}=>Ok(s.into()),
        _=>Err("Invalid effect setting. Colors use #RRGGBB.".into())
    }
}
pub fn config(e:&Value,assets:&Value,folder:&Path)->Result<String,String>{
    let defaults=defaults();
    for (k,d) in defaults.as_object().unwrap(){let v=&e[k];let s=v.as_str().map(str::to_owned).unwrap_or_else(||v.to_string());if std::mem::discriminant(d)!=std::mem::discriminant(v)||&value(k,&s)?!=v{return Err(format!("Invalid saved {k} setting."));}}
    let flag=|k:&str|if e[k]==true{"1"}else{"0"};let is=|k:&str,v:&str|if e[k]==v{"1"}else{"0"};let num=|k:&str|e[k].to_string();
    let c=|k:&str|u32::from_str_radix(&e[k].as_str().unwrap()[1..],16).unwrap().to_string();
    let t=|k:&str|quote(e[k].as_str().unwrap());
    let path=|slot:&str|->Result<String,String>{if assets[slot].is_null(){return Ok(String::new());}
        let name=assets[slot].as_str().ok_or("Invalid asset.")?;
        if name.len()!=68||!name.ends_with(".png")||!name.as_bytes()[..64].iter().all(u8::is_ascii_hexdigit){return Err("Invalid saved camera image.".into());}
        let path=folder.join(name);if !fs::symlink_metadata(&path).map(|m|m.is_file()&&!m.is_symlink()).unwrap_or(false){return Err("A saved camera image is missing.".into());}
        Ok(path.to_string_lossy().into_owned())};
    let bg=path("background")?;let logo=path("logo")?;
    if e["enabled"]==true&&e["backgroundMode"]!="blur"&&bg.is_empty(){return Err("Add a background image to use replacement.".into());}
    if e["logoEnabled"]==true&&logo.is_empty(){return Err("Add a logo image first.".into());}
    if e["pipEnabled"]==true&&e["pipSource"].as_str().unwrap_or("").is_empty(){return Err("Choose a picture-in-picture source.".into());}
    let args=vec!["4".into(),flag("enabled").into(),num("tolerance"),num("softness"),num("spill"),c("keyColor"),is("backgroundFit","contain").into(),is("standbyFit","contain").into(),c("standbyColor"),c("standbyTextColor"),quote(&bg)?,quote(&path("standby")?)?,t("standbyTitle")?,flag("nameEnabled").into(),num("nameCorner"),c("nameColor"),c("nameBackground"),t("nameText")?,flag("logoEnabled").into(),num("logoCorner"),num("logoSize"),quote(&logo)?,flag("pipEnabled").into(),is("pipKind","screen").into(),num("pipMode"),num("pipCorner"),num("pipSize"),is("pipLayout","presentation").into(),t("pipSource")?,flag("mirrorCamera").into(),flag("mirrorOutput").into(),match e["backgroundMode"].as_str(){Some("replace")=>"2",Some("blur")=>"1",_=>"0"}.into()];
    Ok(args.join(" "))
}
pub struct Temp(pub PathBuf);
impl Drop for Temp{fn drop(&mut self){let _=fs::remove_file(&self.0);}}
pub fn temporary(dir:&Path,contents:&str)->Result<Temp,String>{
    static N:AtomicU32=AtomicU32::new(0);fs::create_dir_all(dir).map_err(|e|e.to_string())?;
    loop{let name=dir.join(format!("apply-{}-{}.txt",std::process::id(),N.fetch_add(1,Ordering::Relaxed)));
        match OpenOptions::new().write(true).create_new(true).open(&name){Ok(mut f)=>{let file=Temp(name);f.write_all(contents.as_bytes()).map_err(|e|e.to_string())?;return Ok(file)},Err(e)if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,Err(e)=>return Err(e.to_string())}}
}
pub fn asset(data:&[u8],folder:&Path)->Result<String,String>{
    let img=super::ocr::decode(data,40_000_000)?;
    let mut bytes=Cursor::new(Vec::new());img.write_to(&mut bytes,image::ImageFormat::Png).map_err(|e|e.to_string())?;
    let bytes=bytes.into_inner();let name=format!("{:x}.png",Sha256::digest(&bytes));
    fs::create_dir_all(folder).map_err(|e|e.to_string())?;
    let dest=folder.join(&name);
    if !dest.is_file(){let mut tmp=temporary(folder,"")?;fs::write(&tmp.0,&bytes).and_then(|_|fs::rename(&tmp.0,&dest)).map_err(|e|e.to_string())?;tmp.0=PathBuf::new();}
    Ok(name)
}
pub fn blank()->Value{json!({"effects":defaults(),"assets":{},"scenes":[],"device":"","mode":-1,"available":false,"cameraEnabled":true,"output":[1280,720,30,1]})}
#[cfg(test)]mod tests{use super::*;#[test]fn validate_config(){let p=Path::new(".");let mut e=defaults();let original=config(&e,&json!({}),p).unwrap();assert!(original.starts_with("4 0 35 15 30 65280"));e["pipEnabled"]=true.into();e["pipKind"]="screen".into();e["pipSource"]="display:0".into();assert!(!shared(&preview(&e,&defaults())));assert!(shared(&preview(&e,&e)));assert!(value("nameText","a\nb").is_err());assert!(value("pipSize","51").is_err());assert!(config(&e,&json!({"logo":"../../bad.png"}),p).is_err());}}
