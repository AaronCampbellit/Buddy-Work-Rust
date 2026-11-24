use super::{now, platform};
use crate::Ui;
use std::{collections::{HashMap,HashSet}, ffi::OsStr, fs, io::{Read,Write}, os::windows::fs::MetadataExt, path::{Path,PathBuf}, time::UNIX_EPOCH};
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO};

const MAX_BATCH:u64=512*1024*1024;
const MAX_STORE:u64=2*1024*1024*1024;
const MAX_DEPTH:usize=36;
fn error(e:impl ToString)->String{e.to_string()}
fn restricted_meta(path:&Path)->Result<fs::Metadata,String>{
    let m=fs::symlink_metadata(path).map_err(error)?;
    if m.file_attributes()&0x400!=0||(!m.is_file()&&!m.is_dir()){return Err("Links, junctions and special files cannot be copied to the shelf.".into());}
    Ok(m)
}
fn storage(ui:&Ui)->PathBuf {
    ui.records.get("shelf_location").and_then(|v|v.first()?.first()).map(PathBuf::from).unwrap_or_else(||ui.profile.join("Buddy Saved Files"))
}
#[derive(Default)]
struct Limit{nodes:usize,bytes:u64}
fn copy_tree(src:&Path,dst:&Path,limit:&mut Limit,depth:usize,cap:u64)->Result<(),String>{
    limit.nodes+=1;
    if limit.nodes>10_000||depth>MAX_DEPTH{return Err("Too many nested files for this copy.".into());}
    let meta=restricted_meta(src)?;
    if meta.is_dir(){
        fs::create_dir(dst).map_err(error)?;
        for e in fs::read_dir(src).map_err(error)?{let e=e.map_err(error)?;copy_tree(&e.path(),&dst.join(e.file_name()),limit,depth+1,cap)?;}
    }else{
        if meta.len()>cap.saturating_sub(limit.bytes){return Err("Shelf copy size limit exceeded.".into());}
        let mut input=fs::File::open(src).map_err(error)?;
        let mut output=fs::OpenOptions::new().create_new(true).write(true).open(dst).map_err(error)?;
        let mut copied=0;
        let mut chunk=[0u8;65536];
        loop {let n=input.read(&mut chunk).map_err(error)?;if n==0{break;}
            let n64=n as u64;if limit.bytes.saturating_add(n64)>cap{return Err("Shelf copy size limit exceeded.".into());}
            output.write_all(&chunk[..n]).map_err(error)?;limit.bytes+=n64;copied+=n64;
        }
        output.sync_all().map_err(error)?;
        let after=restricted_meta(src)?;
        if copied!=meta.len()||after.len()!=meta.len()||after.modified().ok()!=meta.modified().ok(){return Err("The source changed while copying. Retry the copy.".into());}
    }
    Ok(())
}
fn bucket(root:&Path)->Result<PathBuf,String>{
    fs::create_dir_all(root).map_err(error)?;restricted_meta(root)?;
    for n in 0..1000 {
        let p=root.join(format!("item-{}-{}",now(),n));
        match fs::create_dir(&p){Ok(())=>return Ok(p),Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,Err(e)=>return Err(error(e))}
    }
    Err("Unable to create a shelf folder.".into())
}
fn unique_paths(paths:Vec<PathBuf>)->Vec<PathBuf>{
    let mut seen=HashSet::new();paths.into_iter().filter(|p|seen.insert(p.to_string_lossy().to_lowercase())).collect()
}
pub(super) fn add(ui:&mut Ui,paths:Vec<PathBuf>,copy:bool)->Result<String,String>{
    let previous=ui.records.get("files").cloned().unwrap_or_default();
    let paths=unique_paths(paths);
    if paths.is_empty(){return Ok("No items selected.".into());}
    let already:HashSet<_>=previous.iter().filter(|r|r.get(2).is_some_and(|s|s=="link")).filter_map(|r|r.first().map(|s|s.to_lowercase())).collect();
    let paths=paths.into_iter().filter(|p|copy||!already.contains(&p.to_string_lossy().to_lowercase())).collect::<Vec<_>>();
    if paths.len()+previous.len()>24{return Err("The shelf holds 24 items.".into());}
    let root=storage(ui);
    let used=previous.iter().filter(|r|r.get(2).is_some_and(|s|s=="copy")).filter_map(|r|r.get(3)?.parse::<u64>().ok()).sum::<u64>();
    let mut made=Vec::new();
    let mut rows=previous.clone();
    let result=(||{
        let mut budget=Limit::default();
        for path in paths {
            if !path.is_absolute()||!path.exists(){return Err("Choose an existing file or folder.".into());}
            let name=path.file_name().ok_or("Choose a file or a named folder.")?;
            let base=budget.bytes;
            let dest=if copy {
                let canonical=fs::canonicalize(&path).map_err(error)?;
                let dir=bucket(&root)?;
                made.push(dir.clone());
                if fs::canonicalize(&dir).map_err(error)?.starts_with(&canonical){return Err("Choose an item outside the saved-copy directory.".into());}
                let dest=dir.join(name);
                copy_tree(&path,&dest,&mut budget,0,MAX_BATCH.min(MAX_STORE.saturating_sub(used)))?;
                dest
            }else{path.clone()};
            rows.insert(0,vec![dest.to_string_lossy().into_owned(),name.to_string_lossy().into_owned(),if copy{"copy"}else{"link"}.into(),(budget.bytes-base).to_string()]);
        }
        ui.records.insert("files".into(),rows);ui.save()
    })();
    if let Err(e)=result{ui.records.insert("files".into(),previous);for p in made{let _=fs::remove_dir_all(p);}return Err(e);}
    Ok("Shelf updated.".into())
}
pub(super) fn relocate(ui:&mut Ui,destination:PathBuf)->Result<String,String>{
    let original=storage(ui);
    fs::create_dir_all(&original).map_err(error)?;
    let source=fs::canonicalize(&original).map_err(error)?;
    let parent=fs::canonicalize(destination).map_err(error)?;
    let new=parent.join("Buddy Saved Files");
    if new==source{return Ok("This is the current shelf location.".into());}
    if new.exists()||new.starts_with(&source)||source.starts_with(&new){return Err("Choose a separate folder without an existing Buddy Saved Files directory.".into());}
    let before=ui.records.clone();
    fs::create_dir(&new).map_err(error)?;
    let result=(||{
        let mut limit=Limit::default();
        for entry in fs::read_dir(&source).map_err(error)?{let entry=entry.map_err(error)?;copy_tree(&entry.path(),&new.join(entry.file_name()),&mut limit,0,MAX_STORE)?;}
        for r in ui.records.entry("files".into()).or_default().iter_mut(){
            if r.get(2).is_some_and(|s|s=="copy") {
                let path=fs::canonicalize(&r[0]).map_err(error)?;
                let relative=path.strip_prefix(&source).map_err(|_|"A copy points outside the active storage folder.".to_string())?;
                let dest=new.join(relative);
                if !dest.exists(){return Err("A saved copy could not be verified.".into());}r[0]=dest.to_string_lossy().into_owned();
            }
        }
        ui.records.insert("shelf_location".into(),vec![vec![new.to_string_lossy().into_owned()]]);ui.save()
    })();
    if let Err(e)=result {ui.records=before;let _=fs::remove_dir_all(&new);return Err(e);}
    Ok(format!("Saved copies moved. Previous copies remain at {}.",original.display()))
}
pub(crate) fn valid_url(s:&str)->bool{
    if s.len()>4096||s.chars().any(|c|c.is_whitespace()||c.is_control()||c=='\\'){return false;}
    let Some((scheme,rest))=s.split_once("://") else{return false};
    let host=rest.split(['/', '?', '#']).next().unwrap_or("");
    ["https","http"].iter().any(|p|scheme.eq_ignore_ascii_case(p))&&!host.is_empty()&&!host.starts_with(':')&&!host.contains('@')
}
pub(super) fn open_selected(ui:&Ui,page:&str,row:&[String],reveal:bool)->Result<String,String>{
    let target=row.first().ok_or("No item selected.")?;
    if page=="quick"&&row.get(2).is_some_and(|s|s=="web"){
        if !valid_url(target){return Err("Only HTTP and HTTPS links may be opened.".into());}
        platform::open(ui.hwnd.0,OsStr::new(target))?;return Ok("Opened.".into());
    }
    let path=Path::new(target);if !path.is_absolute()||!path.exists(){return Err("The item is unavailable.".into());}
    if page=="downloads"{let root=platform::downloads().ok_or("Downloads unavailable.")?;let parent=fs::canonicalize(root).map_err(error)?;
      let path=fs::canonicalize(path).map_err(error)?;if path.parent()!=Some(&parent){return Err("Item is outside Downloads.".into());}}
    if page=="quick"&&!path.extension().is_some_and(|s|s.eq_ignore_ascii_case("exe")){return Err("This app pin is unavailable.".into());}
    if reveal{platform::reveal(path)?;}else{platform::open(ui.hwnd.0,path.as_os_str())?;}Ok("Opened.".into())
}
pub(super) fn trash(ui:&mut Ui,index:usize,row:&[String])->Result<String,String>{
    if row.get(2).map(String::as_str)!=Some("copy"){return Err("Only Buddy-managed copies can be removed here.".into());}
    let root=fs::canonicalize(storage(ui)).map_err(error)?;
    let src=fs::canonicalize(row.first().ok_or("Select a copy.")?).map_err(error)?;
    let bucket=src.parent().ok_or("Invalid saved copy.")?;
    if bucket.parent()!=Some(root.as_path())||!bucket.file_name().is_some_and(|s|s.to_string_lossy().starts_with("item-")){return Err("This path is outside the managed shelf.".into());}
    if !ui.isolated && unsafe{MessageBoxW(ui.hwnd.0,platform::wide("Remove this Buddy saved copy? Your original file or folder is kept.").as_ptr(),platform::wide("Buddy").as_ptr(),MB_YESNO|MB_ICONQUESTION)}!=IDYES{return Ok("Canceled.".into());}
    let previous=ui.records.get("files").cloned().unwrap_or_default();
    ui.records.get_mut("files").ok_or("Select a file.")?.remove(index);
    if let Err(error)=ui.save(){
        ui.records.insert("files".into(),previous);
        return Err(error);
    }
    if let Err(error)=platform::recycle(ui.hwnd.0,&src){
        ui.records.insert("files".into(),previous);
        ui.save()?;
        return Err(error);
    }
    let _=fs::remove_dir(bucket);
    Ok("Moved the saved copy to the Recycle Bin.".into())
}
pub(super) fn location(ui:&Ui)->PathBuf{storage(ui)}
#[derive(Default)]pub(super) struct Downloads {
    seen: HashMap<PathBuf,(u64,u128)>,
    settled: HashMap<PathBuf,(u64,u128,u128)>,
    initialized:bool,
}
impl Downloads {
    pub fn refresh(&mut self,dir:&Path,rows:&mut Vec<Vec<String>>,cleared_at:u128)->bool{
        let Ok(entries)=fs::read_dir(dir) else{return false};
        let stamp=now();
        let mut changed=false;
        let mut current=HashMap::new();let mut files=Vec::new();
        for e in entries.flatten() {
            let Ok(m)=fs::symlink_metadata(e.path()) else{continue};
            if !m.is_file()||m.file_attributes()&0x400!=0{continue;}
            let modified=m.modified().ok().and_then(|t|t.duration_since(UNIX_EPOCH).ok()).map(|d|d.as_millis()).unwrap_or(0);
            current.insert(e.path(),(m.len(),modified));files.push((modified,e.path(),m.len()));
        }
        if !self.initialized{
            files.sort_by_key(|(m,_,_)|std::cmp::Reverse(*m));files.truncate(24);
        }
        files.sort_by_key(|(m,_,_)|*m);
        for (modified,path,len) in files {
            if !self.initialized && cleared_at>0 && modified<=cleared_at {continue;}
            let name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let lower=name.to_lowercase();let partial=[".crdownload",".part",".partial",".download",".opdownload"].iter().any(|x|lower.ends_with(x));
            let old=self.seen.get(&path);
            let stable=self.settled.get(&path).is_some_and(|&(l,m,s)|l==len&&m==modified&&stamp.saturating_sub(s)>=1500);
            let recent=stamp.saturating_sub(modified)<1800;
            let status=if partial||(!stable&&recent){"Downloading"}else{"Complete"};
            let value=path.to_string_lossy().into_owned();
            let found=rows.iter().position(|r|r.first()==Some(&value));
            if self.initialized&&old==Some(&(len,modified))&&!stable{continue;}
            if self.initialized&&old==Some(&(len,modified))&&found.is_none(){continue;}
            let row=vec![value,name,status.into(),len.to_string(),modified.to_string()];
            if found.is_some_and(|i|rows[i]==row){continue;}
            if let Some(i)=found{rows.remove(i);}rows.insert(0,row);changed=true;
            if !stable {self.settled.insert(path,(len,modified,stamp));}
        }
        // Mark vanished files explicitly. When a browser renames a known partial,
        // only the final entry remains visible after the same-directory rescan.
        let before=rows.len();
        rows.retain(|r|r.first().is_some_and(|p|current.contains_key(Path::new(p))));
        changed|=rows.len()!=before;rows.truncate(24);
        self.settled.retain(|p,_|current.contains_key(p));self.seen=current;self.initialized=true;
        changed
    }
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn web_targets(){assert!(valid_url("https://example.com/a"));assert!(!valid_url("file:///C:/"));assert!(!valid_url("https://good.example\\@other"));assert!(!valid_url("https://"));}
    #[test]fn downloads_tracks_renames_and_clear(){let d=std::env::temp_dir().join(format!("buddy-native-dl-{}",now()));fs::create_dir(&d).unwrap();let partial=d.join("report.pdf.crdownload");fs::write(&partial,[1,2,3]).unwrap();
        let mut track=Downloads::default();let mut rows=Vec::new();assert!(track.refresh(&d,&mut rows,0));assert_eq!(rows[0][2],"Downloading");assert_eq!(rows[0][3],"3");fs::rename(partial,d.join("report.pdf")).unwrap();assert!(track.refresh(&d,&mut rows,0));assert_eq!(rows.len(),1);assert_eq!(rows[0][1],"report.pdf");rows.clear();track.refresh(&d,&mut rows,0);assert!(rows.is_empty());let mut restarted=Downloads::default();restarted.refresh(&d,&mut rows,now());assert!(rows.is_empty());fs::remove_dir_all(d).unwrap();}
}

#[cfg(test)]mod copy_tests{
    use super::*;
    #[test]
    fn copies_are_independent_and_a_batch_over_budget_fails() {
        let root=std::env::temp_dir().join(format!("buddy-shelf-test-{}-{}",std::process::id(),now()));
        fs::create_dir(&root).unwrap();
        let source=root.join("source");fs::create_dir(&source).unwrap();
        fs::write(source.join("report.txt"),b"original").unwrap();
        let dest=root.join("copy");copy_tree(&source,&dest,&mut Limit::default(),0,1024).unwrap();
        fs::write(source.join("report.txt"),b"changed").unwrap();
        assert_eq!(fs::read(dest.join("report.txt")).unwrap(),b"original");
        assert!(copy_tree(&source,&root.join("limited"),&mut Limit::default(),0,2).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
