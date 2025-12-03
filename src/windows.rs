use core::iter::once;
use crossbeam_channel::Receiver;
use crossbeam_channel::{Sender, bounded};
use std::sync::LazyLock;
use std::{ffi::c_void, mem::size_of, os::windows::prelude::OsStrExt, time::Duration};
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LRESULT;
use windows::Win32::UI::WindowsAndMessaging::{
    DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE, DBT_DEVNODES_CHANGED, DBT_DEVTYP_DEVICEINTERFACE, DEV_BROADCAST_DEVICEINTERFACE_W,
    REGISTER_NOTIFICATION_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_WINDOW_HANDLE, MSG};
use windows::{
    Win32::{
        Foundation::{HANDLE, LPARAM, WPARAM},
        Graphics::Gdi::HBRUSH,
        NetworkManagement::IpHelper::{CancelMibChangeNotify2, MIB_NOTIFICATION_TYPE},
        Networking::WinSock::{ADDRESS_FAMILY, AF_INET, AF_INET6, AF_UNSPEC},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::HICON,
        UI::{
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::{
                CreateWindowExW, DEVICE_NOTIFY_ALL_INTERFACE_CLASSES, DefWindowProcW, DispatchMessageW, GWL_STYLE, GetMessageW, HCURSOR,
                RegisterClassExW, RegisterDeviceNotificationW, SetWindowLongPtrW, TranslateMessage, WM_DEVICECHANGE, WNDCLASSEXW, WS_EX_LAYERED,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_OVERLAPPED, WS_POPUP, WS_VISIBLE,
            },
        },
    },
    core::{Error, HRESULT, PCWSTR, Result},
};

use crate::SystemEvent;
use crate::{EventNotify, Monitor};

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
        self.event_receiver.recv_timeout(timeout.unwrap_or(Duration::MAX)).ok()
    }

    fn recv_ref(&self) -> &Receiver<SystemEvent> {
        &self.event_receiver
    }
}

impl WindowsSystemMonitor {
    pub fn new(notify: EventNotify<()>) -> Self {
        println!("hello windows system monitor");

        let (tx, rx) = bounded::<isize>(1);
        let (event_sender, event_receiver) = bounded::<SystemEvent>(10);

        // 创建事件窗口
        create_event_window(tx, event_sender, notify);
        let window_hwnd = rx.recv().unwrap();

        let this = Self { event_receiver, window_hwnd };

        this.register_dev_changed_event();

        this
    }
}

impl WindowsSystemMonitor {
    // 在程序退出时, 需要使用 UnregisterDeviceNotification 注销事件?

    // 注册 设备改变 事件
    pub fn register_dev_changed_event(&self) {
        let guids = [
            // USB device
            windows::Win32::Devices::DeviceAndDriverInstallation::GUID_DEVCLASS_WCEUSBS,
        ];

        for guid in guids {
            let mut notify_filter = DEV_BROADCAST_DEVICEINTERFACE_W::default();
            notify_filter.dbcc_size = size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32;
            notify_filter.dbcc_devicetype = DBT_DEVTYP_DEVICEINTERFACE.0;
            notify_filter.dbcc_classguid = guid;

            unsafe {
                RegisterDeviceNotificationW(
                    HANDLE(self.window_hwnd as *mut c_void),
                    &notify_filter as *const DEV_BROADCAST_DEVICEINTERFACE_W as *const c_void,
                    REGISTER_NOTIFICATION_FLAGS(DEVICE_NOTIFY_WINDOW_HANDLE.0 | DEVICE_NOTIFY_ALL_INTERFACE_CLASSES.0),
                )
                .unwrap();
            }
        }

        println!("Register Device Notification");
    }
}

