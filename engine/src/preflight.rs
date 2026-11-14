#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::Command;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 앱 GUI를 실행하기 전, OS별로 구동에 필요한 런타임/라이브러리/보조 도구를 자동 탐지하고
/// 누락된 요소가 있으면 사용자에게 안내한 뒤 승인을 받아 설치합니다.
pub(crate) fn ensure_runtime_dependencies() {
    #[cfg(target_os = "windows")]
    {
        check_windows_webview2();
    }

    #[cfg(target_os = "macos")]
    {
        check_macos_dependencies();
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        check_linux_dependencies();
    }
}

#[cfg(target_os = "windows")]
fn is_webview2_installed() -> bool {
    let reg_keys = [
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        r"HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    ];

    for key in &reg_keys {
        if let Ok(output) = Command::new("reg")
            .args(["query", key, "/v", "pv"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.contains("pv") && !line.contains("0.0.0.0") {
                        return true;
                    }
                }
            }
        }
    }

    // 레지스트리 외 직접 설치 경로 폴백 확인
    if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
        let p = std::path::PathBuf::from(pf86)
            .join("Microsoft")
            .join("EdgeWebView")
            .join("Application");
        if p.exists() {
            return true;
        }
    }
    if let Ok(pf) = std::env::var("ProgramFiles") {
        let p = std::path::PathBuf::from(pf)
            .join("Microsoft")
            .join("EdgeWebView")
            .join("Application");
        if p.exists() {
            return true;
        }
    }

    false
}

#[cfg(target_os = "windows")]
fn show_windows_confirm(title: &str, message: &str) -> bool {
    let t = title.replace('\'', "''");
    let m = message.replace('\'', "''");
    let ps_cmd = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         $r = [System.Windows.Forms.MessageBox]::Show('{}', '{}', \
         [System.Windows.Forms.MessageBoxButtons]::YesNo, \
         [System.Windows.Forms.MessageBoxIcon]::Question); \
         if ($r -eq [System.Windows.Forms.DialogResult]::Yes) {{ exit 0 }} else {{ exit 1 }}",
        m, t
    );

    match Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
    {
        Ok(st) => st.success(),
        Err(_) => false,
    }
}

