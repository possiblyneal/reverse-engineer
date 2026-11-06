//! The ALSA sink: the only code in the audio layer that touches a device.
//!
//! `libasound.so.2` is loaded at run time with `dlopen`, so the crate has no
//! build-time ALSA dependency and a host without the library, the device or a
//! sound card gets a [`SinkError`], not a crash. Tests and CI never open it
//! beyond proving that a missing library is an error.

use std::ffi::{c_int, c_long, c_uint, c_ulong, c_void, CStr, CString};
use std::fmt;
use std::ptr::NonNull;

const LIBRARY: &str = "libasound.so.2";
/// `SND_PCM_STREAM_PLAYBACK`.
const STREAM_PLAYBACK: c_int = 0;
/// `SND_PCM_FORMAT_S16_LE`.
const FORMAT_S16_LE: c_int = 2;
/// `SND_PCM_ACCESS_RW_INTERLEAVED`.
const ACCESS_RW_INTERLEAVED: c_int = 3;
/// Buffer latency requested from ALSA, microseconds.
const LATENCY_US: c_uint = 100_000;

/// Why the sink could not open or write.
#[derive(Debug, PartialEq, Eq)]
pub enum SinkError {
    /// The ALSA library could not be loaded.
    LibraryMissing(String),
    /// The library lacks a function the sink needs.
    Symbol(&'static str),
    /// ALSA refused the device or a write, with its negative errno.
    Device(i32),
}

impl fmt::Display for SinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SinkError::LibraryMissing(why) => write!(f, "cannot load the ALSA library: {why}"),
            SinkError::Symbol(name) => write!(f, "the ALSA library has no {name}"),
            SinkError::Device(errno) => write!(f, "ALSA error {errno}"),
        }
    }
}

impl std::error::Error for SinkError {}

type OpenFn =
    unsafe extern "C" fn(*mut *mut c_void, *const std::ffi::c_char, c_int, c_int) -> c_int;
