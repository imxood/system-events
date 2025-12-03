# 系统事件监控

目前只支持 windows 下的 usb设备插拔 事件

欢迎提 issues 和 pr

## USB插拔

cargo run --example windows_serial_plug

### widnows系统

监控的是 window 的 WM_DEVICECHANGE 事件

https://learn.microsoft.com/zh-cn/windows/win32/devio/wm-devicechange
