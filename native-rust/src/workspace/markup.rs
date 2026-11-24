use super::{bitmap::Bitmap,platform};
#[path="markup_controls.rs"] mod controls;
use controls::{EditorChrome,Overlay};
use serde::{Serialize,Deserialize};
use std::{cell::RefCell,path::{Path,PathBuf},ptr::{null,null_mut},sync::{mpsc,Mutex,OnceLock},collections::HashSet};
use windows_sys::Win32::{Foundation::*,Graphics::Gdi::*,System::LibraryLoader::GetModuleHandleW,UI::{WindowsAndMessaging::*,Input::KeyboardAndMouse::{SetCapture,ReleaseCapture,GetKeyState,VK_CONTROL,VK_SHIFT,VK_DELETE}}};

#[derive(Clone,Copy,Debug,Serialize,Deserialize,PartialEq,Eq)]struct Point{i:i32,j:i32}
#[derive(Clone,Copy,Serialize,Deserialize,PartialEq,Eq)]enum Tool{Select,Pen,Arrow,Rectangle,Ellipse,Text,Highlight,Redact,Number,Crop}
#[derive(Clone,Serialize,Deserialize)]struct Mark {tool:Tool,points:Vec<Point>,text:String,ink:[u8;3],width:i32,#[serde(default="full_opacity")] opacity:u32,#[serde(default)] font_size:i32}
fn full_opacity()->u32{100}
impl Mark{fn text_size(&self)->i32{if self.font_size>0{self.font_size}else{self.width*3+12}}}
#[derive(Clone,Default,Serialize,Deserialize)]struct Document {marks:Vec<Mark>,crop:Option<[i32;4]>}
fn rect(a:Point,b:Point)->[i32;4]{[a.i.min(b.i),a.j.min(b.j),a.i.max(b.i),a.j.max(b.j)]}
fn bounds(m:&Mark)->[i32;4]{
    let p=m.points.first().copied().unwrap_or(Point{i:0,j:0});
    let mut b=[p.i,p.j,p.i,p.j];
    for p in &m.points {b[0]=b[0].min(p.i);b[1]=b[1].min(p.j);b[2]=b[2].max(p.i);b[3]=b[3].max(p.j);}
    if m.tool==Tool::Text {b[2]=b[0]+m.text.chars().count()as i32*(m.text_size()/2+1);b[3]=b[1]+m.text_size();}
    if m.tool==Tool::Number {let d=12+m.width;b=[p.i-d,p.j-d,p.i+d,p.j+d];}b
}
fn paint(b:&mut Bitmap,p:Point,r:i32,ink:[u8;3],opacity:u32) {
    let r=r.max(0);let x0=(p.i-r).max(0);let y0=(p.j-r).max(0);
    for y in y0..=(p.j+r).min(b.height as i32-1) {for x in x0..=(p.i+r).min(b.width as i32-1) {
        if (x-p.i)*(x-p.i)+(y-p.j)*(y-p.j)>r*r {continue;}
        let off=(y as usize*b.width+x as usize)*4;
        for c in 0..3{b.pixels[off+c]=((b.pixels[off+c] as u32*(255-opacity)+ink[c] as u32*opacity)/255) as u8;}
        b.pixels[off+3]=255;
    }}
}
fn line(b:&mut Bitmap,a:Point,z:Point,width:i32,ink:[u8;3],alpha:u32){
    let (mut x,mut y)=(a.i,a.j);
    let dx=(z.i-x).abs();let sx=if x<z.i{1}else{-1};let dy=-(z.j-y).abs();let sy=if y<z.j{1}else{-1};let mut err=dx+dy;
    loop {paint(b,Point{i:x,j:y},width/2,ink,alpha);if x==z.i&&y==z.j{break;}let twice=2*err;if twice>=dy{err+=dy;x+=sx;}if twice<=dx{err+=dx;y+=sy;}}
}
fn info(b:&Bitmap)->BITMAPINFO{
    let mut i=BITMAPINFO::default();i.bmiHeader.biSize=40;i.bmiHeader.biWidth=b.width as i32;i.bmiHeader.biHeight=-(b.height as i32);i.bmiHeader.biPlanes=1;i.bmiHeader.biBitCount=32;i
}
fn label(b:&mut Bitmap,at:Point,s:&str,height:i32,ink:[u8;3],alpha:u32) {unsafe{
    let dc=CreateCompatibleDC(null_mut());if dc.is_null(){return;}
    let mut raw=null_mut();let dib=CreateDIBSection(dc,&info(b),DIB_RGB_COLORS,&mut raw,null_mut(),0);
    if dib.is_null()||raw.is_null(){DeleteDC(dc);return;}
    let previous=SelectObject(dc,dib);
    std::ptr::copy_nonoverlapping(b.pixels.as_ptr(),raw.cast(),b.pixels.len());
    let font=CreateFontW(-height,0,0,0,600,0,0,0,1,0,0,4,0,platform::wide("Segoe UI").as_ptr());
    let oldfont=SelectObject(dc,font);SetBkMode(dc,TRANSPARENT as i32);SetTextColor(dc,colorref(ink));
    let text=platform::wide(s);TextOutW(dc,at.i,at.j,text.as_ptr(),text.len() as i32-1);GdiFlush();
    let new=std::slice::from_raw_parts(raw.cast::<u8>(),b.pixels.len());
    for (out,px) in b.pixels.chunks_exact_mut(4).zip(new.chunks_exact(4)){if out[..3]!=px[..3]{for ch in 0..3{out[ch]=((u32::from(px[ch])*alpha+u32::from(out[ch])*(255-alpha))/255)as u8;}out[3]=255;}}
    SelectObject(dc,oldfont);DeleteObject(font);SelectObject(dc,previous);DeleteObject(dib);DeleteDC(dc);
}}
fn colorref(bgr:[u8;3])->u32{bgr[2] as u32|((bgr[1] as u32)<<8)|((bgr[0] as u32)<<16)}
fn flatten(original:&Bitmap,doc:&Document)->Bitmap{
    let mut out=original.clone();
    for m in &doc.marks{
        let Some(&a)=m.points.first()else{continue};let z=*m.points.last().unwrap();let r=rect(a,z);let alpha=m.opacity.min(100)*255/100;
        match m.tool{
            Tool::Pen=>{paint(&mut out,a,m.width/2,m.ink,alpha);for s in m.points.windows(2){line(&mut out,s[0],s[1],m.width,m.ink,alpha);}}
            Tool::Arrow=>{line(&mut out,a,z,m.width,m.ink,alpha);let angle=((z.j-a.j) as f64).atan2((z.i-a.i) as f64);let len=(12.+2.*m.width as f64).min(((z.i-a.i) as f64).hypot((z.j-a.j) as f64));for rad in [-0.48,0.48]{let tip=Point{i:(z.i as f64-len*(angle+rad).cos()) as i32,j:(z.j as f64-len*(angle+rad).sin())as i32};line(&mut out,z,tip,m.width,m.ink,alpha);}}
            Tool::Rectangle=>{let corners=[a,Point{i:z.i,j:a.j},z,Point{i:a.i,j:z.j},a];for s in corners.windows(2){line(&mut out,s[0],s[1],m.width,m.ink,alpha);}}
            Tool::Ellipse=>{let rx=(r[2]-r[0])as f64/2.;let ry=(r[3]-r[1])as f64/2.;let count=((rx+ry)*3.).clamp(12.,4096.) as usize;let mut prev=None;
                for n in 0..=count{let f=n as f64/count as f64*std::f64::consts::TAU;let pt=Point{i:(r[0]as f64+rx+rx*f.cos())as i32,j:(r[1]as f64+ry+ry*f.sin())as i32};if let Some(prev)=prev{line(&mut out,prev,pt,m.width,m.ink,alpha);}prev=Some(pt);}}
            Tool::Highlight=>{for y in r[1]..=r[3]{for x in r[0]..=r[2]{paint(&mut out,Point{i:x,j:y},0,m.ink,90*alpha/255);}}}
            Tool::Redact=>{let cell=(m.width*2+5)as usize;for by in (r[1]..=r[3]).step_by(cell){for bx in (r[0]..=r[2]).step_by(cell){let mut sum=[0u32;3];let mut count=0;
                let xmax=(bx+cell as i32-1).min(r[2]);let ymax=(by+cell as i32-1).min(r[3]);
                for y in by..=ymax{for x in bx..=xmax{let o=(y as usize*out.width+x as usize)*4;for c in 0..3{sum[c]+=out.pixels[o+c]as u32;}count+=1;}}
                for y in by..=ymax{for x in bx..=xmax{paint(&mut out,Point{i:x,j:y},0,sum.map(|v|(v/count)as u8),alpha);}}
            }}}
            Tool::Text=>label(&mut out,a,&m.text,m.text_size(),m.ink,alpha),
            Tool::Number=>{paint(&mut out,a,12+m.width,m.ink,alpha);label(&mut out,Point{i:a.i-6,j:a.j-11},&m.text,18,[255;3],alpha);}
            _=>{}
        }
    }
    if let Some([x,y,r,b])=doc.crop {
        let (x,y,r,b)=(x as usize,y as usize,r as usize,b as usize);
        let mut pixels=Vec::with_capacity((r-x+1)*(b-y+1)*4);
        for row in y..=b{pixels.extend_from_slice(&out.pixels[(row*out.width+x)*4..(row*out.width+r+1)*4]);}
        return Bitmap{width:r-x+1,height:b-y+1,pixels};
    }
    out
}

