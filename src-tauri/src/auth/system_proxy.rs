use super::inference::SystemProxyRoute;
use std::ffi::c_void;
use std::ptr;
use windows_sys::Win32::Foundation::{GetLastError, GlobalFree, ERROR_FILE_NOT_FOUND};
use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpGetIEProxyConfigForCurrentUser, WinHttpGetProxyForUrl, WinHttpOpen,
    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_NAMED_PROXY,
    WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_AUTOPROXY_AUTO_DETECT, WINHTTP_AUTOPROXY_CONFIG_URL,
    WINHTTP_AUTOPROXY_OPTIONS, WINHTTP_AUTO_DETECT_TYPE_DHCP, WINHTTP_AUTO_DETECT_TYPE_DNS_A,
    WINHTTP_CURRENT_USER_IE_PROXY_CONFIG, WINHTTP_PROXY_INFO,
};

pub(super) fn resolve_current_user(api_url: &str) -> Result<SystemProxyRoute, ()> {
    let mut config = WINHTTP_CURRENT_USER_IE_PROXY_CONFIG::default();
    let config_read = unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) } != 0;
    let config_error = (!config_read).then(|| unsafe { GetLastError() });
    let config = IeProxyConfig(config);
    if let Some(error) = config_error {
        return if error == ERROR_FILE_NOT_FOUND {
            Ok(SystemProxyRoute::UseEnvironment)
        } else {
            Err(())
        };
    }

    let static_proxy = unsafe { wide_pointer_to_string(config.0.lpszProxy) };
    let bypass = unsafe { wide_pointer_to_string(config.0.lpszProxyBypass) }
        .map(|value| parse_bypass_rules(&value))
        .unwrap_or_default();
    let auto_config_url = unsafe { wide_pointer_to_string(config.0.lpszAutoConfigUrl) };
    let auto_detect = config.0.fAutoDetect != 0;

    if auto_detect || auto_config_url.is_some() {
        match resolve_automatic_proxy(config.0.lpszAutoConfigUrl, auto_detect, api_url) {
            Ok(Some((proxy, auto_bypass))) => {
                return route_from_proxy(Some(&proxy), auto_bypass.as_deref());
            }
            Ok(None) => return Ok(SystemProxyRoute::Direct),
            Err(()) if static_proxy.is_some() => {
                return route_from_proxy(static_proxy.as_deref(), bypass_text(&bypass).as_deref());
            }
            Err(()) => return Err(()),
        }
    }

    match static_proxy.as_deref() {
        Some(proxy) if !proxy.trim().is_empty() => {
            route_from_proxy(Some(proxy), bypass_text(&bypass).as_deref())
        }
        _ => Ok(SystemProxyRoute::UseEnvironment),
    }
}

fn resolve_automatic_proxy(
    auto_config_url: *const u16,
    auto_detect: bool,
    api_url: &str,
) -> Result<Option<(String, Option<String>)>, ()> {
    let agent: Vec<u16> = "秘仪".encode_utf16().chain(Some(0)).collect();
    let session = unsafe {
        WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        )
    };
    if session.is_null() {
        return Err(());
    }
    let session = WinHttpSession(session);

    let url: Vec<u16> = api_url.encode_utf16().chain(Some(0)).collect();
    let mut options = WINHTTP_AUTOPROXY_OPTIONS::default();
    if !auto_config_url.is_null() {
        options.dwFlags |= WINHTTP_AUTOPROXY_CONFIG_URL;
        options.lpszAutoConfigUrl = auto_config_url;
    }
    if auto_detect {
        options.dwFlags |= WINHTTP_AUTOPROXY_AUTO_DETECT;
        options.dwAutoDetectFlags = WINHTTP_AUTO_DETECT_TYPE_DHCP | WINHTTP_AUTO_DETECT_TYPE_DNS_A;
    }
    options.fAutoLogonIfChallenged = 0;

    let mut proxy_info = WINHTTP_PROXY_INFO::default();
    let resolved =
        unsafe { WinHttpGetProxyForUrl(session.0, url.as_ptr(), &mut options, &mut proxy_info) }
            != 0;
    let proxy_info = ProxyInfo(proxy_info);
    if !resolved {
        return Err(());
    }
    if proxy_info.0.dwAccessType == WINHTTP_ACCESS_TYPE_NO_PROXY {
        return Ok(None);
    }
    if proxy_info.0.dwAccessType != WINHTTP_ACCESS_TYPE_NAMED_PROXY {
        return Err(());
    }

    let proxy = unsafe { wide_pointer_to_string(proxy_info.0.lpszProxy) }.ok_or(())?;
    let bypass = unsafe { wide_pointer_to_string(proxy_info.0.lpszProxyBypass) };
    Ok(Some((proxy, bypass)))
}

