//! Opt-in visual regression on a private Windows window station. WM_PRINT is
//! dispatched in-process to the normal application window and native children.
use super::*;
use image::{GenericImageView, Rgba, RgbaImage};
use std::{env,fs};

unsafe fn render(hwnd:HWND,area:RECT)->RgbaImage {
    let mut client=RECT::default();assert_ne!(GetClientRect(hwnd,&mut client),0);
    let (w,h)=(client.right,client.bottom);
    assert!(area.left>=0 && area.top>=0 && area.right<=w && area.bottom<=h);
    let dc=CreateCompatibleDC(null_mut());
    let mut info=BITMAPINFO::default();
    info.bmiHeader=BITMAPINFOHEADER{biSize:std::mem::size_of::<BITMAPINFOHEADER>() as _,biWidth:w,biHeight:-h,
        biPlanes:1,biBitCount:32,biCompression:BI_RGB,..Default::default()};
    let mut bits=null_mut();
    let bitmap=CreateDIBSection(dc,&info,DIB_RGB_COLORS,&mut bits,null_mut(),0);
    assert!(!dc.is_null() && !bitmap.is_null() && !bits.is_null());
    let original=SelectObject(dc,bitmap);
    // A sentinel supplies transparency outside each HWND's actual shaped hit
    // region. The native surfaces never use this color.
    let bytes=w as usize*h as usize*4;
    for px in std::slice::from_raw_parts_mut(bits.cast::<u8>(),bytes).chunks_exact_mut(4){px.copy_from_slice(&[219,33,233,0]);}
    SendMessageW(hwnd,WM_PRINT,dc as _,(PRF_CLIENT|PRF_NONCLIENT|PRF_ERASEBKGND|PRF_CHILDREN) as _);
    GdiFlush();
    let pixels=std::slice::from_raw_parts(bits.cast::<u8>(),bytes);
    let result=RgbaImage::from_fn((area.right-area.left) as u32,(area.bottom-area.top) as u32,|x,y|{
        let at=((y as usize+area.top as usize)*w as usize+x as usize+area.left as usize)*4;
        let b=pixels[at];let g=pixels[at+1];let r=pixels[at+2];
        if [r,g,b]==[233,33,219]{Rgba([255,255,255,0])}else{Rgba([r,g,b,255])}
    });
    SelectObject(dc,original);DeleteObject(bitmap);DeleteDC(dc);result
}

