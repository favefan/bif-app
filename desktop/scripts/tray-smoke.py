# SPDX-License-Identifier: Apache-2.0
"""Interactive Windows acceptance test for a running, test-owned bif-app.

Development/QA only: pip install pywinauto==0.6.9. Requires an unlocked desktop.
Never included as a runtime dependency. It temporarily toggles this app's
autostart entry, restores it, and finally chooses Quit in the real tray menu.
"""
import argparse
import time
import urllib.request
import winreg

import win32clipboard
from pywinauto import Application, Desktop


def wait_for(check, message, seconds=15):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        if check():
            return
        time.sleep(0.1)
    raise AssertionError(message)


def autostart():
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Run") as key:
            return winreg.QueryValueEx(key, "bif-app")[0]
    except FileNotFoundError:
        return None


def run(pid, child_pid, port):
    app = Application(backend="uia").connect(process=pid)
    child = Application(backend="win32").connect(process=child_pid)
    native = Application(backend="win32").connect(process=pid)
    desktop = Desktop(backend="uia")

    def menu():
        # Reuse an already open owned menu, otherwise open the observed icon.
        visible = [w for w in app.windows() if w.is_visible() and w.element_info.control_type == "Menu"]
        if visible:
            assert len(visible) == 1
            return visible[0]
        bar = desktop.window(class_name="Shell_TrayWnd")
        icon = bar.child_window(title_re="bif-app .*local Bifrost gateway", control_type="Button")
        if not icon.exists(timeout=2):
            bar.child_window(auto_id="1502", control_type="Button").click_input()
            icon = desktop.window(class_name="NotifyIconOverflowWindow").child_window(title_re="bif-app .*local Bifrost gateway", control_type="Button")
        icon.click_input(button="right")
        wait_for(lambda: any(w.is_visible() and w.element_info.control_type == "Menu" for w in app.windows()), "Tray menu did not open")
        return next(w for w in app.windows() if w.is_visible() and w.element_info.control_type == "Menu")

    def choose(label):
        items = [c for c in menu().descendants(control_type="MenuItem") if c.window_text() == label]
        assert len(items) == 1, label
        items[0].click_input()

    choose("Show bif-app")
    main = native.window(title="bif-app", visible_only=False).wrapper_object()
    wait_for(main.is_visible, "Show did not restore the native window")
    main.close()
    wait_for(lambda: not main.is_visible(), "Close did not hide to tray")
    assert urllib.request.urlopen(f"http://127.0.0.1:{port}/health", timeout=3).status == 200
    choose("Show bif-app")
    wait_for(main.is_visible, "Tray Show did not restore the window")
    print("PASS: close hides, gateway keeps running, tray Show restores", flush=True)

    previous = None
    win32clipboard.OpenClipboard()
    try:
        if win32clipboard.IsClipboardFormatAvailable(win32clipboard.CF_UNICODETEXT):
            previous = win32clipboard.GetClipboardData(win32clipboard.CF_UNICODETEXT)
    finally:
        win32clipboard.CloseClipboard()
    try:
        choose("Copy API Base URL")
        win32clipboard.OpenClipboard()
        try:
            assert win32clipboard.GetClipboardData(win32clipboard.CF_UNICODETEXT) == f"http://127.0.0.1:{port}/v1"
        finally:
            win32clipboard.CloseClipboard()
    finally:
        if previous is not None:
            win32clipboard.OpenClipboard()
            try:
                win32clipboard.EmptyClipboard()
                win32clipboard.SetClipboardText(previous, win32clipboard.CF_UNICODETEXT)
            finally:
                win32clipboard.CloseClipboard()
    print("PASS: tray Copy API Base URL copies the actual endpoint", flush=True)

    original = autostart()
    try:
        choose("Start at Login")
        wait_for(lambda: bool(autostart()) != bool(original), "Autostart toggle failed")
        choose("Start at Login")
        wait_for(lambda: autostart() == original, "Autostart restore failed")
    finally:
        if bool(autostart()) != bool(original):
            choose("Start at Login")
    print("PASS: Start at Login off/on/off, registration restored", flush=True)

    choose("Open Bifrost UI in browser")
    assert main.is_visible(), "Browser action replaced/hid the native window"
    print("PASS: browser action preserves the application window", flush=True)
    choose("Quit")
    wait_for(lambda: not app.is_process_running(), "Tray Quit did not exit the desktop", 45)
    wait_for(lambda: not child.is_process_running(), "Tray Quit orphaned the gateway")
    print("PASS: real tray Quit stops the host and gateway", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--child-pid", type=int, required=True)
    parser.add_argument("--port", type=int, required=True)
    options = parser.parse_args()
    run(options.pid, options.child_pid, options.port)
