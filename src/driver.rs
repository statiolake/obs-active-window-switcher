use std::{
    collections::HashMap,
    thread::{self, JoinHandle},
};

use crossbeam_channel::{bounded, Receiver, Sender};
use windows::Win32::Foundation::HWND;

use crate::{
    foreground_watcher::{ForegroundWatcherCommand, ForegroundWatcherMessage},
    image_viewer::{ImageViewerCommand, ImageViewerMessage},
    stdin_shell::{StdinShellCommand, StdinShellMessage},
    window_capture::{CapturedFrame, WindowCapture, WindowCaptureCommand, WindowCaptureMessage},
};

struct WindowCaptureInterop {
    _tx_cmd: Sender<WindowCaptureCommand>,
    rx_msg: Receiver<WindowCaptureMessage>,
    rx_frame: Receiver<CapturedFrame>,
    thread: JoinHandle<()>,
}

pub struct Driver {
    im_tx_cmd: Sender<ImageViewerCommand>,
    im_rx_msg: Receiver<ImageViewerMessage>,
    fw_tx_cmd: Sender<ForegroundWatcherCommand>,
    fw_rx_msg: Receiver<ForegroundWatcherMessage>,
    sh_tx_cmd: Sender<StdinShellCommand>,
    sh_rx_msg: Receiver<StdinShellMessage>,
    cap_rxs: HashMap<isize, WindowCaptureInterop>,
    current_hwnd: Option<HWND>,
}

impl Driver {
    pub fn new(
        im_tx_cmd: Sender<ImageViewerCommand>,
        im_rx_msg: Receiver<ImageViewerMessage>,
        fw_tx_cmd: Sender<ForegroundWatcherCommand>,
        fw_rx_msg: Receiver<ForegroundWatcherMessage>,
        sh_tx_cmd: Sender<StdinShellCommand>,
        sh_rx_msg: Receiver<StdinShellMessage>,
    ) -> Self {
        Self {
            im_tx_cmd,
            im_rx_msg,
            fw_tx_cmd,
            fw_rx_msg,
            sh_tx_cmd,
            sh_rx_msg,
            cap_rxs: HashMap::new(),
            current_hwnd: None,
        }
    }

    pub fn run(mut self) {
        loop {
            if let Ok(msg) = self.im_rx_msg.try_recv() {
                self.handle_image_viewer_message(msg);
            }

            if let Ok(msg) = self.fw_rx_msg.try_recv() {
                self.handle_foreground_watcher_message(msg);
            }

            if let Ok(msg) = self.sh_rx_msg.try_recv() {
                self.handle_stdin_shell_message(msg);
            }

            self.handle_captures_message();

            self.handle_captures_frames();
        }
    }

    fn handle_image_viewer_message(&self, msg: ImageViewerMessage) {
        match msg {
            ImageViewerMessage::Closed => self.quit(),
        }
    }

    fn handle_foreground_watcher_message(&mut self, msg: ForegroundWatcherMessage) {
        match msg {
            ForegroundWatcherMessage::WindowChanged { hwnd } => {
                self.current_hwnd = Some(hwnd);
                if !self.cap_rxs.contains_key(&hwnd.0) {
                    self.start_capture_for(hwnd);
                }
            }
        }
    }

    fn handle_stdin_shell_message(&self, msg: StdinShellMessage) {
        match msg {}
    }

    fn handle_captures_message(&mut self) {
        let mut to_remove = vec![];
        for WindowCaptureInterop { rx_msg, .. } in self.cap_rxs.values_mut() {
            if let Ok(msg) = rx_msg.try_recv() {
                match msg {
                    WindowCaptureMessage::Closed { hwnd } => {
                        to_remove.push(hwnd);
                    }
                }
            }
        }

        // すでに閉じられたウィンドウを削除する
        for hwnd in to_remove {
            if let Some(cap) = self.cap_rxs.remove(&hwnd.0) {
                let _ = cap.thread.join();
            }
        }
    }

    fn handle_captures_frames(&mut self) {
        for WindowCaptureInterop { rx_frame, .. } in self.cap_rxs.values_mut() {
            if let Ok(frame) = rx_frame.try_recv() {
                if Some(frame.hwnd) == self.current_hwnd {
                    let _ = self.im_tx_cmd.send(ImageViewerCommand::Update(frame));
                }
            }
        }
    }

    fn start_capture_for(&mut self, hwnd: HWND) {
        let (tx_frame, rx_frame) = bounded(5);
        let (capture, tx_cmd, rx_msg) = WindowCapture::new(hwnd, tx_frame);
        let thread = thread::spawn(move || capture.run());
        self.cap_rxs.insert(
            hwnd.0,
            WindowCaptureInterop {
                _tx_cmd: tx_cmd,
                rx_msg,
                rx_frame,
                thread,
            },
        );
    }

    fn quit(&self) {
        let _ = self.im_tx_cmd.send(ImageViewerCommand::Quit);
        let _ = self.fw_tx_cmd.send(ForegroundWatcherCommand::Quit);
        let _ = self.sh_tx_cmd.send(StdinShellCommand::Quit);
    }
}
