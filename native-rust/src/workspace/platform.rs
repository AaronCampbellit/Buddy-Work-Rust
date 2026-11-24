use std::{ffi::{OsStr, OsString}, os::windows::ffi::{OsStrExt,OsStringExt}, path::{Path,PathBuf}, ptr::{null,null_mut}};
use windows_sys::Win32::{Foundation::*, System::{Com::CoTaskMemFree, DataExchange::*,Memory::*,Ole::{CF_BITMAP,CF_DIB,CF_DIBV5,CF_HDROP,CF_UNICODETEXT}}, UI::{Controls::Dialogs::*,Shell::*, WindowsAndMessaging::*}};
use super::bitmap::{Bitmap,MAX_IMAGE};
pub(super) fn wide(value: impl AsRef<OsStr>)->Vec<u16> {value.as_ref().encode_wide().chain(Some(0)).collect()}
pub(super) fn sequence()->u32 {unsafe {GetClipboardSequenceNumber()}}
struct Clipboard;
impl Clipboard {
    fn open(hwnd:HWND)->Result<Self,String> {
        unsafe{if OpenClipboard(hwnd)==0 {return Err("Another application is using the clipboard.".into());}}
        Ok(Self)
    }
}
impl Drop for Clipboard {fn drop(&mut self){unsafe{CloseClipboard();}}}
fn global_bytes(fmt:u32,limit:usize)->Option<Vec<u8>> {unsafe{
    if IsClipboardFormatAvailable(fmt)==0 {return None;}
    let handle=GetClipboardData(fmt);
    if handle.is_null(){return None;}
    let size=GlobalSize(handle);
    if size==0||size>limit{return None;}
    let ptr=GlobalLock(handle);
    if ptr.is_null(){return None;}
    let bytes=std::slice::from_raw_parts(ptr.cast::<u8>(),size).to_vec();
    GlobalUnlock(handle);
    Some(bytes)
}}
pub(crate) fn formatted_clipboard(hwnd:HWND,limit:usize)->Option<(Option<Vec<u8>>,Option<Vec<u8>>)>{
    let _guard=Clipboard::open(hwnd).ok()?;
    let read=|name:&str|global_bytes(unsafe{RegisterClipboardFormatW(wide(name).as_ptr())},limit);
    Some((read("HTML Format"),read("Rich Text Format")))
}
fn png_format()->u32 {unsafe{RegisterClipboardFormatW(wide("PNG").as_ptr())}}
pub(super) enum Clip {Text(String),Image(Bitmap)}
pub(super) fn read_clip(hwnd:HWND,only_image:bool)->Result<Option<Clip>,String>{
    let _guard=Clipboard::open(hwnd)?;
    if let Some(bytes)=global_bytes(png_format(),MAX_IMAGE) {
        if let Ok(image)=Bitmap::from_png(&bytes){return Ok(Some(Clip::Image(image)));}
    }
    for fmt in [CF_DIBV5,CF_DIB] {
        if let Some(data)=global_bytes(fmt as u32,64*1024*1024){
            if let Ok(image)=Bitmap::from_dib(&data){return Ok(Some(Clip::Image(image)));}
        }
    }
    if unsafe { IsClipboardFormatAvailable(CF_BITMAP as u32) } != 0 {
        let handle = unsafe { GetClipboardData(CF_BITMAP as u32) };
        if let Ok(image) = bitmap_handle(handle) { return Ok(Some(Clip::Image(image))); }
    }
    if !only_image {
        if let Some(data)=global_bytes(CF_UNICODETEXT as u32,8*1024*1024){
            let wide:Vec<u16>=data.chunks_exact(2).map(|v|u16::from_le_bytes([v[0],v[1]])).take_while(|&v|v!=0).take(20_000).collect();
            let text=String::from_utf16_lossy(&wide);
            if !text.trim().is_empty(){return Ok(Some(Clip::Text(text)));}
        }
    }
    Ok(None)
}
fn set_data(fmt:u32,data:&[u8])->Result<(),String> {unsafe{
    let handle=GlobalAlloc(GMEM_MOVEABLE,data.len());
    if handle.is_null(){return Err("Windows could not allocate a clipboard buffer.".into());}
    let dest=GlobalLock(handle);
    if dest.is_null(){GlobalFree(handle);return Err("Windows could not lock the clipboard buffer.".into());}
    std::ptr::copy_nonoverlapping(data.as_ptr(),dest.cast(),data.len());GlobalUnlock(handle);
    if SetClipboardData(fmt,handle).is_null(){GlobalFree(handle);return Err("Windows could not publish this clipboard format.".into());}
    Ok(())
}}
fn replace(hwnd:HWND)->Result<Clipboard,String>{
    let guard=Clipboard::open(hwnd)?;
    if unsafe{EmptyClipboard()}==0{return Err("Windows could not update the clipboard.".into());}
    Ok(guard)
}
pub(super) fn copy_text(hwnd:HWND,value:&str)->Result<(),String>{
    let data:Vec<u8>=wide(value).into_iter().flat_map(u16::to_le_bytes).collect();
    let _guard=replace(hwnd)?;set_data(CF_UNICODETEXT as u32,&data)
}
pub(super) fn copy_bitmap(hwnd:HWND,img:&Bitmap)->Result<(),String>{
    let png=img.png()?;let dib=img.dib();
    let _guard=replace(hwnd)?;set_data(CF_DIB as u32,&dib)?;
    set_data(png_format(),&png)
}
pub(super) fn copy_path_item(hwnd:HWND,path:&Path)->Result<(),String>{
    if !path.exists(){return Err("Item is unavailable.".into());}
    let mut data=vec![0;20];
    data[0..4].copy_from_slice(&20u32.to_le_bytes());data[16..20].copy_from_slice(&1u32.to_le_bytes());
    data.extend(wide(path).into_iter().chain(Some(0)).flat_map(u16::to_le_bytes));
    let _guard=replace(hwnd)?;set_data(CF_HDROP as u32,&data)
}
pub(super) fn choose_files(hwnd:HWND,save:bool,apps:bool)->Result<Vec<PathBuf>,String>{ unsafe {
    let mut buf=vec![0u16;32768];
    let filter=wide(if save {"PNG image\0*.png\0"}else if apps {"Applications\0*.exe\0"}else {"All files\0*.*\0"});
    let ext=wide("png");
    let mut dlg=OPENFILENAMEW::default();
    dlg.lStructSize=std::mem::size_of::<OPENFILENAMEW>() as u32;dlg.hwndOwner=hwnd;
    dlg.lpstrFile=buf.as_mut_ptr();dlg.nMaxFile=buf.len() as u32;dlg.lpstrFilter=filter.as_ptr();dlg.nFilterIndex=1;
    dlg.Flags=OFN_EXPLORER|OFN_PATHMUSTEXIST|OFN_NOCHANGEDIR|if save{OFN_OVERWRITEPROMPT}else if apps{OFN_FILEMUSTEXIST}else{OFN_FILEMUSTEXIST|OFN_ALLOWMULTISELECT};
    if save{dlg.lpstrDefExt=ext.as_ptr();}
    let ok=if save{GetSaveFileNameW(&mut dlg)}else{GetOpenFileNameW(&mut dlg)};
    if ok==0 {return if CommDlgExtendedError()==0{Ok(vec![])}else{Err("Windows could not open the file picker.".into())};}
    let parts:Vec<PathBuf>=buf.split(|&x|x==0).take_while(|s|!s.is_empty()).map(|s|PathBuf::from(OsString::from_wide(s))).collect();
    if parts.len()<2{return Ok(parts);}
    Ok(parts[1..].iter().map(|p|parts[0].join(p)).collect())
}}
pub(super) fn choose_folder(hwnd:HWND)->Option<PathBuf>{unsafe{
    let title=wide("Choose a folder");
    let info=BROWSEINFOW{hwndOwner:hwnd,lpszTitle:title.as_ptr(),ulFlags:BIF_RETURNONLYFSDIRS|BIF_NEWDIALOGSTYLE,..Default::default()};
    let item=SHBrowseForFolderW(&info);
    if item.is_null(){return None;}
    let mut buf=vec![0u16;32768];
    let ok=SHGetPathFromIDListEx(item,buf.as_mut_ptr(),buf.len() as u32,0);
    CoTaskMemFree(item.cast());
    (ok!=0).then(||PathBuf::from(OsString::from_wide(&buf[..buf.iter().position(|&x|x==0).unwrap_or(0)])))
}}
pub(super) fn downloads()->Option<PathBuf>{unsafe{
    let mut p=null_mut();
    if SHGetKnownFolderPath(&FOLDERID_Downloads,0,null_mut(),&mut p)<0||p.is_null(){return None;}
    let mut n=0;while n<32768&&*p.add(n)!=0{n+=1;}
    let path=PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(p,n)));CoTaskMemFree(p.cast());Some(path)
}}
pub(super) fn open(hwnd:HWND,target:&OsStr)->Result<(),String>{unsafe{
    let ret=ShellExecuteW(hwnd,wide("open").as_ptr(),wide(target).as_ptr(),null(),null(),SW_SHOWNORMAL);
    if ret as isize<=32{Err("Windows could not open this item.".into())}else{Ok(())}
}}
pub(super) fn reveal(path:&Path)->Result<(),String>{unsafe{
    let id=ILCreateFromPathW(wide(path).as_ptr());
    if id.is_null(){return Err("The file is unavailable.".into());}
    let hr=SHOpenFolderAndSelectItems(id,0,null(),0);CoTaskMemFree(id.cast());
    if hr<0{Err("Windows could not reveal this file.".into())}else{Ok(())}
}}


