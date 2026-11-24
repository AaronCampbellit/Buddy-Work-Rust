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

#[test]
#[ignore="opt-in transition capture; uses the same private profile and desktop as the endpoint captures"]
fn note_image_fades_with_the_workspace_during_island_motion(){
    use std::time::Duration;
    let supplied=env::var_os("BUDDY_ISLAND_FIXTURES").map(PathBuf::from);
    let fixtures=supplied.clone().unwrap_or_else(||env::temp_dir().join(format!("Buddy-motion-capture-{}-{}",std::process::id(),core::now())));
    if supplied.is_none(){seed_profiles(&fixtures);}
    let output=PathBuf::from(env::var_os("BUDDY_UI_CAPTURE_DIR").expect("BUDDY_UI_CAPTURE_DIR"));
    fs::create_dir_all(&output).unwrap();
    workspace::with_private_test_desktop(||unsafe{
        let hwnd=create_app(&fixtures.join("fixture-image"),true,&[]);
        with_ui(|ui|{
            assert!(!shell::reduce_motion(ui),"this visual capture requires Windows client animations on its test station");
            ui.navigate("notes");
        });
        let open=render(hwnd,island::frame().body);
        let red=[250,103,81];
        let mut background=0;
        with_ui(|ui|background=ui.appearance.0);
        let paper=[background as u8,(background>>8) as u8,(background>>16) as u8];
        let anchors:Vec<_>=open.enumerate_pixels().filter_map(|(x,y,pixel)|
            (pixel.0[..3]==red).then_some((x,y))).collect();
        assert!(anchors.len()>100,"fixture image must be visible at the open endpoint");
        with_ui(|ui|{shell::expand(ui,false);shell::capture_transition_at(ui,Duration::from_millis(240));});
        with_ui(|ui|shell::expand(ui,true));
        for elapsed in [24,82,160,240]{
            with_ui(|ui|shell::capture_transition_at(ui,Duration::from_millis(elapsed)));
            let frame=island::frame();
            let image=render(hwnd,frame.body);
            save(&output,&format!("motion-note-open-{elapsed:03}ms"),&image);
            assert!(ink(&image,image.width()/3,0,image.width()/3,35)>12,"persistent clock at {elapsed} ms");
            if elapsed!=82{continue}
            assert!(frame.body_alpha>0.1 && frame.body_alpha<0.9);
            let dx=(frame.body.left+frame.body.right-frame.expanded_body.left-frame.expanded_body.right)/2
                +frame.expanded_body.left-frame.body.left;
            let expected=std::array::from_fn::<_,3,_>(|i|paper[i] as f32+(red[i] as f32-paper[i] as f32)*frame.body_alpha);
            let mut checked=0;let mut matching=0;
            for &(x,y) in &anchors{
                let x=x as i32+dx;
                if x<9 || x>=image.width() as i32-9 || y+12>=image.height(){continue}
                checked+=1;
                let pixel=image.get_pixel(x as u32,y);
                if (0..3).all(|i|(pixel[i] as f32-expected[i]).abs()<6.0){matching+=1;}
            }
            assert!(checked>100 && matching*100>checked*90,
                "{matching}/{checked} inline image pixels follow the body opacity");
        }
        close_app(hwnd);
    });
    if supplied.is_none(){fs::remove_dir_all(fixtures).unwrap();}
}

#[path="ui/scroll_tests.rs"]
mod scroll_tests;

