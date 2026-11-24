use super::*;

#[derive(Clone,Copy)]
#[repr(usize)]
pub(super) enum Symbol{Clipboard,Type,Link,Image,Trash,Folder,FolderOpen,Download,File,FileText,App,Globe,More,Pin,Pencil,Save,Copy,Close,Search,Plus,ChevronDown=22,ClipboardCopy,Pointer,Arrow,Square,Circle,Highlight,Redact,Number,Crop,Undo,Redo,History,ZoomOut,ZoomIn,Minus,Eye}
fn dib(width:i32,height:i32)->BITMAPINFO{
    let mut info=BITMAPINFO::default();info.bmiHeader=BITMAPINFOHEADER{biSize:std::mem::size_of::<BITMAPINFOHEADER>()as _,biWidth:width,
        biHeight:-height,biPlanes:1,biBitCount:32,biCompression:BI_RGB,..Default::default()};info
}
pub(super) fn symbol(dc:HDC,x:i32,y:i32,side:i32,sym:Symbol,ink:u32,base:u32){
    use std::sync::OnceLock;
    static ART:OnceLock<image::GrayImage>=OnceLock::new();
    let art=ART.get_or_init(||image::load_from_memory(include_bytes!("workspaces_icons.png")).expect("workspace icon atlas").into_luma8());
    if !(1..=160).contains(&side){return}
    let len=side as u32;let offset=sym as u32*64;let mut pixels=vec![0u32;(side*side)as usize];
    for y in 0..len{for x in 0..len{
        let x0=x*64/len;let x1=((x+1)*64/len).max(x0+1);let y0=y*64/len;let y1=((y+1)*64/len).max(y0+1);
        let mut a=0;for sy in y0..y1{for sx in x0..x1{a+=u32::from(art.get_pixel(offset+sx,sy)[0]);}}
        let c=mix(base,ink,a/((x1-x0)*(y1-y0)));
        pixels[(y*len+x)as usize]=((c&255)<<16)|(c&0xff00)|((c>>16)&255);
    }}
    unsafe{SetDIBitsToDevice(dc,x,y,len,len,0,0,0,len,pixels.as_ptr().cast(),&dib(side,side),DIB_RGB_COLORS);}
}

pub(super) struct Picture{width:i32,height:i32,pixels:Vec<u8>}
impl Picture{
    pub(super) fn new(image:image::RgbaImage)->Self{
        let (width,height)=image.dimensions();let mut pixels=image.into_raw();
        for px in pixels.chunks_exact_mut(4){px.swap(0,2);}
        Self{width:width as i32,height:height as i32,pixels}
    }
    fn draw(&self,dc:HDC,r:RECT,cover:bool,bg:u32){
        let w=r.right-r.left;let h=r.bottom-r.top;if w<=0||h<=0{return}
        // Center-crop the source rectangle, retaining the image's aspect ratio.
        let (mut sx,mut sy,mut sw,mut sh)=(0,0,self.width,self.height);
        let (mut dx,mut dy,mut dw,mut dh)=(r.left,r.top,w,h);
        if cover{
            if w*sh>h*sw{sh=(sw*h/w).max(1);sy=(self.height-sh)/2;}else{sw=(sh*w/h).max(1);sx=(self.width-sw)/2;}
        }else if w*sh>h*sw{dw=(h*sw/sh).max(1);dx+=(w-dw)/2}else{dh=(w*sh/sw).max(1);dy+=(h-dh)/2}
        let transparent=self.pixels.chunks_exact(4).any(|p|p[3]<255);let blended;
        let data=if transparent{
            let mut rgb=self.pixels.clone();let backdrop=[(bg>>16)as u8,(bg>>8)as u8,bg as u8];
            for p in rgb.chunks_exact_mut(4){for j in 0..3{p[j]=((p[j]as u32*p[3]as u32+backdrop[j]as u32*(255-p[3]as u32)+127)/255)as u8;}}blended=rgb;&blended
        }else{&self.pixels};
        unsafe{let mode=SetStretchBltMode(dc,HALFTONE);SetBrushOrgEx(dc,0,0,null_mut());
            StretchDIBits(dc,dx,dy,dw,dh,sx,sy,sw,sh,data.as_ptr().cast(),&dib(self.width,self.height),DIB_RGB_COLORS,SRCCOPY);SetStretchBltMode(dc,mode);}
    }
}

