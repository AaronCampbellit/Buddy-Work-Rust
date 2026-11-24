//! Geometry, animation and hit testing for the top-edge island.
//! RECTs in a Frame are physical pixels relative to the stable popup. Only
//! Layout::host is in virtual-desktop screen coordinates.
use std::cell::RefCell;
use std::time::Instant;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, SetWindowRgn, HRGN,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Widget { Camera, Focus, Stopwatch, Note, Clipboard }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement { Left, Center, Right }

#[derive(Clone)]
pub struct Frame {
    pub body: RECT,
    pub widgets: Vec<(Widget, RECT)>,
    pub widget_view: RECT,
    pub progress: f32,
    pub body_alpha: f32,
    pub header_progress: f32,
    pub widgets_alpha: u8,
    pub expanded: bool,
    pub animating: bool,
    pub radius: i32,
    pub expanded_body: RECT,
}

impl Frame {
    pub fn visible_workspace(&self,scale:f32)->RECT{
        let pad=(6.0*scale).ceil() as i32;
        RECT{left:self.body.left+pad,right:self.body.right-pad,top:(46.0*scale).round() as i32,
            bottom:self.body.bottom-(10.0*scale).round() as i32}
    }
    pub fn workspace_offset(&self)->i32{
        (self.body.left+self.body.right-self.expanded_body.left-self.expanded_body.right)/2
    }
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> RECT {
    RECT {left:x, top:y, right:x+w, bottom:y+h}
}

fn anchor(total: i32, width: i32, placement: Placement) -> i32 {
    match placement {
        Placement::Left => 0,
        Placement::Center => (total-width)/2,
        Placement::Right => total-width,
    }
}

#[derive(Clone)]
pub struct Layout {
    pub host: RECT,
    pub scale: f32,
    pub limit: i32,
    closed: RECT,
    open: RECT,
    widget_view: RECT,
    widgets: Vec<(Widget,RECT)>,
    closed_radius: i32,
    open_radius: i32,
}

impl Layout {
    pub fn new(work:RECT, dpi:u32, zoom:i32, placement:Placement, height:i32, widgets:&[Widget])->Self {
        let device = dpi.max(48) as f32 / 96.0;
        let scale = device * zoom.clamp(75,175) as f32 / 100.0;
        let px = |dip:i32| ((dip as f32 * scale).round() as i32).max(1);
        let width=(work.right-work.left).max(1);
        let limit=((work.bottom-work.top).max(1)*9/10).max(1);
        let margin=px(12).min(width/20);
        let available=(width-margin*2).max(1);
        let gap=px(8);
        let base=px(346).min(available);
        let small_height=px(40).min(limit);
        let large_width=px(840).min(available);
        let large_height=px(height.max(300)).min(limit);
        let card_width=|kind:Widget|px(if kind==Widget::Camera{292}else{400});
        let natural_width=widgets.iter().fold(base,|w,&kind|w+gap+card_width(kind));
        let train_width=natural_width.min(available);
        let small_x=anchor(available,train_width,placement);
        let large_x=anchor(available,large_width,placement);
        // Keep the parent fixed across open/close. The separately owned card
        // strip can extend past it without allocating a monitor-sized bitmap.
        let host_left=small_x.min(large_x);
        let host_right=(small_x+base).max(large_x+large_width);
        let host=rect(work.left+margin+host_left,work.top,host_right-host_left,large_height.max(small_height));
        let closed=rect(small_x-host_left,0,base,small_height);
        let open=rect(large_x-host_left,0,large_width,large_height);
        let mut natural_x=closed.right+gap;
        let mut cards=Vec::with_capacity(widgets.len());
        for &kind in widgets {
            let w=card_width(kind);
            let h=if kind==Widget::Camera{small_height}else{px(400).min(limit)};
            cards.push((kind,rect(natural_x,0,w,h)));
            natural_x+=w+gap;
        }
        let visible_width=(train_width-base-gap).max(0);
        let card_height=cards.iter().map(|(_,r)|r.bottom).max().unwrap_or(0);
        let widget_view=rect(closed.right+gap,0,visible_width,card_height);
        Self {host,scale,limit,closed,open,widget_view,widgets:cards,
              closed_radius:(20.0*device).round() as i32,open_radius:(26.0*device).round() as i32}
    }

