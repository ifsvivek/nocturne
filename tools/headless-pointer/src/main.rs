//! headless-pointer wheel <dx> <dy> | drag <x1> <y1> <x2> <y2> — scroll (discrete steps of 15 units plus continuous axis) at the current position, or press at x1,y1, move in steps and release at x2,y2.
//! headless-pointer <x> <y> [click] — move a virtual pointer to x,y (pixels of the headless output) and optionally left-click.
//! Test tool for the invisible headless sway session only: it refuses to run unless `scripts/headless-session.sh run` set NOCTURNE_HEADLESS=1, so it can never move the pointer on the user's real desktop.
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_pointer::{Axis, AxisSource, ButtonState}, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

struct S;
impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for S {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<wl_seat::WlSeat, ()> for S {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ZwlrVirtualPointerManagerV1, ()> for S {
    fn event(_: &mut Self, _: &ZwlrVirtualPointerManagerV1, _: <ZwlrVirtualPointerManagerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<ZwlrVirtualPointerV1, ()> for S {
    fn event(_: &mut Self, _: &ZwlrVirtualPointerV1, _: <ZwlrVirtualPointerV1 as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

fn main() {
    if std::env::var("NOCTURNE_HEADLESS").as_deref() != Ok("1") {
        eprintln!("refusing: run me through scripts/headless-session.sh pointer (never on the real desktop)");
        std::process::exit(2);
    }
    let a: Vec<String> = std::env::args().collect();
    let mode = a.get(1).map(String::as_str).unwrap_or("");
    let wheel = mode == "wheel";
    let drag = mode == "drag";
    if (wheel && a.len() < 4) || (drag && a.len() < 6) || (!wheel && !drag && a.len() < 3) {
        eprintln!("usage: headless-pointer <x> <y> [click] | wheel <dx> <dy> | drag <x1> <y1> <x2> <y2>");
        std::process::exit(2);
    }
    let (x, y): (u32, u32) = if wheel { (0, 0) } else if drag { (a[2].parse().expect("x1"), a[3].parse().expect("y1")) } else { (a[1].parse().expect("x"), a[2].parse().expect("y")) };
    // The output size of the headless session (set by headless-session.sh from its start size).
    let ext = std::env::var("NOCTURNE_HEADLESS_SIZE").unwrap_or_else(|_| "1920x1080".into());
    let (w, h) = ext.split_once('x').map(|(w, h)| (w.parse().expect("w"), h.parse().expect("h"))).expect("WxH");
    let conn = Connection::connect_to_env().expect("wayland");
    let (g, mut q) = registry_queue_init::<S>(&conn).expect("registry");
    let qh = q.handle();
    let seat: wl_seat::WlSeat = g.bind(&qh, 1..=1, ()).expect("seat");
    let m: ZwlrVirtualPointerManagerV1 = g.bind(&qh, 1..=1, ()).expect("virtual pointer protocol");
    let p = m.create_virtual_pointer(Some(&seat), &qh, ());
    let wait = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
    if wheel {
        // Positive dy scrolls down. Each 15 units is one wheel notch (axis_discrete 1) plus the continuous value, like a real wheel.
        let (dx, dy): (f64, f64) = (a[2].parse().expect("dx"), a[3].parse().expect("dy"));
        for (axis, v) in [(Axis::HorizontalScroll, dx), (Axis::VerticalScroll, dy)] {
            if v == 0.0 { continue; }
            let notches = (v.abs() / 15.0).ceil().max(1.0) as i32;
            for i in 0..notches {
                p.axis_source(AxisSource::Wheel);
                p.axis_discrete(2000 + i as u32 * 20, axis, 15.0 * v.signum(), v.signum() as i32);
                p.frame();
                q.roundtrip(&mut S).expect("roundtrip");
                wait(20);
            }
        }
        wait(300);
        return;
    }
    p.motion_absolute(1000, x, y, w, h);
    p.frame();
    q.roundtrip(&mut S).expect("roundtrip");
    wait(300);
    if drag {
        let (x2, y2): (u32, u32) = (a[4].parse().expect("x2"), a[5].parse().expect("y2"));
        p.button(1200, 0x110, ButtonState::Pressed);
        p.frame();
        q.roundtrip(&mut S).expect("roundtrip");
        wait(100);
        for i in 1..=20u32 {
            let (cx, cy) = ((x as i64 + (x2 as i64 - x as i64) * i as i64 / 20) as u32, (y as i64 + (y2 as i64 - y as i64) * i as i64 / 20) as u32);
            p.motion_absolute(1300 + i * 16, cx, cy, w, h);
            p.frame();
            q.roundtrip(&mut S).expect("roundtrip");
            wait(16);
        }
        wait(100);
        p.button(1700, 0x110, ButtonState::Released);
        p.frame();
        q.roundtrip(&mut S).expect("roundtrip");
        wait(200);
        return;
    }
    // A second, one-pixel move so hover state settles in clients that only react to motion.
    p.motion_absolute(1050, x + 1, y, w, h);
    p.frame();
    q.roundtrip(&mut S).expect("roundtrip");
    if a.get(3).map(String::as_str) == Some("click") {
        wait(150);
        p.button(1200, 0x110, ButtonState::Pressed);
        p.frame();
        q.roundtrip(&mut S).expect("roundtrip");
        wait(80);
        p.button(1300, 0x110, ButtonState::Released);
        p.frame();
    }
    q.roundtrip(&mut S).expect("roundtrip");
    wait(300);
}
