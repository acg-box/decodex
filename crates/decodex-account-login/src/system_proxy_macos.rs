//! Destination-specific macOS proxy resolution, including bounded PAC execution.
//! Derived from OpenAI Codex 595cc91 (Apache-2.0); see THIRD_PARTY_NOTICES.md.

use std::{
	ffi::c_void,
	ptr,
	time::{Duration, Instant},
};

use super::{RouteFailureClass, SystemProxyDecision};
use core_foundation::{
	array::{CFArray, CFArrayRef},
	base::{CFEqual, CFGetTypeID, CFIndex, CFType, CFTypeRef, TCFType, kCFAllocatorDefault},
	dictionary::{CFDictionary, CFDictionaryRef},
	error::CFErrorRef,
	number::CFNumber,
	runloop::{
		CFRunLoop, CFRunLoopSource, CFRunLoopSourceInvalidate, CFRunLoopSourceRef,
		kCFRunLoopDefaultMode,
	},
	string::{CFString, CFStringRef},
	url::{CFURL, CFURLCreateWithString, CFURLGetTypeID, CFURLRef},
};

const PAC_EXECUTION_TIMEOUT: Duration = Duration::from_secs(5);

type ProxyDictionary = CFDictionary<CFString, CFType>;
type ProxyArray = CFArray<ProxyDictionary>;

#[repr(C)]
struct CFStreamClientContext {
	version: CFIndex,
	info: *mut c_void,
	retain: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
	release: Option<unsafe extern "C" fn(*mut c_void)>,
	copy_description: Option<unsafe extern "C" fn(*mut c_void) -> CFStringRef>,
}

type CFProxyAutoConfigurationResultCallback =
	unsafe extern "C" fn(*mut c_void, CFArrayRef, CFErrorRef);

#[link(name = "CFNetwork", kind = "framework")]
unsafe extern "C" {
	fn CFNetworkCopySystemProxySettings() -> CFDictionaryRef;
	static kCFProxyTypeKey: CFStringRef;
	static kCFProxyHostNameKey: CFStringRef;
	static kCFProxyPortNumberKey: CFStringRef;
	static kCFProxyAutoConfigurationURLKey: CFStringRef;
	static kCFProxyAutoConfigurationJavaScriptKey: CFStringRef;
	static kCFProxyTypeNone: CFStringRef;
	static kCFProxyTypeHTTP: CFStringRef;
	static kCFProxyTypeHTTPS: CFStringRef;
	static kCFProxyTypeSOCKS: CFStringRef;
	static kCFProxyTypeAutoConfigurationURL: CFStringRef;
	static kCFProxyTypeAutoConfigurationJavaScript: CFStringRef;

	fn CFNetworkCopyProxiesForURL(url: CFURLRef, proxy_settings: CFDictionaryRef) -> CFArrayRef;
	fn CFNetworkExecuteProxyAutoConfigurationURL(
		proxy_auto_config_url: CFURLRef,
		target_url: CFURLRef,
		callback: CFProxyAutoConfigurationResultCallback,
		client_context: *mut CFStreamClientContext,
	) -> CFRunLoopSourceRef;
	fn CFNetworkExecuteProxyAutoConfigurationScript(
		proxy_auto_config_script: CFStringRef,
		target_url: CFURLRef,
		callback: CFProxyAutoConfigurationResultCallback,
		client_context: *mut CFStreamClientContext,
	) -> CFRunLoopSourceRef;
}

pub(super) fn resolve(request_url: &str) -> SystemProxyDecision {
	let Some(target_url) = cf_url(request_url) else {
		return SystemProxyDecision::Unavailable { failure: RouteFailureClass::InvalidProxyConfig };
	};

	let Some(settings) = system_proxy_settings() else {
		return SystemProxyDecision::Unavailable {
			failure: RouteFailureClass::ProxyResolutionUnavailable,
		};
	};

	let Some(proxies) = copy_proxies_for_url(&target_url, &settings) else {
		return SystemProxyDecision::Unavailable {
			failure: RouteFailureClass::ProxyResolutionUnavailable,
		};
	};

	proxy_array_decision(&proxies, &target_url)
}