fn bitmap_handle(bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP) -> Result<Bitmap,String> {
    use windows_sys::Win32::Graphics::Gdi::*;
    let mut bm=BITMAP::default();
    if bitmap.is_null() || unsafe { GetObjectW(bitmap,std::mem::size_of::<BITMAP>() as i32,(&mut bm as *mut BITMAP).cast()) }==0 || bm.bmWidth<=0 || bm.bmHeight<=0 {
        return Err("Windows could not read this image.".into());
    }
    let (width,height)=(bm.bmWidth as usize,bm.bmHeight as usize);
    let mut pixels=vec![0;Bitmap::size(width,height)?];
    let mut info=BITMAPINFO::default();
    info.bmiHeader.biSize=40;
    info.bmiHeader.biHeight= -bm.bmHeight;
    info.bmiHeader.biWidth=bm.bmWidth;
    info.bmiHeader.biPlanes=1;
    info.bmiHeader.biBitCount=32;
    unsafe {
        let dc=CreateCompatibleDC(null_mut());
        if dc.is_null(){return Err("Windows could not create an image context.".into());}
        let rows=GetDIBits(dc,bitmap,0,height as u32,pixels.as_mut_ptr().cast(),&mut info,DIB_RGB_COLORS);
        DeleteDC(dc);
        if rows!=height as i32 {return Err("Windows could not decode all rows of the image.".into());}
    }
    if pixels.chunks_exact(4).all(|p|p[3]==0){for p in pixels.chunks_exact_mut(4){p[3]=255;}}
    Ok(Bitmap{width,height,pixels})
}

