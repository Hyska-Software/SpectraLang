use spectra_runtime::ffi::{clear_host_functions, lookup_host_function};
use spectra_runtime::register;

#[test]
fn synthetic_reactor_io_hosts_are_absent_from_release_builds() {
    clear_host_functions();
    register();
    assert!(
        lookup_host_function("spectra.async.reactor.io_register").is_none(),
        "synthetic io_register host must not exist outside cfg(test)"
    );
    assert!(
        lookup_host_function("spectra.async.reactor.io_notify").is_none(),
        "synthetic io_notify host must not exist outside cfg(test)"
    );
    // Production reactor surface stays registered.
    assert!(lookup_host_function("spectra.async.reactor.backend").is_some());
    assert!(lookup_host_function("spectra.async.reactor.timer").is_some());
    assert!(lookup_host_function("spectra.async.reactor.poll").is_some());
}
