use pty;
use libc;
use nix::sys::signal;
use std::io::Read;
use std::os::unix::io::AsRawFd;
use mio::*;
use super::PtyHandler;
use winsize;

pub const INPUT: Token = Token(0);
pub const OUTPUT: Token = Token(1);

static mut sigwinch_count: i32 = 0;
extern "C" fn handle_sigwinch(_: i32) {
    unsafe {
        sigwinch_count += 1;
    }
}

pub struct RawHandler {
    pub input: unix::PipeReader,
    pub output: unix::PipeReader,
    pub pty: pty::ChildPTY,
    pub handler: Box<PtyHandler>,
    pub resize_count: i32,
}

pub enum Instruction {
    Shutdown,
    Resize,
}

impl RawHandler {
    pub fn new(input: unix::PipeReader, output: unix::PipeReader, pty: pty::ChildPTY, handler: Box<PtyHandler>) -> Self {
        RawHandler {
            input: input,
            output: output,
            pty: pty,
            handler: handler,
            resize_count: Self::sigwinch_count(),
        }
    }

    pub fn register_sigwinch_handler() {
        let sig_action = signal::SigAction::new(handle_sigwinch, signal::signal::SA_RESTART, signal::SigSet::empty());

        unsafe {
            signal::sigaction(signal::SIGWINCH, &sig_action).unwrap();
        }
    }

    pub fn sigwinch_count() -> i32 {
        unsafe { sigwinch_count }
    }

    fn should_resize(&self) -> bool {
        let last = Self::sigwinch_count();

        last > self.resize_count
    }
}

impl Handler for RawHandler {
    type Timeout = ();
    type Message = Instruction;

    fn ready(&mut self, event_loop: &mut EventLoop<RawHandler>, token: Token, events: EventSet) {
        match token {
            INPUT => {
                if events.is_readable() {
                    let mut buf = [0; 128];
                    let nread = self.input.read(&mut buf).unwrap();

                    (&mut *self.handler).input(&buf[..nread]);
                }
            }
            OUTPUT => {
                if events.is_readable() {
                    let mut buf = [0; 1024 * 10];
                    let nread = self.output.read(&mut buf).unwrap_or(0);


                    if nread <= 0 {
                        event_loop.shutdown();
                    } else {
                        (&mut *self.handler).output(&buf[..nread]);
                    }
                }
            }
            _ => unimplemented!()
        }
    }

    fn notify(&mut self, event_loop: &mut EventLoop<RawHandler>, message: Instruction) {
        match message {
            Instruction::Shutdown => event_loop.shutdown(),
            Instruction::Resize => {
                let winsize = winsize::from_fd(libc::STDIN_FILENO).unwrap();
                winsize::set(self.pty.as_raw_fd(), &winsize);

                (&mut *self.handler).resize(&winsize);

                self.resize_count = Self::sigwinch_count();
            }
        }
    }

    fn tick(&mut self, event_loop: &mut EventLoop<RawHandler>) {
        if self.should_resize() {
            let _ = event_loop.channel().send(Instruction::Resize);
        }
    }
}