#[test]
#[ignore="live Win32 animation and focus on a private desktop"]
fn opening_tab_click_and_reversal_preserve_visible_targets(){
    use std::time::Duration;
    let supplied=env::var_os("BUDDY_ISLAND_FIXTURES").map(PathBuf::from);
    let fixtures=supplied.clone().unwrap_or_else(||env::temp_dir().join(format!("Buddy-animation-input-{}-{}",std::process::id(),core::now())));
    if supplied.is_none(){seed_profiles(&fixtures);}
    workspace::with_private_test_desktop(||unsafe{
        let hwnd=create_app(&fixtures.join("fixture-empty"),false,&[]);
        let left=|id|{
            let mut rect=RECT::default();GetWindowRect(GetDlgItem(hwnd,id),&mut rect);rect.left
        };
        let compact_bell=left(ui::COMPACT_TOOLS as i32);
        with_ui(|u|shell::expand(u,true));
        assert_eq!(left(180),compact_bell,"the first opening frame keeps the bell in place");
        with_ui(|u|shell::capture_transition_at(u,Duration::from_millis(95)));
        let bell=left(180);
        with_ui(|u|shell::expand(u,false));
        assert_eq!(left(180),bell,"reversing from opening preserves the displayed bell position");
        with_ui(|u|shell::expand(u,true));
        assert_eq!(left(180),bell,"reversing from closing preserves the displayed bell position");

        let tab=GetDlgItem(hwnd,173);let mut rc=RECT::default();GetWindowRect(tab,&mut rc);
        let mut point=POINT{x:(rc.left+rc.right)/2,y:(rc.top+rc.bottom)/2};
        ScreenToClient(hwnd,&mut point);point.x+=island::frame().workspace_offset();
        let at=((point.y as u16 as usize)<<16 | point.x as u16 as usize) as isize;
        SendMessageW(hwnd,WM_LBUTTONDOWN,1,at);
        SendMessageW(hwnd,WM_LBUTTONUP,0,0);
        assert!(NAV_PRESS.get().is_null());
        let mut msg=MSG::default();
        assert_eq!(PeekMessageW(&mut msg,tab,BM_CLICK,BM_CLICK,PM_REMOVE),0,"dragging off cancels the click");

        SendMessageW(hwnd,WM_LBUTTONDOWN,1,at);
        SendMessageW(hwnd,WM_LBUTTONUP,0,at);
        assert_ne!(PeekMessageW(&mut msg,tab,BM_CLICK,BM_CLICK,PM_REMOVE),0,"the visible tab receives the click during the fade");
        DispatchMessageW(&msg);
        with_ui(|ui|assert_eq!(ui.page,"notes"));
        assert!(island::frame().animating,"a tab change does not snap the island to full width");
        close_app(hwnd);
    });
    if supplied.is_none(){fs::remove_dir_all(fixtures).unwrap();}
}

