use std::cell::RefCell;
use windows::{core::{w, BSTR, Interface}, Win32::{
    Media::{DirectShow::*, MediaFoundation::{CLSID_CaptureGraphBuilder2,CLSID_FilterGraph,CLSID_SystemDeviceEnum,CLSID_VideoInputDeviceCategory,MEDIATYPE_Video,PIN_CATEGORY_PREVIEW}},
    System::{Com::{CoCreateInstance, CreateBindCtx, CLSCTX_INPROC_SERVER, IMoniker, StructuredStorage::IPropertyBag}, Variant::{VARIANT,VariantClear,VariantToStringAlloc}}
}};

#[derive(Clone)]
pub struct Camera {pub name:String, id:String, moniker:IMoniker}
pub fn list()->Result<Vec<Camera>,String>{
    unsafe {
        let dev:ICreateDevEnum=CoCreateInstance(&CLSID_SystemDeviceEnum,None,CLSCTX_INPROC_SERVER).map_err(|e|e.to_string())?;
        let mut en=None; dev.CreateClassEnumerator(&CLSID_VideoInputDeviceCategory,&mut en,0).map_err(|e|e.to_string())?;
        let Some(en)=en else{return Ok(vec![])};let ctx=CreateBindCtx(0).map_err(|e|e.to_string())?;
        let mut cameras=Vec::new();
        while cameras.len()<128{
            let mut row=[None];if en.Next(&mut row,None).is_err(){break}
            let Some(m)=row[0].take() else{break};
            let id=m.GetDisplayName(&ctx,None).ok().map(|p|super::audio::take_text(p)).unwrap_or_default();
            let name=m.BindToStorage::<_,_,IPropertyBag>(&ctx,None).ok().and_then(|bag|{
                let mut v=VARIANT::default();
                let name=bag.Read(w!("FriendlyName"),&mut v,None).and_then(|_|VariantToStringAlloc(&v)).ok().map(|p|super::audio::take_text(p));
                let _=VariantClear(&mut v);name
            }).unwrap_or_else(||id.clone());
            if !id.is_empty() && !name.to_lowercase().contains("buddy camera"){cameras.push(Camera{name,id,moniker:m});}
        }
        Ok(cameras)
    }
}
struct Preview {id:String, source:IBaseFilter, control:IMediaControl, view:IVideoWindow, _graph:IGraphBuilder}
impl Drop for Preview {fn drop(&mut self){unsafe{let _=self.control.Stop();let _=self.view.SetVisible(0);let _=self.view.SetOwner(0);}}}
thread_local!{static PREVIEW:RefCell<Option<Preview>>=const{RefCell::new(None)};}
pub fn stop(){PREVIEW.with(|p|{p.borrow_mut().take();});}
pub fn release_hidden_preview() {
    PREVIEW.with(|slot| {
        let hidden = slot.borrow().as_ref().is_some_and(|p| unsafe {
            p.view.Visible().map_or(true, |visible| visible == 0)
        });
        if hidden {slot.borrow_mut().take();}
    });
}
fn source(camera:&Camera)->windows::core::Result<IBaseFilter>{
    if let Some(s)=PREVIEW.with(|p|p.borrow().as_ref().filter(|p|p.id==camera.id).map(|p|p.source.clone())){return Ok(s);}
    unsafe{camera.moniker.BindToObject(&CreateBindCtx(0)?,None)}
}
pub fn preview(camera:&Camera,host:windows_sys::Win32::Foundation::HWND)->Result<(),String>{
    stop();
    let result=(|| -> windows::core::Result<Preview>{unsafe {
        let src=source(camera)?;
        let graph:IGraphBuilder=CoCreateInstance(&CLSID_FilterGraph,None,CLSCTX_INPROC_SERVER)?;
        let builder:ICaptureGraphBuilder2=CoCreateInstance(&CLSID_CaptureGraphBuilder2,None,CLSCTX_INPROC_SERVER)?;
        builder.SetFiltergraph(&graph)?;graph.AddFilter(&src,w!("Selected camera"))?;
        builder.RenderStream(Some(&PIN_CATEGORY_PREVIEW),&MEDIATYPE_Video,&src,None,None)?;
        let view:IVideoWindow=graph.cast()?;view.SetAutoShow(0)?;
        view.SetCaption(&BSTR::from("Buddy Camera Preview — Stop in Buddy"))?;
        if host.is_null(){view.SetWindowStyle(view.WindowStyle()? & !0x80000)?;}else{
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect,WS_CHILD,WS_CLIPSIBLINGS};
            let mut r=Default::default();GetClientRect(host,&mut r);view.SetOwner(host as isize)?;
            view.SetWindowStyle((WS_CHILD|WS_CLIPSIBLINGS) as i32)?;view.SetWindowPosition(0,0,r.right.max(1),r.bottom.max(1))?;
        }
        let control:IMediaControl=graph.cast()?;
        let preview=Preview{id:camera.id.clone(),source:src,control,view,_graph:graph};
        preview.control.Run()?;preview.view.SetVisible(-1)?;Ok(preview)
    }})();
    PREVIEW.with(|p|{*p.borrow_mut()=Some(result.map_err(|e|format!("Could not preview this camera ({e}). Check Windows camera privacy and other apps."))?);Ok(())})
}
// DirectShow reports native units, bounds, manual/automatic modes and step sizes.
const PROPERTIES: &[(&str, bool, i32)] = &[
    ("Pan", false, 0), ("Tilt", false, 1), ("Zoom", false, 3),
    ("Exposure", false, 4), ("Focus", false, 6),
    ("Brightness", true, 0), ("Contrast", true, 1), ("Saturation", true, 3),
    ("Sharpness", true, 4), ("White balance", true, 7),
];
struct Axis { min:i32, max:i32, step:i32, value:i32, default:i32, caps:i32, flags:i32 }
enum Control { Lens(IAMCameraControl), Video(IAMVideoProcAmp) }
impl Control {
    fn open(s:&IBaseFilter, video:bool)->windows::core::Result<Self>{
        if video { s.cast().map(Self::Video) } else { s.cast().map(Self::Lens) }
    }
    fn read(&self, index:i32)->windows::core::Result<Axis>{
        let mut a=Axis{min:0,max:0,step:0,value:0,default:0,caps:0,flags:0};
        unsafe {
            match self {
                Self::Lens(c)=>{c.GetRange(index,&mut a.min,&mut a.max,&mut a.step,&mut a.default,&mut a.caps)?;c.Get(index,&mut a.value,&mut a.flags)?;}
                Self::Video(c)=>{c.GetRange(index,&mut a.min,&mut a.max,&mut a.step,&mut a.default,&mut a.caps)?;c.Get(index,&mut a.value,&mut a.flags)?;}
            }
        }
        a.step=a.step.max(1);
        Ok(a)
    }
    fn apply(&self,index:i32,n:i32,mode:i32)->windows::core::Result<()>{
        unsafe { match self { Self::Lens(c)=>c.Set(index,n,mode), Self::Video(c)=>c.Set(index,n,mode) } }
    }
}
pub fn ranges(camera:&Camera)->Result<String,String>{
    let s=source(camera).map_err(|e|format!("Cannot open camera controls ({e})."))?;
    let rows:Vec<_>=PROPERTIES.iter().filter_map(|(name,video,index)|{
        let a=Control::open(&s,*video).and_then(|c|c.read(*index)).ok()?;
        Some(format!("{name}    {}{} ({} to {}, step {}, default {})",a.value,if a.flags&1!=0{" auto"}else{""},a.min,a.max,a.step,a.default))
    }).collect();
    if rows.is_empty(){Err("This camera exposes no adjustable Windows controls.".into())}else{Ok(rows.join("\r\n"))}
}
pub fn set(camera:&Camera,key:&str,input:&str)->Result<(),String>{
    let &(_,video,index)=PROPERTIES.iter().find(|(name,_,_)|name.eq_ignore_ascii_case(key)).ok_or("Unknown camera property.")?;
    let c=source(camera).and_then(|s|Control::open(&s,video)).map_err(|_|"Camera controls are unavailable.")?;
    let a=c.read(index).map_err(|_|"The camera does not support this setting.")?;
    let input=input.trim();
    let (n,mode)=if input.eq_ignore_ascii_case("auto") {(a.value,1)} else if input.eq_ignore_ascii_case("default"){(a.default,2)}else{
        (input.parse::<i32>().map_err(|_|"Enter a numeric value, auto or default.")?,2)
    };
    if a.caps&mode==0{return Err("This camera does not support the requested manual or automatic mode.".into());}
    if mode==2&&(n<a.min||n>a.max||(i64::from(n)-i64::from(a.min))%i64::from(a.step)!=0){
        return Err(format!("Choose a value between {} and {} in steps of {}.",a.min,a.max,a.step));
    }
    c.apply(index,n,mode).map_err(|e|e.to_string())?;
    let actual=c.read(index).map_err(|e|e.to_string())?;
    if actual.flags&mode==0 || (mode==2 && actual.value!=n){return Err("The camera did not confirm the requested setting.".into());}
    Ok(())
}

pub(super) fn previewing()->bool{PREVIEW.with(|s|s.borrow().is_some())}

pub(super) fn size_preview(w:i32,h:i32){PREVIEW.with(|s|{if let Ok(b)=s.try_borrow(){if let Some(p)=b.as_ref(){unsafe{let _=p.view.SetWindowPosition(0,0,w,h);}}}})}
