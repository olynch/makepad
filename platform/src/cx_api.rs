use {
    std::{
        any::{TypeId, Any},
        collections::HashSet,
    },
    crate::{
        makepad_math::Vec2,
        gpu_info::GpuInfo,
        cx::{Cx, OsType},
        event::{
            DraggedItem,
            Timer,
            Trigger,
            Signal,
            WebSocketAutoReconnect,
            WebSocket,
            NextFrame,
        },
        draw_list::{
            DrawListId
        },
        window::{
            WindowId
        },
        cursor::{
            MouseCursor
        },
        area::{
            Area,
            DrawListArea
        },
        menu::{
            Menu,
        },
        pass::{
            PassId,
            CxPassParent
        },
    }
};


pub trait CxOsApi {
    fn post_signal(signal: Signal);
    fn spawn_thread<F>(&mut self, f: F) where F: FnOnce() + Send + 'static;
    
    fn web_socket_open(&mut self, url: String, rec: WebSocketAutoReconnect) -> WebSocket;
    fn web_socket_send(&mut self, socket: WebSocket, data: Vec<u8>);
    
    //fn start_midi_input(&mut self);
    //fn spawn_audio_output<F>(&mut self, f: F) where F: FnMut(AudioTime, &mut dyn AudioOutputBuffer) + Send + 'static;
}

#[derive(PartialEq)]
pub enum CxOsOp {
    CreateWindow(WindowId),
    CloseWindow(WindowId),
    MinimizeWindow(WindowId),
    MaximizeWindow(WindowId),
    FullscreenWindow(WindowId),
    NormalizeWindow(WindowId),
    RestoreWindow(WindowId),
    SetTopmost(WindowId, bool),
    XrStartPresenting(WindowId),
    XrStopPresenting(WindowId),
    
    ShowTextIME(Vec2),
    HideTextIME,
    SetCursor(MouseCursor),
    StartTimer {timer_id: u64, interval: f64, repeats: bool},
    StopTimer(u64),
    StartDragging(DraggedItem),
    UpdateMenu(Menu)
}

impl Cx {
    
    pub fn get_dependency(&self, path:&str)->Result<&Vec<u8>, String>{
        if let Some(data) = self.dependencies.get(path){
            if let Some(data) = &data.data{
                return match data{
                    Ok(data)=>Ok(data),
                    Err(s)=>Err(s.clone())
                }
            }
        }
        Err(format!("Dependency not loaded {}", path))
    }
    
    pub fn redraw_id(&self) -> u64 {self.redraw_id}
    
    pub fn platform_type(&self) -> &OsType {&self.platform_type}
    pub fn cpu_cores(&self)->usize{self.cpu_cores}
    pub fn gpu_info(&self) -> &GpuInfo {&self.gpu_info}
    
    pub fn update_menu(&mut self, menu: Menu) {
        self.platform_ops.push(CxOsOp::UpdateMenu(menu));
    }
    
    pub fn push_unique_platform_op(&mut self, op: CxOsOp) {
        if self.platform_ops.iter().find( | o | **o == op).is_none() {
            self.platform_ops.push(op);
        }
    }
    
    pub fn show_text_ime(&mut self, pos: Vec2) {
        self.platform_ops.push(CxOsOp::ShowTextIME(pos));
    }
    
    pub fn hide_text_ime(&mut self) {
        self.platform_ops.push(CxOsOp::HideTextIME);
    }
    
    pub fn start_dragging(&mut self, dragged_item: DraggedItem) {
        self.platform_ops.iter().for_each( | p | {
            if let CxOsOp::StartDragging(_) = p {
                panic!("start drag twice");
            }
        });
        self.platform_ops.push(CxOsOp::StartDragging(dragged_item));
    }
    
    pub fn set_cursor(&mut self, cursor: MouseCursor) {
        // down cursor overrides the hover cursor
        if let Some(p) = self.platform_ops.iter_mut().find( | p | match p {
            CxOsOp::SetCursor(_) => true,
            _ => false
        }) {
            *p = CxOsOp::SetCursor(cursor)
        }
        else {
            self.platform_ops.push(CxOsOp::SetCursor(cursor))
        }
    }
    
    pub fn start_timer(&mut self, interval: f64, repeats: bool) -> Timer {
        self.timer_id += 1;
        self.platform_ops.push(CxOsOp::StartTimer {
            timer_id: self.timer_id,
            interval,
            repeats
        });
        Timer(self.timer_id)
    }
    
    pub fn stop_timer(&mut self, timer: Timer) {
        if timer.0 != 0 {
            self.platform_ops.push(CxOsOp::StopTimer(timer.0));
        }
    }
    