impl State{
    fn palette(&self)->(u32,u32,u32,u32,u32,u32){
        let (bg,field,ink,accent)=self.colors.get();(bg,field,ink,accent,mix(bg,ink,44),mix(bg,ink,162))
    }
    fn ico(&self,dc:HDC,x:i32,y:i32,side:i32,id:Symbol,color:u32,base:u32){symbol(dc,self.px(x),self.px(y),self.px(side),id,color,base);}
    fn line(&self,dc:HDC,r:RECT,s:&str,font:usize,color:u32,flags:u32){text(dc,r,s,self.fonts.get()[font],color,flags);}
    fn caption(&self,dc:HDC,r:RECT,s:&str,ico:Option<Symbol>,font:usize,color:u32,base:u32){
        if let Some(icon)=ico{
            let size=self.px(13);let length=if s.is_empty(){0}else{chrome::width(dc,s,self.fonts.get()[font])+self.px(5)};
            let x=r.left+((r.right-r.left-size-length)/2).max(self.px(1));symbol(dc,x,r.top+(r.bottom-r.top-size)/2,size,icon,color,base);
            if !s.is_empty(){self.line(dc,RECT{left:x+size+self.px(5),..r},s,font,color,LEFT);}
        }else{self.line(dc,r,s,font,color,CENTER)}
    }
    pub(super) fn paint(&self,dc:HDC,grid:bool){
        let (bg,field,ink,accent,edge,muted)=self.palette();let area=client(if grid{self.grid.get()}else{self.view.get()});
        fill(dc,area,bg);let page=self.page.get();
        if grid{
            if !self.cards.borrow().is_empty(){return}
            let populated=self.counts.get()[0]>0;
            let (title,detail,glyph)=match page{
                "clipboard" if populated=>("No matching clipboard items","Try another search or type filter.",Symbol::Search),
                "clipboard"=>("No clipboard history yet","Copy text, links, or an image and Buddy will keep it here on this device.",Symbol::ClipboardCopy),
                "downloads"=>("No recent downloads.","",Symbol::Download),
                _ if populated=>("No matching items","Try another search or item type.",Symbol::FolderOpen),
                _=>("No items in Quick Access","Add files, folders, apps, or links to keep them handy.",Symbol::FolderOpen),
            };
            let width=chrome::width(dc,title,self.fonts.get()[3]).max(chrome::width(dc,detail,self.fonts.get()[0]));
            let start=((area.right-width-self.px(43))/2).max(self.px(2));let cy=area.bottom/2;
            symbol(dc,start,cy-self.px(13),self.px(27),glyph,muted,bg);let x=start+self.px(43);
            if detail.is_empty(){self.line(dc,rect(x,cy-self.px(10),area.right-x,self.px(20)),title,0,muted,LEFT);}
            else{self.line(dc,rect(x,cy-self.px(18),area.right-x,self.px(19)),title,3,ink,LEFT);
                self.line(dc,rect(x,cy+self.px(2),area.right-x,self.px(17)),detail,0,muted,LEFT);}
            return
        }
        let sr=self.search_rect.get();
        if sr.right>0{
            let r=self.area(sr.left,sr.top,sr.right-sr.left,sr.bottom-sr.top);
            panel(dc,r,self.px(16),field,if unsafe{GetFocus()}==self.hwnd(QUERY){accent}else{edge});
            self.ico(dc,sr.left+9,sr.top+7,14,Symbol::Search,muted,field);
        }
        if self.form.get()&&shelf(page){
            let y=self.grid_top.get()-49;fill(dc,self.area(0,y,self.dp(area.right),1),edge);
            for (id,name) in [(NAME,"Label"),(TARGET,if self.typed_path.get(){"Path"}else{"Target"}),(KIND,"Type")]{
                let h=self.hwnd(id);if !on(h){continue}
                let mut b=RECT::default();unsafe{GetWindowRect(h,&mut b);let mut p=POINT{x:b.left,y:b.top};ScreenToClient(self.view.get(),&mut p);self.line(dc,rect(p.x,p.y-self.px(16),b.right-b.left,self.px(15)),name,2,muted,LEFT);}
            }
        }
        if page=="downloads"{let (active,recent)=self.download_counts.get();self.line(dc,self.area(0,0,200,30),&format!("{active} active · {recent} recent"),2,muted,LEFT);}
    }
    pub(super) fn draw(&self,di:&DRAWITEMSTRUCT){
        let id=di.CtlID as u16;let dc=di.hDC;let r=di.rcItem;let (bg,field,ink,accent,edge,muted)=self.palette();
        let hovered=chrome::hovered(di.hwndItem)||di.itemState&ODS_SELECTED!=0;
        let focus=di.itemState&ODS_FOCUS!=0&&di.itemState&ODS_NOFOCUSRECT==0;
        let disabled=di.itemState&ODS_DISABLED!=0;
        if let Some((card,part))=self.card(id){
            let w=self.dp(r.right-r.left);let h=self.dp(r.bottom-r.top);
            if part==0{
                let face=if hovered&&!disabled{mix(field,ink,14)}else{field};fill(dc,r,bg);
                let border=if focus||(card.section=="clipboard"&&self.highlight.get()==Some(card.index)){accent}else if card.pinned{mix(bg,accent,114)}else{edge};
                let radius=match card.section{"clipboard"=>14,"downloads"=>8,_=>10};
                panel(dc,r,self.px(radius),face,border);
                // SelectClipRgn works in device coordinates, whereas WM_PRINT maps
                // each child to its offset within the island's off-screen DC.
                let inner=inset(r,1);
                let mut corners=[POINT{x:inner.left,y:inner.top},POINT{x:inner.right+1,y:inner.bottom+1}];
                let cl=unsafe{
                    LPtoDP(dc,corners.as_mut_ptr(),2);
                    CreateRoundRectRgn(corners[0].x,corners[0].y,corners[1].x,corners[1].y,self.px(radius*2),self.px(radius*2))
                };
                let saved=unsafe{SaveDC(dc)};unsafe{ExtSelectClipRgn(dc,cl,RGN_AND);DeleteObject(cl);}
                let term=if disabled{muted}else{ink};
                match card.section{
                    "clipboard"=>{
                        if card.kind=="Image"{
                            let image_box=self.area(1,1,w-2,h-34);
                            if let Some(Some(pic))=self.pictures.borrow().get(&card.image){pic.draw(dc,image_box,true,face)}else{self.ico(dc,(w-24)/2,(h-32-24)/2,24,Symbol::Image,muted,face);}
                        }else{
                            let fh=self.px(15);let text_box=self.area(11,9,w-22,if card.kind=="Link"{60}else{90});
                            let font=self.fonts.get()[usize::from(card.kind=="Link")];let max=(text_box.bottom-text_box.top)/fh.max(1);
                            let lines=lines(dc,&card.title,font,text_box.right-text_box.left,max as usize);
                            for (i,line) in lines.iter().enumerate(){text(dc,rect(text_box.left,text_box.top+i as i32*fh,text_box.right-text_box.left,fh),line,font,ink,LEFT);}
                            if card.kind=="Link"{self.line(dc,self.area(11,13+lines.len()as i32*15,w-22,15),&card.meta,2,accent,LEFT);}
                        }
                        self.ico(dc,10,h-23,11,card.icon,muted,face);self.line(dc,self.area(24,h-26,34,16),&card.kind,2,muted,LEFT);
                        self.line(dc,self.area(52,h-26,(w-54-card.actions().len()as i32*25-4).max(0),16),&card.stamp,2,muted,LEFT);
                    }
                    "downloads"=>{
                        if let Some(Some(pic))=self.pictures.borrow().get(&card.image){pic.draw(dc,self.area(2,2,43,h-4),true,face)}else{self.ico(dc,13,(h-24)/2,24,card.icon,muted,face)}
                        self.line(dc,self.area(48,10,w-110,20),&card.title,1,term,LEFT);
                        self.line(dc,self.area(48,32,w-110,17),&card.meta,2,muted,LEFT);
                    }
                    _=>{
                        if let Some(Some(pic))=self.pictures.borrow().get(&card.image){pic.draw(dc,self.area(6,9,32,h-18),false,face)}else{self.ico(dc,9,(h-26)/2,26,card.icon,if card.kind=="Folder"{accent}else{muted},face);}
                        let kind=if card.section=="files"{format!("{}   {}",card.kind.to_uppercase(),if card.stored{"Saved copy"}else{"Link"})}else{card.kind.to_uppercase()};
                        self.line(dc,self.area(44,10,w-70,15),&kind,2,muted,LEFT);
                        self.line(dc,self.area(44,27,w-51,20),&card.title,1,term,LEFT);
                        self.line(dc,self.area(44,47,w-51,15),&card.meta,2,muted,LEFT);
                    }
                }
                unsafe{RestoreDC(dc,saved);}
                if focus{unsafe{DrawFocusRect(dc,&inset(r,self.px(3)));}}
            }else if let Some((action,glyph,_))=card.actions().get(part-1){
                let active=*action=="pin"&&card.pinned;
                let danger=if bg&255>150{0x2d23b4}else{0x8b8bff};
                let color=if disabled{mix(field,muted,88)}else if hovered&&*action=="remove"{danger}else if active{accent}else if hovered{ink}else{muted};
                let base=if active{mix(field,accent,29)}else if hovered{mix(field,color,23)}else if shelf(card.section){bg}else{field};
                let outline=if focus{accent}else if shelf(card.section){edge}else{base};
                fill(dc,r,field);panel(dc,r,self.px(6),base,outline);let side=self.px(14);symbol(dc,r.left+(r.right-r.left-side)/2,r.top+(r.bottom-r.top-side)/2,side,*glyph,color,base);
            }
            return
        }
        let page=self.page.get();let mut glyph=None;let mut face=field;let mut border=edge;let mut color=muted;let mut rad=7;let mut font=2;
        let caption=match id{
            x if (FILTER..FILTER+6).contains(&x)=>{
                let idx=(x-FILTER)as usize;rad=16;
                if idx==self.filter.get(){face=ink;color=bg;border=ink;}
                if page=="clipboard"{glyph=Some([Symbol::Clipboard,Symbol::Type,Symbol::Link,Symbol::Image][idx.min(3)]);CLIP_FILTERS[idx.min(3)].into()}else{format!("{}  {}",SHELF_FILTERS[idx],self.counts.get()[idx])}
            }
            EXTRA_FILTERS=>{
                let index=self.hidden_filter.get().min(5);let selected=self.filter.get()>=index;
                let inside=if selected{ink}else{field};let color=if selected{bg}else{muted};
                let chip=if hovered{mix(inside,color,18)}else{inside};
                fill(dc,r,bg);let extended=rect(r.left,r.top,self.px(62),r.bottom-r.top);
                panel(dc,extended,self.px(16),chip,if focus{accent}else{edge});
                self.line(dc,inset(extended,0),&format!("{}  {}",SHELF_FILTERS[index],self.counts.get()[index]),2,color,CENTER);
                return
            }
            CLEAR=>{face=bg;glyph=Some(Symbol::Trash);font=0;if self.dp(r.right-r.left)<50{String::new()}else{if page=="clipboard"{"Clear unpinned"}else{"Clear history"}.into()}}
            TOGGLE|CHOOSE|ADD|FILES=>{face=accent;color=if bg&255>150{0xffffff}else{0x171307};border=face;font=1;
                match id{TOGGLE=>if self.form.get(){"Done"}else{"Add app / link"}.into(),CHOOSE=>{glyph=Some(Symbol::FolderOpen);if self.dp(r.right-r.left)>45{"Choose app".into()}else{String::new()}},FILES=>{glyph=Some(Symbol::Plus);if self.dp(r.right-r.left)>45{"Add files".into()}else{String::new()}},_=>{glyph=Some(Symbol::Plus);"Add".into()}}
            }
            FOLDER=>{face=mix(bg,accent,23);color=accent;border=mix(bg,accent,105);glyph=Some(Symbol::Folder);if self.dp(r.right-r.left)>45{"Add folder".into()}else{String::new()}}
            DOWNLOADS=>{face=bg;glyph=Some(Symbol::Download);String::new()}
            CLEAR_QUERY=>{border=field;glyph=Some(Symbol::Close);String::new()}
            KIND=>{glyph=Some(Symbol::ChevronDown);if self.app.get(){"App"}else{"Website"}.into()}
            _=>String::new(),
        };
        if disabled{face=mix(bg,face,115);color=mix(bg,color,115);border=mix(bg,border,115);}else if hovered{face=mix(face,color,18);}
        fill(dc,r,if id==CLEAR_QUERY{field}else{bg});panel(dc,r,self.px(rad),face,if focus{accent}else{border});
        self.caption(dc,r,&caption,glyph,font,color,face);
    }
}

