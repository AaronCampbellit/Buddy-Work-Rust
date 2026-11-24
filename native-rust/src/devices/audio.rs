use std::{ffi::c_void,os::windows::ffi::OsStringExt,ptr};
use windows::{core::{GUID,HRESULT,HSTRING,IUnknown,Interface,PCWSTR,PWSTR},Win32::{
    Media::Audio::{*,Endpoints::{IAudioEndpointVolume,IAudioMeterInformation}},
    Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
    System::Com::{CoCreateInstance,CoTaskMemFree,CLSCTX_ALL,STGM_READ,StructuredStorage::{PropVariantClear,PropVariantToStringAlloc}}
}};

pub(super) unsafe fn take_text(p:PWSTR)->String {
    if p.is_null(){return String::new()}
    let text=unsafe{p.to_string()}.unwrap_or_default(); unsafe{CoTaskMemFree(Some(p.0.cast()))}; text
}
#[derive(Clone,Debug)]
pub struct Row {
    pub id:String, pub session:Option<String>, pub name:String, pub active:bool,
    pub flow:EDataFlow, pub volume:Option<f32>, pub mute:Option<bool>, pub peak:Option<f32>, pub roles:Vec<&'static str>,
}
impl Row {
    pub fn label(&self)->String{
        let kind=if self.session.is_some(){"App"}else if self.flow==eCapture{"Microphone"}else{"Speaker"};
        let volume=self.volume.map(|v|format!(" · {:.0}%",v*100.)).unwrap_or_default();
        let mute=if self.mute==Some(true){" · muted"}else{""};
        let peak=self.peak.map(|p|format!(" · peak {:.0}%",p*100.)).unwrap_or_default();
        let default=if self.roles.is_empty(){String::new()}else{format!(" · {}",self.roles.join(", "))};
        format!("{kind} · {}{volume}{mute}{peak}{default}{}",self.name,if self.active{""}else{" · unavailable"})
    }
}
fn default_id(e:&IMMDeviceEnumerator,flow:EDataFlow,role:ERole)->Option<String>{
    unsafe{e.GetDefaultAudioEndpoint(flow,role).and_then(|d|d.GetId()).ok().map(|p|take_text(p))}
}
fn enumerator()->windows::core::Result<IMMDeviceEnumerator>{unsafe{CoCreateInstance(&MMDeviceEnumerator,None,CLSCTX_ALL)}}
fn endpoint_name(d:&IMMDevice)->Option<String>{
    unsafe {
        let store=d.OpenPropertyStore(STGM_READ).ok()?; let mut v=store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let name=PropVariantToStringAlloc(&v).ok().map(|p|take_text(p)); let _=PropVariantClear(&mut v);name.filter(|s|!s.is_empty())
    }
}
fn sessions(d:&IMMDevice)->windows::core::Result<Vec<(String,IAudioSessionControl,IAudioSessionControl2,ISimpleAudioVolume)>>{
    unsafe {
        let m:IAudioSessionManager2=d.Activate(CLSCTX_ALL,None)?;let list=m.GetSessionEnumerator()?;let mut result=Vec::new();
        for i in 0..list.GetCount()?.min(256){
            let item=(|| -> windows::core::Result<_>{
                let s=list.GetSession(i)?;let v:ISimpleAudioVolume=s.cast()?;let s2:IAudioSessionControl2=s.cast()?;
                Ok((take_text(s2.GetSessionInstanceIdentifier()?),s,s2,v))
            })();
            if let Ok(row)=item {if row.1.GetState().ok()!=Some(AudioSessionStateExpired){result.push(row);}}
        } Ok(result)
    }
}
fn app_name(pid:u32)->Option<String>{
    use windows::Win32::{Foundation::CloseHandle,System::Threading::{
        OpenProcess,QueryFullProcessImageNameW,PROCESS_NAME_WIN32,PROCESS_QUERY_LIMITED_INFORMATION
    }};
    if pid==0 {return Some("System sounds".into());}
    unsafe {
        let handle=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,false,pid).ok()?;
        let mut buffer=[0u16;1024];
        let mut n=buffer.len() as u32;
        let result=QueryFullProcessImageNameW(handle,PROCESS_NAME_WIN32,PWSTR(buffer.as_mut_ptr()),&mut n);
        let _=CloseHandle(handle);
        result.ok()?;
        let path=std::ffi::OsString::from_wide(&buffer[..n as usize]);
        std::path::Path::new(&path).file_stem().map(|s|s.to_string_lossy().into_owned())
    }
}
pub fn list()->Result<Vec<Row>,String>{
    let e=enumerator().map_err(|e|e.to_string())?;
    let mut rows=Vec::new();
    for flow in [eRender,eCapture] {
        let defaults=[(eConsole,"console"),(eMultimedia,"default"),(eCommunications,"communications")].map(|(r,l)|(default_id(&e,flow,r),l));
        unsafe {
            let list=e.EnumAudioEndpoints(flow,DEVICE_STATE(DEVICE_STATEMASK_ALL)).map_err(|e|e.to_string())?;
            for idx in 0..list.GetCount().map_err(|e|e.to_string())?.min(128) {
                let Ok(d)=list.Item(idx) else{continue};
                let Ok(id)=d.GetId().map(|p|take_text(p)) else{continue};
                let active=d.GetState().ok()==Some(DEVICE_STATE_ACTIVE);
                let vol=if active {d.Activate::<IAudioEndpointVolume>(CLSCTX_ALL,None).ok()} else {None};
                let peak=if active {d.Activate::<IAudioMeterInformation>(CLSCTX_ALL,None).and_then(|m|m.GetPeakValue()).ok()} else {None};
                let roles=defaults.iter().filter(|(s,_)|s.as_deref()==Some(&id)).map(|(_,n)|*n).collect();
                rows.push(Row{id:id.clone(),session:None,name:endpoint_name(&d).unwrap_or_else(||id.clone()),active,flow,
                    volume:vol.as_ref().and_then(|v|v.GetMasterVolumeLevelScalar().ok()),mute:vol.and_then(|v|v.GetMute().ok()).map(|b|b.as_bool()),roles,peak});
                if active{for (sid,s,s2,v) in sessions(&d).unwrap_or_default(){
                    let text=s.GetDisplayName().map(|p|take_text(p)).unwrap_or_default();
                    let pid=s2.GetProcessId().unwrap_or(0);
                    let name=if pid==0{"System sounds".into()}else if text.is_empty()||text.starts_with('@'){app_name(pid).unwrap_or_else(||format!("Audio app (PID {pid})"))}else{text};
                    rows.push(Row{id:id.clone(),session:Some(sid),name,active,flow,volume:v.GetMasterVolume().ok(),mute:v.GetMute().ok().map(|b|b.as_bool()),roles:Vec::new(),peak:s.cast::<IAudioMeterInformation>().and_then(|m|m.GetPeakValue()).ok()});
                }}
            }
        }
    } Ok(rows)
}
// Optional system policy interface also used by the original Buddy audio bridge.
// QueryInterface checks the exact IID before invoking the sole method Buddy uses.
#[repr(C)]struct PolicyVtable{base:windows::core::IUnknown_Vtbl,unused:[usize;10],set_default:unsafe extern "system" fn(*mut c_void,PCWSTR,ERole)->HRESULT}
fn change_default(e:&IMMDeviceEnumerator,row:&Row,roles:&[ERole])->Result<(),String>{
    unsafe {
        let obj:IUnknown=CoCreateInstance(&GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9),None,CLSCTX_ALL).map_err(|_|"Use Windows Sound settings to change the default.")?;
        let mut raw=ptr::null_mut();
        (obj.vtable().QueryInterface)(obj.as_raw(),&GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8), &mut raw).ok().map_err(|_|"This Windows version does not expose default-device controls.")?;
        let policy=IUnknown::from_raw(raw);let table=&**(raw as *const *const PolicyVtable);
        let set=|id:&str,r|{let s=HSTRING::from(id);(table.set_default)(policy.as_raw(),PCWSTR(s.as_ptr()),r).ok()};
        let old=roles.iter().map(|r|default_id(e,row.flow,*r).ok_or("Current Windows default is unavailable.")).collect::<Result<Vec<_>,_>>()?;
        let mut changed=Vec::new();
        for (i,&role) in roles.iter().enumerate(){
            if old[i]==row.id {continue}
            if set(&row.id,role).is_err() || default_id(e,row.flow,role).as_deref()!=Some(&row.id){
                let _=set(&old[i],role);
                for previous in changed.into_iter().rev(){let pi:usize=previous;let _=set(&old[pi],roles[pi]);}
                return Err("Windows did not confirm the default-device change.".into());
            }
            changed.push(i);
        }
        Ok(())
    }
}
pub fn apply(row:&Row,action:&str,input:&str)->Result<(),String>{
    let e=enumerator().map_err(|e|e.to_string())?;
    unsafe{
        let d=e.GetDevice(&HSTRING::from(&row.id)).map_err(|_|"Device is no longer available.")?;
        if d.GetState().ok()!=Some(DEVICE_STATE_ACTIVE){return Err("Device is no longer active.".into());}
        if action.starts_with("default") {
            if row.session.is_some(){return Err("Select a microphone or speaker.".into());}
            let roles=match action{"default"=>vec![eConsole,eMultimedia],"default-comm"=>vec![eCommunications],"default-all"=>vec![eConsole,eMultimedia,eCommunications],_=>return Err("Unknown routing role.".into())};
            return change_default(&e,row,&roles)
        }
        let volume=if action=="volume"{let n=input.trim().trim_end_matches('%').parse::<f32>().map_err(|_|"Enter volume from 0 to 100.")?;
            if !n.is_finite() || !(0.0..=100.0).contains(&n){return Err("Enter volume from 0 to 100.".into());} Some(n/100.)}else if action=="mute"{None}else{return Err("Unknown audio control.".into())};
        if let Some(sid)=&row.session {
            let (_,_,_,v)=sessions(&d).map_err(|e|e.to_string())?.into_iter().find(|s|&s.0==sid).ok_or("That app's audio session has ended.")?;
            if let Some(vol)=volume {v.SetMasterVolume(vol,ptr::null())}else{v.SetMute(!v.GetMute().map_err(|e|e.to_string())?.as_bool(),ptr::null())}
        } else {
            let v:IAudioEndpointVolume=d.Activate(CLSCTX_ALL,None).map_err(|_|"This endpoint has no volume control.")?;
            if let Some(vol)=volume {v.SetMasterVolumeLevelScalar(vol,ptr::null())}else{v.SetMute(!v.GetMute().map_err(|e|e.to_string())?.as_bool(),ptr::null())}
        }.map_err(|e|e.to_string())
    }
}