pub(super) fn thumbnail(path:&Path)->Result<Bitmap,String>{
    use windows::{core::HSTRING,Win32::{Foundation::SIZE,UI::Shell::{IShellItemImageFactory,SHCreateItemFromParsingName,SIIGBF_BIGGERSIZEOK}}};
    let factory: IShellItemImageFactory=unsafe { SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()),None) }.map_err(|e|e.to_string())?;
    let image=unsafe { factory.GetImage(SIZE{cx:768,cy:768},SIIGBF_BIGGERSIZEOK) }.map_err(|e|e.to_string())?;
    let result=bitmap_handle(image.0);
    unsafe { windows_sys::Win32::Graphics::Gdi::DeleteObject(image.0); }
    result
}

pub(super) fn recycle(hwnd:HWND,path:&Path)->Result<(),String>{
    use windows::{core::HSTRING,Win32::{Foundation::HWND as Window,System::Com::{CoCreateInstance,CLSCTX_INPROC_SERVER},UI::Shell::*}};
    unsafe{
        let op:IFileOperation=CoCreateInstance(&FileOperation,None,CLSCTX_INPROC_SERVER).map_err(|e|e.to_string())?;
        op.SetOwnerWindow(Window(hwnd)).map_err(|e|e.to_string())?;
        op.SetOperationFlags(FOFX_RECYCLEONDELETE|FOFX_ADDUNDORECORD|FOFX_EARLYFAILURE|FOF_NOERRORUI|FOF_SILENT|FOF_WANTNUKEWARNING).map_err(|e|e.to_string())?;
        let item:IShellItem=SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()),None).map_err(|e|e.to_string())?;
        op.DeleteItem(&item,None).map_err(|e|e.to_string())?;
        op.PerformOperations().map_err(|e|e.to_string())?;
        if op.GetAnyOperationsAborted().map_err(|e|e.to_string())?.as_bool()||path.exists(){return Err("Windows did not move the saved copy to the Recycle Bin.".into());}
    }
    Ok(())
}
pub(super) struct FolderChanges(windows::Win32::Foundation::HANDLE);
impl FolderChanges {
    pub fn watch(dir:&Path)->Option<Self>{
        use windows::{core::HSTRING,Win32::Storage::FileSystem::*};
        let flags=FILE_NOTIFY_CHANGE_FILE_NAME|FILE_NOTIFY_CHANGE_SIZE|FILE_NOTIFY_CHANGE_LAST_WRITE;
        unsafe { FindFirstChangeNotificationW(&HSTRING::from(dir.as_os_str()),false,flags) }.ok().map(Self)
    }
    pub fn changed(&self)->bool {
        use windows::Win32::{Foundation::WAIT_OBJECT_0,Storage::FileSystem::FindNextChangeNotification,System::Threading::WaitForSingleObject};
        if unsafe{WaitForSingleObject(self.0,0)}!=WAIT_OBJECT_0{return false;}
        let _=unsafe{FindNextChangeNotification(self.0)};
        true
    }
}
impl Drop for FolderChanges{
    fn drop(&mut self){let _=unsafe{windows::Win32::Storage::FileSystem::FindCloseChangeNotification(self.0)};}
}