fn route_from_proxy(proxy: Option<&str>, bypass: Option<&str>) -> Result<SystemProxyRoute, ()> {
    let Some(proxy) = proxy.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(SystemProxyRoute::Direct);
    };

    let has_protocol_mappings = proxy
        .split(';')
        .any(|entry| entry.split_once('=').is_some());
    let (http_proxy, https_proxy) = if has_protocol_mappings {
        let mut http_proxy = None;
        let mut https_proxy = None;
        for entry in proxy.split(';') {
            let Some((protocol, server)) = entry.split_once('=') else {
                continue;
            };
            match protocol.trim().to_ascii_lowercase().as_str() {
                "http" => http_proxy = Some(normalize_proxy_server(server)?),
                "https" => https_proxy = Some(normalize_proxy_server(server)?),
                _ => {}
            }
        }
        if http_proxy.is_none() && https_proxy.is_none() {
            return Err(());
        }
        (http_proxy, https_proxy)
    } else {
        let first_server = proxy.split(';').next().ok_or(())?;
        let server = normalize_proxy_server(first_server)?;
        (Some(server.clone()), Some(server))
    };

    Ok(SystemProxyRoute::Explicit {
        http_proxy,
        https_proxy,
        bypass: bypass.map(parse_bypass_rules).unwrap_or_default(),
    })
}

fn normalize_proxy_server(server: &str) -> Result<String, ()> {
    let server = server.trim();
    if server.is_empty() {
        return Err(());
    }
    let normalized = if server.contains("://") {
        server.to_owned()
    } else {
        format!("http://{server}")
    };
    let parsed = url::Url::parse(&normalized).map_err(|_| ())?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(());
    }
    Ok(normalized)
}

fn parse_bypass_rules(bypass: &str) -> Vec<String> {
    bypass
        .split([';', ','])
        .map(str::trim)
        .filter(|rule| !rule.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn bypass_text(rules: &[String]) -> Option<String> {
    (!rules.is_empty()).then(|| rules.join(";"))
}

unsafe fn wide_pointer_to_string(pointer: *const u16) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    let mut length = 0;
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
    }
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(pointer, length) });
    Some(value)
}

struct IeProxyConfig(WINHTTP_CURRENT_USER_IE_PROXY_CONFIG);

impl Drop for IeProxyConfig {
    fn drop(&mut self) {
        unsafe {
            free_global_string(self.0.lpszAutoConfigUrl);
            free_global_string(self.0.lpszProxy);
            free_global_string(self.0.lpszProxyBypass);
        }
    }
}

struct ProxyInfo(WINHTTP_PROXY_INFO);

impl Drop for ProxyInfo {
    fn drop(&mut self) {
        unsafe {
            free_global_string(self.0.lpszProxy);
            free_global_string(self.0.lpszProxyBypass);
        }
    }
}

struct WinHttpSession(*mut c_void);

impl Drop for WinHttpSession {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}

unsafe fn free_global_string(pointer: *mut u16) {
    if !pointer.is_null() {
        let _ = unsafe { GlobalFree(pointer.cast()) };
    }
}