fn lines(dc:HDC,s:&str,font:HFONT,max_width:i32,max:usize)->Vec<String>{
    // DrawTextW does not wrap long URLs. Break at CSS word boundaries first,
    // splitting overlong tokens at character boundaries only when necessary.
    let mut result=Vec::new();
    for paragraph in s.lines(){
        let mut line=String::new();
        for word in paragraph.split_whitespace(){
            let candidate=if line.is_empty(){word.to_owned()}else{format!("{line} {word}")};
            if chrome::width(dc,&candidate,font)<=max_width{line=candidate;continue}
            if !line.is_empty(){result.push(std::mem::take(&mut line));if result.len()>=max{return result}}
            for c in word.chars(){let prev=line.len();line.push(c);
                if prev>0&&chrome::width(dc,&line,font)>max_width{
                    line.truncate(prev);result.push(std::mem::take(&mut line));if result.len()>=max{return result}line.push(c);
                }
            }
        }
        result.push(line);if result.len()>=max{break}
    }
    result
}

pub(super) unsafe fn print_children(h:HWND,dc:HDC){
    let top=GetTopWindow(h);if top.is_null(){return}
    let mut child=GetWindow(top,GW_HWNDLAST);
    while !child.is_null(){
        if on(child){
            let mut r=RECT::default();GetWindowRect(child,&mut r);let mut origin=POINT{x:r.left,y:r.top};ScreenToClient(h,&mut origin);
            let save=SaveDC(dc);IntersectClipRect(dc,origin.x,origin.y,origin.x+r.right-r.left,origin.y+r.bottom-r.top);
            OffsetViewportOrgEx(dc,origin.x,origin.y,null_mut());SendMessageW(child,WM_PRINT,dc as _,(PRF_CLIENT|PRF_NONCLIENT|PRF_CHILDREN)as _);RestoreDC(dc,save);
        }
        child=GetWindow(child,GW_HWNDPREV);
    }
}