    fn sample(&self,p:f32,alpha:f32,expanded:bool,animating:bool)->Frame {
        let interp=|a:i32,b:i32|(a as f32+(b-a) as f32*p).round() as i32;
        let body=RECT {left:interp(self.closed.left,self.open.left),top:0,
                       right:interp(self.closed.right,self.open.right),bottom:interp(self.closed.bottom,self.open.bottom)};
        Frame {body,expanded_body:self.open,progress:p,body_alpha:alpha,header_progress:p,widgets_alpha:255,expanded,animating,
               radius:interp(self.closed_radius,self.open_radius),
               widgets:if !expanded && !animating{self.widgets.clone()}else{vec![]},widget_view:self.widget_view}
    }
}

fn bezier(t:f32, x1:f32,y1:f32,x2:f32,y2:f32)->f32 {
    if t<=0.0{return 0.0}
    if t>=1.0{return 1.0}
    let sample=|k:f32,a:f32,b:f32| {
        let inv=1.0-k;
        3.0*inv*inv*k*a+3.0*inv*k*k*b+k*k*k
    };
    let (mut low,mut high)=(0.0,1.0);
    for _ in 0..18 {
        let mid=(low+high)*0.5;
        if sample(mid,x1,x2)<t{low=mid}else{high=mid}
    }
    sample((low+high)*0.5,y1,y2)
}

#[derive(Clone,Copy)]
struct Motion {
    from:f32,
    alpha_from:f32,
    header_from:f32,
    target:f32,
    duration:f32,
    since:Instant,
}

struct Model {
    layout:Layout,
    p:f32,
    alpha:f32,
    header:f32,
    reveal_since:Option<Instant>,
    widgets_alpha:u8,
    expanded:bool,
    motion:Option<Motion>,
}

impl Model {
    fn new(layout:Layout, expanded:bool)->Self {
        let endpoint=if expanded{1.0}else{0.0};
        Self{layout,p:endpoint,alpha:endpoint,header:endpoint,reveal_since:None,widgets_alpha:255,expanded,motion:None}
    }
    fn sample(&mut self, now:Instant) {
        if let Some(m)=self.motion {
            let ms=now.saturating_duration_since(m.since).as_secs_f32()*1000.0;
            let shape=bezier(ms/m.duration,0.2,0.7,0.2,1.0);
            self.p=m.from+(m.target-m.from)*shape;
            // CSS transitions shorten reversals from the current displayed value.
            // The persistent header and body opacity have their own easing clocks.
            let css=|from:f32,duration:f32,delay:f32|{
                let distance=(m.target-from).abs();
                if distance==0.0 || ms-delay>=duration*distance{return m.target}
                from+(m.target-from)*bezier((ms-delay)/(duration*distance),0.25,0.1,0.25,1.0)
            };
            self.header=css(m.header_from,180.0,0.0);
            self.alpha=if self.expanded{css(m.alpha_from,120.0,40.0)}else{css(m.alpha_from,100.0,0.0)};
            if ms>=m.duration && self.header==m.target && self.alpha==m.target {
                self.p=m.target;
                self.motion=None;
                if !self.expanded && !self.layout.widgets.is_empty(){
                    self.reveal_since=Some(now);
                }else{self.widgets_alpha=255;}
            }
        }
        if let Some(since)=self.reveal_since {
            let t=now.saturating_duration_since(since).as_secs_f32()/0.100;
            self.widgets_alpha=(255.0*bezier(t,0.25,0.1,0.25,1.0)).round() as u8;
            if t>=1.0{self.reveal_since=None;}
        }
    }
    fn request(&mut self,open:bool,immediate:bool,now:Instant) {
        // Reverse from the last presented frame. No screen update occurs
        // between the previous timer tick and this input event.
        let target=if open{1.0}else{0.0};
        if immediate {
            self.motion=None;self.p=target;self.alpha=target;self.header=target;self.expanded=open;
            self.reveal_since=None;self.widgets_alpha=255;return
        }
        if self.expanded==open{return}
        let duration=((if open{220.0}else{180.0})*f32::abs(target-self.p)).max(60.0);
        self.motion=Some(Motion{from:self.p,alpha_from:self.alpha,header_from:self.header,target,duration,since:now});
        self.reveal_since=None;self.widgets_alpha=0;
        self.expanded=open;
    }
    fn frame(&self)->Frame{
        let mut frame=self.layout.sample(self.p,self.alpha,self.expanded,self.motion.is_some());
        frame.header_progress=self.header;
        frame.widgets_alpha=self.widgets_alpha;
        frame
    }
    fn needs_tick(&self)->bool{self.motion.is_some() || self.reveal_since.is_some()}
}

thread_local! {
    static MODEL:RefCell<Option<Model>>=const{RefCell::new(None)};
}

pub fn configure(layout:Layout,expanded:bool){
    MODEL.with(|s|match &mut *s.borrow_mut(){
        Some(m)=>m.layout=layout,
        slot@None=>*slot=Some(Model::new(layout,expanded))
    });
}
pub fn request(open:bool,immediate:bool){
    MODEL.with(|s|if let Some(m)=s.borrow_mut().as_mut(){m.request(open,immediate,Instant::now());});
}
pub fn layout_ready(){
    MODEL.with(|s|if let Some(motion)=s.borrow_mut().as_mut().and_then(|m|m.motion.as_mut()){
        motion.since=Instant::now();
    });
}
pub fn cards_presented(){
    MODEL.with(|s|if let Some(m)=s.borrow_mut().as_mut(){
        if m.reveal_since.is_some() && m.widgets_alpha==0{m.reveal_since=Some(Instant::now());}
    });
}
pub fn sample(){
    MODEL.with(|s|if let Some(m)=s.borrow_mut().as_mut(){m.sample(Instant::now());});
}
#[cfg(test)]
pub fn sample_after(elapsed:std::time::Duration){
    MODEL.with(|s|if let Some(model)=s.borrow_mut().as_mut(){
        let motion=model.motion.expect("motion capture needs an active transition");
        model.sample(motion.since+elapsed);
    });
}
pub fn needs_tick()->bool{
    MODEL.with(|s|s.borrow().as_ref().is_some_and(Model::needs_tick))
}
pub fn frame()->Frame{
    MODEL.with(|s|s.borrow().as_ref().map(Model::frame).unwrap_or_else(||{
        Layout::new(rect(0,0,1024,768),96,100,Placement::Center,300,&[]).sample(0.0,0.0,false,false)
    }))
}
pub fn scale_and_limit()->(f32,i32){
    MODEL.with(|s|s.borrow().as_ref().map(|m|(m.layout.scale,m.layout.limit)).unwrap_or((1.0,720)))
}
pub fn same_rect(a:RECT,b:RECT)->bool{
    a.left==b.left && a.top==b.top && a.right==b.right && a.bottom==b.bottom
}

fn region(frame:&Frame)->HRGN {
    let r=frame.body;
    let rad=frame.radius.min((r.right-r.left)/2).min(r.bottom-r.top).max(0);
    unsafe {
        // Extending the round rectangle above the surface before clipping puts
        // the square top edge directly against the display edge.
        let mask=CreateRectRgn(r.left,r.top,r.right,r.bottom);
        if mask.is_null() || rad==0{return mask}
        let rounded=CreateRoundRectRgn(r.left,r.top-rad,r.right+1,r.bottom+1,rad*2,rad*2);
        if rounded.is_null(){return mask}
        CombineRgn(mask,mask,rounded,windows_sys::Win32::Graphics::Gdi::RGN_AND);
        DeleteObject(rounded);
        mask
    }
}

pub fn apply_region(hwnd:HWND,frame:&Frame){
    unsafe {
        let mask=region(frame);
        if !mask.is_null() && SetWindowRgn(hwnd,mask,0)==0{DeleteObject(mask);}
    }
}
pub fn contains(frame:&Frame,x:i32,y:i32)->bool{
    let r=frame.body;
    if x<r.left || x>=r.right || y<r.top || y>=r.bottom{return false}
    let rad=frame.radius.min((r.right-r.left)/2).min(r.bottom-r.top).max(0);
    if y<r.bottom-rad || (x>=r.left+rad && x<r.right-rad){return true}
    let cx=if x<r.left+rad{r.left+rad}else{r.right-rad};
    let cy=r.bottom-rad;
    let dx=x as f32+0.5-cx as f32;
    let dy=y as f32+0.5-cy as f32;
    dx*dx+dy*dy<=(rad*rad) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr::{null,null_mut};
    use std::time::Duration;
    use windows_sys::Win32::Graphics::Gdi::{GetWindowRgn,PtInRegion};
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW,DestroyWindow,WS_POPUP};

