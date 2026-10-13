// Receives the files Finder asks the app to open: a double-click, Open With, or files
// dropped on the Dock icon. Winit 0.30 owns the application delegate and doesn't handle
// this, so the method is added to its delegate class.
use objc2::ffi;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::sel;
use objc2_foundation::{NSArray, NSURL};
use std::path::PathBuf;
use std::sync::OnceLock;
use winit::event_loop::EventLoopProxy;

static PROXY: OnceLock<EventLoopProxy<Vec<PathBuf>>> = OnceLock::new();

const WINIT_DELEGATE: &str = "WinitApplicationDelegate";

// -[NSApplicationDelegate application:openURLs:]
extern "C" fn open_urls(
    _this: *mut AnyObject,
    _cmd: Sel,
    _app: *mut AnyObject,
    urls: *mut NSArray<NSURL>,
) {
    let Some(urls) = (unsafe { urls.as_ref() }) else {
        return;
    };
    let paths: Vec<PathBuf> = urls
        .iter()
        .filter_map(|url| unsafe { url.path() })
        .map(|path| PathBuf::from(path.to_string()))
        .collect();
    if let Some(proxy) = PROXY.get() {
        let _ = proxy.send_event(paths);
    }
}

// Call after the event loop exists and before it runs. Opened files arrive as user events.
pub fn handle_open_files(proxy: EventLoopProxy<Vec<PathBuf>>) {
    let _ = PROXY.set(proxy);
    let Some(class) = AnyClass::get(WINIT_DELEGATE) else {
        eprintln!("recview: {WINIT_DELEGATE} not found, files can't be opened from Finder");
        return;
    };
    let imp: unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>) =
        open_urls;
    unsafe {
        ffi::class_addMethod(
            class as *const AnyClass as *mut ffi::objc_class,
            sel!(application:openURLs:).as_ptr(),
            Some(std::mem::transmute::<
                unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>),
                unsafe extern "C" fn(),
            >(imp)),
            c"v@:@@".as_ptr(),
        );
    }
}
