//! Persistent virtual keyboard and pointer for a headless wlroots compositor.
//!
//! `wtype`-style tools create a device per invocation with a synthetic keymap,
//! which breaks games that read physical key positions and races the client
//! binding the keyboard. This keeps one keyboard (standard US keymap, real
//! evdev keycodes) and one pointer alive for the whole session and reads
//! commands, one per line, from a FIFO:
//!
//! ```text
//! key <name> down|up     tap <name>        move <dx> <dy> [steps] [ms]
//! button left|right down|up                click left|right
//! sleep <ms>             quit
//! ```
//!
//! Usage: `hl-input <fifo>` with `WAYLAND_DISPLAY` set to the compositor.

use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1, zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ZwpVirtualKeyboardV1);
delegate_noop!(State: ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ZwlrVirtualPointerV1);

/// evdev key codes for the names the harness uses.
fn keycode(name: &str) -> Option<u32> {
    let letters = "qwertyuiop\0\0\0\0asdfghjkl\0\0\0\0\0zxcvbnm";
    if name.len() == 1 {
        let c = name.chars().next()?;
        if let Some(i) = letters.find(c) {
            // q=16 .. p=25, a=30 .. l=38, z=44 .. m=50.
            return Some(16 + i as u32);
        }
        if let Some(d) = c.to_digit(10) {
            return Some(if d == 0 { 11 } else { 1 + d });
        }
    }
    Some(match name {
        "esc" | "escape" => 1,
        "tab" => 15,
        "enter" | "return" => 28,
        "lctrl" | "ctrl" => 29,
        "lshift" | "shift" => 42,
        "space" => 57,
        "lalt" | "alt" => 56,
        "f1" => 59,
        _ => return None,
    })
}

const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_SIDE: u32 = 0x113;
const BTN_EXTRA: u32 = 0x114;

fn button(name: &str) -> Option<u32> {
    match name {
        "left" | "button1" => Some(BTN_LEFT),
        "right" | "button3" => Some(BTN_RIGHT),
        "mouse4" | "side" => Some(BTN_SIDE),
        "mouse5" | "extra" => Some(BTN_EXTRA),
        _ => None,
    }
}

fn main() {
    let fifo = std::env::args().nth(1).expect("usage: hl-input <fifo>");
    let conn = Connection::connect_to_env().expect("connect to WAYLAND_DISPLAY");
    let (globals, mut queue) = registry_queue_init::<State>(&conn).expect("registry");
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).expect("wl_seat");
    let kb_manager: ZwpVirtualKeyboardManagerV1 = globals.bind(&qh, 1..=1, ()).expect("virtual keyboard manager");
    let ptr_manager: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=2, ()).expect("virtual pointer manager");
    let keyboard = kb_manager.create_virtual_keyboard(&seat, &qh, ());
    let pointer = ptr_manager.create_virtual_pointer(Some(&seat), &qh, ());

    // Standard US keymap so keycodes mean physical key positions.
    let keymap = std::process::Command::new("xkbcli")
        .args(["compile-keymap", "--layout", "us"])
        .output()
        .expect("run xkbcli")
        .stdout;
    let fd = rustix::fs::memfd_create("keymap", rustix::fs::MemfdFlags::CLOEXEC).expect("memfd");
    let mut file = std::fs::File::from(fd);
    file.write_all(&keymap).and_then(|_| file.write_all(&[0])).expect("write keymap");
    keyboard.keymap(1, file.as_fd(), keymap.len() as u32 + 1);
    queue.roundtrip(&mut State).expect("roundtrip");

    let start = Instant::now();
    let now = || start.elapsed().as_millis() as u32;
    let key = |code: u32, down: bool| keyboard.key(now(), code, down as u32);
    let btn = |code: u32, down: bool| {
        pointer.button(now(), code, if down { wayland_client::protocol::wl_pointer::ButtonState::Pressed } else { wayland_client::protocol::wl_pointer::ButtonState::Released });
        pointer.frame();
    };
    eprintln!("hl-input: ready on {fifo}");

    loop {
        let reader = BufReader::new(std::fs::File::open(&fifo).expect("open fifo"));
        for line in reader.lines() {
            let line = line.unwrap_or_default();
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["key", name, state] => match keycode(name) {
                    Some(code) => key(code, *state == "down"),
                    None => eprintln!("hl-input: unknown key {name}"),
                },
                ["tap", name] => match keycode(name) {
                    Some(code) => {
                        key(code, true);
                        conn.flush().ok();
                        std::thread::sleep(Duration::from_millis(60));
                        key(code, false);
                    }
                    None => eprintln!("hl-input: unknown key {name}"),
                },
                ["move", dx, dy, rest @ ..] => {
                    let (dx, dy): (f64, f64) = (dx.parse().unwrap_or(0.0), dy.parse().unwrap_or(0.0));
                    let steps: u32 = rest.first().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
                    let ms: u64 = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                    for _ in 0..steps {
                        pointer.motion(now(), dx / steps as f64, dy / steps as f64);
                        pointer.frame();
                        conn.flush().ok();
                        if ms > 0 {
                            std::thread::sleep(Duration::from_millis(ms / steps as u64));
                        }
                    }
                }
                ["button", name, state] => match button(name) {
                    Some(code) => btn(code, *state == "down"),
                    None => eprintln!("hl-input: unknown button {name}"),
                },
                ["click", name] => match button(name) {
                    Some(code) => {
                        btn(code, true);
                        conn.flush().ok();
                        std::thread::sleep(Duration::from_millis(60));
                        btn(code, false);
                    }
                    None => eprintln!("hl-input: unknown button {name}"),
                },
                ["sleep", ms] => std::thread::sleep(Duration::from_millis(ms.parse().unwrap_or(0))),
                ["quit"] => return,
                [] => {}
                _ => eprintln!("hl-input: bad command {line:?}"),
            }
            conn.flush().ok();
            queue.dispatch_pending(&mut State).ok();
        }
    }
}