#[cfg(target_os = "windows")]
fn show_windows_info(title: &str, message: &str, is_error: bool) {
    let t = title.replace('\'', "''");
    let m = message.replace('\'', "''");
    let icon = if is_error { "Error" } else { "Information" };
    let ps_cmd = format!(
        "Add-Type -AssemblyName System.Windows.Forms; \
         [System.Windows.Forms.MessageBox]::Show('{}', '{}', \
         [System.Windows.Forms.MessageBoxButtons]::OK, \
         [System.Windows.Forms.MessageBoxIcon]::{} ) | Out-Null",
        m, t, icon
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps_cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

#[cfg(target_os = "windows")]
fn check_windows_webview2() {
    if is_webview2_installed() {
        return;
    }

    let approved = show_windows_confirm(
        "S-conc 필수 구성 요소 설치 안내",
        "한국어 말뭉치 통합 검색기(S-conc)를 실행하려면 'Microsoft Edge WebView2 런타임'이 필요합니다.\n\n\
         현재 PC에서 WebView2 런타임이 감지되지 않았습니다.\n\
         Microsoft 공식 설치 프로그램을 자동으로 다운로드하여 설치하시겠습니까?",
    );

    if !approved {
        show_windows_info(
            "S-conc 실행 취소",
            "필수 구성 요소(Microsoft Edge WebView2 런타임)가 설치되지 않아 프로그램을 종료합니다.\n\
             추후 WebView2 런타임 설치 후 다시 실행해 주세요.",
            false,
        );
        std::process::exit(1);
    }

    let installer_path = std::env::temp_dir().join("MicrosoftEdgeWebView2Setup.exe");
    let installer_str = installer_path.to_string_lossy().replace('\'', "''");
    let download_ps = format!(
        "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; \
         Invoke-WebRequest -Uri 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' -OutFile '{}' -UseBasicParsing",
        installer_str
    );

    let dl_ok = Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &download_ps])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !dl_ok || !installer_path.exists() {
        show_windows_info(
            "S-conc 다운로드 오류",
            "WebView2 설치 파일을 다운로드하지 못했습니다.\n\
             인터넷 연결 상태를 확인하거나 Microsoft 공식 페이지에서 WebView2 런타임을 직접 설치해 주세요.",
            true,
        );
        std::process::exit(1);
    }

    let install_ok = Command::new(&installer_path)
        .args(["/install"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let _ = std::fs::remove_file(&installer_path);

    if !install_ok && !is_webview2_installed() {
        show_windows_info(
            "S-conc 설치 실패",
            "WebView2 런타임 설치가 완료되지 않았습니다.\n\
             관리자 권한으로 다시 실행하거나 WebView2 런타임을 수동 설치한 후 재시도해 주세요.",
            true,
        );
        std::process::exit(1);
    }
}

#[cfg(target_os = "macos")]
fn check_macos_dependencies() {
    let mut missing: Vec<&str> = Vec::new();

    if !std::path::Path::new("/System/Library/Frameworks/WebKit.framework").exists() {
        missing.push("macOS WebKit 시스템 프레임워크");
    }

    // osascript는 기본 포함이지만 누락 여부를 점검
    let has_osascript = std::path::Path::new("/usr/bin/osascript").exists();
    if !has_osascript {
        missing.push("osascript (파일/폴더 선택 대화상자)");
    }

    if missing.is_empty() {
        return;
    }

    let list_str = missing.join(", ");
    if has_osascript {
        let script = format!(
            "display dialog \"S-conc 실행에 필요한 시스템 구성 요소가 감지되지 않았습니다:\\n{}\\n\\nmacOS 소프트웨어 업데이트 또는 Apple 명령어 도구를 확인하여 설치를 진행할까요?\" buttons {{\"취소\", \"확인 및 안내\"}} default button \"확인 및 안내\" with title \"S-conc 구성 요소 점검\"",
            list_str.replace('"', "\\\"")
        );
        if let Ok(out) = Command::new("osascript").args(["-e", &script]).output() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.contains("확인 및 안내") {
                let _ = Command::new("xcode-select").args(["--install"]).spawn();
            } else {
                std::process::exit(1);
            }
        }
    } else {
        eprintln!(
            "[S-conc] 필수 구성 요소 누락: {}. macOS 시스템 환경에서 해당 요소를 설치해 주세요.",
            list_str
        );
        std::process::exit(1);
    }
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn check_linux_dependencies() {
    let has_webkit = [
        "/usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0",
        "/usr/lib/x86_64-linux-gnu/libwebkit2gtk-4.0.so.0",
        "/usr/lib/aarch64-linux-gnu/libwebkit2gtk-4.1.so.0",
        "/usr/lib/aarch64-linux-gnu/libwebkit2gtk-4.0.so.0",
        "/usr/lib64/libwebkit2gtk-4.1.so.0",
        "/usr/lib64/libwebkit2gtk-4.0.so.0",
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists());

    if has_webkit {
        return;
    }

    // zenity가 있으면 GUI 확인, 없으면 stderr 출력
    let has_zenity = Command::new("which")
        .arg("zenity")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if has_zenity {
        let ok = Command::new("zenity")
            .args([
                "--question",
                "--title=S-conc 필수 라이브러리 설치",
                "--text=S-conc 실행에 필요한 WebKit2GTK 라이브러리(libwebkit2gtk)가 없습니다.\n지금 패키지 관리자로 설치를 시도할까요?",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            let _ = Command::new("pkexec")
                .args(["apt-get", "install", "-y", "libwebkit2gtk-4.1-0"])
                .status();
        } else {
            std::process::exit(1);
        }
    } else {
        eprintln!("[S-conc] 필수 라이브러리 libwebkit2gtk가 설치되어 있지 않습니다.");
    }
}