type SetParamsFn =
    unsafe extern "C" fn(*mut c_void, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int;
type WriteFn = unsafe extern "C" fn(*mut c_void, *const c_void, c_ulong) -> c_long;
type RecoverFn = unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_int;
type PcmFn = unsafe extern "C" fn(*mut c_void) -> c_int;

/// A mono 16-bit playback stream on an ALSA device.
pub struct AlsaSink {
    library: NonNull<c_void>,
    pcm: NonNull<c_void>,
    writei: WriteFn,
    recover: RecoverFn,
    drain: PcmFn,
    close: PcmFn,
}

impl AlsaSink {
    /// Open the `default` device at `rate_hz`.
    pub fn open(rate_hz: u32) -> Result<AlsaSink, SinkError> {
        AlsaSink::open_from(LIBRARY, "default", rate_hz)
    }

    /// Open `device` through the ALSA library file `library`.
    pub fn open_from(library: &str, device: &str, rate_hz: u32) -> Result<AlsaSink, SinkError> {
        let name = CString::new(library).map_err(|_| SinkError::LibraryMissing(library.into()))?;
        let device = CString::new(device).map_err(|_| SinkError::Device(-libc::EINVAL))?;
        // SAFETY: a valid NUL-terminated path; the handle is closed on every
        // error path below or by Drop.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        let library = NonNull::new(handle).ok_or_else(last_dl_error)?;
        match Self::open_device(library, &device, rate_hz) {
            Ok(sink) => Ok(sink),
            Err(e) => {
                // SAFETY: the handle came from dlopen and nothing uses it now.
                unsafe { libc::dlclose(library.as_ptr()) };
                Err(e)
            }
        }
    }

    fn open_device(
        library: NonNull<c_void>,
        device: &CStr,
        rate_hz: u32,
    ) -> Result<AlsaSink, SinkError> {
        // SAFETY: each symbol is looked up by its documented ALSA name and cast to
        // its documented C signature (alsa/pcm.h).
        let (open, set_params, writei, recover, drain, close) = unsafe {
            (
                std::mem::transmute::<*mut c_void, OpenFn>(symbol(library, c"snd_pcm_open")?),
                std::mem::transmute::<*mut c_void, SetParamsFn>(symbol(
                    library,
                    c"snd_pcm_set_params",
                )?),
                std::mem::transmute::<*mut c_void, WriteFn>(symbol(library, c"snd_pcm_writei")?),
                std::mem::transmute::<*mut c_void, RecoverFn>(symbol(library, c"snd_pcm_recover")?),
                std::mem::transmute::<*mut c_void, PcmFn>(symbol(library, c"snd_pcm_drain")?),
                std::mem::transmute::<*mut c_void, PcmFn>(symbol(library, c"snd_pcm_close")?),
            )
        };
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: `raw` is a valid out-pointer and `device` is NUL-terminated.
        let err = unsafe { open(&mut raw, device.as_ptr(), STREAM_PLAYBACK, 0) };
        if err < 0 {
            return Err(SinkError::Device(err));
        }
        let pcm = NonNull::new(raw).ok_or(SinkError::Device(-libc::ENODEV))?;
        // SAFETY: `pcm` was just opened; the arguments are the documented ones.
        let err = unsafe {
            set_params(
                pcm.as_ptr(),
                FORMAT_S16_LE,
                ACCESS_RW_INTERLEAVED,
                1,
                rate_hz,
                1,
                LATENCY_US,
            )
        };
        if err < 0 {
            // SAFETY: `pcm` is open and unused afterwards.
            unsafe { close(pcm.as_ptr()) };
            return Err(SinkError::Device(err));
        }
        Ok(AlsaSink {
            library,
            pcm,
            writei,
            recover,
            drain,
            close,
        })
    }

    /// Play `frames` (mono), blocking until ALSA has taken them all.
    pub fn write(&mut self, frames: &[i16]) -> Result<(), SinkError> {
        let mut rest = frames;
        while !rest.is_empty() {
            // SAFETY: `rest` is `rest.len()` valid 16-bit frames and `pcm` is open.
            let n = unsafe {
                (self.writei)(
                    self.pcm.as_ptr(),
                    rest.as_ptr().cast(),
                    rest.len() as c_ulong,
                )
            };
            if n < 0 {
                let errno = c_int::try_from(n).unwrap_or(-libc::EIO);
                // SAFETY: `pcm` is open; recover re-prepares it after an underrun.
                let err = unsafe { (self.recover)(self.pcm.as_ptr(), errno, 1) };
                if err < 0 {
                    return Err(SinkError::Device(err));
                }
                continue;
            }
            rest = &rest[usize::try_from(n).unwrap_or(0).min(rest.len())..];
        }
        Ok(())
    }
}

impl Drop for AlsaSink {
    fn drop(&mut self) {
        // SAFETY: both handles are live and are not used after this.
        unsafe {
            (self.drain)(self.pcm.as_ptr());
            (self.close)(self.pcm.as_ptr());
            libc::dlclose(self.library.as_ptr());
        }
    }
}

fn last_dl_error() -> SinkError {
    // SAFETY: dlerror returns NULL or a NUL-terminated string valid until the
    // next dl call on this thread; it is copied at once.
    let why = unsafe {
        let msg = libc::dlerror();
        if msg.is_null() {
            String::from("unknown error")
        } else {
            CStr::from_ptr(msg).to_string_lossy().into_owned()
        }
    };
    SinkError::LibraryMissing(why)
}

/// # Safety
/// `library` must be a live `dlopen` handle.
unsafe fn symbol(library: NonNull<c_void>, name: &'static CStr) -> Result<*mut c_void, SinkError> {
    // SAFETY: the caller guarantees the handle; `name` is NUL-terminated.
    let found = unsafe { libc::dlsym(library.as_ptr(), name.as_ptr()) };
    if found.is_null() {
        Err(SinkError::Symbol(name.to_str().unwrap_or("symbol")))
    } else {
        Ok(found)
    }
}