static DEVICE_EVENT_TARGET_WINDOW_CLASS: LazyLock<Vec<u16>> = LazyLock::new(|| unsafe {
    let class_name = encode_wide("windows device monitor");

    let class = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: Default::default(),
        lpfnWndProc: Some(call_default_window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into(),
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
});

// 用于把 str 转换成 宽字符串
pub fn encode_wide(string: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    string.as_ref().encode_wide().chain(once(0)).collect()
}

pub unsafe extern "system" fn call_default_window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn create_event_window(tx: Sender<isize>, event_sender: Sender<SystemEvent>, notify: EventNotify<()>) {
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
                None,
                None,
                Some(GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into()),
                None,
            )
        }
        .unwrap();

        tx.send(window.0 as isize).unwrap();

        // 设置子窗口处理程序
        if unsafe {
            !SetWindowSubclass(
                window,
                Some(window_proc),
                0,
                Box::into_raw(Box::new(SubclassInput { event_sender, notify })) as usize,
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
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
}

struct SubclassInput {
    event_sender: Sender<SystemEvent>,
    notify: EventNotify<()>,
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

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _: usize, subclass_input_ptr: usize) -> LRESULT {
    unsafe {
        let subclass_input = (subclass_input_ptr as *mut SubclassInput).as_ref().unwrap();
        if msg == WM_DEVICECHANGE {
            // println!("WM_DEVICECHANGE message received, wparam: {}", wparam.0);

            let dev_broadcast = lparam.0 as *const DEV_BROADCAST_DEVICEINTERFACE_W;
            if !dev_broadcast.is_null() {
                let dev_info = &*dev_broadcast;

                // 确认是设备接口通知
                if dev_info.dbcc_devicetype == DBT_DEVTYP_DEVICEINTERFACE.0 {
                    // 获取设备类 GUID
                    let guid = dev_info.dbcc_classguid;

                    // 可选：获取设备路径
                    // let device_name = PCWSTR::from_raw(dev_info.dbcc_name.as_ptr());
                    // println!("Device path: {:?}", device_name.to_string());

                    match guid {
                        windows::Win32::Devices::DeviceAndDriverInstallation::GUID_DEVCLASS_WCEUSBS => {
                            // println!("Device type: USB Device");
                        }
                        // 只处理串口设备
                        windows::Win32::System::Ioctl::GUID_DEVINTERFACE_COMPORT => match wparam.0 as u32 {
                            DBT_DEVICEARRIVAL => {
                                // println!("DBT_DEVICE ARRIVAL");
                                subclass_input.send(SystemEvent::DevAdded);
                                subclass_input.notify.exec(());
                            }
                            DBT_DEVICEREMOVECOMPLETE => {
                                // println!("DBT_DEVICE REMOVE COMPLETE");
                                subclass_input.send(SystemEvent::DevRemoved);
                                subclass_input.notify.exec(());
                            }
                            DBT_DEVNODES_CHANGED => {
                                // println!("DBT_DEV CHANGED");
                                subclass_input.send(SystemEvent::DevNodesChanged);
                                subclass_input.notify.exec(());
                            }
                            _ => {
                                println!("WM_DEVICECHANGE: {}", wparam.0);
                            }
                        },
                        _ => {
                            println!("Device type: Unknown (GUID: {:?})", guid);
                        }
                    }
                }
            }

            // println!("wparam: {wparam:?}  lparam: {lparam:?}");
            return LRESULT(0);
        }

        DefSubclassProc(hwnd, msg, wparam, lparam)
    }
}

// // 定义回调函数
// unsafe extern "system" fn ip_interface_callback(
//     caller_context: *mut c_void,
//     row: *mut MIB_IPINTERFACE_ROW,
//     notification_type: MIB_NOTIFICATION_TYPE,
// ) {
//     // 获取上下文（这里简单打印）
//     let context = caller_context as *const &str;
//     let context_str = unsafe { *context };
//     println!("回调触发！上下文: {}", context_str);

//     // 检查通知类型
//     match notification_type {
//         MIB_PARAMETER_NOTIFICATION => println!("网络接口参数变化"),
//         MIB_ADD_INSTANCE => println!("网络接口添加"),
//         MIB_DELETE_INSTANCE => println!("网络接口移除"),
//         _ => println!("未知通知类型: {:?}", notification_type),
//     }

//     // 如果 row 非空，打印接口信息
//     if !row.is_null() {
//         let interface_row = unsafe { &*row };
//         println!("接口索引: {}", interface_row.InterfaceIndex);
//         println!("接口 LUID: {:?}", interface_row.InterfaceLuid);
//     }
// }