    #[test]
    fn shared_host_preserves_all_three_original_anchors_and_pixel_sizes() {
        let work=rect(-1920,48,1920,1032);
        for (placement,small,large) in [(Placement::Left,-1908,-1908),(Placement::Center,-1133,-1380),(Placement::Right,-358,-852)] {
            let layout=Layout::new(work,96,100,placement,300,&[]);
            let closed=layout.sample(0.,0.,false,false);
            let open=layout.sample(1.,1.,true,false);
            assert_eq!(layout.host.left+closed.body.left,small);
            assert_eq!(layout.host.left+open.body.left,large);
            assert_eq!(layout.host.top,48);
            assert_eq!((closed.body.right-closed.body.left,closed.body.bottom),(346,40));
            assert_eq!((open.body.right-open.body.left,open.body.bottom),(840,300));
            assert_eq!((closed.radius,open.radius),(20,26));
        }
        let high=Layout::new(rect(2200,12,1800,1200),144,125,Placement::Right,420,&[]);
        assert_eq!(high.open.right-high.open.left,1575);
        assert_eq!(high.open.bottom,788);
        assert_eq!(high.host.right,4000-23);
        let tiny=Layout::new(rect(0,0,500,300),192,175,Placement::Center,2000,&[]);
        assert!(tiny.open.right-tiny.open.left<=500);
        assert_eq!(tiny.open.bottom,270);
    }