fn system_proxy_settings() -> Option<CFDictionary<CFString, CFType>> {
	// SAFETY: CFNetwork returns an owned immutable system settings snapshot.
	let settings = unsafe { CFNetworkCopySystemProxySettings() };
	if settings.is_null() {
		None
	} else {
		Some(unsafe { CFDictionary::wrap_under_create_rule(settings) })
	}
}

fn copy_proxies_for_url(
	target_url: &CFURL,
	settings: &CFDictionary<CFString, CFType>,
) -> Option<ProxyArray> {
	let proxies = unsafe {
		CFNetworkCopyProxiesForURL(target_url.as_concrete_TypeRef(), settings.as_concrete_TypeRef())
	};
	if proxies.is_null() {
		None
	} else {
		Some(unsafe { ProxyArray::wrap_under_create_rule(proxies) })
	}
}

fn proxy_array_decision(proxies: &ProxyArray, target_url: &CFURL) -> SystemProxyDecision {
	let mut saw_unsupported = false;
	let mut saw_unavailable = false;

	// CFNetwork returns candidates in failover order, but the shared resolver currently carries
	// only one route. This matches the Windows limitation; cross-platform retry requires request
	// replay semantics and is intentionally deferred.
	for proxy in proxies {
		match proxy_entry_decision(&proxy, target_url) {
			ProxyEntryDecision::Direct => return SystemProxyDecision::Direct,
			ProxyEntryDecision::Proxy { url } => return SystemProxyDecision::Proxy { url },
			ProxyEntryDecision::UnsupportedScheme => saw_unsupported = true,
			ProxyEntryDecision::Unavailable => saw_unavailable = true,
		}
	}

	if saw_unsupported {
		SystemProxyDecision::Unavailable { failure: RouteFailureClass::UnsupportedProxyScheme }
	} else if saw_unavailable {
		SystemProxyDecision::Unavailable { failure: RouteFailureClass::ProxyResolutionUnavailable }
	} else {
		SystemProxyDecision::Direct
	}
}

fn proxy_entry_decision(proxy: &ProxyDictionary, target_url: &CFURL) -> ProxyEntryDecision {
	let Some(proxy_type) = cf_string_value(proxy, unsafe { kCFProxyTypeKey }) else {
		return ProxyEntryDecision::Unavailable;
	};

	if cf_string_equals(&proxy_type, unsafe { kCFProxyTypeNone }) {
		return ProxyEntryDecision::Direct;
	}

	if cf_string_equals(&proxy_type, unsafe { kCFProxyTypeHTTP }) {
		return concrete_proxy_entry(proxy, "http");
	}

	if cf_string_equals(&proxy_type, unsafe { kCFProxyTypeHTTPS }) {
		// CFNetwork's HTTPS proxy type is a tunneling proxy for HTTPS destinations; it does not
		// preserve an explicit TLS-to-proxy transport. See https://developer.apple.com/documentation/cfnetwork/kcfproxytypehttps.
		return concrete_proxy_entry(proxy, "http");
	}

	if cf_string_equals(&proxy_type, unsafe { kCFProxyTypeSOCKS }) {
		return ProxyEntryDecision::UnsupportedScheme;
	}

	if cf_string_equals(&proxy_type, unsafe { kCFProxyTypeAutoConfigurationURL }) {
		let Some(pac_url) = cf_url_value(proxy, unsafe { kCFProxyAutoConfigurationURLKey }) else {
			return ProxyEntryDecision::Unavailable;
		};
		return pac_decision(execute_pac_url(&pac_url, target_url), target_url);
	}

	if cf_string_equals(&proxy_type, unsafe { kCFProxyTypeAutoConfigurationJavaScript }) {
		let Some(script) =
			cf_string_value(proxy, unsafe { kCFProxyAutoConfigurationJavaScriptKey })
		else {
			return ProxyEntryDecision::Unavailable;
		};
		return pac_decision(
			execute_pac(|callback, context| unsafe {
				CFNetworkExecuteProxyAutoConfigurationScript(
					script.as_concrete_TypeRef(),
					target_url.as_concrete_TypeRef(),
					callback,
					context,
				)
			}),
			target_url,
		);
	}

	ProxyEntryDecision::Unavailable
}

