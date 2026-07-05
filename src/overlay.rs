use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    reexports::client::{
        globals::registry_queue_init,
        protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
        Connection, QueueHandle,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::{
    os::fd::{AsRawFd, OwnedFd},
    sync::mpsc,
    thread::{self, JoinHandle},
};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub enum Cmd {
    Start,
    Move(i32, i32),
    Finish,
    Quit,
}

pub struct Overlay {
    tx: mpsc::Sender<Cmd>,
    handle: Option<JoinHandle<()>>,
    wakeup: OwnedFd,
}

impl Overlay {
    pub fn spawn() -> Result<Self> {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (read_fd, write_fd) =
            nix::unistd::pipe().context("overlay wakeup pipe")?;
        let read_owned = read_fd;
        let write_owned = write_fd;
        let handle = thread::Builder::new()
            .name("arcglyph-overlay".into())
            .spawn(move || {
                if let Err(e) = run(rx, read_owned) {
                    eprintln!("overlay thread error: {:#}", e);
                }
            })?;
        Ok(Overlay {
            tx,
            handle: Some(handle),
            wakeup: write_owned,
        })
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
        let _ = nix::unistd::write(&self.wakeup, &[1u8]);
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        let _ = nix::unistd::write(&self.wakeup, &[1u8]);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn run(rx: mpsc::Receiver<Cmd>, wakeup: OwnedFd) -> Result<()> {
    let conn = Connection::connect_to_env().context("connect wayland")?;
    let (globals, mut event_queue) = registry_queue_init::<State>(&conn)?;
    let qh: QueueHandle<State> = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).context("compositor")?;
    let layer_shell = LayerShell::bind(&globals, &qh).context("wlr-layer-shell")?;
    let shm = Shm::bind(&globals, &qh).context("wl_shm")?;
    let seat_state = SeatState::new(&globals, &qh);

    let mut state = State {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state,
        pointer: None,
        shm,
        compositor,
        layer_shell,
        qh: qh.clone(),
        pool: None,
        surface: None,
        width: 0,
        height: 0,
        configured: false,
        active: false,
        points: Vec::new(),
        dirty: false,
    };

    let fd = conn.backend().poll_fd().as_raw_fd();
    let wakeup_fd = wakeup.as_raw_fd();

    loop {
        event_queue.flush().ok();
        let read = event_queue.prepare_read();

        let mut fds = [
            nix::poll::PollFd::new(
                unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) },
                nix::poll::PollFlags::POLLIN,
            ),
            nix::poll::PollFd::new(
                unsafe { std::os::fd::BorrowedFd::borrow_raw(wakeup_fd) },
                nix::poll::PollFlags::POLLIN,
            ),
        ];

        if read.is_some() {
            nix::poll::poll(&mut fds, nix::poll::PollTimeout::NONE).ok();
        }

        if let Some(guard) = read {
            if fds[0]
                .revents()
                .unwrap_or(nix::poll::PollFlags::empty())
                .contains(nix::poll::PollFlags::POLLIN)
            {
                let _ = guard.read();
            }
        }
        event_queue.dispatch_pending(&mut state)?;

        if fds[1]
            .revents()
            .unwrap_or(nix::poll::PollFlags::empty())
            .contains(nix::poll::PollFlags::POLLIN)
        {
            let mut buf = [0u8; 64];
            let _ = nix::unistd::read(wakeup_fd, &mut buf);
        }

        let mut quit = false;
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                Cmd::Start => state.start(),
                Cmd::Move(dx, dy) => state.push(dx, dy),
                Cmd::Finish => state.finish(),
                Cmd::Quit => {
                    quit = true;
                    break;
                }
            }
        }
        if quit {
            break;
        }

        if state.dirty {
            state.redraw()?;
        }
    }
    Ok(())
}

struct State {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    pointer: Option<wl_pointer::WlPointer>,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    qh: QueueHandle<State>,
    pool: Option<SlotPool>,
    surface: Option<LayerSurface>,
    width: u32,
    height: u32,
    configured: bool,
    active: bool,
    points: Vec<(f32, f32)>,
    dirty: bool,
}

impl State {
    fn ensure_surface(&mut self) {
        if self.surface.is_some() {
            return;
        }
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(
            &self.qh,
            surface,
            Layer::Overlay,
            Some("arcglyph-overlay"),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(region) = Region::new(&self.compositor) {
            layer.wl_surface().set_input_region(Some(region.wl_region()));
        }
        layer.commit();
        self.surface = Some(layer);
        self.configured = false;
    }

    fn destroy_surface(&mut self) {
        self.surface = None;
        self.pool = None;
        self.configured = false;
        self.width = 0;
        self.height = 0;
    }

    fn start(&mut self) {
        self.points.clear();
        self.active = true;
        self.ensure_surface();
        if let Some(surface) = self.surface.as_ref() {
            // absorb pointer input again (unset input region -> infinite)
            surface.wl_surface().set_input_region(None);
            surface.wl_surface().commit();
        }
        self.dirty = true;
    }

    fn push(&mut self, _dx: i32, _dy: i32) {}

    fn finish(&mut self) {
        self.active = false;
        self.points.clear();
        if let Some(surface) = self.surface.as_ref() {
            if let Ok(region) = Region::new(&self.compositor) {
                surface
                    .wl_surface()
                    .set_input_region(Some(region.wl_region()));
            }
        }
        // paint a clear frame so nothing is visible; surface stays mapped so
        // KWin doesn't play a close animation.
        self.dirty = true;
        let _ = self.redraw();
    }

    fn redraw(&mut self) -> Result<()> {
        self.dirty = false;
        if !self.configured || self.width == 0 || self.height == 0 {
            eprintln!(
                "overlay: skip redraw (configured={}, {}x{})",
                self.configured, self.width, self.height
            );
            return Ok(());
        }
        eprintln!(
            "overlay: redraw {}x{} points={} active={}",
            self.width,
            self.height,
            self.points.len(),
            self.active
        );
        let surface = match self.surface.as_ref() {
            Some(s) => s,
            None => return Ok(()),
        };
        let w = self.width;
        let h = self.height;
        let stride = w as i32 * 4;

        if self.pool.is_none() {
            self.pool = Some(
                SlotPool::new((w * h * 4) as usize, &self.shm)
                    .context("create shm pool")?,
            );
        }
        let pool = self.pool.as_mut().unwrap();
        let (buffer, canvas) =
            match pool.create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("overlay: create_buffer failed: {:?}", e);
                    return Ok(());
                }
            };