    #[test]
    fn side_card_viewport_clips_the_train_without_shrinking_the_individual_cards() {
        let kinds=[Widget::Camera,Widget::Focus,Widget::Stopwatch,Widget::Note,Widget::Clipboard];
        let layout=Layout::new(rect(0,0,1024,768),96,100,Placement::Center,300,&kinds);
        let f=layout.sample(0.,0.,false,false);
        assert_eq!(f.widgets.len(),5);
        assert_eq!(f.widgets[0].1.left-f.body.right,8);
        assert_eq!((f.widgets[0].1.right-f.widgets[0].1.left,f.widgets[0].1.bottom),(292,40));
        assert_eq!(f.widgets[1].1.left-f.widgets[0].1.right,8);
        assert_eq!((f.widgets[1].1.right-f.widgets[1].1.left,f.widgets[1].1.bottom),(400,400));
        assert_eq!(layout.host.left+f.widget_view.right,1012);
        assert!(f.widgets.last().unwrap().1.right>f.widget_view.right);
        assert!(layout.sample(0.5,0.2,true,true).widgets.is_empty());
        assert!(layout.sample(1.,1.,true,false).widgets.is_empty());
    }

    #[test]
    fn geometry_and_content_fade_reverse_from_the_displayed_frame() {
        let start=Instant::now();
        let layout=Layout::new(rect(0,0,1920,1080),96,100,Placement::Center,300,&[]);
        let mut model=Model::new(layout,false);
        model.request(true,false,start);
        model.sample(start+Duration::from_millis(30));
        assert!(model.p>0.20 && model.p<0.6);
        assert_eq!(model.alpha,0.);
        model.sample(start+Duration::from_millis(80));
        let p=model.p;
        let alpha=model.alpha;
        let before=model.frame().body;
        model.request(false,false,start+Duration::from_millis(80));
        assert!(same_rect(before,model.frame().body));
        assert_eq!(model.alpha,alpha);
        assert!((model.motion.unwrap().duration-(180.0*p).max(60.)).abs()<0.05);
        model.sample(start+Duration::from_millis(115));
        assert!(model.p<p && model.alpha<alpha);
        model.request(true,false,start+Duration::from_millis(115));
        model.sample(start+Duration::from_millis(400));
        assert_eq!((model.p,model.alpha,model.motion.is_none()),(1.0,1.0,true));
        model.request(false,true,start+Duration::from_millis(401));
        assert_eq!((model.p,model.alpha,model.motion.is_none()),(0.0,0.0,true));
    }

