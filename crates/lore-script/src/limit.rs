//! The time limit on one call into a script.
//!
//! Luau asks the engine whether to carry on at every function call and loop
//! turn. The engine's answer is given in two steps. [`check`] is handed to
//! Luau directly and decides, without touching the script's stack, whether
//! time is up. Only when it is does it pass the question on to the function
//! mlua installs for [`Lua::set_interrupt`], which raises the error.
//!
//! mlua's function is not called every time because it borrows a slot at the
//! bottom of the running script function's stack whenever the script has just
//! caught an error made by the engine, and each loan leaves that function's
//! stack two slots taller for as long as it runs. A loop that caught engine
//! errors at the top of a script got slower with each one (mlua 0.12.2).

use std::cell::RefCell;
use std::ffi::c_int;
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{Lua, VmState, ffi};

use crate::api::Shared;
use crate::error::TookTooLong;

/// How long one call into the script may run before it is stopped. A healthy
/// tick takes well under a millisecond; this only catches loops that never end.
const TIME_LIMIT: Duration = Duration::from_secs(1);

/// The time limit reads the clock once in this many checks.
const CLOCK_EVERY: u32 = 256;

/// What Luau calls to ask whether a script may carry on.
pub(crate) type Interrupt = unsafe extern "C-unwind" fn(*mut ffi::lua_State, c_int);

thread_local! {
    /// The script call this thread is in the middle of, if any. One thread
    /// runs one script call at a time.
    static RUNNING: RefCell<Option<Rc<Shared>>> = const { RefCell::new(None) };
}

/// Makes `lua` stop a call that runs past the time limit.
pub(crate) fn install(lua: &Lua, shared: &Rc<Shared>) -> mlua::Result<()> {
    let limit = Rc::clone(shared);
    // Only reached once `check` has found that time is up.
    lua.set_interrupt(move |lua| {
        if limit.deadline.get().is_none() {
            return Ok(VmState::Continue);
        }
        // Once time is up every check fails, so a script that catches the
        // error with pcall still cannot carry on.
        let mut stopped = limit.stopped.borrow().clone();
        if stopped.is_none() {
            let place = lua.inspect_stack(0, |running| {
                let file = running.source().source;
                let file = file.and_then(|file| Some(file.strip_prefix('@')?.to_owned()));
                (file, running.current_line())
            });
            let (file, line) = place.unwrap_or_default();
            let line = line.and_then(|line| u32::try_from(line).ok());
            stopped = Some(TookTooLong { file, line });
            limit.stopped.borrow_mut().clone_from(&stopped);
        }
        match stopped {
            Some(stopped) => Err(mlua::Error::external(stopped)),
            None => Ok(VmState::Continue),
        }
    });
    // SAFETY: `state` is this VM's, alive for the whole call. `lua_callbacks`
    // gives the VM's own table of callbacks, which outlives every script call,
    // and only this thread reads or writes it. `check` has the signature Luau
    // expects of an interrupt.
    unsafe {
        lua.exec_raw::<()>((), |state| {
            let callbacks = ffi::lua_callbacks(state);
            shared.raise.set((*callbacks).interrupt);
            (*callbacks).interrupt = Some(check);
        })
    }
}

/// Runs `call` with the time limit armed.
///
/// Running out of time is an error even if the script caught it with `pcall`
/// and returned normally.
pub(crate) fn within_time_limit<R>(
    shared: &Rc<Shared>,
    call: impl FnOnce() -> mlua::Result<R>,
) -> mlua::Result<R> {
    shared.stopped.replace(None);
    shared.deadline.set(Some(Instant::now() + TIME_LIMIT));
    let before = RUNNING.replace(Some(Rc::clone(shared)));
    let result = call();
    RUNNING.set(before);
    shared.deadline.set(None);
    match shared.stopped.take() {
        Some(stopped) => Err(mlua::Error::external(stopped)),
        None => result,
    }
}

/// Whether the call in progress has run out of time. Called at every function
/// call and loop turn of a script, so the clock is only read now and then.
fn time_is_up(shared: &Shared) -> bool {
    let Some(deadline) = shared.deadline.get() else {
        return false;
    };
    if shared
        .stopped
        .try_borrow()
        .is_ok_and(|stopped| stopped.is_some())
    {
        return true;
    }
    let checks = shared.checks.get().wrapping_add(1);
    shared.checks.set(checks);
    checks.is_multiple_of(CLOCK_EVERY) && Instant::now() >= deadline
}

/// Luau's question "may this script carry on?". Returns to say yes; raises a
/// script error, through mlua, to say no.
///
/// # Safety
///
/// Only Luau may call this, as the interrupt of a VM set up by [`install`].
unsafe extern "C-unwind" fn check(state: *mut ffi::lua_State, gc: c_int) {
    // At or above zero Luau is reporting on its garbage collector, where a
    // script cannot be stopped.
    if gc >= 0 {
        return;
    }
    let raise = RUNNING
        .try_with(|running| {
            let running = running.try_borrow().ok()?;
            let shared = running.as_ref()?;
            time_is_up(shared).then(|| shared.raise.get()).flatten()
        })
        .ok()
        .flatten();
    // Nothing is borrowed and nothing needs dropping by now: raising the
    // error leaves this function without returning.
    if let Some(raise) = raise {
        // SAFETY: `raise` is the interrupt mlua installed on this thread's
        // running VM, which `state` belongs to, and Luau is calling this
        // function at a point where an interrupt may raise an error.
        unsafe { raise(state, gc) }
    }
}