    pub fn get_dpi_factor_of(&mut self, area: &Area) -> f32 {
        match area {
            Area::Instance(ia) => {
                let pass_id = self.draw_lists[ia.draw_list_id].pass_id.unwrap();
                return self.get_delegated_dpi_factor(pass_id)
            },
            Area::DrawList(va) => {
                let pass_id = self.draw_lists[va.draw_list_id].pass_id.unwrap();
                return self.get_delegated_dpi_factor(pass_id)
            },
            _ => ()
        }
        return 1.0;
    }
    
    pub fn get_delegated_dpi_factor(&mut self, pass_id: PassId) -> f32 {
        let mut pass_id_walk = pass_id;
        for _ in 0..25 {
            match self.passes[pass_id_walk].parent {
                CxPassParent::Window(window_id) => {
                    if !self.windows[window_id].is_created {
                        panic!();
                    }
                    return self.windows[window_id].window_geom.dpi_factor;
                },
                CxPassParent::Pass(next_pass_id) => {
                    pass_id_walk = next_pass_id;
                },
                _ => {break;}
            }
        }
        1.0
    }
    
    pub fn redraw_pass_of(&mut self, area: Area) {
        // we walk up the stack of area
        match area {
            Area::Empty => (),
            Area::Instance(instance) => {
                self.redraw_pass_and_parent_passes(self.draw_lists[instance.draw_list_id].pass_id.unwrap());
            },
            Area::DrawList(listarea) => {
                self.redraw_pass_and_parent_passes(self.draw_lists[listarea.draw_list_id].pass_id.unwrap());
            }
        }
    }
    
    pub fn redraw_pass_and_parent_passes(&mut self, pass_id: PassId) {
        let mut walk_pass_id = pass_id;
        loop {
            if let Some(main_view_id) = self.passes[walk_pass_id].main_draw_list_id {
                self.redraw_area_and_children(Area::DrawList(DrawListArea {redraw_id: 0, draw_list_id: main_view_id}));
            }
            match self.passes[walk_pass_id].parent.clone() {
                CxPassParent::Pass(next_pass_id) => {
                    walk_pass_id = next_pass_id;
                },
                _ => {
                    break;
                }
            }
        }
    }
    
    pub fn repaint_pass(&mut self, pass_id: PassId) {
        let cxpass = &mut self.passes[pass_id];
        cxpass.paint_dirty = true;
    }
    
    pub fn redraw_pass_and_child_passes(&mut self, pass_id: PassId) {
        let cxpass = &self.passes[pass_id];
        if let Some(main_list) = cxpass.main_draw_list_id {
            self.redraw_area_and_children(Area::DrawList(DrawListArea {redraw_id: 0, draw_list_id: main_list}));
        }
        // lets redraw all subpasses as well
        for sub_pass_id in self.passes.id_iter() {
            if let CxPassParent::Pass(dep_pass_id) = self.passes[sub_pass_id].parent.clone() {
                if dep_pass_id == pass_id {
                    self.redraw_pass_and_child_passes(sub_pass_id);
                }
            }
        }
    }
    
    pub fn redraw_all(&mut self) {
        self.new_draw_event.redraw_all = true;
    }
    
    pub fn redraw_area(&mut self, area: Area) {
        if let Some(draw_list_id) = area.draw_list_id() {
            if self.new_draw_event.draw_lists.iter().position( | v | *v == draw_list_id).is_some() {
                return;
            }
            self.new_draw_event.draw_lists.push(draw_list_id);
        }
    }
    
    pub fn redraw_area_and_children(&mut self, area: Area) {
        if let Some(draw_list_id) = area.draw_list_id() {
            if self.new_draw_event.draw_lists_and_children.iter().position( | v | *v == draw_list_id).is_some() {
                return;
            }
            self.new_draw_event.draw_lists_and_children.push(draw_list_id);
        }
    }
    
    
    pub fn set_scroll_x(&mut self, draw_list_id: DrawListId, scroll_pos: f32) {
        if let Some(pass_id) = self.draw_lists[draw_list_id].pass_id {
            let fac = self.get_delegated_dpi_factor(pass_id);
            let cxview = &mut self.draw_lists[draw_list_id];
            cxview.unsnapped_scroll.x = scroll_pos;
            let snapped = scroll_pos - scroll_pos % (1.0 / fac);
            if cxview.snapped_scroll.x != snapped {
                cxview.snapped_scroll.x = snapped;
                self.passes[cxview.pass_id.unwrap()].paint_dirty = true;
            }
        }
    }
    
    
    pub fn set_scroll_y(&mut self, draw_list_id: DrawListId, scroll_pos: f32) {
        if let Some(pass_id) = self.draw_lists[draw_list_id].pass_id {
            let fac = self.get_delegated_dpi_factor(pass_id);
            let cxview = &mut self.draw_lists[draw_list_id];
            cxview.unsnapped_scroll.y = scroll_pos;
            let snapped = scroll_pos - scroll_pos % (1.0 / fac);
            if cxview.snapped_scroll.y != snapped {
                cxview.snapped_scroll.y = snapped;
                self.passes[cxview.pass_id.unwrap()].paint_dirty = true;
            }
        }
    }
    