fn pac_decision(
	result: Result<ProxyArray, RouteFailureClass>,
	target_url: &CFURL,
) -> ProxyEntryDecision {
	let proxies = match result {
		Ok(proxies) => proxies,
		Err(RouteFailureClass::UnsupportedProxyScheme) => {
			return ProxyEntryDecision::UnsupportedScheme;
		},
		Err(_) => return ProxyEntryDecision::Unavailable,
	};

	match proxy_array_decision(&proxies, target_url) {
		SystemProxyDecision::Direct => ProxyEntryDecision::Direct,
		SystemProxyDecision::Proxy { url } => ProxyEntryDecision::Proxy { url },
		SystemProxyDecision::Unavailable { failure: RouteFailureClass::UnsupportedProxyScheme } =>
			ProxyEntryDecision::UnsupportedScheme,
		SystemProxyDecision::Unavailable { failure: _ } => ProxyEntryDecision::Unavailable,
	}
}

fn execute_pac_url(pac_url: &CFURL, target_url: &CFURL) -> Result<ProxyArray, RouteFailureClass> {
	execute_pac(|callback, context| unsafe {
		CFNetworkExecuteProxyAutoConfigurationURL(
			pac_url.as_concrete_TypeRef(),
			target_url.as_concrete_TypeRef(),
			callback,
			context,
		)
	})
}

fn execute_pac(
	create_source: impl FnOnce(
		CFProxyAutoConfigurationResultCallback,
		*mut CFStreamClientContext,
	) -> CFRunLoopSourceRef,
) -> Result<ProxyArray, RouteFailureClass> {
	let mut state = PacRunLoopState { result: None };
	let mut context = CFStreamClientContext {
		version: 0,
		info: (&mut state as *mut PacRunLoopState).cast::<c_void>(),
		retain: None,
		release: None,
		copy_description: None,
	};

	let source = create_source(pac_result_callback, &mut context);
	if source.is_null() {
		return Err(RouteFailureClass::ProxyResolutionUnavailable);
	}

	let source = unsafe { CFRunLoopSource::wrap_under_create_rule(source) };
	let run_loop = CFRunLoop::get_current();
	let mode = unsafe { kCFRunLoopDefaultMode };
	run_loop.add_source(&source, mode);

	let started_at = Instant::now();
	while state.result.is_none() && started_at.elapsed() < PAC_EXECUTION_TIMEOUT {
		CFRunLoop::run_in_mode(mode, Duration::from_millis(50), true);
	}

	if state.result.is_none() {
		unsafe { CFRunLoopSourceInvalidate(source.as_concrete_TypeRef()) };
	}
	run_loop.remove_source(&source, mode);
	state.result.unwrap_or(Err(RouteFailureClass::ConnectTimeout))
}

unsafe extern "C" fn pac_result_callback(
	client: *mut c_void,
	proxies: CFArrayRef,
	error: CFErrorRef,
) {
	let state = unsafe { &mut *client.cast::<PacRunLoopState>() };
	state.result = if !error.is_null() || proxies.is_null() {
		Some(Err(RouteFailureClass::ProxyResolutionUnavailable))
	} else {
		Some(Ok(unsafe { ProxyArray::wrap_under_get_rule(proxies) }))
	};
	CFRunLoop::get_current().stop();
}

struct PacRunLoopState {
	result: Option<Result<ProxyArray, RouteFailureClass>>,
}