        // clear to transparent
        for b in canvas.iter_mut() {
            *b = 0;
        }

        if self.active && !self.points.is_empty() {
            let mut pixmap = match Pixmap::new(w, h) {
                Some(p) => p,
                None => return Ok(()),
            };

            let path = build_smooth_path(&self.points);
            if let Some(path) = path {
                // outer white halo (two additive passes)
                for (thickness, alpha) in [(22.0f32, 0.10f32), (13.0, 0.22)] {
                    let mut paint = Paint::default();
                    paint.set_color(Color::from_rgba(1.0, 1.0, 1.0, alpha).unwrap());
                    paint.anti_alias = true;
                    paint.blend_mode = tiny_skia::BlendMode::Plus;
                    let mut stroke = Stroke::default();
                    stroke.width = thickness;
                    stroke.line_cap = tiny_skia::LineCap::Round;
                    stroke.line_join = tiny_skia::LineJoin::Round;
                    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
                // crisp blue core
                let mut paint = Paint::default();
                paint.set_color(Color::from_rgba8(0x0a, 0x84, 0xff, 0xff));
                paint.anti_alias = true;
                let mut stroke = Stroke::default();
                stroke.width = 5.0;
                stroke.line_cap = tiny_skia::LineCap::Round;
                stroke.line_join = tiny_skia::LineJoin::Round;
                pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
            }

            // head cap: white dot rimmed in blue
            if let Some(&(hx, hy)) = self.points.last() {
                let mut pb = PathBuilder::new();
                pb.push_circle(hx, hy, 7.5);
                if let Some(dot) = pb.finish() {
                    let mut fill = Paint::default();
                    fill.set_color(Color::from_rgba(1.0, 1.0, 1.0, 0.98).unwrap());
                    fill.anti_alias = true;
                    pixmap.fill_path(
                        &dot,
                        &fill,
                        tiny_skia::FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                    let mut rim = Paint::default();
                    rim.set_color(Color::from_rgba8(0x0a, 0x84, 0xff, 0xff));
                    rim.anti_alias = true;
                    let mut rim_stroke = Stroke::default();
                    rim_stroke.width = 2.0;
                    pixmap.stroke_path(&dot, &rim, &rim_stroke, Transform::identity(), None);
                }
            }

            let src = pixmap.data();
            // tiny-skia RGBA (non-premultiplied for API, but pixmap stores premultiplied)
            // wl_shm ARGB8888 little-endian in memory is B, G, R, A.
            for (dst, s) in canvas.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                dst[0] = s[2];
                dst[1] = s[1];
                dst[2] = s[0];
                dst[3] = s[3];
            }
        }

        let wl_surface = surface.wl_surface();
        if let Err(e) = buffer.attach_to(wl_surface) {
            eprintln!("overlay: attach_to failed: {:?}", e);
            return Ok(());
        }
        wl_surface.damage_buffer(0, 0, w as i32, h as i32);
        wl_surface.commit();
        Ok(())
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.destroy_surface();
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        eprintln!("overlay: configure {}x{}", w, h);
        if w != 0 && h != 0 {
            if w != self.width || h != self.height {
                self.pool = None;
            }
            self.width = w;
            self.height = h;
        } else if self.width == 0 {
            self.width = 1920;
            self.height = 1080;
        }
        self.configured = true;
        self.dirty = true;
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(State);
delegate_output!(State);
delegate_layer!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_pointer!(State);
delegate_registry!(State);

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Pointer && self.pointer.is_none() {
            if let Ok(p) = self.seat_state.get_pointer(qh, &seat) {
                self.pointer = Some(p);
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let target = match self.surface.as_ref() {
            Some(s) => s.wl_surface().clone(),
            None => return,
        };
        for ev in events {
            if ev.surface != target {
                continue;
            }
            match ev.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    if self.active {
                        let (x, y) = ev.position;
                        self.points.push((x as f32, y as f32));
                        self.dirty = true;
                    }
                }
                _ => {}
            }
        }
    }
}

fn build_smooth_path(points: &[(f32, f32)]) -> Option<tiny_skia::Path> {
    if points.is_empty() {
        return None;
    }
    if points.len() == 1 {
        let p = points[0];
        let mut pb = PathBuilder::new();
        pb.move_to(p.0, p.1);
        pb.line_to(p.0 + 0.01, p.1);
        return pb.finish();
    }
    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);
    if points.len() == 2 {
        pb.line_to(points[1].0, points[1].1);
        return pb.finish();
    }
    // For each middle point pi, draw quadratic curve using pi as control and
    // the midpoint(pi, pi+1) as endpoint. Ends with a line to the last point.
    for i in 1..points.len() - 1 {
        let p = points[i];
        let n = points[i + 1];
        let mx = (p.0 + n.0) * 0.5;
        let my = (p.1 + n.1) * 0.5;
        pb.quad_to(p.0, p.1, mx, my);
    }
    let last = points[points.len() - 1];
    pb.line_to(last.0, last.1);
    pb.finish()
}