    pub fn update_area_refs(&mut self, old_area: Area, new_area: Area) -> Area {
        if old_area == Area::Empty {
            return new_area
        }
        
        self.fingers.update_area(old_area, new_area);
        self.finger_drag.update_area(old_area, new_area);
        self.keyboard.update_area(old_area, new_area);
        
        new_area
    }
    
    pub fn set_key_focus(&mut self, focus_area: Area) {
        self.keyboard.set_key_focus(focus_area);
    }
    
    pub fn revert_key_focus(&mut self) {
        self.keyboard.revert_key_focus();
    }
    
    pub fn has_key_focus(&self, focus_area: Area) -> bool {
        self.keyboard.has_key_focus(focus_area)
    }
    
    pub fn new_next_frame(&mut self) -> NextFrame {
        let res = NextFrame(self.next_frame_id);
        self.next_frame_id += 1;
        self.new_next_frames.insert(res);
        res
    }
    
    pub fn send_signal(&mut self, signal: Signal) {
        self.signals.insert(signal);
    }
    
    pub fn send_trigger(&mut self, area: Area, trigger: Trigger) {
        if let Some(triggers) = self.triggers.get_mut(&area) {
            triggers.insert(trigger);
        }
        else {
            let mut new_set = HashSet::new();
            new_set.insert(trigger);
            self.triggers.insert(area, new_set);
        }
    }
    
    pub fn set_global<T: 'static + Any + Sized>(&mut self, value:T){
        if !self.globals.iter().any(|v| v.0 == TypeId::of::<T>()){
            self.globals.push((TypeId::of::<T>(), Box::new(value)));
        }
    }
    
    pub fn get_global<T: 'static + Any>(&mut self)->&mut T{
        let item = self.globals.iter_mut().find(|v| v.0 == TypeId::of::<T>()).unwrap();
        item.1.downcast_mut().unwrap()
    }
    
        
    pub fn has_global<T: 'static + Any>(&mut self)->bool{
        self.globals.iter_mut().find(|v| v.0 == TypeId::of::<T>()).is_some()
    }
}


#[macro_export]
macro_rules!main_app {
    ( $ app: ident) => {
        #[cfg(not(target_arch = "wasm32"))]
        fn main() {
            let app = std::rc::Rc::new(std::cell::RefCell::new(None));
            let mut cx = Cx::new(Box::new(move | cx, event | {
                
                if let Event::Construct = event {
                    *app.borrow_mut() = Some($app::new_main(cx));
                }
                
                app.borrow_mut().as_mut().unwrap().handle_event(cx, event);
            }));
            live_register(&mut cx);
            cx.live_expand();
            cx.live_scan_dependencies();
            cx.desktop_load_dependencies();
            cx.event_loop();
        }
        
        #[cfg(target_arch = "wasm32")]
        fn main() {}
        
        #[export_name = "wasm_create_app"]
        #[cfg(target_arch = "wasm32")]
        pub extern "C" fn create_wasm_app() -> u32 {
            
            let app = std::rc::Rc::new(std::cell::RefCell::new(None));
            let mut cx = Box::new(Cx::new(Box::new(move | cx, event | {
                if let Event::Construct = event {
                    *app.borrow_mut() = Some($app::new_main(cx));
                }
                app.borrow_mut().as_mut().unwrap().handle_event(cx, event);
            })));
            
            live_register(&mut cx);
            cx.live_expand();
            cx.live_scan_dependencies();
            Box::into_raw(cx) as u32
        }

        #[export_name = "wasm_process_msg"]
        #[cfg(target_arch = "wasm32")]
        pub unsafe extern "C" fn wasm_process_msg(msg_ptr: u32, cx_ptr: u32) -> u32 {
            let cx = cx_ptr as *mut Cx;
            (*cx).process_to_wasm(msg_ptr)
        }
    }
}

#[macro_export]
macro_rules!register_component_factory {
    ( $ cx: ident, $ registry: ident, $ ty: ty, $ factory: ident) => {
        let module_id = LiveModuleId::from_str(&module_path!()).unwrap();
        if let Some((reg, _)) = $ cx.live_registry.borrow().components.get_or_create::< $ registry>().map.get(&LiveType::of::< $ ty>()) {
            if reg.module_id != module_id {
                panic!("Component already registered {} {}", stringify!( $ ty), reg.module_id);
            }
        }
        $ cx.live_registry.borrow().components.get_or_create::< $ registry>().map.insert(
            LiveType::of::< $ ty>(),
            (LiveComponentInfo {
                name: LiveId::from_str(stringify!( $ ty)).unwrap(),
                module_id
            }, Box::new( $ factory()))
        );
    }
}
