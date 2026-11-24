use super::{Outcome,View};
use std::ffi::c_void;
use windows::{Foundation::Uri, core::HSTRING};

type H = *mut c_void;
#[link(name="winhttp")]
unsafe extern "system" {
    fn WinHttpOpen(agent:*const u16,access:u32,proxy:*const u16,bypass:*const u16,flags:u32)->H;
    fn WinHttpConnect(session:H,host:*const u16,port:u16,reserved:u32)->H;
    fn WinHttpOpenRequest(connection:H,verb:*const u16,target:*const u16,version:*const u16,referer:*const u16,accept:*const *const u16,flags:u32)->H;
    fn WinHttpSetTimeouts(session:H,resolve:i32,connect:i32,send:i32,receive:i32)->i32;
    fn WinHttpSetOption(request:H,option:u32,data:*mut c_void,size:u32)->i32;
    fn WinHttpSendRequest(request:H,headers:*const u16,hlen:u32,data:*const c_void,len:u32,total:u32,context:usize)->i32;
    fn WinHttpReceiveResponse(request:H,reserved:*mut c_void)->i32;
    fn WinHttpQueryHeaders(request:H,level:u32,name:*const u16,buffer:*mut c_void,size:*mut u32,index:*mut u32)->i32;
    fn WinHttpReadData(request:H,buffer:*mut c_void,size:u32,read:*mut u32)->i32;
    fn WinHttpCloseHandle(h:H)->i32;
}
struct Handle(H);
impl Handle {fn new(h:H)->Result<Self,String>{if h.is_null(){Err("Windows HTTPS connection failed.".into())}else{Ok(Self(h))}}}
impl Drop for Handle {fn drop(&mut self){unsafe{WinHttpCloseHandle(self.0);}}}
fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
fn url(text:&str)->Result<Uri,String>{
    if text.len()>4096 || text.chars().any(|c|c.is_control() || c=='\\') {return Err("Invalid update URL.".into());}
    let uri=Uri::CreateUri(&HSTRING::from(text)).map_err(|_|"Invalid update URL.")?;
    if uri.SchemeName().map_err(|_|"Invalid scheme.")?.to_string()!="https" || uri.Host().map_err(|_|"Invalid host.")?.is_empty() ||
       !uri.UserName().map_err(|_|"Invalid URL.")?.is_empty() || !uri.Password().map_err(|_|"Invalid URL.")?.is_empty(){return Err("A credential-free HTTPS URL is required.".into());}
    Ok(uri)
}
fn get(uri:&Uri)->Result<Vec<u8>,String>{
    use std::ptr::{null,null_mut};
    let host=wide(&uri.Host().map_err(|_|"Invalid host.")?.to_string());
    let target=wide(&format!("{}{}",uri.Path().map_err(|_|"Invalid path.")?,uri.Query().map_err(|_|"Invalid query.")?));
    let port=u16::try_from(uri.Port().map_err(|_|"Invalid port.")?).map_err(|_|"Invalid port.")?;
    unsafe {
        let session=Handle::new(WinHttpOpen(wide("Buddy Work Rust").as_ptr(),4,null(),null(),0))?;
        if WinHttpSetTimeouts(session.0,10000,10000,10000,10000)==0 {return Err("Unable to set HTTPS timeouts.".into());}
        let conn=Handle::new(WinHttpConnect(session.0,host.as_ptr(),port,0))?;
        let req=Handle::new(WinHttpOpenRequest(conn.0,wide("GET").as_ptr(),target.as_ptr(),null(),null(),null(),0x00800000))?;
        // WINHTTP_OPTION_DISABLE_FEATURE with WINHTTP_DISABLE_REDIRECTS, AUTHENTICATION and COOKIES.
        let mut disable=1u32|2|4;
        if WinHttpSetOption(req.0,63,(&mut disable as *mut u32).cast(),4)==0 || WinHttpSendRequest(req.0,null(),0,null(),0,0,0)==0 || WinHttpReceiveResponse(req.0,null_mut())==0 {return Err("The update channel is unavailable.".into());}
        let mut status=0u32;let mut bytes=4u32;
        if WinHttpQueryHeaders(req.0,19|0x2000_0000,null(),(&mut status as *mut u32).cast(),&mut bytes,null_mut())==0 || status!=200 {return Err("The update channel returned an unexpected status.".into());}
        let mut data=Vec::new();let mut buffer=[0u8;8192];
        loop {
            bytes=0;
            if WinHttpReadData(req.0,buffer.as_mut_ptr().cast(),buffer.len() as u32,&mut bytes)==0{return Err("Unable to read update manifest.".into());}
            if bytes==0 {return Ok(data);}
            if data.len()+bytes as usize>65536{return Err("Manifest exceeds 64 KiB.".into());}
            data.extend_from_slice(&buffer[..bytes as usize]);
        }
    }
}
fn order(version:&str)->Option<(u32,u32,u32,bool,u32)>{
    let (main,beta)=match version.split_once("-beta.") {Some((m,b))=>(m,Some(b.parse().ok()?)),None=>(version,None)};
    let mut v=main.split('.');
    let out=(v.next()?.parse().ok()?,v.next()?.parse().ok()?,v.next()?.parse().ok()?,beta.is_none(),beta.unwrap_or(0));
    v.next().is_none().then_some(out)
}
pub(super) fn check()->Outcome {
    let Some(endpoint)=std::env::var_os("BUDDY_UPDATE_MANIFEST_URL") else{return Ok(View::status("No update channel is configured for this work edition."));};
    let data=get(&url(&endpoint.to_string_lossy())?)?;
    let manifest:serde_json::Value=serde_json::from_slice(&data).map_err(|_|"Invalid update manifest.")?;
    let version=manifest["version"].as_str().ok_or("Missing release version.")?;
    let release=manifest["url"].as_str().ok_or("Missing release URL.")?;
    url(release)?;
    let current=env!("CARGO_PKG_VERSION");
    if order(version).ok_or("Invalid release version.")? > order(current).ok_or("Invalid build version.")? {
        Ok(View{url:Some(release.into()),status:format!("Buddy {version} is available."),..Default::default()})
    } else {Ok(View::status(&format!("Buddy {current} is up to date.")))}
}
#[cfg(test)]
mod tests {
    #[test] fn stable_promotes_beta(){assert!(super::order("0.1.1")>super::order("0.1.1-beta.41"));assert!(super::order("2.0.0-beta.10")>super::order("2.0.0-beta.2"));assert!(super::order("1.0.1.2").is_none());}
}