#[cfg(test)]
pub(super) mod isolation_test {
    use super::*;

    #[test]
    fn watches_file_changes_without_polling_contents(){
        let folder=std::env::temp_dir().join(format!("buddy-watch-{}-{}",std::process::id(),super::super::now()));
        std::fs::create_dir(&folder).unwrap();
        let watcher=FolderChanges::watch(&folder).expect("register Windows directory notification");
        assert!(!watcher.changed());
        std::fs::write(folder.join("fixture"),b"x").unwrap();
        let until=std::time::Instant::now()+std::time::Duration::from_secs(2);
        loop{
            if watcher.changed(){break;}
            assert!(std::time::Instant::now()<until,"Windows file-change notification timed out");
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        drop(watcher);
        std::fs::remove_dir_all(folder).unwrap();
    }

    // This test is opt-in because both the process window station and the
    // calling thread's desktop change for its duration. Run it alone in its
    // own test process. Its clipboard is separate from the user's desktop.
    #[link(name="user32")]
    extern "system" {
        fn CreateWindowStationW(name:*const u16,flags:u32,access:u32,attrs:*const std::ffi::c_void)->HANDLE;
        fn CloseWindowStation(station:HANDLE)->i32;
        fn GetProcessWindowStation()->HANDLE;
        fn SetProcessWindowStation(station:HANDLE)->i32;
        fn CreateDesktopW(name:*const u16,device:*const u16,mode:*const std::ffi::c_void,flags:u32,access:u32,attrs:*const std::ffi::c_void)->HANDLE;
        fn CloseDesktop(desktop:HANDLE)->i32;
        fn GetThreadDesktop(thread:u32)->HANDLE;
        fn SetThreadDesktop(desktop:HANDLE)->i32;
    }
    #[link(name="kernel32")]extern "system"{fn GetCurrentThreadId()->u32;}
    struct IsolatedDesktop{old_station:HANDLE,old_desktop:HANDLE,station:HANDLE,desktop:HANDLE}
    impl Drop for IsolatedDesktop {
        fn drop(&mut self){unsafe{
            if !self.old_desktop.is_null(){SetThreadDesktop(self.old_desktop);}
            if !self.old_station.is_null(){SetProcessWindowStation(self.old_station);}
            if !self.desktop.is_null(){CloseDesktop(self.desktop);}
            if !self.station.is_null(){CloseWindowStation(self.station);}
        }}
    }
    pub(in crate::workspace) fn with_private_desktop(test:impl FnOnce()) {
        unsafe{
            let station=CreateWindowStationW(wide(format!("BuddyClipTest{}",std::process::id())).as_ptr(),0,0x000F037F,null());
            assert!(!station.is_null(),"private station: {}",std::io::Error::last_os_error());
            let mut isolated=IsolatedDesktop{old_station:GetProcessWindowStation(),old_desktop:GetThreadDesktop(GetCurrentThreadId()),station,desktop:null_mut()};
            assert_ne!(SetProcessWindowStation(station),0,"switch to private station");
            isolated.desktop=CreateDesktopW(wide("Synthetic").as_ptr(),null(),null(),0,0x000F01FF,null());
            assert!(!isolated.desktop.is_null(),"private desktop: {}",std::io::Error::last_os_error());
            assert_ne!(SetThreadDesktop(isolated.desktop),0,"switch to private desktop");
            test();
        }
    }
    #[test]
    #[ignore="run alone with --ignored --test-threads=1; uses its own Windows window station"]
    fn private_desktop_clipboard_roundtrip() {with_private_desktop(||unsafe {
            let hwnd=CreateWindowExW(0,wide("STATIC").as_ptr(),wide("Buddy synthetic test").as_ptr(),0,0,0,10,10,null_mut(),null_mut(),null_mut(),null());
            assert!(!hwnd.is_null());
            copy_text(hwnd,"Synthetic clipboard ✓").unwrap();
            assert!(matches!(read_clip(hwnd,false).unwrap(),Some(Clip::Text(s)) if s=="Synthetic clipboard ✓"));
            let bitmap=Bitmap{width:2,height:1,pixels:vec![0,0,255,255,255,0,0,120]};
            copy_bitmap(hwnd,&bitmap).unwrap();
            let Some(Clip::Image(actual))=read_clip(hwnd,true).unwrap() else{panic!("No image")};
            assert_eq!(actual.pixels,bitmap.pixels);
            let x=std::env::current_exe().unwrap();copy_path_item(hwnd,&x).unwrap();
            let _open=Clipboard::open(hwnd).unwrap();
            assert_ne!(IsClipboardFormatAvailable(CF_HDROP as u32),0);
            let drop=GetClipboardData(CF_HDROP as u32);
            let len=DragQueryFileW(drop,0,null_mut(),0);
            let mut wide=vec![0u16;len as usize+1];
            let n=DragQueryFileW(drop,0,wide.as_mut_ptr(),len+1);
            assert_eq!(PathBuf::from(OsString::from_wide(&wide[..n as usize])),x);
            drop_guard(_open);
            DestroyWindow(hwnd);
        });
    }
    fn drop_guard(owned:Clipboard){drop(owned);}
}
