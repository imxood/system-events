use core::iter::once;
use crossbeam_channel::Receiver;
use crossbeam_channel::{bounded, Sender};
use lazy_static::lazy_static;
use std::{ffi::c_void, mem::size_of, os::windows::prelude::OsStrExt, time::Duration};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::UI::WindowsAndMessaging::{
    DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE, DBT_DEVNODES_CHANGED, DBT_DEVTYP_DEVICEINTERFACE,
    DEV_BROADCAST_DEVICEINTERFACE_W, HMENU, REGISTER_NOTIFICATION_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_WINDOW_HANDLE, MSG};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{HANDLE, LPARAM, WPARAM},
        Graphics::Gdi::HBRUSH,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::HICON,
        UI::{
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassExW,
                RegisterDeviceNotificationW, SetWindowLongPtrW, TranslateMessage,
                DEVICE_NOTIFY_ALL_INTERFACE_CLASSES, GWL_STYLE, HCURSOR, WM_DEVICECHANGE,
                WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
                WS_OVERLAPPED, WS_POPUP, WS_VISIBLE,
            },
        },
    },
};

use crate::Monitor;
use crate::SystemEvent;

#[derive(Clone)]
pub struct WindowsSystemMonitor {
    window_hwnd: isize,
    event_receiver: Receiver<SystemEvent>,
}

impl Monitor for WindowsSystemMonitor {
    fn into_inner(self) -> Receiver<SystemEvent> {
        self.event_receiver
    }

    fn try_recv(&self) -> Option<SystemEvent> {
        self.event_receiver.try_recv().ok()
    }

    fn recv(&self, timeout: Option<Duration>) -> Option<SystemEvent> {
        self.event_receiver
            .recv_timeout(timeout.unwrap_or(Duration::MAX))
            .ok()
    }

    fn recv_ref(&self) -> &Receiver<SystemEvent> {
        &self.event_receiver
    }
}

impl Default for WindowsSystemMonitor {
    fn default() -> Self {
        let (tx, rx) = bounded::<isize>(1);
        let (event_sender, event_receiver) = bounded::<SystemEvent>(10);

        // 创建事件窗口
        create_event_window(tx, event_sender);
        let window_hwnd = rx.recv().unwrap();

        Self {
            event_receiver,
            window_hwnd,
        }
    }
}

impl WindowsSystemMonitor {
    // 注册 设备改变 事件
    pub fn register_dev_changed_event(&self) {
        let mut notify_filter = DEV_BROADCAST_DEVICEINTERFACE_W::default();
        notify_filter.dbcc_size = size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32;
        notify_filter.dbcc_devicetype = DBT_DEVTYP_DEVICEINTERFACE.0;
        notify_filter.dbcc_classguid = windows::core::GUID::from_values(
            0x25dbce51,
            0x6c8f,
            0x4a72,
            [0x8a, 0x6d, 0xb5, 0x4c, 0x2b, 0x4f, 0xc8, 0x35],
        );
        unsafe {
            RegisterDeviceNotificationW(
                HANDLE(self.window_hwnd),
                &notify_filter as *const DEV_BROADCAST_DEVICEINTERFACE_W as *const c_void,
                REGISTER_NOTIFICATION_FLAGS(
                    DEVICE_NOTIFY_WINDOW_HANDLE.0 | DEVICE_NOTIFY_ALL_INTERFACE_CLASSES.0,
                ),
            );
        }
        println!("Register Device Notification");
    }
}

lazy_static! {
    static ref DEVICE_EVENT_TARGET_WINDOW_CLASS: Vec<u16> = unsafe {
        let class_name= encode_wide("windows api demo");

        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: Default::default(),
            lpfnWndProc: Some(call_default_window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(PCWSTR::null()).unwrap_or_default(),
            hIcon: HICON::default(),
            hCursor: HCURSOR::default(), // must be null in order for cursor state to work properly
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
            hIconSm: HICON::default(),
        };

        // 这个 Class 必须注册
        RegisterClassExW(&class);
        class_name
    };
}

// 用于把 str 转换成 宽字符串
pub fn encode_wide(string: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    string.as_ref().encode_wide().chain(once(0)).collect()
}

pub unsafe extern "system" fn call_default_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn create_event_window(tx: Sender<isize>, event_sender: Sender<SystemEvent>) {
    std::thread::spawn(move || {
        // 创建创建窗口 需要和事件处理 放在同一个线程中.

        // 创建 窗口句柄
        let window = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                PCWSTR::from_raw(DEVICE_EVENT_TARGET_WINDOW_CLASS.clone().as_ptr()),
                PCWSTR::null(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                HWND::default(),
                HMENU::default(),
                GetModuleHandleW(PCWSTR::null()).unwrap_or_default(),
                None,
            )
        };

        tx.send(window.0).unwrap();

        // 设置子窗口处理程序
        if unsafe {
            !SetWindowSubclass(
                window,
                Some(window_proc),
                0,
                Box::into_raw(Box::new(SubclassInput { event_sender })) as usize,
            )
            .as_bool()
        } {
            panic!("windows platform, set window subclass failed");
        }

        unsafe {
            SetWindowLongPtrW(window, GWL_STYLE, (WS_VISIBLE | WS_POPUP).0 as _);
        }

        // 消息处理
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

struct SubclassInput {
    event_sender: Sender<SystemEvent>,
}

impl Drop for SubclassInput {
    fn drop(&mut self) {
        println!("SubclassInput dropped");
    }
}

impl SubclassInput {
    pub fn send(&self, event: SystemEvent) {
        self.event_sender.try_send(event).ok();
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    subclass_input_ptr: usize,
) -> LRESULT {
    let subclass_input = (subclass_input_ptr as *mut SubclassInput).as_ref().unwrap();
    if msg == WM_DEVICECHANGE {
        match wparam.0 as u32 {
            DBT_DEVICEARRIVAL => {
                subclass_input.send(SystemEvent::DevAdded);
            }
            DBT_DEVICEREMOVECOMPLETE => {
                subclass_input.send(SystemEvent::DevRemoved);
            }
            DBT_DEVNODES_CHANGED => {
                subclass_input.send(SystemEvent::DevNodesChanged);
            }
            _ => {
                println!(
                    "WM_DEVICECHANGE message received, value {} unhandled",
                    wparam.0
                );
            }
        }
        // println!("wparam: {wparam:?}  lparam: {lparam:?}");
        return LRESULT(0);
    }

    DefSubclassProc(hwnd, msg, wparam, lparam)
}