#[test]
#[ignore="checks first-frame rich note colors in real app and widget windows on a private Windows station"]
fn rich_note_uses_theme_on_first_workspace_and_widget_frame(){
    use windows::Win32::UI::Controls::RichEdit::{EM_CANREDO,EM_GETSCROLLPOS,EM_REDO,EM_STOPGROUPTYPING};
    let temp=env::temp_dir().join(format!("Buddy-rich-note-frame-{}-{}",std::process::id(),core::now()));
    let profile=temp.join("fixture-image");fs::create_dir_all(profile.join("images")).unwrap();
    if let Some(source)=env::var_os("BUDDY_ISLAND_FIXTURES"){
        let source=PathBuf::from(source).join("fixture-image");
        fs::copy(source.join("state-v1.dpapi"),profile.join("state-v1.dpapi")).unwrap();
        for file in fs::read_dir(source.join("images")).unwrap().map(Result::unwrap){
            fs::copy(file.path(),profile.join("images").join(file.file_name())).unwrap();
        }
    }else{seed_profiles(&temp);}
    workspace::with_private_test_desktop(||unsafe{
        let widgets=[island::Widget::Note];
        let hwnd=create_app(&profile,true,&widgets);
        let rtf=r"{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}{\colortbl ;\red0\green105\blue109;}\f0\fs20 {\b\fs30\cf1 Map palette}\par\par Sea glass marks the trail. Navy is for place names.\par Base map: https://www.openstreetmap.org\par}";
        with_ui(|ui|{
            let image=ui.records["clipboard"][0][1].clone();
            let mut note=["map","Coastal trail guide","Map palette\n\nSea glass marks the trail. Navy is for place names.\nBase map: https://www.openstreetmap.org","1","1","","1"].map(str::to_owned).to_vec();
            note.push(image);
            ui.records.insert("notes".into(),vec![note]);
            ui.records.insert(inline_images::offset_key("map"),vec![vec!["12".into()]]);
            ui.records.insert(inline_images::format_key("map"),vec![vec![rtf.into()]]);
            core::set(ui,"noteWidget","map");core::set(ui,"motion","off");
        });
        let mut saved_stamp=String::from("1");
        for (name,theme) in [("dark","dark"),("light","light"),("reopened","dark")]{
            for opened in [true,false]{
                let mut ink=0;
                with_ui(|ui|{
                    core::set(ui,"theme",theme);
                    let layout=island::Layout::new(RECT{left:0,top:0,right:1920,bottom:1080},96,100,island::Placement::Center,376,&widgets);
                    let h=layout.host;SetWindowPos(hwnd,null_mut(),h.left,h.top,h.right-h.left,h.bottom-h.top,SWP_NOACTIVATE|SWP_NOZORDER);
                    ui.expanded=opened;island::configure(layout,opened);island::request(opened,true);
                    island::apply_region(hwnd,&island::frame());ui.apply_appearance();
                    ui.active_note=Some("map".into());ui.navigate(if opened{"notes"}else{"home"});ui.layout();side_widgets::sync(ui);
                    ink=ui.appearance.2;
                });
                let target=if opened{hwnd}else{FindWindowW(wide("BuddyRustSideWidgets").as_ptr(),null())};
                assert!(!target.is_null());let mut area=island::frame().body;if !opened{GetClientRect(target,&mut area);}
                // Capture immediately, before pumping messages or asking the native
                // editor to measure any characters. Both can mask a stale OLE layout.
                let frame=render(target,area);
                with_ui(|ui|{
                    assert!(!ui.dirty_note,"temporary paint marked the note dirty in {name}");
                    assert_eq!(ui.records["notes"][0][6],saved_stamp,"repainting changed the note timestamp in {name}");
                });
                let surface=if opened{"workspace"}else{"widget"};
                if let Some(dir)=env::var_os("BUDDY_UI_CAPTURE_DIR"){
                    let dir=PathBuf::from(dir);fs::create_dir_all(&dir).unwrap();
                    save(&dir,&format!("rich-{name}-{surface}"),&frame);
                }
                assert!(fixture_pixels(&frame)>120,"inline image on {name} {surface}");
                let parent=if opened{hwnd}else{FindWindowExW(target,null_mut(),wide("BuddyRustSideCard").as_ptr(),null())};
                let edit=GetDlgItem(parent,inline_images::NOTE_EDITOR as i32);assert!(!edit.is_null());
                let mut point=POINT::default();
                SendMessageW(edit,0xd6,&mut point as *mut _ as _,14); // EM_POSFROMCHAR, first ordinary text after image.
                MapWindowPoints(edit,target,&mut point,1);
                let (x,y)=((point.x-area.left).max(0)as u32,(point.y-area.top).max(0)as u32);
                let rgb=ink.to_le_bytes();
                let pixels=image::imageops::crop_imm(&frame,x,y,150.min(frame.width()-x),16.min(frame.height()-y)).pixels()
                    .filter(|(_,_,p)|p[3]>0 && (0..3).all(|ch|p[ch].abs_diff(rgb[ch])<35)).count();
                assert!(pixels>30,"{name} {surface}: only {pixels} body pixels with theme ink {ink:#x} at {x},{y}");
                if name!="reopened"{continue}

                let marker=if opened{'Ω'}else{'Ψ'};
                let editor_view=||{
                    let (mut a,mut b)=(0u32,0u32);SendMessageW(edit,0xb0,&mut a as *mut _ as _,&mut b as *mut _ as _);
                    let mut scroll=POINT::default();SendMessageW(edit,EM_GETSCROLLPOS,0,&mut scroll as *mut _ as _);
                    (a,b,scroll.x,scroll.y)
                };
                let repaint=|theme:&str|{
                    with_ui(|ui|{core::set(ui,"theme",theme);ui.apply_appearance();ui.layout();side_widgets::sync(ui);});
                    let _=render(target,area);
                };
                SendMessageW(edit,0xb1,34,34); // place caret inside the ordinary text
                SendMessageW(edit,WM_CHAR,marker as usize,1);SendMessageW(edit,EM_STOPGROUPTYPING,0,0);
                assert!(ui::window_text(edit,4096).contains(marker),"typing works in {surface}");
                let before=editor_view();repaint("light");
                assert_eq!(before,editor_view(),"theme repaint moved the selection or scroll in {surface}");
                assert_ne!(SendMessageW(edit,0xc6,0,0),0,"typing has undo in {surface}");
                assert_ne!(SendMessageW(edit,0xc7,0,0),0,"native EM_UNDO in {surface}");
                assert!(!ui::window_text(edit,4096).contains(marker),"first undo removes the typed character in {surface}");
                let before=editor_view();repaint("dark");
                assert_eq!(before,editor_view(),"repaint after undo moved the selection or scroll in {surface}");
                assert_ne!(SendMessageW(edit,EM_CANREDO,0,0),0,"redo survives theme repaint in {surface}");
                SendMessageW(edit,EM_REDO,0,0);
                assert!(ui::window_text(edit,4096).contains(marker),"redo restores the character in {surface}");
                with_ui(|ui|{
                    if opened{assert!(ui.dirty_note);core::save_note(ui).unwrap();}else{side_widgets::flush(ui).unwrap();}
                    assert!(ui.records["notes"][0][2].contains(marker),"save actual {surface} edit");
                    let stamp=ui.records["notes"][0][6].clone();assert_ne!(stamp,saved_stamp,"timestamp of actual {surface} edit");
                    saved_stamp=stamp;
                });
            }
        }
        close_app(hwnd);
    });
    fs::remove_dir_all(temp).unwrap();
}