fn ink(im:&RgbaImage,x:u32,y:u32,w:u32,h:u32)->usize {
    image::imageops::crop_imm(im,x,y,w,h).pixels().filter(|(_,_,p)|p[3]>0 && p[0]<150 && p[1]<150 && p[2]<150).count()
}
fn fixture_pixels(im:&RgbaImage)->usize {
    im.pixels().filter(|p|[[250,103,81],[16,171,168],[239,188,68]].contains(&[p[0],p[1],p[2]]) && p[3]>0).count()
}
unsafe fn create_app(folder:&Path,open:bool,widgets:&[island::Widget])->HWND {
    assert!(folder.join("state-v1.dpapi").is_file(),"missing synthetic fixture {}",folder.display());
    let name=wide(CLASS);let inst=GetModuleHandleW(null());
    let class=WNDCLASSW{lpfnWndProc:Some(window_proc),hInstance:inst,lpszClassName:name.as_ptr(),..Default::default()};
    RegisterClassW(&class);
    let hwnd=CreateWindowExW(WS_EX_NOACTIVATE|WS_EX_TOOLWINDOW,name.as_ptr(),wide("Isolated Buddy visual capture").as_ptr(),
        WS_POPUP|WS_CLIPCHILDREN,0,0,840,300,null_mut(),null_mut(),inst,null());
    assert!(!hwnd.is_null());
    let mut ui=Ui::new(TypedWindow(hwnd),folder.to_owned(),true).unwrap();
    assert!(ui.read_error.is_none(),"fixture unreadable {:?}",ui.read_error);
    ui.expanded=open;ui.zoom=core::setting(&ui,"scale","100").parse().unwrap();
    let layout=island::Layout::new(RECT{left:0,top:0,right:1920,bottom:1080},96,ui.zoom,island::Placement::Center,300,widgets);
    let host=layout.host;island::configure(layout,open);island::request(open,true);
    SetWindowPos(hwnd,null_mut(),host.left,host.top,host.right-host.left,host.bottom-host.top,SWP_NOZORDER|SWP_NOACTIVATE);
    island::apply_region(hwnd,&island::frame());
    ui.apply_appearance();ui.reorder_navigation();ui.navigate("home");ui.layout();
    WINDOW.with(|cell|*cell.borrow_mut()=Some(ui));
    ShowWindow(hwnd,SW_SHOWNOACTIVATE);
    with_ui(side_widgets::sync);
    hwnd
}
unsafe fn close_app(hwnd:HWND){
    DestroyWindow(hwnd);WINDOW.with(|slot|{slot.borrow_mut().take();});
}
fn save(folder:&Path,name:&str,pixels:&RgbaImage) {
    let file=folder.join(format!("{name}.png"));pixels.save(&file).unwrap();
    println!("{} {}x{} {} bytes, {} fixture pixels",file.display(),pixels.width(),pixels.height(),fs::metadata(&file).unwrap().len(),fixture_pixels(pixels));
}
fn seed_profiles(folder:&Path) {
    use sha2::{Digest,Sha256};
    let pixels=image::RgbImage::from_fn(112,42,|x,_|{
        image::Rgb(if x<40{[250u8,103,81]}else if x<72{[16,171,168]}else{[239,188,68]})
    });
    let mut png=std::io::Cursor::new(Vec::new());pixels.write_to(&mut png,image::ImageFormat::Png).unwrap();
    let id=format!("{:x}",Sha256::digest(png.get_ref()));
    let row=|fields:&[&str]|fields.iter().map(|v|v.to_string()).collect::<Vec<String>>();
    for name in ["empty","image","scaled","cards","clip"]{
        let profile=folder.join(format!("fixture-{name}"));
        let mut data=storage::Records::new();
        let zoom=if name=="scaled"{"150"}else{"100"};
        let mut settings=vec![row(&["theme","light"]),row(&["scale",zoom]),row(&["open","hover"])];
        if name!="empty" {
            let clip=row(&["image",&id,"123","0","112","42"]);
            data.insert("notes".into(),vec![row(&["example","Project sketch","Before the image. After the image.","0","1770000000","","1770000000",&id])]);
            data.insert("clipboard".into(),vec![clip.clone()]);
            data.insert(inline_images::offset_key("example"),vec![row(&["18"])]);
            if name=="clip"{data.insert("clipboard_widget".into(),vec![clip]);}
            if name=="cards"{settings.extend([row(&["watchWidget","on"]),row(&["pomWidget","on"]),row(&["noteWidget","example"])]);}
            storage::write_protected(&profile.join("images").join(format!("{id}.dpapi")),png.get_ref()).unwrap();
        }
        data.insert("settings".into(),settings);storage::save(&profile,&data).unwrap();
    }
}
#[test]
#[ignore="opt-in visual capture; run alone with BUDDY_UI_CAPTURE_DIR on a private Windows station"]
fn capture_real_workspaces_and_cards() {
    // A signed-out Windows service can read pre-seeded synthetic fixtures.
    // In a signed-in session, create disposable fixtures with production DPAPI.
    let supplied=env::var_os("BUDDY_ISLAND_FIXTURES").map(PathBuf::from);
    let fixtures=supplied.clone().unwrap_or_else(||env::temp_dir().join(format!("Buddy-island-capture-{}-{}",std::process::id(),core::now())));
    if supplied.is_none(){seed_profiles(&fixtures);}
    let output=PathBuf::from(env::var_os("BUDDY_UI_CAPTURE_DIR").expect("BUDDY_UI_CAPTURE_DIR"));
    fs::create_dir_all(&output).unwrap();
    workspace::with_private_test_desktop(||unsafe{
        let empty=create_app(&fixtures.join("fixture-empty"),true,&[]);
        let home=render(empty,island::frame().body);save(&output,"home-empty",&home);
        with_ui(|u|u.navigate("notes"));
        let notes=render(empty,island::frame().body);save(&output,"notes-empty",&notes);
        SendMessageW(GetDlgItem(empty,181),BM_CLICK,0,0);
        with_ui(|u|assert_eq!(u.page,"widgets","Widgets header command opens its workspace"));
        let viewport=GetDlgItem(empty,1900);assert!(!viewport.is_null());
        assert_ne!(GetWindowLongW(viewport,GWL_STYLE) as u32&WS_VISIBLE,0);
        assert!(!GetDlgItem(viewport,1901).is_null() && !GetDlgItem(viewport,1920).is_null());
        let widgets=render(empty,island::frame().body);save(&output,"widgets-expanded",&widgets);
        assert!(ink(&widgets,39,94,220,40)>40,"Side widgets title and description");
        assert!(ink(&widgets,39,205,120,48)>65,"Pomodoro time");
        assert!(ink(&widgets,425,205,152,48)>65,"Stopwatch time");
        SendMessageW(viewport,WM_VSCROLL,SB_PAGEDOWN as _,0);
        let scrolled=render(empty,island::frame().body);save(&output,"widgets-scrolled",&scrolled);
        assert_ne!(widgets.as_raw(),scrolled.as_raw(),"scroll reveals controls below timers");
        with_ui(|u|{u.expanded=false;island::request(false,true);island::apply_region(empty,&island::frame());u.layout();});
        let compact=render(empty,island::frame().body);save(&output,"compact",&compact);
        close_app(empty);
        let mut images=Vec::new();
        for (fixture,name) in [("image","inline-image"),("scaled","inline-scaled")]{
            let hwnd=create_app(&fixtures.join(format!("fixture-{fixture}")),true,&[]);
            with_ui(|u|u.navigate("notes"));
            let frame=render(hwnd,island::frame().body);save(&output,name,&frame);images.push(frame);
            if fixture=="image"{
                with_ui(|u|{u.navigate("clipboard");u.select_row(0);u.run_action("select");});
                let clip=render(hwnd,island::frame().body);save(&output,"clipboard-image",&clip);images.push(clip);
            }
            close_app(hwnd);
        }
        for (fixture,kinds) in [
            ("cards",vec![island::Widget::Focus,island::Widget::Stopwatch,island::Widget::Note]),
            ("clip",vec![island::Widget::Clipboard]),
        ]{
            let hwnd=create_app(&fixtures.join(format!("fixture-{fixture}")),false,&kinds);
            let strip=FindWindowW(wide("BuddyRustSideWidgets").as_ptr(),null());
            assert!(!strip.is_null());
            let mut client=RECT::default();GetClientRect(strip,&mut client);
            let cards=render(strip,client);save(&output,&format!("side-{fixture}"),&cards);
            assert!(ink(&cards,10,5,cards.width()-20,90)>80,"native side titles, times and actions");
            close_app(hwnd);
        }
        assert_eq!(home.dimensions(),(840,300));assert_eq!(compact.dimensions(),(346,40));
        assert!(ink(&home,350,0,140,44)>30,"native header");
        assert!(ink(&home,155,54,527,24)>70,"native workspace navigation controls");
        assert!(ink(&home,340,170,155,90)>25,"native home action inside card");
        assert!(ink(&notes,37,164,194,95)>70,"native Notes empty state");
        assert!(ink(&compact,108,3,130,35)>20,"compact clock");
        assert_eq!(images[2].dimensions(),(1260,450));
        for im in images{assert!(fixture_pixels(&im)>=120,"saved image colors in native frame {:?}",im.dimensions());}
    });
    if supplied.is_none(){fs::remove_dir_all(fixtures).unwrap();}
}
