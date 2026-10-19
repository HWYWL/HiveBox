//! The host functions, as an application sees them.
//!
//! Every one of these is a call out of the sandbox, which costs more than a line of arithmetic and
//! far less than a mistake about what time it is. Nothing here blocks: a host service that waits is
//! a frame that does not get drawn, so the ones that talk to the world are asynchronous in a later
//! ABI and this one has only the question-and-answer kind.

/// One line on the serial log, without a trailing newline.
pub fn log(message: &str) {
    imp::log(message.as_ptr() as u32, message.len() as u32)
}

/// Milliseconds since boot.
pub fn now_ms() -> i64 {
    imp::now_ms()
}

/// The width the host would draw a string at, in logical pixels.
pub fn text_width(text: &str, size: f32) -> i32 {
    imp::text_width(text.as_ptr() as u32, text.len() as u32, size as i32)
}

/// A remembered value. Returns how many bytes were written into `out`.
pub fn pref_get(key: &str, out: &mut [u8]) -> Option<usize> {
    let len = imp::pref_get(
        key.as_ptr() as u32,
        key.len() as u32,
        out.as_mut_ptr() as u32,
        out.len() as u32,
    );

    (len >= 0).then_some(len as usize)
}

/// Remembers a value. `false` when the host could not write it.
pub fn pref_set(key: &str, value: &[u8]) -> bool {
    imp::pref_set(
        key.as_ptr() as u32,
        key.len() as u32,
        value.as_ptr() as u32,
        value.len() as u32,
    ) == 0
}

/// Reads a file from the application's private directory.
pub fn file_read(name: &str, out: &mut [u8]) -> Option<usize> {
    let len = imp::file_read(
        name.as_ptr() as u32,
        name.len() as u32,
        out.as_mut_ptr() as u32,
        out.len() as u32,
    );

    (len >= 0).then_some(len as usize)
}

/// Writes a file in the application's private directory.
pub fn file_write(name: &str, data: &[u8]) -> bool {
    imp::file_write(
        name.as_ptr() as u32,
        name.len() as u32,
        data.as_ptr() as u32,
        data.len() as u32,
    ) == 0
}

/// Asks for another frame without an input event: what an application calls when something it
/// started in the background has an answer.
pub fn invalidate() {
    imp::invalidate()
}

/// Closes the application.
pub fn exit() {
    imp::exit()
}

#[cfg(target_arch = "wasm32")]
mod imp {
    // The imported names are the ABI's (`hb_*`); the names on this side are what the wrappers above
    // call. `link_name` is the seam, and each `unsafe` is one call out of the sandbox — nothing in
    // an application outside this module is unsafe.
    #[link(wasm_import_module = "host")]
    extern "C" {
        #[link_name = "hb_log"]
        fn raw_log(ptr: u32, len: u32);
        #[link_name = "hb_now_ms"]
        fn raw_now_ms() -> i64;
        #[link_name = "hb_text_width"]
        fn raw_text_width(ptr: u32, len: u32, size: i32) -> i32;
        #[link_name = "hb_pref_get"]
        fn raw_pref_get(key_ptr: u32, key_len: u32, out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_pref_set"]
        fn raw_pref_set(key_ptr: u32, key_len: u32, value_ptr: u32, value_len: u32) -> i32;
        #[link_name = "hb_file_read"]
        fn raw_file_read(name_ptr: u32, name_len: u32, out_ptr: u32, out_len: u32) -> i32;
        #[link_name = "hb_file_write"]
        fn raw_file_write(name_ptr: u32, name_len: u32, data_ptr: u32, data_len: u32) -> i32;
        #[link_name = "hb_invalidate"]
        fn raw_invalidate();
        #[link_name = "hb_exit"]
        fn raw_exit();
    }

    pub fn log(ptr: u32, len: u32) {
        unsafe { raw_log(ptr, len) }
    }

    pub fn now_ms() -> i64 {
        unsafe { raw_now_ms() }
    }

    pub fn text_width(ptr: u32, len: u32, size: i32) -> i32 {
        unsafe { raw_text_width(ptr, len, size) }
    }

    pub fn pref_get(key_ptr: u32, key_len: u32, out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_pref_get(key_ptr, key_len, out_ptr, out_len) }
    }

    pub fn pref_set(key_ptr: u32, key_len: u32, value_ptr: u32, value_len: u32) -> i32 {
        unsafe { raw_pref_set(key_ptr, key_len, value_ptr, value_len) }
    }

    pub fn file_read(name_ptr: u32, name_len: u32, out_ptr: u32, out_len: u32) -> i32 {
        unsafe { raw_file_read(name_ptr, name_len, out_ptr, out_len) }
    }

    pub fn file_write(name_ptr: u32, name_len: u32, data_ptr: u32, data_len: u32) -> i32 {
        unsafe { raw_file_write(name_ptr, name_len, data_ptr, data_len) }
    }

    pub fn invalidate() {
        unsafe { raw_invalidate() }
    }

    pub fn exit() {
        unsafe { raw_exit() }
    }
}

/// The same names on a PC, answered as "this host does not have that".
///
/// They exist so that an application's arithmetic — and every test of it — can be compiled and run
/// with `cargo test`, where there is no sandbox and no board. Nothing in the SDK calls them on the
/// panel.
#[cfg(not(target_arch = "wasm32"))]
mod imp {
    pub fn log(_ptr: u32, _len: u32) {}
    pub fn now_ms() -> i64 {
        0
    }
    pub fn text_width(_ptr: u32, _len: u32, _size: i32) -> i32 {
        0
    }
    pub fn pref_get(_key_ptr: u32, _key_len: u32, _out_ptr: u32, _out_len: u32) -> i32 {
        -1
    }
    pub fn pref_set(_key_ptr: u32, _key_len: u32, _value_ptr: u32, _value_len: u32) -> i32 {
        -1
    }
    pub fn file_read(_name_ptr: u32, _name_len: u32, _out_ptr: u32, _out_len: u32) -> i32 {
        -1
    }
    pub fn file_write(_name_ptr: u32, _name_len: u32, _data_ptr: u32, _data_len: u32) -> i32 {
        -1
    }
    pub fn invalidate() {}
    pub fn exit() {}
}
