use serde_json::Value;
use std::{ffi::OsStr,io::{BufRead,BufReader,Write},os::windows::process::CommandExt,path::{Path,PathBuf},process::{Child,ChildStdin,Command,Stdio},sync::mpsc::{sync_channel,Receiver},thread,time::Duration};

pub fn installed()->Result<PathBuf,String>{
    // ProgramW6432 points to native Program Files even for a redirected caller.
    let base=std::env::var_os("ProgramW6432").or_else(||std::env::var_os("ProgramFiles")).ok_or("Cannot find Windows Program Files.")?;
    let p=PathBuf::from(base).join("Buddy Camera").join("BuddyCameraHost.exe");
    if !p.is_file(){return Err("Install the Buddy Camera component to use the studio.".into());} Ok(p)
}
const MAX_LINE:usize=4*1024*1024;
pub fn reply<R:BufRead>(reader:&mut R)->Result<Value,String>{
    let mut bytes=Vec::new();
    loop{
        let available=reader.fill_buf().map_err(|e|e.to_string())?;
        if available.is_empty(){return Err("Camera helper disconnected.".into());}
        let end=available.iter().position(|b|*b==b'\n');
        let n=end.unwrap_or(available.len());
        if bytes.len()+n>MAX_LINE{return Err("Camera helper response exceeds its limit.".into());}
        bytes.extend_from_slice(&available[..n]);reader.consume(n+usize::from(end.is_some()));
        if end.is_some(){break}
    }
    let v:Value=serde_json::from_slice(&bytes).map_err(|_|"Unrecognized camera helper response.")?;
    if !v["ok"].is_boolean(){return Err("Incomplete camera response.".into());} Ok(v)
}
pub fn accepted(v:Value)->Result<Value,String>{
    if v["ok"]==true{Ok(v)}else{Err(v["message"].as_str().unwrap_or("Camera command failed.").to_owned())}
}
pub struct Producer {child:Child,stdin:Option<ChildStdin>,rx:Receiver<Result<Value,String>>}
impl Producer{
    pub fn start(path:&Path,args:&[impl AsRef<OsStr>])->Result<Self,String>{
        let mut child=Command::new(path).args(args).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e|e.to_string())?;
        let out=child.stdout.take().ok_or("Camera stdout unavailable.")?;
        let stdin=child.stdin.take();
        let (tx,rx)=sync_channel(1);thread::spawn(move||{
            let mut stream=BufReader::new(out);
            loop {let line=reply(&mut stream);let failed=line.is_err();if tx.send(line).is_err()||failed{break;}}
        });
        Ok(Self{child,stdin,rx})
    }
    pub fn read(&mut self,secs:u64)->Result<Value,String>{
        let result=self.rx.recv_timeout(Duration::from_secs(secs)).map_err(|_|"Camera helper timed out or closed.".to_owned()).and_then(|v|v);
        if result.is_err(){let _=self.child.kill();} result
    }
    pub fn send(&mut self,line:&str)->Result<Value,String>{
        if line.contains(['\r','\n','\0']){return Err("Invalid camera command.".into());}
        let stream=self.stdin.as_mut().ok_or("Camera stopped.")?;
        writeln!(stream,"{line}").and_then(|_|stream.flush()).map_err(|e|e.to_string())?;
        self.read(15)
    }
    pub fn alive(&mut self)->bool{self.child.try_wait().ok()==Some(None)}
    pub fn close(&mut self,detach:bool){
        if self.alive(){if let Some(mut s)=self.stdin.take(){let _=writeln!(s,"{}",if detach{"detach"}else{"stop"});}}
        for _ in 0..25{if !self.alive(){return}thread::sleep(Duration::from_millis(100));}
        let _=self.child.kill();let _=self.child.wait();
    }
}
impl Drop for Producer{fn drop(&mut self){if self.alive(){let _=self.child.kill();}let _=self.child.wait();}}
pub fn query(path:&Path,args:&[impl AsRef<OsStr>],secs:u64)->Result<Value,String>{
    let mut h=Producer::start(path,args)?;let r=h.read(secs);
    h.stdin.take();for _ in 0..25{if !h.alive(){break;}thread::sleep(Duration::from_millis(20));}r.and_then(accepted)
}
pub fn quote(input:&str)->Result<String,String>{
    if input.len()>32768||input.chars().any(char::is_control){return Err("Camera parameter contains invalid text.".into());}
    Ok(format!("\"{}\"",input.replace('\\',"\\\\").replace('"',"\\\"")))
}
#[cfg(test)]mod tests{use super::*;#[test]fn line_protocol(){let mut b=BufReader::new(b"{\"ok\":true}\n{\"ok\":false,\"message\":\"busy\"}\n".as_slice());assert_eq!(reply(&mut b).unwrap()["ok"],true);assert_eq!(accepted(reply(&mut b).unwrap()).unwrap_err(),"busy");assert!(reply(&mut b).is_err());assert!(quote("x\ny").is_err());assert_eq!(quote(r#"C:\me\a"b"#).unwrap(),r#""C:\\me\\a\"b""#);}}
pub fn installer()->Result<PathBuf,String>{
    use sha2::{Digest,Sha256};
    let folder=std::env::current_exe().map_err(|e|e.to_string())?.parent().ok_or("Missing application folder.")?.join("camera");
    let manifest:Value=serde_json::from_str(include_str!("component-manifest.json")).expect("embedded camera manifest");
    for (name,hash) in manifest["hashes"].as_object().unwrap(){
        let bytes=std::fs::read(folder.join(name)).map_err(|_|format!("Camera package is missing {name}."))?;
        if hash.as_str()!=Some(format!("{:x}",Sha256::digest(bytes)).as_str()){return Err("Camera package integrity check failed. Reinstall Buddy.".into());}
    }
    Ok(folder.join("BuddyCameraHost.exe"))
}
