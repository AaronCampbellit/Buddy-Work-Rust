//! Actual Win32 scrolling and focus on a private desktop with disposable records.
use super::*;

unsafe fn info(window: HWND) -> SCROLLINFO {
    let mut value = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as _,
        fMask: SIF_ALL,
        ..Default::default()
    };
    assert_ne!(GetScrollInfo(window, SB_VERT, &mut value), 0);
    value
}

unsafe fn child_y(window: HWND, parent: HWND) -> i32 {
    let mut bounds = RECT::default();
    GetWindowRect(window, &mut bounds);
    let mut origin = POINT { x: bounds.left, y: bounds.top };
    ScreenToClient(parent, &mut origin);
    origin.y
}

unsafe fn wheel(window: HWND, delta: i16, repeats: usize) {
    let signed_delta = (delta as u16 as usize) << 16;
    for _ in 0..repeats {
        SendMessageW(window, WM_MOUSEWHEEL, signed_delta, 0);
    }
}

unsafe fn click(parent: HWND, id: i32) {
    let child=GetDlgItem(parent,id);
    assert!(!child.is_null());
    SendMessageW(child, BM_CLICK, 0, 0);
}

#[test]
#[ignore="real Win32 scroll and keyboard checks; run alone on a private window station"]
fn overflow_and_nested_controls() {
    let root=env::temp_dir().join(format!("Buddy-scroll-{}-{}",std::process::id(),core::now()));
    fs::create_dir_all(&root).unwrap();
    let supplied=env::var_os("BUDDY_ISLAND_FIXTURES").map(PathBuf::from);
    if let Some(fixtures)=supplied {
        fs::copy(fixtures.join("fixture-empty/state-v1.dpapi"),root.join("state-v1.dpapi")).unwrap();
    } else {
        storage::save(&root,&storage::Records::new()).unwrap();
    }
    let mut failures=Vec::new();
    let mut check=|ok:bool,message:&str|{
        println!("{} {message}",if ok{"PASS"}else{"FAIL"});
        if !ok {failures.push(message.to_owned());}
    };
    workspace::with_private_test_desktop(|| unsafe {
        let h=create_app(&root,true,&[]);
        with_ui(|u|{
            let clips=(0..240).map(|n|vec!["text".into(),format!("Snippet {n:03}"),"0".into(),"0".into()]).collect();
            let files=(0..24).map(|n|vec![root.join(format!("File {n}.txt")).to_string_lossy().into(),"Local file".into(),"link".into()]).collect();
            let pins=(0..16).map(|n|vec!["https://example.com".into(),format!("Pin {n}"),"web".into()]).collect();
            let notes=(0..120).map(|n|vec![format!("id-{n}"),format!("Note {n:03}"),"Body".into(),"0".into(),"100".into(),"".into(),format!("{n:04}")]).collect();
            u.records.insert("notes".into(),notes);u.records.insert("clipboard".into(),clips);u.records.insert("files".into(),files);u.records.insert("quick".into(),pins);
            u.navigate("quick");
        });
        let view=GetDlgItem(h,2200);let grid=GetDlgItem(view,2201);
        // At this width, the vertical scrollbar changes the shelf from five
        // columns to four. Force it to disappear, then restore the long list.
        let search=GetDlgItem(view,2202);
        SetWindowTextW(search,wide("none of these items").as_ptr());
        SetWindowPos(view,null_mut(),0,0,775,200,SWP_NOZORDER|SWP_NOMOVE|SWP_NOACTIVATE);
        SetWindowTextW(search,wide("").as_ptr());
        let mut grid_rect=RECT::default();GetClientRect(grid,&mut grid_rect);
        let columns=((grid_rect.right+5)/154).max(1);
        let rows=(40+columns-1)/columns;
        let range=info(grid);
        // Five px at the top and up to one row gap after the last card.
        let last_card_bottom=5+(rows-1)*85+76;
        println!("Shelf columns={columns} max={} page={} final-card-bottom={last_card_bottom}",range.nMax,range.nPage);
        check(range.nMax+1>=last_card_bottom,"shelf scroll extent reaches the final rendered card after scrollbar width changes");

        with_ui(|u|u.navigate("clipboard"));
        let card=GetDlgItem(grid,2240);
        let mut lines=3u32;
        SystemParametersInfoW(SPI_GETWHEELSCROLLLINES,0,(&mut lines as *mut u32).cast(),0);
        if lines>0{
            wheel(card,-1,120);
            let fine=info(grid).nPos;
            SendMessageW(grid,WM_VSCROLL,SB_TOP as _,0);
            wheel(card,-120,1);
            let notch=info(grid).nPos;
            println!("Clipboard 120 fine deltas={fine} one notch={notch}");
            check(fine==notch && fine>0,"precision wheel keeps sub-pixel deltas over a card");
        }
        SendMessageW(grid,WM_VSCROLL,SB_TOP as _,0);
        SetFocus(card);
        SendMessageW(card,WM_KEYDOWN,VK_END as _,0);
        let focused=GetFocus();
        check(ui::window_text(focused,100).contains("Snippet 239"),"End focuses the last clipboard item");
        SendMessageW(focused,WM_KEYDOWN,VK_HOME as _,0);
        check(ui::window_text(GetFocus(),100).contains("Snippet 000"),"Home focuses the first clipboard item");
        SendMessageW(grid,WM_VSCROLL,SB_BOTTOM as _,0);
        check(GetFocus()==grid||IsChild(grid,GetFocus())==0||ui::window_text(GetFocus(),100).contains("Snippet 000"),
              "scrolling cannot silently redirect the focused card action to a different snippet");

        with_ui(|u|{
            u.navigate("notes");
            let list=u.list;check(SendMessageW(list,LB_GETCOUNT,0,0)==120,"all 120 notes are available in the native list");
            SendMessageW(list,WM_VSCROLL,SB_BOTTOM as _,0);
            let bottom=SendMessageW(list,LB_GETTOPINDEX,0,0);
            check(bottom>=100,"Notes scrolls down to the end of a long list");
            SendMessageW(list,WM_VSCROLL,SB_TOP as _,0);
            wheel(list,-120,1);
            check(lines==0 || SendMessageW(list,LB_GETTOPINDEX,0,0)>0,"wheel scrolls the Notes list");
            SendMessageW(list,LB_SETTOPINDEX,56,0);
            let before=SendMessageW(list,LB_GETTOPINDEX,0,0)as usize;
            let anchor=u.note_ids[before].clone();
            u.navigate("clipboard");
            check(u.list!=list && GetDlgItem(h,ui::LIST as i32)==u.list,"clipboard commands use the current list");
            u.navigate("notes");
            check(list==u.list && GetDlgItem(h,ui::LIST as i32)==list,"reopening Notes retains the native list");
            check(SendMessageW(list,LB_GETTOPINDEX,0,0)as usize==before,"an unchanged return preserves the scroll position");
            u.navigate("home");
            let records=u.records.get_mut("notes").unwrap();
            records[0][1]="Edited while away".into();
            let edited_id=records[0][0].clone();
            records.insert(0,vec!["new".into(),"Another note".into(),"".into(),"1".into(),"0".into(),"".into(),"100".into()]);
            u.navigate("notes");
            let edited=u.note_ids.iter().position(|id|id==&edited_id).unwrap();
            let len=SendMessageW(list,LB_GETTEXTLEN,edited,0);
            assert!((0..512).contains(&len));
            let mut caption=vec![0u16;len as usize+1];
            assert_eq!(SendMessageW(list,LB_GETTEXT,edited,caption.as_mut_ptr() as _),len);
            check(String::from_utf16_lossy(&caption[..len as usize]).contains("Edited while away"),"renames update accessible native captions on return");
            check(u.note_ids[0]=="new" && SendMessageW(list,LB_GETCOUNT,0,0)==121,"insert and reorder update the retained list");
            check(u.selected().map(|i|&u.note_ids[i])==u.active_note.as_ref(),"native selection follows the active note after reorder");
            let top=SendMessageW(list,LB_GETTOPINDEX,0,0)as usize;
            println!("Note refresh top {before}->{top}, anchored {anchor}, current {:?}",u.note_ids.get(top));
            check(u.note_ids.get(top)==Some(&anchor),"refreshing or reordering notes keeps the top visible note");
            SendMessageW(list,WM_KEYDOWN,VK_END as _,0);
            check(SendMessageW(list,LB_GETCURSEL,0,0)==120,"End still selects the last note");
        });
        with_ui(|u|u.navigate("settings"));
        let categories=GetDlgItem(h,3210);let settings=GetDlgItem(h,3211);click(categories,3201);
        let slider=GetDlgItem(settings,3624);let button=GetDlgItem(settings,3601);
        let top=child_y(slider,settings);
        SendMessageW(settings,WM_VSCROLL,SB_BOTTOM as _,0);
        let bottom=info(settings).nPos;
        println!("Appearance bottom={bottom} slider original={top} displayed={}",child_y(slider,settings));
        check(bottom>0&&child_y(slider,settings)==top-bottom,"settings controls below the fold move with their painted labels");
        SendMessageW(settings,WM_VSCROLL,SB_TOP as _,0);SetFocus(slider);
        let y=child_y(slider,settings);let mut client=RECT::default();GetClientRect(settings,&mut client);
        check(y>=0&&y<client.bottom,"tabbing to the offscreen scale slider reveals it");
        SetFocus(button);SendMessageW(settings,WM_VSCROLL,SB_TOP as _,0);
        if lines>0{
            let initial_slider=SendMessageW(slider,0x400,0,0);
            wheel(slider,-120,1);
            check(info(settings).nPos>0&&SendMessageW(slider,0x400,0,0)==initial_slider,
                  "wheel over the scale slider scrolls Settings without changing interface scale");
        }
        SendMessageW(settings,WM_VSCROLL,SB_BOTTOM as _,0);click(categories,3204);
        let after=info(settings);check(after.nPos==0,"switching from a long to short category clamps its position");
        let mut widget_wheel=true;let mut filters_reset=true;
        for _ in 0..8{
            with_ui(|u|u.navigate("widgets"));
            let timers=GetDlgItem(h,1900);let picker=GetDlgItem(timers,1903);
            if lines>0{
                let selected=SendMessageW(picker,CB_GETCURSEL,0,0);
                wheel(picker,-120,1);
                widget_wheel &= info(timers).nPos>0 && SendMessageW(picker,CB_GETCURSEL,0,0)==selected;
            }
            with_ui(|u|u.navigate("clipboard"));
            let query=GetDlgItem(view,2202);
            SetWindowTextW(query,wide("Snippet 239").as_ptr());
            filters_reset &= info(grid).nPos==0;
            SetWindowTextW(query,wide("").as_ptr());
            SendMessageW(grid,WM_VSCROLL,SB_BOTTOM as _,0);
        }
        check(widget_wheel,"wheel over a closed widget selector scrolls the page and preserves the selection");
        check(filters_reset,"eight page switches and filters to one row clear the old scroll offset");
        close_app(h);
    });
    let _=fs::remove_dir_all(root);
    assert!(failures.is_empty(),"{}",failures.join("\n"));
}