fn valid(doc:&Document,b:&Bitmap)->bool{
    let valid=|p:Point|p.i>=0&&p.j>=0&&(p.i as usize)<b.width&&(p.j as usize)<b.height;
    doc.marks.len()<=500&&doc.marks.iter().all(|m|(1..=24).contains(&m.width)&&(10..=100).contains(&m.opacity)&&(m.font_size==0||(10..=96).contains(&m.font_size))&&!m.points.is_empty()&&m.points.len()<=10_000&&m.points.iter().all(|p|valid(*p))&&m.text.len()<=2048)
    &&doc.crop.is_none_or(|[l,t,r,b]|l<=r&&t<=b&&valid(Point{i:l,j:t})&&valid(Point{i:r,j:b}))
}
struct Editor{
    chrome:EditorChrome,
    revision:u64,
    pending:Option<(u64,bool,mpsc::Receiver<Result<crate::devices::Scan,String>>)>,
    words:Option<crate::devices::Scan>,
    text_selection:Option<(usize,usize)>,selecting_text:bool,zoom:f64,pan:(i32,i32),panning:Option<(i32,i32)> ,
    original:Bitmap,shown:Bitmap,doc:Document,file:PathBuf,undo:Vec<Document>,redo:Vec<Document>,
    tool:Tool,ink:[u8;3],width:i32,drag:Vec<Point>,selected:Option<usize>,original_mode:bool,edit:HWND,
}
impl Editor{
    fn edited(&mut self) {
        self.revision=self.revision.wrapping_add(1);
        self.words=None;self.text_selection=None;self.selecting_text=false;
    }
    fn apply(&mut self,next:Document)->Result<(),String>{
        if !valid(&next,&self.original){return Err("Annotation limit or image boundary reached.".into());}
        let data=serde_json::to_vec(&next).map_err(|e|e.to_string())?;
        if data.len()>3*1024*1024{return Err("Markup document is too large.".into());}
        crate::storage::write_protected(&self.file,&data)?;
        if self.undo.len()>=24{self.undo.remove(0);}self.undo.push(self.doc.clone());self.redo.clear();self.doc=next;
        self.edited();self.shown=flatten(&self.original,&self.doc);self.original_mode=false;Ok(())
    }
    fn history(&mut self,undo:bool)->Result<(),String>{
        let stack=if undo{&mut self.undo}else{&mut self.redo};let Some(previous)=stack.last()else{return Ok(())};
        let bytes=serde_json::to_vec(previous).map_err(|e|e.to_string())?;crate::storage::write_protected(&self.file,&bytes)?;
        let next=stack.pop().unwrap();let other=if undo{&mut self.redo}else{&mut self.undo};other.push(std::mem::replace(&mut self.doc,next));self.edited();self.shown=flatten(&self.original,&self.doc);self.original_mode=false;self.selected=None;Ok(())
    }
    fn view(&self)->&Bitmap{if self.original_mode{&self.original}else{&self.shown}}
    fn layout(&self,hwnd:HWND)->(i32,i32,i32,i32){
        let mut r=RECT::default();unsafe{GetClientRect(hwnd,&mut r)};let b=self.view();
        let stage=self.chrome.stage(r);let pw=stage.right-stage.left;let ph=stage.bottom-stage.top;
        let scale=(pw.max(1)as f64/b.width as f64).min(ph.max(1)as f64/b.height as f64);
        let scale=scale*self.zoom;
        let (w,h)=((b.width as f64*scale).max(1.) as i32,(b.height as f64*scale).max(1.) as i32);
        (stage.left+(pw-w).max(0)/2+self.pan.0,stage.top+self.pan.1,w,h)
    }
    fn point(&self,h:HWND,lp:LPARAM,clamp:bool)->Option<Point>{
        let x=(lp as i16) as i32;let y=((lp>>16) as i16) as i32;let(l,t,w,ht)=self.layout(h);let b=self.view();
        if !clamp&&(x<l||y<t||x>=l+w||y>=t+ht){return None;}
        let crop=if self.original_mode{[0;4]}else{self.doc.crop.unwrap_or([0;4])};
        Some(Point{i:crop[0]+((x-l)as i64*b.width as i64/w as i64).clamp(0,b.width as i64-1) as i32,j:crop[1]+((y-t) as i64*b.height as i64/ht as i64).clamp(0,b.height as i64-1)as i32})
    }
    fn finish(&mut self,p:Point,resize:bool)->Result<(),String>{
        let Some(&start)=self.drag.first()else{return Ok(())};let mut doc=self.doc.clone();
        match self.tool{
            Tool::Select=>{if let Some(idx)=self.selected{
                let m=&mut doc.marks[idx];let b=bounds(m);let (dx,dy)=(p.i-start.i,p.j-start.j);
                if dx==0&&dy==0{return Ok(());}
                for pt in &mut m.points{let(x,y)=if resize{(b[0]+((pt.i-b[0])as i64*(b[2]-b[0]+dx).max(1)as i64/(b[2]-b[0]).max(1)as i64)as i32,b[1]+((pt.j-b[1])as i64*(b[3]-b[1]+dy).max(1)as i64/(b[3]-b[1]).max(1)as i64)as i32)}else{(pt.i+dx,pt.j+dy)};
                    pt.i=x.clamp(0,self.original.width as i32-1);pt.j=y.clamp(0,self.original.height as i32-1);}
            }else{return Ok(());}}
            Tool::Crop=>{if start==p{return Ok(());}doc.crop=Some(rect(start,p));}
            tool=>{let text=if tool==Tool::Number{(doc.marks.iter().filter(|m|m.tool==Tool::Number).count()+1).to_string()}else if tool==Tool::Text{let mut buf=[0u16;513];let n=unsafe{GetWindowTextW(self.edit,buf.as_mut_ptr(),buf.len()as i32)};let value=String::from_utf16_lossy(&buf[..n.max(0)as usize]);if value.trim().is_empty(){return Err("Type a label above the image first.".into());}value}else{String::new()};
                let points=if tool==Tool::Pen {let mut v=self.drag.clone();if v.last()!=Some(&p)&&v.len()<10_000{v.push(p);}v}else{vec![start,p]};
                doc.marks.push(Mark{tool,points,text,ink:self.ink,width:self.width,opacity:self.chrome.opacity,font_size:self.chrome.font_size});}
        }
        self.apply(doc)
    }
}
fn registry()->&'static Mutex<HashSet<PathBuf>>{static REG:OnceLock<Mutex<HashSet<PathBuf>>>=OnceLock::new();REG.get_or_init(Default::default)}
pub(super) fn show(ui:&crate::Ui,file:&Path,image:Bitmap,scan_on_open:bool)->Result<(),String>{
    let file=file.to_path_buf();
    let doc:Document=if file.exists(){let bytes=crate::storage::read_protected(&file,3*1024*1024)?;serde_json::from_slice(&bytes).map_err(|_|"Cannot read saved markup.")?}else{Document::default()};
    if !valid(&doc,&image){return Err("Invalid saved annotations.".into());}
    if !registry().lock().map_err(|_|"Editor is unavailable.")?.insert(file.clone()){return Err("This image is already open in a markup window.".into());}
    let overlay=Overlay::for_ui(ui);
    let active=file.clone();
    let (ready,started)=mpsc::sync_channel(1);
    let spawned=std::thread::Builder::new().name("Buddy image editor".into()).spawn(move ||{
        if let Err(err)=overlay.attach_desktop(){let _=ready.send(Err(err));if let Ok(mut r)=registry().lock(){r.remove(&file);}return}
        let shown=flatten(&image,&doc);
        let editor=RefCell::new(Editor{chrome:EditorChrome::new(overlay),revision:0,pending:None,words:None,text_selection:None,selecting_text:false,zoom:1.0,pan:(0,0),panning:None,original:image,shown,doc,file:file.clone(),undo:vec![],redo:vec![],tool:Tool::Pen,ink:controls::COLORS[0],width:4,drag:vec![],selected:None,original_mode:false,edit:null_mut()});
        let result=unsafe{window(&editor,|h|{let _=ready.send(Ok(()));if scan_on_open{PostMessageW(h,WM_COMMAND,7,0);}})};
        if let Err(message)=result{let _=ready.send(Err(message));}
        if let Ok(mut r)=registry().lock(){r.remove(&file);}
    });
    if let Err(e)=spawned{if let Ok(mut r)=registry().lock(){r.remove(&active);}return Err(e.to_string());}
    started.recv().map_err(|_|"The image editor exited before opening.".to_owned())?
}
const TOOLS:[(Tool,&str);10]=[(Tool::Select,"Select and move"),(Tool::Pen,"Pen"),(Tool::Arrow,"Arrow"),(Tool::Rectangle,"Rectangle"),(Tool::Ellipse,"Ellipse"),(Tool::Text,"Text label"),(Tool::Highlight,"Highlight"),(Tool::Redact,"Pixelate"),(Tool::Number,"Numbered callout"),(Tool::Crop,"Crop")];
unsafe fn window(editor:&RefCell<Editor>,opened:impl FnOnce(HWND))->Result<(),String>{
    let class=platform::wide("BuddyWorkMarkupWindow");let instance=GetModuleHandleW(null());
    let wc=WNDCLASSW{lpfnWndProc:Some(proc),hInstance:instance,lpszClassName:class.as_ptr(),hCursor:LoadCursorW(null_mut(),IDC_CROSS),..Default::default()};RegisterClassW(&wc);
    let overlay=editor.borrow().chrome.overlay;
    let [x,y,w,height]=overlay.geometry;
    let hwnd=CreateWindowExW(WS_EX_TOOLWINDOW|WS_EX_TOPMOST,class.as_ptr(),platform::wide("Buddy image markup · Ready").as_ptr(),
        WS_POPUP|WS_CLIPCHILDREN,x,y,w,height,null_mut(),null_mut(),instance,editor as *const _ as _);
    if hwnd.is_null(){return Err(format!("Cannot open image editor: {}",std::io::Error::last_os_error()));}
    ShowWindow(hwnd,SW_SHOW);opened(hwnd);
    // Attach ownership after signaling readiness. Creating an owned window across
    // UI threads synchronously while its owner is waiting can deadlock at startup.
    if overlay.owner!=0 && IsWindow(hwnd)!=0{
        SetWindowLongPtrW(hwnd,GWLP_HWNDPARENT,overlay.owner as isize);
        SetWindowPos(hwnd,HWND_TOPMOST,0,0,0,0,SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE|SWP_SHOWWINDOW);
    }
    let mut msg=MSG::default();
    while GetMessageW(&mut msg,null_mut(),0,0)>0{
        crate::chrome::scroll::route_wheel(&mut msg);
        if msg.message==WM_KEYDOWN{
            let e=editor.borrow();let typing=msg.hwnd==e.edit;drop(e);
            let ctrl=GetKeyState(VK_CONTROL as i32)<0;
            let cmd=if typing{match msg.wParam{13=>440,27=>441,_=>0}}
                else if msg.wParam==27{6}else if ctrl{match msg.wParam{83=>2,67=>if editor.borrow().words.is_some(){8}else{1},90=>11,89=>12,_=>0}}
                else{"VPARETHDNX".bytes().position(|b|usize::from(b)==msg.wParam).map_or(0,|i|100+i)};
            if cmd!=0{SendMessageW(hwnd,WM_COMMAND,cmd,0);continue}
        }
        if IsDialogMessageW(hwnd,&msg)==0{TranslateMessage(&msg);DispatchMessageW(&msg);}
    }
    Ok(())
}
unsafe fn command(h:HWND,e:&mut Editor,id:usize)->Result<(),String>{
    match id {
        1=>platform::copy_bitmap(h,&e.shown)?,
        2|4=>if let Some(path)=platform::choose_files(h,true,false)?.first(){let byte=if id==4{e.original.png()?}else{e.shown.png()?};std::fs::write(path,byte).map_err(|err|err.to_string())?;},
        3=>{e.edited();e.original_mode=!e.original_mode;},
        5|7=>{
            if e.pending.is_some(){return Err("A text scan is already running.".into());}
            e.chrome.scan_mode=true;let source=e.view().clone();
            let (tx,rx)=mpsc::channel();
            if SetTimer(h,1,100,None)==0{return Err("Cannot start the text scan.".into());}
            if let Err(err)=std::thread::Builder::new().name("Buddy markup OCR".into()).spawn(move||{
                let _=tx.send(source.png().and_then(|bytes|crate::devices::recognize_words(&bytes)));
            }){
                KillTimer(h,1);return Err(err.to_string());
            }
            e.pending=Some((e.revision,id==5,rx));
            SetWindowTextW(h,platform::wide("Buddy image markup · Recognizing text…").as_ptr());
        }
        6=>{DestroyWindow(h);return Ok(());},
        8=>{
            let scan=e.words.as_ref().ok_or("Choose Select image text first.")?;
            let text=selected_text(scan,e.text_selection);
            platform::copy_text(h,&text)?;
            SetWindowTextW(h,platform::wide("Buddy image markup · Text copied").as_ptr());
        }
        300..=306=>{
            match id {
                300=>e.zoom=(e.zoom+0.25).min(3.),
                301=>e.zoom=(e.zoom-0.25).max(0.25),
                302=>{e.zoom=1.0;e.pan=(0,0);}
                303=>e.pan.0+=80,304=>e.pan.0-=80,305=>e.pan.1+=80,306=>e.pan.1-=80,_=>{}
            }
        }
        11|12=>e.history(id==11)?,
        13=>if let Some(i)=e.selected.take(){let mut d=e.doc.clone();if i<d.marks.len(){d.marks.remove(i);e.apply(d)?;}},
        14=>{let mut d=e.doc.clone();d.crop=None;e.apply(d)?;}
        100..=109=>{let(tool,label)=TOOLS[id-100];e.edited();e.chrome.scan_mode=false;e.tool=tool;e.original_mode=false;SetWindowTextW(h,platform::wide(format!("Buddy image markup · {label}")).as_ptr());}
        200..=205=>e.ink=controls::COLORS[id-200],
        210..=212=>e.width=[2,4,9][id-210],_=>controls::command(h,e,id)?
    }
    controls::layout(h,e);InvalidateRect(h,null(),1);Ok(())
}
unsafe fn finish_scan(h:HWND,e:&mut Editor)->Result<(),String> {
    let Some((revision,copy,rx))=e.pending.as_ref()else{return Ok(())};
    let result=match rx.try_recv(){
        Ok(result)=>result,
        Err(mpsc::TryRecvError::Empty)=>return Ok(()),
        Err(mpsc::TryRecvError::Disconnected)=>Err("The scan stopped before completing.".into())
    };
    let (revision,copy)=(*revision,*copy);
    KillTimer(h,1);
    e.pending=None;
    if revision!=e.revision{return Err("The image changed during the scan.".into());}
    let scan=result?;
    if scan.text.trim().is_empty(){return Err("No text found.".into());}
    let title=if copy {
        platform::copy_text(h,&scan.text)?;
        "Buddy image markup · Recognized text copied"
    }else{
        e.words=Some(scan);
        "Buddy image markup · Drag to select characters, Ctrl+C to copy, Shift-click for a line"
    };
    SetWindowTextW(h,platform::wide(title).as_ptr());
    InvalidateRect(h,null(),0);
    Ok(())
}
fn text_content(scan:&crate::devices::Scan)->String{
    scan.lines.iter().map(|line|line.iter().map(|w|w.text.as_str()).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n")
}
fn selected_text(scan:&crate::devices::Scan,selection:Option<(usize,usize)>)->String{
    let text=text_content(scan);
    if let Some((a,b))=selection.filter(|(a,b)|a!=b){
        text.chars().skip(a.min(b)).take(a.abs_diff(b)).collect()
    }else{text}
}
struct Hit{caret:usize,word:(usize,usize),line:(usize,usize)}
fn hit_text(scan:&crate::devices::Scan,p:Point,nearest:bool)->Option<Hit>{
    let mut offset=0;
    let mut best=None;
    let mut distance=i64::MAX;
    for line in &scan.lines{
        let line_start=offset;
        let line_len=line.iter().map(|w|w.text.chars().count()).sum::<usize>()+line.len().saturating_sub(1);
        for word in line{
            let len=word.text.chars().count();
            let r=word.x+word.width.max(1);let b=word.y+word.height.max(1);
            let (dx,dy)=(p.i-p.i.clamp(word.x,r),p.j-p.j.clamp(word.y,b));
            let d=i64::from(dx)*i64::from(dx)+i64::from(dy)*i64::from(dy);
            if d<distance&&(nearest||d==0){
                let relative=((p.i-word.x).clamp(0,word.width.max(1))as f64*len as f64/word.width.max(1)as f64).round()as usize;
                best=Some(Hit{caret:offset+relative,word:(offset,offset+len),line:(line_start,line_start+line_len)});distance=d;
            }
            offset+=len+1;
        }
        offset=line_start+line_len+1;
    }
    best
}
unsafe fn text_pointer(h:HWND,e:&mut Editor,lp:LPARAM,stage:u8)->Result<(),String>{
    let Some(scan)=e.words.as_ref()else{return Ok(())};
    let Some(p)=e.point(h,lp,stage!=0)else{return Ok(())};
    let crop=if e.original_mode{[0;4]}else{e.doc.crop.unwrap_or([0;4])};
    if let Some(hit)=hit_text(scan,Point{i:p.i-crop[0],j:p.j-crop[1]},stage!=0){
        if stage==0{
            if GetKeyState(VK_SHIFT as i32)<0{
                e.text_selection=Some(hit.line);e.selecting_text=false;
            }else{e.text_selection=Some((hit.caret,hit.caret));e.selecting_text=true;SetCapture(h);}
        }else if let Some((a,_))=e.text_selection{
            e.text_selection=Some(if stage==2&&a==hit.caret{hit.word}else{(a,hit.caret)});
        }
    }
    if stage==2{e.selecting_text=false;ReleaseCapture();}
    InvalidateRect(h,null(),1);Ok(())
}
unsafe extern "system" fn proc(h:HWND,msg:u32,wp:WPARAM,lp:LPARAM)->LRESULT{
    if msg==WM_NCCREATE {let cs=&*(lp as *const CREATESTRUCTW);SetWindowLongPtrW(h,GWLP_USERDATA,cs.lpCreateParams as isize);}
    if msg==WM_DESTROY { SetWindowLongPtrW(h,GWLP_USERDATA,0); PostQuitMessage(0); return 0; }
    let ptr=GetWindowLongPtrW(h,GWLP_USERDATA)as *const RefCell<Editor>;
    if ptr.is_null(){return DefWindowProcW(h,msg,wp,lp);}
    let Ok(mut editor)=(*ptr).try_borrow_mut() else { return DefWindowProcW(h,msg,wp,lp); };
    let e=&mut *editor;
    let result=match msg {
        WM_CREATE=>{controls::create(h,e);controls::layout(h,e);Ok(())}
        WM_DRAWITEM if lp!=0=>{controls::draw(h,e,&*(lp as *const windows_sys::Win32::UI::Controls::DRAWITEMSTRUCT));return 1;}
        WM_CTLCOLORLISTBOX|WM_CTLCOLOREDIT=>{let (_,field,ink,_)=e.chrome.overlay.colors;let dc=wp as HDC;SetTextColor(dc,ink);SetBkColor(dc,field);SetDCBrushColor(dc,field);return GetStockObject(DC_BRUSH)as _;}
        WM_ERASEBKGND=>return 1,
        WM_PAINT|WM_PRINT|WM_PRINTCLIENT=>{let mut ps=PAINTSTRUCT::default();let dc=if msg==WM_PAINT{BeginPaint(h,&mut ps)}else{wp as HDC};if dc.is_null(){return 0}let save=SaveDC(dc);let(l,t,w,ht)=e.layout(h);let b=e.view();let mut client=RECT::default();GetClientRect(h,&mut client);
            controls::paint(h,e,dc,client);controls::clip_history(e,dc,client);let stage=e.chrome.stage(client);IntersectClipRect(dc,0,stage.top-e.chrome.px(14),client.right,client.bottom-e.chrome.px(27));SetStretchBltMode(dc,HALFTONE);StretchDIBits(dc,l,t,w,ht,0,0,b.width as i32,b.height as i32,b.pixels.as_ptr().cast(),&info(b),DIB_RGB_COLORS,SRCCOPY);
            let crop=if e.original_mode{[0;4]}else{e.doc.crop.unwrap_or([0;4])};let xy=|p:Point|(l+((p.i-crop[0])as i64*w as i64/b.width as i64)as i32,t+((p.j-crop[1])as i64*ht as i64/b.height as i64)as i32);
            if let Some(idx)=e.selected.filter(|&i|i<e.doc.marks.len()&&!e.original_mode){let [x,y,r,b]=bounds(&e.doc.marks[idx]);let(l,t)=xy(Point{i:x,j:y});let(r,b)=xy(Point{i:r,j:b});DrawFocusRect(dc,&RECT{left:l,top:t,right:r.max(l+5),bottom:b.max(t+5)});}
            if let Some(scan)=&e.words {
                let selection=e.text_selection.map(|(a,b)|(a.min(b),a.max(b)));
                let mut start=0;
                for line in &scan.lines{
                    for word in line{
                        let len=word.text.chars().count();
                        let top=Point{i:word.x+crop[0],j:word.y+crop[1]};
                        let (x,y)=xy(top);
                        let (r,b)=xy(Point{i:top.i+word.width,j:top.j+word.height});

                        if let Some((a,z))=selection.filter(|(a,z)|*a<start+len&&*z>start){
                            let l=x+((r-x)as i64*(a.max(start)-start)as i64/len.max(1)as i64)as i32;
                            let end=x+((r-x)as i64*(z.min(start+len)-start)as i64/len.max(1)as i64)as i32;
                            InvertRect(dc,&RECT{left:l,top:y,right:end,bottom:b});
                        }
                        start+=len+1;
                    }
                    if line.is_empty(){start+=1;}
                }
            }
            if e.drag.len()>1{let pen=CreatePen(PS_SOLID,2,colorref(e.ink));let last=SelectObject(dc,pen);let (x,y)=xy(e.drag[0]);MoveToEx(dc,x,y,null_mut());for p in &e.drag[1..]{let (x,y)=xy(*p);LineTo(dc,x,y);}SelectObject(dc,last);DeleteObject(pen);}RestoreDC(dc,save);if msg==WM_PAINT{EndPaint(h,&ps);}else if lp as i32&PRF_CHILDREN!=0{controls::print_controls(h,e,dc);}Ok(())}
        WM_TIMER if wp==1=>{let result=finish_scan(h,e);controls::layout(h,e);result},
        WM_SIZE=>{controls::layout(h,e);InvalidateRect(h,null(),1);Ok(())}
        WM_COMMAND if lp==0||(wp>>16)==BN_CLICKED as usize=>command(h,e,wp&65535),
        WM_COMMAND if wp&65535==405&&(wp>>16)==LBN_SELCHANGE as usize=>controls::select_history(h,e),
        WM_COMMAND if wp&65535==450&&(wp>>16)==EN_KILLFOCUS as usize=>controls::command(h,e,440),
        WM_KEYDOWN=>{let ctrl=GetKeyState(VK_CONTROL as i32)<0;
            if ctrl&&wp==67&&e.words.is_some(){command(h,e,8)}
            else if ctrl&&wp==65&&e.words.is_some(){let len=text_content(e.words.as_ref().unwrap()).chars().count();e.text_selection=Some((0,len));InvalidateRect(h,null(),1);Ok(())}
            else if wp==27 {e.drag.clear();e.selecting_text=false;e.panning=None;ReleaseCapture();InvalidateRect(h,null(),1);Ok(())}
            else if wp==VK_DELETE as usize {command(h,e,13)}else if ctrl&&wp==90 {command(h,e,11)}else if ctrl&&wp==89{command(h,e,12)}else{return DefWindowProcW(h,msg,wp,lp);}}
        WM_MOUSEWHEEL=>command(h,e,if ((wp>>16)as i16)>0{300}else{301}),
        WM_MBUTTONDOWN=>{e.panning=Some(((lp as i16)as i32,((lp>>16)as i16)as i32));SetCapture(h);Ok(())}
        WM_MBUTTONUP=>{e.panning=None;ReleaseCapture();Ok(())}
        WM_MOUSEMOVE if e.panning.is_some()=>{let last=e.panning.unwrap();let next=((lp as i16)as i32,((lp>>16)as i16)as i32);e.pan.0+=next.0-last.0;e.pan.1+=next.1-last.1;e.panning=Some(next);InvalidateRect(h,null(),1);Ok(())}
        WM_LBUTTONDOWN if e.tool==Tool::Text&&e.words.is_none()&&!e.original_mode=>controls::start_text(h,e,lp),
        WM_LBUTTONDOWN if e.chrome.scan_mode&&e.words.is_some()=>text_pointer(h,e,lp,0),
        WM_MOUSEMOVE if e.selecting_text=>text_pointer(h,e,lp,1),
        WM_LBUTTONUP if e.selecting_text=>text_pointer(h,e,lp,2),
        WM_LBUTTONDOWN=>{if !e.original_mode{if let Some(p)=e.point(h,lp,false){e.drag=vec![p];if e.tool==Tool::Select{e.selected=e.doc.marks.iter().rposition(|m|{let[l,t,r,b]=bounds(m);p.i>=l-5&&p.j>=t-5&&p.i<=r+5&&p.j<=b+5});}SetCapture(h);InvalidateRect(h,null(),0);}}Ok(())}
        WM_MOUSEMOVE=>{if !e.drag.is_empty(){if let Some(p)=e.point(h,lp,true){if e.tool!=Tool::Pen{e.drag.truncate(1);}if e.drag.len()<10_000&&e.drag.last()!=Some(&p){e.drag.push(p);}InvalidateRect(h,null(),0);}}Ok(())}
        WM_LBUTTONUP=>{if e.drag.is_empty(){return 0;}let p=e.point(h,lp,true).unwrap_or(e.drag[0]);let r=e.finish(p,GetKeyState(VK_SHIFT as i32)<0);e.drag.clear();ReleaseCapture();InvalidateRect(h,null(),1);r}
        WM_CAPTURECHANGED|WM_KILLFOCUS=>{e.drag.clear();e.panning=None;e.selecting_text=false;Ok(())}
        _=>return DefWindowProcW(h,msg,wp,lp)
    };
    if let Err(message)=result{SetWindowTextW(h,platform::wide(format!("Buddy markup · {message}")).as_ptr());InvalidateRect(h,null(),0);}
    0
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]
    fn legacy_markup_keeps_original_ink_and_size(){
        let bytes=br#"{"marks":[{"tool":"Rectangle","points":[{"i":1,"j":1},{"i":3,"j":3}],"text":"","ink":[0,0,255],"width":2}],"crop":null}"#;
        let mut doc:Document=serde_json::from_slice(bytes).unwrap();
        let image=Bitmap{width:6,height:6,pixels:vec![255;6*6*4]};
        assert!(valid(&doc,&image));
        assert_eq!(doc.marks[0].opacity,100);
        assert_eq!(doc.marks[0].text_size(),18);
        assert_eq!(&flatten(&image,&doc).pixels[28..31],&[0,0,255]);
        doc.marks[0].opacity=50;
        let blend=flatten(&image,&doc).pixels[28];
        assert!(blend>0&&blend<255);
    }
    #[test]fn crop_boundaries_and_flatten(){
        let src=Bitmap{width:8,height:8,pixels:vec![255;8*8*4]};
        let mut doc=Document{marks:vec![Mark{tool:Tool::Rectangle,points:vec![Point{i:1,j:1},Point{i:6,j:6}],text:String::new(),ink:[0,0,255],width:2,opacity:100,font_size:0}],crop:Some([1,1,4,4])};
        assert!(valid(&doc,&src));let result=flatten(&src,&doc);assert_eq!((result.width,result.height),(4,4));assert_eq!(&result.pixels[..4],&[0,0,255,255]);assert_eq!(src.pixels[0],255);doc.crop=Some([0,0,8,8]);assert!(!valid(&doc,&src));}
    #[test]
    #[ignore="run alone with --ignored --test-threads=1; uses its own Windows window station"]
    fn private_desktop_markup_window(){
        platform::isolation_test::with_private_desktop(||{
            let original=Bitmap{width:64,height:64,pixels:vec![255;64*64*4]};
            let file=std::env::temp_dir().join(format!("buddy-markup-{}-{}.dpapi",std::process::id(),super::super::now()));
            let e=RefCell::new(Editor{
                chrome:EditorChrome::new(Overlay::default()),revision:0,pending:None,words:None,text_selection:None,selecting_text:false,zoom:1.0,pan:(0,0),panning:None,shown:original.clone(),original,doc:Document::default(),file:file.clone(),
                undo:vec![],redo:vec![],tool:Tool::Pen,ink:[0,0,255],width:2,drag:vec![],selected:None,original_mode:false,edit:null_mut()
            });
            unsafe{window(&e,|h|{
                SendMessageW(h,WM_COMMAND,103,0);
                assert!(e.borrow().tool==Tool::Rectangle);
                let(l,t,w,ht)=e.borrow().layout(h);
                let loc=|n:i32|((l+w*n/10)as isize&0xffff) | (((t+ht*n/10)as isize&0xffff)<<16);
                SendMessageW(h,WM_LBUTTONDOWN,0,loc(2));
                SendMessageW(h,WM_MOUSEMOVE,0,loc(6));
                SendMessageW(h,WM_LBUTTONUP,0,loc(6));
                if file.exists(){
                    assert_eq!(e.borrow().doc.marks.len(),1);
                    let bytes=crate::storage::read_protected(&file,1024*1024).unwrap();
                    let doc:Document=serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(doc.marks.len(),1);
                    assert!(!std::fs::read(&file).unwrap().windows(9).any(|s|s==b"Rectangle"));
                    SendMessageW(h,WM_COMMAND,11,0);
                    assert!(e.borrow().doc.marks.is_empty());
                    SendMessageW(h,WM_COMMAND,12,0);
                    assert_eq!(e.borrow().doc.marks.len(),1);
                    println!("Real editor saved encrypted rectangle, undo and redo.");
                }else{
                    let mut title=[0u16;512];
                    let n=GetWindowTextW(h,title.as_mut_ptr(),title.len() as i32);
                    assert!(String::from_utf16_lossy(&title[..n as usize]).contains("encryption is unavailable"));
                    println!("Real editor denied a save when Windows user encryption was unavailable.");
                    assert!(e.borrow().doc.marks.is_empty());
                }
                SendMessageW(h,WM_COMMAND,1,0);
                let Some(platform::Clip::Image(img))=platform::read_clip(h,true).unwrap()else{panic!("No image copied")};
                assert_eq!(img.pixels,e.borrow().shown.pixels);
                SendMessageW(h,WM_COMMAND,6,0);
            })}.unwrap();
            if file.exists(){std::fs::remove_file(file).unwrap();}
        });
    }

    #[test]
    fn selected_characters_across_words_and_lines(){
        let word=|text:&str,x:i32,y:i32,width:i32|crate::devices::Word{text:text.into(),x,y,width,height:10};
        let scan=crate::devices::Scan{text:"Hello 世界\nnext".into(),lines:vec![
            vec![word("Hello",0,0,50),word("世界",60,0,20)],vec![word("next",0,20,40)]
        ]};
        let a=hit_text(&scan,Point{i:20,j:2},false).unwrap();
        let b=hit_text(&scan,Point{i:70,j:2},false).unwrap();
        assert_eq!(selected_text(&scan,Some((a.caret,b.caret))),"llo 世");
        let c=hit_text(&scan,Point{i:20,j:25},false).unwrap();
        assert_eq!(selected_text(&scan,Some((c.caret,b.caret))),"界\nne");
        assert_eq!(selected_text(&scan,Some(b.word)),"世界");
        assert_eq!(selected_text(&scan,Some(a.line)),"Hello 世界");
        assert!(hit_text(&scan,Point{i:100,j:60},false).is_none());
    }

}