    #[test]
    fn header_and_cards_have_independent_clocks_and_reduced_motion_settles_them() {
        let start=Instant::now();
        let at=|ms|start+Duration::from_millis(ms);
        let layout=Layout::new(rect(0,0,1920,1080),96,100,Placement::Center,300,&[Widget::Note]);
        let mut model=Model::new(layout,false);
        model.request(true,false,start);
        model.sample(at(35));
        assert!(model.header>0.0 && model.header<model.p && model.alpha==0.0);
        model.sample(at(220));
        model.request(false,false,at(220));
        model.sample(at(399));
        assert!(model.frame().widgets.is_empty());
        model.sample(at(400));
        assert_eq!((model.frame().widgets.len(),model.frame().widgets_alpha),(1,0));
        model.sample(at(450));
        assert!((202..=207).contains(&model.frame().widgets_alpha));
        model.request(true,false,at(450));
        assert!(model.frame().widgets.is_empty());
        model.request(false,true,at(451));
        assert_eq!((model.frame().widgets.len(),model.frame().widgets_alpha),(1,255));
        assert!(!model.needs_tick());
    }

    #[test]
    fn native_region_leaves_desktop_and_rounded_corners_outside_the_hwnd() {
        let layout=Layout::new(rect(0,0,1920,1080),96,100,Placement::Center,300,&[]);
        let f=layout.sample(0.,0.,false,false);
        unsafe{
            let class:Vec<u16>="STATIC\0".encode_utf16().collect();
            let window=CreateWindowExW(0,class.as_ptr(),null(),WS_POPUP,0,0,840,300,null_mut(),null_mut(),null_mut(),null());
            assert!(!window.is_null());
            apply_region(window,&f);
            let mask=CreateRectRgn(0,0,0,0);
            assert!(GetWindowRgn(window,mask)>0);
            for (x,y,want) in [
                (f.body.left,0,true),(f.body.right-1,0,true),(f.body.left-1,20,false),
                (f.body.left,39,false),(f.body.left+25,39,true),(f.body.right-1,39,false),
                ((f.body.left+f.body.right)/2,100,false),
            ]{
                assert_eq!(PtInRegion(mask,x,y)!=0,want,"real region at {x},{y}");
                assert_eq!(contains(&f,x,y),want,"hover hit at {x},{y}");
            }
            DeleteObject(mask);DestroyWindow(window);
        }
    }
}