fn concrete_proxy_entry(proxy: &ProxyDictionary, proxy_scheme: &str) -> ProxyEntryDecision {
	let Some(host) = cf_string_value(proxy, unsafe { kCFProxyHostNameKey })
		.map(|host| host.to_string())
		.filter(|host| !host.is_empty())
	else {
		return ProxyEntryDecision::Unavailable;
	};

	let host = bracket_ipv6_host(&host);
	let url = match cf_i32_value(proxy, unsafe { kCFProxyPortNumberKey }) {
		Some(port) if port > 0 => format!("{proxy_scheme}://{host}:{port}"),
		_ => format!("{proxy_scheme}://{host}"),
	};
	ProxyEntryDecision::Proxy { url }
}

fn bracket_ipv6_host(host: &str) -> String {
	if host.contains(':') && !host.starts_with('[') {
		format!("[{host}]")
	} else {
		host.to_string()
	}
}

fn cf_string_value(proxy: &ProxyDictionary, key: CFStringRef) -> Option<CFString> {
	proxy.find(key).and_then(|value| value.downcast::<CFString>())
}

fn cf_i32_value(proxy: &ProxyDictionary, key: CFStringRef) -> Option<i32> {
	proxy.find(key).and_then(|value| value.downcast::<CFNumber>()).and_then(|value| value.to_i32())
}

fn cf_url_value(proxy: &ProxyDictionary, key: CFStringRef) -> Option<CFURL> {
	proxy.find(key).and_then(|value| {
		if unsafe { CFGetTypeID(value.as_CFTypeRef()) == CFURLGetTypeID() } {
			Some(unsafe { CFURL::wrap_under_get_rule(value.as_CFTypeRef() as CFURLRef) })
		} else {
			value.downcast::<CFString>().and_then(|value| cf_url(value.to_string().as_str()))
		}
	})
}

fn cf_string_equals(value: &CFString, expected: CFStringRef) -> bool {
	unsafe { CFEqual(value.as_CFTypeRef(), expected as CFTypeRef) != 0 }
}

fn cf_url(value: &str) -> Option<CFURL> {
	let value = CFString::new(value);
	let url = unsafe {
		CFURLCreateWithString(kCFAllocatorDefault, value.as_concrete_TypeRef(), ptr::null())
	};
	if url.is_null() { None } else { Some(unsafe { CFURL::wrap_under_create_rule(url) }) }
}

enum ProxyEntryDecision {
	Direct,
	Proxy { url: String },
	UnsupportedScheme,
	Unavailable,
}

#[cfg(test)]
mod tests {
	use super::{
		CFNetworkExecuteProxyAutoConfigurationScript, SystemProxyDecision, cf_url, execute_pac,
		proxy_array_decision,
	};
	use core_foundation::{base::TCFType as _, string::CFString};

	#[test]
	fn pac_routes_the_destination_host_and_preserves_direct_first() {
		let script = CFString::new(
			"function FindProxyForURL(url, host) { if (host === 'auth.example.com') return 'PROXY 127.0.0.1:8123'; return 'DIRECT; PROXY 127.0.0.1:8124'; }",
		);
		for (url, expected_proxy) in [
			("https://auth.example.com/oauth/token", true),
			("https://other.example.com/oauth/token", false),
			("http://auth.example.com/oauth/token", true),
			("http://other.example.com/other", false),
		] {
			let target = cf_url(url).expect("target URL");
			let result = execute_pac(|callback, context| unsafe {
				CFNetworkExecuteProxyAutoConfigurationScript(
					script.as_concrete_TypeRef(),
					target.as_concrete_TypeRef(),
					callback,
					context,
				)
			})
			.unwrap_or_else(|_| panic!("CFNetwork PAC evaluation failed"));
			match proxy_array_decision(&result, &target) {
				SystemProxyDecision::Proxy { url } if expected_proxy =>
					assert_eq!(url, "http://127.0.0.1:8123"),
				SystemProxyDecision::Direct if !expected_proxy => {},
				SystemProxyDecision::Direct => panic!("PAC selected DIRECT for {url}"),
				SystemProxyDecision::Proxy { url } => panic!("PAC selected proxy {url}"),
				SystemProxyDecision::Unavailable { .. } => panic!("PAC unavailable"),
			}
		}
	}
}
